//! Continuous source engine layers on an explicit native occupancy gate and RPM input.
use super::*;
use crate::{
    presentation::append,
    tiger::audio::bank::{
        Bank,
        looping::{Curve, Layer, Phase},
    },
};

const RPM: u32 = 0x5daeb921;
const OCCUPIED: u32 = 0x157aa4e5;
const BUS: u32 = 2_945_946_222;

fn scalar(value: &Value, label: &str) -> Result<f32> {
    let value = value.as_f64().with_context(|| format!("Engine {label}"))? as f32;
    ensure!(value.is_finite(), "Engine {label} is not finite");
    Ok(value)
}

fn fade(value: &Value) -> Result<u32> {
    let seconds = scalar(&value["seconds"], "fade")?;
    ensure!(
        value["mode"] == 0 && (0.0..=60.0).contains(&seconds),
        "Unsupported engine fade"
    );
    Ok((seconds * 1000.).round() as u32)
}

fn pitch_span(ranges: &[Value]) -> Result<[f32; 2]> {
    let idle = ranges
        .iter()
        .find(|range| range["name"] == "idle")
        .context("Engine idle range")?;
    let high = ranges
        .iter()
        .find(|range| range["name"] == "hi")
        .context("Engine high range")?;
    let pitch = [
        scalar(&idle["parameters"]["natural_pitch_cents"], "idle pitch")?,
        scalar(&high["parameters"]["bend_bounds_cents"][1], "high pitch")?,
    ];
    ensure!(pitch[1] > pitch[0], "Engine pitch span is empty");
    Ok(pitch)
}

fn curves(range: &Value, pitch: [f32; 2]) -> Result<Vec<Curve>> {
    // An authored four-range blend. The source's inverted gain bounds are
    // preserved in the receipt and are not treated as a proven curve.
    let name = range["name"].as_str().context("Engine pitch range name")?;
    let index = match name {
        "idle" => 0,
        "low" => 1,
        "med" => 2,
        "hi" => 3,
        _ => anyhow::bail!("Unsupported engine pitch range {name}"),
    };
    let natural = scalar(
        &range["parameters"]["natural_pitch_cents"],
        "recording pitch",
    )?;
    Ok(vec![
        Curve {
            parameter: RPM,
            target: 0,
            points: (0..=12)
                .map(|point| {
                    let x = point as f32 / 12.;
                    let weight = (1. - (x * 3. - index as f32).abs()).max(0.);
                    let gain = if weight == 0. {
                        -96.
                    } else {
                        20. * (weight * std::f32::consts::FRAC_PI_2).sin().log10()
                    };
                    [x, gain]
                })
                .collect(),
        },
        Curve {
            parameter: RPM,
            target: 2,
            points: vec![[0., pitch[0] - natural], [1., pitch[1] - natural]],
        },
    ])
}

