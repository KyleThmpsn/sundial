//! A level's ambience as native sound points. Reach plays two kinds of looping sound (`lsnd`) in a
//! level: each area's background loop while the listener is in one of its clusters, and the loops
//! of the sound scenery the scenario places. Each becomes a private Wwise bank of its tracks and
//! detail sounds, a sound record naming it, and rows of a copy of the donor's sound point table,
//! one per placement, at its position.
//!
//! | Record | Offset | Contents |
//! | --- | --- | --- |
//! | Scenario sound scenery, `0x8C` rows | `+0x1C8` | Palette `+0x00` (`i16`), position `+0x08`, full volume within `+0x78`, silent beyond `+0x84` (world units, 0 for the loop's own) |
//! | Sound scenery palette, `0x10` rows | `+0x1D4` | The `ssce` tag, whose object attachments (`+0x104`, `0x20` rows) name its loop at `+0x00` |
//! | Structure BSP area palette, `0x78` rows | `+0x18C` | Background loop `+0x20` |
//! | Structure BSP clusters, `0x8C` rows | `+0x138` | Bounds as x, y and z ranges `+0x00`, area palette row `+0x1C` (`i16`) |
//! | Looping sound | `+0x0C` | Full volume within and silent beyond, in world units |
//! | Looping sound tracks, `0xB0` rows | `+0x20` | Gain `+0x08` (dB), start, loop and end sounds `+0x0C`, `+0x1C` and `+0x2C`, fade in and out `+0x7C` and `+0x84` (seconds) |
//! | Looping sound details, `0x48` rows | `+0x2C` | Sound `+0x04`, the wait between plays `+0x18` (two seconds), gain `+0x20` (dB) |
//!
//! A cluster whose bounds hold every other cluster's is the level's outside, so its area's loop
//! plays everywhere, without position. Another area's loop plays at each of its clusters, at full
//! volume within the cluster's bounds. A placed loop falls off with the inverse of distance from
//! its full-volume distance, faded to silence at its farthest.
use super::{
    super::{
        audio, behavior,
        cache::{Cache, Tag},
    },
    METRES_PER_UNIT,
};
use crate::{
    presentation::{Graph, put},
    tiger::{
        audio::{
            bank::{
                Bank,
                looping::{Layer, Phase},
            },
            transcode,
        },
        reader::Reader,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const SCENERY: (usize, usize) = (0x1C8, 0x8C);
const SCENERY_POSITION: usize = 0x08;
const SCENERY_MINIMUM: usize = 0x78;
const SCENERY_MAXIMUM: usize = 0x84;
const SCENERY_PALETTE: (usize, usize) = (0x1D4, 0x10);
const ATTACHMENTS: (usize, usize) = (0x104, 0x20);
const AREAS: (usize, usize) = (0x18C, 0x78);
const AREA_LOOP: usize = 0x20;
const CLUSTERS: (usize, usize) = (0x138, 0x8C);
const CLUSTER_AREA: usize = 0x1C;
const LOOP_DISTANCE: usize = 0x0C;
const TRACKS: (usize, usize) = (0x20, 0xB0);
const TRACK_GAIN: usize = 0x08;
const TRACK_FADE_IN: usize = 0x7C;
const TRACK_FADE_OUT: usize = 0x84;
const DETAILS: (usize, usize) = (0x2C, 0x48);
const DETAIL_SOUND: usize = 0x04;
const DETAIL_GAP: usize = 0x18;
const DETAIL_GAIN: usize = 0x20;

/// Native sound point placement table rows (`808099D8`, `0x90` bytes): translation `+0x20`,
/// rotation `+0x10`, identity `+0x70` and a self-relative resource offset `+0x78`.
const TABLE_ROW_CLASS: u32 = 0x8080_99D8;
const ARRAY_MARKER: u32 = 0x8080_9FBD;
const ROW_SIZE: usize = 0x90;
const ROW_ROTATION: usize = 0x10;
const ROW_TRANSLATION: usize = 0x20;
const ROW_IDENTITY: usize = 0x70;
const ROW_RESOURCE: usize = 0x78;
/// A sound point (`80806B5B`), `0x20` bytes behind its class word, names its sound record at
/// `+0x10`.
const POINT_CLASS: u32 = 0x8080_6B5B;
const POINT_SIZE: usize = 0x20;
const POINT_SOUND: usize = 0x10;
/// A sound record (`80809802`): play and stop events `+0x08` and `+0x0C`, attenuation record
/// `+0x10`, bank `+0x14`, media count `+0x18` and `+0x40`, media from `+0x50`.
const SOUND_CLASS: u32 = 0x8080_9802;
const SOUND_PLAY: usize = 0x08;
const SOUND_STOP: usize = 0x0C;
const SOUND_ATTENUATION: usize = 0x10;
const SOUND_BANK: usize = 0x14;
const SOUND_MEDIA: usize = 0x50;
/// An attenuation record (`80808E68`) holds the distance beyond which its sound is silent at
/// `+0x0C`, in meters, as the bank's own attenuation does.
const ATTENUATION_CLASS: u32 = 0x8080_8E68;
const ATTENUATION_REACH: usize = 0x0C;

/// The farthest a sound that plays everywhere reaches, in meters.
const EVERYWHERE: f32 = 2000.;
/// How far past its cluster's bounds an area's loop fades out, in meters.
const AREA_FADE: f32 = 6.;

/// The donor's sound point table, whose first row and point the map's points copy, and the
/// decoder for the level's sounds, which Reach keeps in `fmod/pc/sfx.fsb` beside its maps.
pub struct Templates {
    pub points: u32,
    pub decoder: PathBuf,
}

/// One looping sound as it plays in the map: its source loop, and how its volume falls off with
/// distance, or none when it plays everywhere.
#[derive(Clone, PartialEq)]
struct Sound {
    source: Tag,
    curve: Option<Vec<[f32; 2]>>,
    reach: f32,
}

/// `minimum` and `maximum` in meters as a volume curve over distance: full volume within
/// `minimum`, then falling with the inverse of distance and faded to silence at `maximum`.
fn falloff(minimum: f32, maximum: f32) -> Vec<[f32; 2]> {
    let minimum = minimum.max(0.1);
    let maximum = maximum.max(minimum * 1.5);
    let mut curve = vec![[0., 0.], [minimum, 0.]];
    for t in [0.25, 0.5, 0.75, 0.9] {
        let distance = minimum + (maximum - minimum) * t;
        let fade = 1. - t * t;
        let gain = 20. * (minimum / distance * fade).log10();
        curve.push([distance, gain.max(-95.)]);
    }
    curve.push([maximum, -96.]);
    curve
}

fn round(value: f32) -> f32 {
    (value * 100.).round() / 100.
}

/// Seconds as whole milliseconds.
fn milliseconds(seconds: f32) -> u32 {
    (seconds.clamp(0., 60.) * 1000.).round() as u32
}

/// The FSB samples a sound plays, every variation of every pitch range, its gain in dB, and how
/// many of it its sound class lets play at once, when it limits them.
fn variations(cache: &Cache, tag: &Tag) -> Result<(Vec<u32>, f32, Option<u16>)> {
    let sound = behavior::sound(cache, tag)?;
    let limit = sound["playback_limits"]["maximum_per_tag"]
        .as_i64()
        .and_then(|limit| u16::try_from(limit).ok())
        .filter(|&limit| limit > 0);
    let mut samples = Vec::new();
    let mut gains = Vec::new();
    for range in sound["pitch_ranges"].as_array().into_iter().flatten() {
        for variant in range["variants"].as_array().into_iter().flatten() {
            samples.push(u32::try_from(
                variant["fsb_info"]
                    .as_u64()
                    .context("Sound sample identity")?,
            )?);
            gains.push(variant["gain_encoded"].as_f64().unwrap_or(0.) as f32);
        }
    }
    ensure!(!samples.is_empty(), "Sound {} has no samples", tag.path);
    let playback = sound["playback"]["gain_db"].as_f64().unwrap_or(0.) as f32;
    Ok((
        samples,
        playback + gains.iter().sum::<f32>() / gains.len() as f32,
        limit,
    ))
}

/// The layers of looping sound `tag`: each track's start, loop and end sounds, and each detail
/// sound playing at random. Also the most instances any of its sounds may play at once.
fn layers(cache: &Cache, tag: &Tag) -> Result<(Vec<Layer>, Option<u16>)> {
    let at = tag.address()?;
    let mut layers = Vec::new();
    let mut limit = None;
    for track in cache.block(at + TRACKS.0, TRACKS.1)? {
        let gain = cache.f32(track + TRACK_GAIN)?;
        for (offset, phase) in [
            (0x0C, Phase::Start),
            (0x1C, Phase::Sustain),
            (0x2C, Phase::Stop),
        ] {
            let Some(sound) = cache.reference(track + offset, Some("snd!"))? else {
                continue;
            };
            let (media, recording, own) = variations(cache, &sound)?;
            limit = limit.max(own);
            layers.push(Layer {
                media,
                gain_db: (gain + recording).clamp(-96., 24.),
                phase,
                fade_in_ms: if matches!(phase, Phase::Sustain) {
                    milliseconds(cache.f32(track + TRACK_FADE_IN)?)
                } else {
                    0
                },
                fade_out_ms: milliseconds(cache.f32(track + TRACK_FADE_OUT)?),
                curves: Vec::new(),
            });
        }
    }
    for detail in cache.block(at + DETAILS.0, DETAILS.1)? {
        let Some(sound) = cache.reference(detail + DETAIL_SOUND, Some("snd!"))? else {
            continue;
        };
        let (media, recording, own) = variations(cache, &sound)?;
        limit = limit.max(own);
        let gap = cache.floats::<2>(detail + DETAIL_GAP)?;
        layers.push(Layer {
            media,
            gain_db: (cache.f32(detail + DETAIL_GAIN)? + recording).clamp(-96., 24.),
            phase: Phase::Detail {
                gap: [gap[0].max(0.), gap[1].max(gap[0]).max(0.)],
            },
            fade_in_ms: 0,
            fade_out_ms: 0,
            curves: Vec::new(),
        });
    }
    ensure!(
        !layers.is_empty(),
        "Looping sound {} plays nothing",
        tag.path
    );
    Ok((layers, limit))
}

/// The level's sounds and where each plays: area loops at the clusters of `bsps`, and the loops of
/// the sound scenery placed in them ([`super::objects::placed_in`]). Winter Contingency's
/// `zoneset_insert` loads three of its BSPs, and 190 of its looping sound placements, such as the
/// windmills in its fifth, belong to others.
#[allow(clippy::type_complexity)]
fn placements(
    cache: &Cache,
    bsps: &[Tag],
    offset: [f32; 3],
) -> Result<(Vec<Sound>, Vec<(usize, [f32; 3])>)> {
    let mut sounds = Vec::<Sound>::new();
    let mut emitters = Vec::new();
    let mut add = |sound: Sound, position: [f32; 3]| {
        let index = match sounds.iter().position(|known| *known == sound) {
            Some(index) => index,
            None => {
                sounds.push(sound);
                sounds.len() - 1
            }
        };
        emitters.push((index, std::array::from_fn(|k| position[k] + offset[k])));
    };
    for bsp in bsps {
        let at = bsp.address()?;
        let areas = cache
            .block(at + AREAS.0, AREAS.1)?
            .into_iter()
            .map(|row| cache.reference(row + AREA_LOOP, Some("lsnd")))
            .collect::<Result<Vec<_>>>()?;
        let clusters = cache
            .block(at + CLUSTERS.0, CLUSTERS.1)?
            .into_iter()
            .map(|row| {
                let bounds = cache.floats::<6>(row)?.map(|value| value * METRES_PER_UNIT);
                Ok((
                    [bounds[0], bounds[2], bounds[4]],
                    [bounds[1], bounds[3], bounds[5]],
                    cache.i16(row + CLUSTER_AREA)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let holds_all = |index: usize| {
            let (low, high, _) = clusters[index];
            clusters
                .iter()
                .all(|(l, h, _)| (0..3).all(|k| low[k] <= l[k] && high[k] >= h[k]))
        };
        let inner = clusters
            .iter()
            .enumerate()
            .filter(|&(index, _)| !holds_all(index))
            .map(|(_, cluster)| cluster)
            .collect::<Vec<_>>();
        let middle = |set: &[&([f32; 3], [f32; 3], i16)]| -> [f32; 3] {
            std::array::from_fn(|k| {
                let low = set.iter().map(|c| c.0[k]).fold(f32::MAX, f32::min);
                let high = set.iter().map(|c| c.1[k]).fold(f32::MIN, f32::max);
                (low + high) / 2.
            })
        };
        let source = |area: i16| {
            usize::try_from(area)
                .ok()
                .and_then(|area| areas.get(area))
                .cloned()
                .flatten()
        };
        // A loop the outside plays everywhere plays at none of its other clusters.
        let outside = clusters
            .iter()
            .enumerate()
            .filter(|&(index, _)| holds_all(index))
            .filter_map(|(_, &(_, _, area))| source(area).map(|tag| tag.datum))
            .collect::<BTreeSet<_>>();
        let mut everywhere = BTreeSet::new();
        for (index, &(low, high, area)) in clusters.iter().enumerate() {
            let Some(source) = source(area) else {
                continue;
            };
            if holds_all(index) {
                // One point, at the middle of the level, plays the outside everywhere.
                if everywhere.insert(source.datum) && !inner.is_empty() {
                    add(
                        Sound {
                            source,
                            curve: None,
                            reach: EVERYWHERE,
                        },
                        middle(&inner),
                    );
                }
                continue;
            }
            if outside.contains(&source.datum) {
                continue;
            }
            let center = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
            let radius = round(
                (0..3)
                    .map(|k| (high[k] - low[k]) / 2.)
                    .map(|half| half * half)
                    .sum::<f32>()
                    .sqrt(),
            );
            add(
                Sound {
                    source,
                    curve: Some(vec![[0., 0.], [radius, 0.], [radius + AREA_FADE, -96.]]),
                    reach: radius + AREA_FADE,
                },
                center,
            );
        }
    }
    let scenario = cache.only("scnr")?.address()?;
    let loaded = super::bsp_mask(cache, bsps)?;
    let palette = cache
        .block(scenario + SCENERY_PALETTE.0, SCENERY_PALETTE.1)?
        .into_iter()
        .map(|row| {
            let Some(scenery) = cache.reference(row, Some("ssce"))? else {
                return Ok(None);
            };
            for attachment in cache.block(scenery.address()? + ATTACHMENTS.0, ATTACHMENTS.1)? {
                if let Some(source) = cache.reference(attachment, None)?
                    && source.group == "lsnd"
                {
                    return Ok(Some(source));
                }
            }
            Ok(None)
        })
        .collect::<Result<Vec<_>>>()?;
    for row in cache.block(scenario + SCENERY.0, SCENERY.1)? {
        if !super::objects::placed_in(cache, row, loaded)? {
            continue;
        }
        let Some(source) = usize::try_from(cache.i16(row)?)
            .ok()
            .and_then(|index| palette.get(index))
            .cloned()
            .flatten()
        else {
            continue;
        };
        let own = cache.floats::<2>(source.address()? + LOOP_DISTANCE)?;
        let placed = [
            cache.f32(row + SCENERY_MINIMUM)?,
            cache.f32(row + SCENERY_MAXIMUM)?,
        ];
        let [minimum, maximum] =
            (if placed[1] > 0. { placed } else { own }).map(|value| round(value * METRES_PER_UNIT));
        let curve = falloff(minimum, maximum);
        let reach = curve.last().map_or(maximum, |point| point[0]);
        add(
            Sound {
                source,
                curve: Some(curve),
                reach,
            },
            cache
                .floats::<3>(row + SCENERY_POSITION)?
                .map(|value| value * METRES_PER_UNIT),
        );
    }
    Ok((sounds, emitters))
}

/// The output bus of `bank`'s sounds: the bus of the nearest container or actor-mixer above them
/// that names one. Every sound of the bank must reach the same one.
fn output_bus(bank: &[u8]) -> Result<u32> {
    let word = |bytes: &[u8], at: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            bytes
                .get(at..at + 4)
                .context("Short bank word")?
                .try_into()?,
        ))
    };
    let mut nodes = BTreeMap::new();
    let mut sounds = Vec::new();
    let mut at = 0;
    while at + 8 <= bank.len() {
        let size = word(bank, at + 4)? as usize;
        let body = bank
            .get(at + 8..at + 8 + size)
            .context("Bank chunk exceeds payload")?;
        if &bank[at..at + 4] == b"BKHD" {
            ensure!(word(body, 0)? == 113, "Donor bank is not version 113");
        }
        if &bank[at..at + 4] == b"HIRC" {
            let mut cursor = 4;
            for _ in 0..word(body, 0)? {
                let kind = *body.get(cursor).context("Bank object type")?;
                let size = word(body, cursor + 1)? as usize;
                let object = body
                    .get(cursor + 5..cursor + 5 + size)
                    .context("Bank object exceeds chunk")?;
                // A node's effects count is followed by its bus and parent. Sounds keep their
                // source description first.
                let node = match kind {
                    2 => Some(18),
                    5 | 6 | 7 | 9 => Some(4),
                    _ => None,
                };
                if let Some(node) = node
                    && object.get(node + 1) == Some(&0)
                {
                    nodes.insert(
                        word(object, 0)?,
                        (word(object, node + 3)?, word(object, node + 7)?),
                    );
                    if kind == 2 {
                        sounds.push(word(object, 0)?);
                    }
                }
                cursor += 5 + size;
            }
        }
        at += 8 + size;
    }
    let mut buses = BTreeSet::new();
    for sound in sounds {
        let mut id = sound;
        let mut bus = 0;
        for _ in 0..16 {
            let Some(&(own, parent)) = nodes.get(&id) else {
                break;
            };
            if own != 0 {
                bus = own;
                break;
            }
            id = parent;
        }
        ensure!(
            bus != 0,
            "Donor sound {sound:08X} names no output bus in its bank"
        );
        buses.insert(bus);
    }
    ensure!(
        buses.len() == 1,
        "Donor bank sounds reach {} output buses",
        buses.len()
    );
    Ok(buses.into_iter().next().unwrap())
}

fn fnv1a64(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// Adds the level's ambience to `graph`: media, banks, attenuations and sound records for its
/// sounds, and a copy of the donor's sound point table placing them. Returns the table's symbol,
/// or none when the level places no looping sound, and a report. `bsps` are the structure BSPs the
/// level loads.
#[allow(clippy::too_many_arguments)]
pub(super) fn add(
    cache: &Cache,
    pages: &mut crate::halo_reach::resource::Pages,
    bsps: &[Tag],
    maps: &Path,
    map: &str,
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    offset: [f32; 3],
) -> Result<(Option<String>, Value)> {
    let (sounds, emitters) = placements(cache, bsps, offset)?;
    if emitters.is_empty() {
        return Ok((None, json!({"points": 0})));
    }
    let table = native.tag(templates.points, None)?;
    let rows = table.array(0x08, ROW_SIZE, Some(TABLE_ROW_CLASS))?;
    let row = *rows.first().context("Donor sound point table is empty")?;
    let resource = table.pointer(row + ROW_RESOURCE)?;
    ensure!(
        table.u32(resource - 4)? == POINT_CLASS,
        "Donor sound point table places no sound point first"
    );
    let row_template = table.0[row..row + ROW_SIZE].to_vec();
    let point_template = table.0[resource..resource + POINT_SIZE].to_vec();
    let record_tag = table.u32(resource + POINT_SOUND)?;
    let record = native.tag(record_tag, Some(SOUND_CLASS))?;
    ensure!(
        record.0.len() >= SOUND_MEDIA && record.pointer(0x20)? == 0x40,
        "Donor sound record envelope differs"
    );
    let attenuation_tag = record.u32(SOUND_ATTENUATION)?;
    let attenuation = native.tag(attenuation_tag, Some(ATTENUATION_CLASS))?;
    let bank_tag = record.u32(SOUND_BANK)?;
    let bus = output_bus(&native.tag(bank_tag, None)?.0)?;
    let media_template = record.u32(SOUND_MEDIA)?;

    // Every sample the sounds play, decoded once. A sample the bank lacks leaves its layer, and a
    // sound left with no layer leaves the map with its points.
    let (mut sound_layers, limits): (Vec<_>, Vec<_>) = sounds
        .iter()
        .map(|sound| layers(cache, &sound.source))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .unzip();
    let samples = sound_layers
        .iter()
        .flatten()
        .flat_map(|layer| layer.media.iter().copied())
        .collect::<BTreeSet<_>>();
    let fsb = maps.join("../fmod/pc/sfx.fsb");
    pages.record(&fsb.canonicalize()?)?;
    pages.record(&templates.decoder.canonicalize()?)?;
    let (decoded, missing) = audio::decode(
        &fsb,
        &templates.decoder,
        &samples,
        &graph.root.join("ambience"),
    )?;
    ensure!(
        !decoded.is_empty(),
        "None of the level's ambience samples are in {}",
        fsb.display()
    );
    let mut dropped = Vec::new();
    for (sound, layers) in sounds.iter().zip(&mut sound_layers) {
        for layer in layers.iter_mut() {
            layer.media.retain(|id| !missing.contains(id));
        }
        let before = layers.len();
        layers.retain(|layer| !layer.media.is_empty());
        if layers.len() < before {
            dropped.push(json!({"loop": sound.source.path, "layers": before - layers.len()}));
        }
    }
    let kept = sound_layers
        .iter()
        .map(|layers| !layers.is_empty())
        .collect::<Vec<_>>();
    let mut index = Vec::with_capacity(sounds.len());
    let mut next = 0;
    for &keep in &kept {
        index.push(keep.then_some(next));
        next += usize::from(keep);
    }
    let emitters = emitters
        .into_iter()
        .filter_map(|(sound, position)| index[sound].map(|sound| (sound, position)))
        .collect::<Vec<_>>();
    let (sounds, sound_layers): (Vec<_>, Vec<_>) = sounds
        .into_iter()
        .zip(sound_layers.into_iter().zip(limits))
        .filter(|(_, (layers, _))| !layers.is_empty())
        .unzip();
    // Each placed loop plays as many instances at once as its Reach sound class allows, the
    // nearest first.
    let banks = sounds
        .iter()
        .zip(&sound_layers)
        .map(|(sound, (layers, limit))| {
            match &sound.curve {
                Some(curve) => Bank::looping_curve(layers, bus, curve, *limit),
                None => Bank::looping_nonspatial(layers, bus),
            }
            .with_context(|| format!("Ambience bank of {}", sound.source.path))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut media_bytes = 0usize;
    for (id, file) in &decoded {
        let wem = transcode::normalize_pcm_wem(&fs::read(file)?)?;
        media_bytes += wem.len();
        graph.add(
            &format!("ambience-media-{id:08X}"),
            media_template,
            &wem,
            None,
            Vec::new(),
        )?;
        graph.nodes.last_mut().unwrap()["storage"] = json!("audio_media");
    }
    let mut records = Vec::with_capacity(sounds.len());
    for (index, (sound, bank)) in sounds.iter().zip(&banks).enumerate() {
        let media = bank
            .media_fields()
            .into_iter()
            .map(|(_, id)| id)
            .collect::<BTreeSet<_>>();
        let bank_symbol = format!("ambience-bank-{index}");
        graph.add(&bank_symbol, bank_tag, &bank.bytes, None, Vec::new())?;
        let node = graph.nodes.last_mut().unwrap();
        node["storage"] = json!("audio_bank");
        node["audio_bank"] = serde_json::to_value(bank)?;
        node["audio_media"] = json!(
            media
                .iter()
                .map(|id| json!({"id": id, "symbol": format!("ambience-media-{id:08X}")}))
                .collect::<Vec<_>>()
        );
        let attenuation_symbol = format!("ambience-attenuation-{index}");
        let mut bytes = attenuation.0.clone();
        put(&mut bytes, ATTENUATION_REACH, &sound.reach.to_le_bytes())?;
        graph.add(
            &attenuation_symbol,
            attenuation_tag,
            &bytes,
            None,
            Vec::new(),
        )?;
        let mut bytes = record.0[..SOUND_MEDIA].to_vec();
        bytes.resize(SOUND_MEDIA + media.len() * 4, 0xFF);
        let size = bytes.len() as u64;
        put(&mut bytes, 0, &size.to_le_bytes())?;
        put(&mut bytes, SOUND_PLAY, &1u32.to_le_bytes())?;
        put(&mut bytes, SOUND_STOP, &2u32.to_le_bytes())?;
        for at in [SOUND_ATTENUATION, SOUND_BANK] {
            put(&mut bytes, at, &u32::MAX.to_le_bytes())?;
        }
        for at in [0x18, 0x40] {
            put(&mut bytes, at, &(media.len() as u64).to_le_bytes())?;
        }
        let mut patches = vec![
            json!({"offset": SOUND_ATTENUATION, "symbol": attenuation_symbol}),
            json!({"offset": SOUND_BANK, "symbol": bank_symbol}),
        ];
        for (k, id) in media.iter().enumerate() {
            patches.push(json!({"offset": SOUND_MEDIA + k * 4, "symbol": format!("ambience-media-{id:08X}")}));
        }
        let symbol = format!("ambience-sound-{index}");
        graph.add(&symbol, record_tag, &bytes, None, patches)?;
        let node = graph.nodes.last_mut().unwrap();
        node["audio_events"] =
            json!([{"offset": SOUND_PLAY, "id": 1}, {"offset": SOUND_STOP, "id": 2}]);
        node["audio_namespace"] = json!(bank_symbol);
        records.push(symbol);
    }

    // The table: its rows, then each row's point behind its class word, 8-aligned.
    let mut bytes = vec![0u8; 0x30];
    let count = emitters.len() as u64;
    put(&mut bytes, 0x08, &count.to_le_bytes())?;
    put(&mut bytes, 0x10, &0x10u64.to_le_bytes())?;
    put(&mut bytes, 0x1C, &ARRAY_MARKER.to_le_bytes())?;
    put(&mut bytes, 0x20, &count.to_le_bytes())?;
    put(&mut bytes, 0x28, &TABLE_ROW_CLASS.to_le_bytes())?;
    let first_row = bytes.len();
    for (index, &(_, position)) in emitters.iter().enumerate() {
        let mut row = row_template.clone();
        for (k, value) in [0f32, 0., 0., 1.].into_iter().enumerate() {
            put(&mut row, ROW_ROTATION + k * 4, &value.to_le_bytes())?;
        }
        for (k, value) in position.into_iter().enumerate() {
            put(&mut row, ROW_TRANSLATION + k * 4, &value.to_le_bytes())?;
        }
        let identity = fnv1a64(&format!("parhelion/map/{map}/ambience/{index}")) | 1;
        put(&mut row, ROW_IDENTITY, &identity.to_le_bytes())?;
        bytes.extend_from_slice(&row);
    }
    let mut patches = Vec::with_capacity(emitters.len());
    for (index, &(sound, _)) in emitters.iter().enumerate() {
        bytes.resize(bytes.len() + 4, 0);
        bytes.extend_from_slice(&POINT_CLASS.to_le_bytes());
        let body = bytes.len();
        bytes.extend_from_slice(&point_template);
        put(&mut bytes, body + POINT_SOUND, &u32::MAX.to_le_bytes())?;
        patches.push(json!({"offset": body + POINT_SOUND, "symbol": records[sound]}));
        let field = first_row + index * ROW_SIZE + ROW_RESOURCE;
        put(&mut bytes, field, &((body - field) as u64).to_le_bytes())?;
    }
    let size = bytes.len() as u64;
    put(&mut bytes, 0, &size.to_le_bytes())?;
    graph.add("ambience-points", templates.points, &bytes, None, patches)?;
    Ok((
        Some("ambience-points".into()),
        json!({
            "points": emitters.len(),
            "sounds": sounds.iter().enumerate().map(|(index, sound)| json!({
                "loop": sound.source.path,
                "spatial": sound.curve.is_some(),
                "reach_m": sound.reach,
                "points": emitters.iter().filter(|(placed, _)| *placed == index).count(),
            })).collect::<Vec<_>>(),
            "samples": decoded.len(),
            "missing_samples": missing.iter().map(|id| format!("{id:08X}")).collect::<Vec<_>>(),
            "dropped_layers": dropped,
            "media_bytes": media_bytes,
            "output_bus": format!("{bus:08X}"),
        }),
    ))
}