fn sound_layers(source: &Value, track: &Value, phase: Phase) -> Result<Vec<Layer>> {
    let gain = scalar(&track["gain_db"], "track gain")?
        + scalar(&source["playback"]["gain_db"], "sound gain")?;
    let ranges = source["pitch_ranges"]
        .as_array()
        .context("Engine pitch ranges")?;
    let sustain = matches!(phase, Phase::Sustain);
    ensure!(
        !ranges.is_empty() && (!sustain || ranges.len() == 4),
        "Unsupported engine pitch range layout"
    );
    let pitch = if sustain {
        pitch_span(ranges)?
    } else {
        [0.; 2]
    };
    let mut seen = BTreeSet::new();
    ranges
        .iter()
        .map(|range| {
            let curves = if sustain {
                ensure!(
                    seen.insert(range["name"].as_str()),
                    "Repeated engine pitch range"
                );
                curves(range, pitch)?
            } else {
                Vec::new()
            };
            let variants = range["variants"].as_array().context("Engine variations")?;
            ensure!(!variants.is_empty(), "Engine range has no recordings");
            let variation_gain = scalar(&variants[0]["gain_encoded"], "variation gain")?;
            ensure!(
                variants.iter().all(|variant| variant["gain_encoded"]
                    == variants[0]["gain_encoded"]
                    && variant["skip_fraction_encoded"] == 0),
                "Engine variations require individual gain or skip rules"
            );
            let media = variants
                .iter()
                .map(|variant| {
                    u32::try_from(
                        variant["fsb_info"]
                            .as_u64()
                            .context("Engine recording ID")?,
                    )
                    .map_err(Into::into)
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Layer {
                media,
                gain_db: gain + variation_gain,
                phase,
                fade_in_ms: if sustain { fade(&track["fade_in"])? } else { 0 },
                fade_out_ms: fade(&track["fade_out"])?,
                curves,
            })
        })
        .collect()
}

fn source_layers(behavior: &Value) -> Result<(Vec<Layer>, [f32; 2], Value)> {
    let nodes = behavior["nodes"]
        .as_object()
        .context("Engine source nodes")?;
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Engine source root")?;
    let attachments = behavior["edges"]
        .as_array()
        .context("Engine attachments")?
        .iter()
        .filter(|edge| {
            edge["owner"] == root && edge["role"] == "attachment" && edge["tag"]["group"] == "lsnd"
        })
        .map(|edge| {
            edge["tag"]["datum"]
                .as_u64()
                .context("Engine attachment identity")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let sound = |tag: &Value| -> Result<&Value> {
        let id = tag["datum"].as_u64().context("Engine track sound")?;
        let node = nodes
            .get(&format!("{id:08X}"))
            .context("Engine sound closure")?;
        ensure!(
            node["tag"]["group"] == "snd!",
            "Engine track is not a sound"
        );
        Ok(&node["data"])
    };
    let mut layers = Vec::new();
    let mut tracks = Vec::new();
    let mut distance: Option<[f32; 2]> = None;
    for id in attachments {
        let node = nodes
            .get(&format!("{id:08X}"))
            .context("Engine loop closure")?;
        if node["data"]["class"] != 23 {
            continue;
        }
        for track in node["data"]["track_controls"]
            .as_array()
            .context("Engine tracks")?
        {
            // The close engine tracks expose loaded and coasting recordings.
            // A separate distant LOD track is not another simultaneous close layer.
            if track["alternate_loop"].is_null() {
                continue;
            }
            ensure!(
                track["output_effect"] == 0
                    && track["layer"] == 0
                    && track["transition_in"].is_null()
                    && track["transition_out"].is_null(),
                "Engine track requires source transitions or effects"
            );
            let bounds = [
                scalar(&node["data"]["distance_bounds_source"][0], "near distance")?
                    * super::super::METRES,
                scalar(&node["data"]["distance_bounds_source"][1], "far distance")?
                    * super::super::METRES,
            ];
            ensure!(
                distance.is_none_or(|previous| previous == bounds),
                "Engine track distances differ"
            );
            distance = Some(bounds);
            for (key, phase) in [
                ("start", Phase::Start),
                ("loop", Phase::Sustain),
                ("end", Phase::Stop),
            ] {
                if track[key].is_null() {
                    continue;
                }
                layers.extend(sound_layers(sound(&track[key])?, track, phase)?);
            }
            tracks.push(json!({"loop":node["tag"],"track":track}));
        }
    }
    ensure!(!tracks.is_empty(), "No supported source engine tracks");
    Ok((layers, distance.context("Engine distance")?, json!(tracks)))
}

impl Assembly {
    pub fn engine_audio(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        behavior: &Value,
        motion: &motion::Motion,
    ) -> Result<Value> {
        let (layers, distance, source) = source_layers(behavior)?;
        let mut candidates = Vec::new();
        for (&owner, payload) in &self.payloads {
            let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
            if payload.u32(definition - 4)? != 0x808084e9 {
                continue;
            }
            let inputs = payload.array(definition + 0x188, 40, Some(0x80809789))?;
            let rpm = inputs
                .iter()
                .position(|&row| payload.u32(row + 32).ok() == Some(RPM));
            let occupied = inputs
                .iter()
                .position(|&row| payload.u32(row + 32).ok() == Some(OCCUPIED));
            let events = payload.array(definition + 0x168, 24, Some(0x808093e6))?;
            if let (Some(rpm), Some(occupied)) = (rpm, occupied)
                && events.len() >= 2
                && events.iter().all(|&row| {
                    payload
                        .pointer(row + 16)
                        .and_then(|at| payload.u32(at - 4))
                        .ok()
                        == Some(0x80806b38)
                })
            {
                candidates.push((owner, definition, inputs, events, rpm, occupied));
            }
        }
        ensure!(
            candidates.len() == 1,
            "Native engine flow is absent or ambiguous"
        );
        let (owner, definition, inputs, events, rpm, occupied) = candidates.pop().unwrap();
        let original = &self.payloads[&owner];
        let event = original.pointer(events[0] + 16)?;
        let cue_tag = original.u32(event + 0x40)?;
        let cue = native.tag(cue_tag, Some(0x80809802))?;
        ensure!(
            ![0, u32::MAX, 0x811c9dc5].contains(&cue.u32(12)?),
            "Engine cue has no stop event"
        );
        let bank_template = cue.u32(0x14)?;
        let native_bank = native.tag(bank_template, None)?;
        crate::halo_reach::audio::verify_bus(&native_bank.0, BUS)?;
        let media_template = cue.u32(
            *cue.array(0x18, 4, Some(0x80800014))?
                .first()
                .context("Engine media template")?,
        )?;
        let ids = layers
            .iter()
            .flat_map(|layer| layer.media.iter().copied())
            .collect::<BTreeSet<_>>();
        let source_root = graph
            .root
            .parent()
            .context("Engine source directory")?
            .join("source/audio");
        let mut media = Vec::new();
        for id in &ids {
            let symbol = format!("vehicle-engine-media-{id:08X}");
            let bytes = crate::tiger::audio::transcode::normalize_pcm_wem(&std::fs::read(
                source_root.join(format!("{id:08X}.wav")),
            )?)?;
            graph.add(&symbol, media_template, &bytes, None, Vec::new())?;
            graph.nodes.last_mut().unwrap()["storage"] = json!("audio_media");
            media.push(json!({"id":id,"symbol":symbol}));
        }
        let bank = Bank::looping(&layers, BUS, distance)?;
        graph.add(
            "vehicle-engine-bank",
            bank_template,
            &bank.bytes,
            None,
            Vec::new(),
        )?;
        let node = graph.nodes.last_mut().unwrap();
        node["storage"] = json!("audio_bank");
        node["audio_bank"] = serde_json::to_value(&bank)?;
        node["audio_media"] = json!(media);
        ensure!(
            cue.0.len() >= 0x50 && cue.pointer(0x20)? == 0x40,
            "Engine cue envelope differs"
        );
        let mut bytes = cue.0[..0x50].to_vec();
        bytes.resize(0x50 + ids.len() * 4, 0xff);
        let size = bytes.len() as u64;
        put(&mut bytes, 0, &size.to_le_bytes())?;
        put(&mut bytes, 8, &1u32.to_le_bytes())?;
        put(&mut bytes, 12, &2u32.to_le_bytes())?;
        put(&mut bytes, 20, &u32::MAX.to_le_bytes())?;
        for at in [0x18, 0x40] {
            put(&mut bytes, at, &(ids.len() as u64).to_le_bytes())?;
        }
        let mut patches = vec![json!({"offset":20,"symbol":"vehicle-engine-bank"})];
        for (index, id) in ids.iter().enumerate() {
            patches.push(
                json!({"offset":0x50+index*4,"symbol":format!("vehicle-engine-media-{id:08X}")}),
            );
        }
        graph.add("vehicle-engine-cue", cue_tag, &bytes, None, patches)?;
        graph.nodes.last_mut().unwrap()["audio_events"] =
            json!([{"offset":8,"id":1},{"offset":12,"id":2}]);
        self.resource(owner, event + 0x40, "vehicle-engine-cue".into())?;
        let object = links::Object {
            owner: motion::OWNER,
            class: 0x80809ae1,
            offset: (motion::DEFINITION + 0x450) as u64,
        };
        let metadata = graph.read("vehicle-car-output-1")?;
        let contract = links::Interface::read(&motion.payload, object, &metadata, false)?;
        ensure!(
            contract.methods.len() == 1 && contract.methods[0].index == 7,
            "Car RPM provider differs"
        );
        let channel = self
            .channel_bridge(native, graph, "vehicle-engine-input", &[(object, 0)])?
            .remove(0);
        let mut rewired = 0;
        for connection in self
            .links
            .connections
            .iter_mut()
            .chain(&mut self.links.named_connections)
        {
            if connection
                .consumer
                .object
                .is_some_and(|value| value.owner == owner && value.offset == inputs[rpm] as u64)
            {
                connection.provider = channel.provider.clone();
                connection.channel = channel.index;
                rewired += 1;
            }
        }
        ensure!(rewired == 1, "Engine RPM binding is absent or ambiguous");
        let payload = self.payloads.get_mut(&owner).unwrap();
        let controls = payload.array(definition + 0x158, 24, Some(0x808093e6))?;
        ensure!(controls.len() >= 2, "Engine occupancy controls are absent");
        let first = payload.pointer(controls[0] + 16)?;
        let gate = payload.pointer(controls[1] + 16)?;
        let root_children = payload.array(first + 0x38, 4, Some(0x808093fb))?;
        ensure!(
            payload.u32(gate - 4)? == 0x808093cd
                && payload.u32(gate + 0x60)? as usize == occupied
                && root_children.len() == 1
                && payload.bytes::<4>(root_children[0])? == [0, 0, 1, 0],
            "Engine flow does not have the checked health and occupancy gates"
        );
        append(&mut payload.0, gate + 0x38, 0x808093fb, &[1, 0, 0, 0], 4)?;
        append(
            &mut payload.0,
            event + 0x48,
            0x80800007,
            &(rpm as u32).to_le_bytes(),
            4,
        )?;
        let symbol = &self.symbols[&owner];
        let declared = super::patches(graph, symbol)?;
        // Only immutable definition arrays were appended. The existing mutable
        // event instances keep their original allocation and copied span.
        let length = payload.0.len() as u64;
        put(&mut payload.0, 0, &length.to_le_bytes())?;
        graph.replace(symbol, &payload.0, declared)?;
        Ok(
            json!({"source_tracks":source,"layers":layers.len(),"recordings":ids.len(),"flow_owner":format!("{owner:08X}"),
            "event":event,"rpm_input":rpm,"rpm_channel":channel.index,"engine_method":7,"output_bus":BUS,
            "blend":"Authored idle, low, medium and high crossfades driven by native engine RPM",
            "limits":["The source PCM, track gains and lifecycle fades are translated. Pitch-range selection is an authored blend, not verified Reach playback parity. Coasting alternate tracks and distant LOD audio are not used. Native occupancy transitions and cleanup need gameplay acceptance."]}),
        )
    }
}
