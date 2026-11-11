//! A source gun's aiming overlay translated onto a native aim chain.
//!
//! A Reach turret aims through its `aiming` overlay, whose frames turn the gun's yaw bone about the
//! vertical and its pitch bones about the lateral axis, each frame a delta on the bind pose. Native
//! turrets aim through two additive clips that the pose definition's aim table names, sampled at the
//! relative aim angles (`tiger::animation::aim`). The translation keeps the native carrier's aim
//! bones, clip templates and turn axes, so the native aim convention holds, and takes from the source
//! the pivots the gun turns about, the yaw coverage and the pitch limits.
use super::{codec::Motion, playback};
use crate::{
    halo_reach::{Scene, model::Model, rig},
    tiger::{
        animation::{aim, clip, pivot},
        payload::Payload,
    },
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

/// How a gun bone moves with the aim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::halo_reach) enum Turn {
    Yaw,
    Pitch,
}

/// A source gun's aim, in the vehicle's object space and native metres.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::halo_reach) struct Gun {
    pub model: usize,
    pub clip: String,
    /// Per bone of the gun model, how it moves with the aim. A bone that does not move stays where
    /// it is drawn.
    pub turns: Vec<Option<Turn>>,
    pub yaw_bone: String,
    pub pitch_bone: String,
    pub yaw_pivot: [f32; 3],
    pub pitch_pivot: [f32; 3],
    /// Unit axes of the source turns, signed toward object +Z and +Y.
    pub yaw_axis: [f32; 3],
    pub pitch_axis: [f32; 3],
    /// The yaw the overlay covers in radians, or `None` for every direction.
    pub yaw: Option<[f32; 2]>,
    /// The pitch the overlay covers in radians, positive turning the muzzle down about object +Y,
    /// as native aim clips count it.
    pub pitch: [f32; 2],
    /// The gun's `primary_trigger` marker, where the source fires from.
    pub muzzle: Option<[f32; 3]>,
}

/// Each model's placement in the vehicle's object space, as the drawn geometry is placed.
pub(in crate::halo_reach) fn placements(scene: &Scene) -> Result<Vec<[f32; 16]>> {
    let mut placement: Vec<[f32; 16]> = Vec::with_capacity(scene.models.len());
    for placed in &scene.models {
        let transform = if let Some(parent) = &placed.parent {
            let mut transform = *placement
                .get(parent.model)
                .context("Attachment parent follows its child")?;
            if let Some(bone) = parent.bone {
                transform = rig::multiply(
                    &transform,
                    rig::world(&scene.models[parent.model].model.bones)?
                        .get(bone)
                        .context("Attachment parent bone")?,
                );
            }
            rig::multiply(&transform, &parent.transform)
        } else {
            rig::IDENTITY
        };
        placement.push(transform);
    }
    Ok(placement)
}

fn axis_angle(q: [f32; 4]) -> ([f32; 3], f32) {
    let q = if q[3] < 0. { q.map(|v| -v) } else { q };
    let angle = 2. * q[3].clamp(-1., 1.).acos();
    let s = (1. - q[3] * q[3]).max(0.).sqrt();
    if s < 1e-6 {
        ([0.; 3], 0.)
    } else {
        ([q[0] / s, q[1] / s, q[2] / s], angle)
    }
}

fn wrap(angle: f32) -> f32 {
    let turn = std::f32::consts::TAU;
    let mut value = angle % turn;
    if value > std::f32::consts::PI {
        value -= turn;
    } else if value <= -std::f32::consts::PI {
        value += turn;
    }
    value
}

/// The overlay clip an `aiming` route names, preferring the vehicle mode.
fn overlay(graph: &Value) -> Option<String> {
    let routes = graph["routes"].as_array()?;
    let mut found = routes
        .iter()
        .filter(|r| r["state"] == "aiming" && r["kind"] == "overlay")
        .filter_map(|r| r["clip"].as_str())
        .collect::<Vec<_>>();
    found.sort_by_key(|name| !name.starts_with("vehicle:"));
    found.first().map(|name| (*name).to_owned())
}

/// Bones whose subtree carries skinned geometry.
fn carriers(model: &Model) -> BTreeSet<usize> {
    let mut used = BTreeSet::new();
    for primitive in &model.primitives {
        for vertex in &primitive.vertices {
            for (joint, weight) in vertex.joints.iter().zip(vertex.weights) {
                if weight > 0. {
                    used.insert(usize::from(*joint));
                }
            }
        }
    }
    let mut carriers = BTreeSet::new();
    for bone in used {
        let mut at = Some(bone);
        while let Some(index) = at.filter(|&i| i < model.bones.len()) {
            if !carriers.insert(index) {
                break;
            }
            at = model.bones[index].parent;
        }
    }
    carriers
}

fn depth(model: &Model, bone: usize) -> usize {
    let mut depth = 0;
    let mut at = model.bones[bone].parent;
    while let Some(index) = at {
        depth += 1;
        at = model.bones[index].parent;
    }
    depth
}

fn under(model: &Model, bone: usize, ancestor: usize) -> bool {
    let mut at = Some(bone);
    while let Some(index) = at {
        if index == ancestor {
            return true;
        }
        at = model.bones[index].parent;
    }
    false
}

/// Read gun model `index`'s aiming overlay from the decoded source graphs under `source`.
/// `nodes` are the source turret block's physical yaw and pitch node names. A named node is the
/// pivot when the overlay turns it and geometry hangs below it, since a gun can name an aim
/// reference bone at the turret base. Otherwise the turning bone nearest the root with geometry
/// below it is the pivot.
pub(in crate::halo_reach) fn gun(
    scene: &Scene,
    source: &Path,
    index: usize,
    nodes: [Option<&str>; 2],
) -> Result<Option<Gun>> {
    let placed = scene.models.get(index).context("Source gun model")?;
    let Some(tag) = &placed.animation else {
        return Ok(None);
    };
    let placement = placements(scene)?[index];
    gun_model(&placed.model, tag, placement, source, index, nodes)
}

/// [`gun`] for a model placed by `placement` in the frame the pivots are wanted in, such as a
/// mounted turret entity's own frame. `index` only names the model in the result.
pub(in crate::halo_reach) fn gun_model(
    model: &Model,
    tag: &crate::halo_reach::cache::Tag,
    placement: [f32; 16],
    source: &Path,
    index: usize,
    nodes: [Option<&str>; 2],
) -> Result<Option<Gun>> {
    let graphs: Vec<Value> = serde_json::from_slice(&fs::read(source.join("animations.json"))?)?;
    let Some(graph) = graphs.iter().find(|g| g["tag"]["datum"] == tag.datum) else {
        return Ok(None);
    };
    let Some(name) = overlay(graph) else {
        return Ok(None);
    };
    let clips = graph["clips"]
        .as_array()
        .context("Source animation clips")?
        .iter()
        .filter(|c| c["name"] == name)
        .collect::<Vec<_>>();
    let [clip] = clips[..] else {
        anyhow::bail!("Source aiming overlay {name} is missing or ambiguous");
    };
    let shared = clip["shared"]
        .get(0)
        .context("Source aiming overlay has no variant")?;
    ensure!(
        shared["animation_type"] == 2,
        "Source aiming clip {name} is not an overlay"
    );
    let member = playback::member(graph, shared)?;
    let file = member["motion"]["file"]
        .as_str()
        .context("Source aiming overlay has no decoded motion")?;
    let motion: Motion = serde_json::from_slice(&fs::read(source.join(file))?)?;
    let map = playback::mapping(graph, model)?;
    ensure!(
        motion.tracks.len() == map.len() && motion.frames > 1,
        "Source aiming overlay dimensions disagree"
    );
    let world = rig::world(&model.bones)?;
    let object = |bone: usize| rig::multiply(&placement, &world[bone]);
    // Each bone's turns: per moving frame, the object-space axis and the angle.
    let mut moves: BTreeMap<usize, Vec<([f32; 3], f32)>> = BTreeMap::new();
    for (track, &bone) in motion.tracks.iter().zip(&map) {
        for frame in 0..motion.frames {
            let Some(&q) = track
                .rotation
                .get(if track.rotation.len() == 1 { 0 } else { frame })
            else {
                continue;
            };
            let (axis, angle) = axis_angle(rig::quaternion(q)?);
            if angle.to_degrees() < 0.5 {
                continue;
            }
            let axis = rig::normalize(rig::point(&object(bone), axis, false))?;
            moves.entry(bone).or_default().push((axis, angle));
        }
    }
    let kind = |axis: [f32; 3]| {
        if axis[2].abs() > 0.9 {
            Some(Turn::Yaw)
        } else if axis[1].abs() > 0.9 {
            Some(Turn::Pitch)
        } else {
            None
        }
    };
    // A bone the overlay turns about both axes in separate frames, as a tripod gun's single gun
    // bone, is both the yaw and the pitch pivot. Its yaw frames and pitch frames each turn about one
    // axis, so the two native aim bones split them exactly.
    let mut turning: BTreeMap<usize, BTreeSet<Turn>> = BTreeMap::new();
    for (&bone, frames) in &moves {
        let kinds = frames
            .iter()
            .filter_map(|(axis, _)| kind(*axis))
            .collect::<BTreeSet<_>>();
        if !kinds.is_empty() {
            turning.insert(bone, kinds);
        }
    }
    let carriers = carriers(model);
    let pick = |turn: Turn, named: Option<&str>, geometry: bool| -> Option<usize> {
        let candidates = turning
            .iter()
            .filter(|(bone, t)| t.contains(&turn) && (!geometry || carriers.contains(*bone)))
            .map(|(&bone, _)| bone)
            .collect::<Vec<_>>();
        named
            .and_then(|name| model.bones.iter().position(|b| b.name == name))
            .filter(|bone| candidates.contains(bone))
            .or_else(|| {
                candidates
                    .iter()
                    .copied()
                    .min_by_key(|&b| (depth(model, b), b))
            })
    };
    // An overlay that turns only aim reference bones moves no drawn gun.
    let Some(yaw) = pick(Turn::Yaw, nodes[0], true) else {
        return Ok(None);
    };
    let Some(pitch) = pick(Turn::Pitch, nodes[1], true) else {
        return Ok(None);
    };
    let signed = |bone: usize, lane: usize| -> Vec<f32> {
        moves[&bone]
            .iter()
            .filter(|(axis, _)| kind(*axis) == Some([Turn::Yaw, Turn::Pitch][lane - 1]))
            .map(|(axis, angle)| wrap(angle * axis[[0, 2, 1][lane]].signum()))
            .chain([0.])
            .collect()
    };
    let mut yaws = signed(yaw, 1);
    yaws.sort_by(f32::total_cmp);
    // The overlay covers every direction when no two neighbouring yaw frames, around the turn, are
    // more than a quarter turn apart.
    let widest = yaws
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .chain([yaws[0] + std::f32::consts::TAU - yaws[yaws.len() - 1]])
        .fold(0f32, f32::max);
    let yaw_range =
        (widest > std::f32::consts::FRAC_PI_2 + 0.01).then(|| [yaws[0], yaws[yaws.len() - 1]]);
    let pitches = signed(pitch, 2);
    let pitch_range = [
        pitches.iter().copied().fold(f32::INFINITY, f32::min),
        pitches.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    ];
    ensure!(
        pitch_range[0] < pitch_range[1],
        "Source aiming overlay does not pitch"
    );
    // `lane` is 2 for the yaw axis (Z) and 1 for the pitch axis (Y). Only that turn's frames count.
    let axis = |bone: usize, lane: usize| -> [f32; 3] {
        let wanted = if lane == 2 { Turn::Yaw } else { Turn::Pitch };
        let (axis, _) = moves[&bone]
            .iter()
            .copied()
            .filter(|(axis, _)| kind(*axis) == Some(wanted))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        let sign = axis[lane].signum();
        axis.map(|v| v * sign)
    };
    let turns = (0..model.bones.len())
        .map(|bone| {
            if turning
                .iter()
                .any(|(&b, t)| t.contains(&Turn::Pitch) && under(model, bone, b))
            {
                Some(Turn::Pitch)
            } else if turning
                .iter()
                .any(|(&b, t)| t.contains(&Turn::Yaw) && under(model, bone, b))
            {
                Some(Turn::Yaw)
            } else {
                None
            }
        })
        .collect();
    let mut triggers = Vec::new();
    for marker in model.markers.iter().filter(|m| m.name == "primary_trigger") {
        if model.marker_selected(marker)? {
            triggers.push(marker);
        }
    }
    let muzzle = match triggers[..] {
        [marker] => {
            let point = match marker.bone {
                Some(bone) => rig::point(
                    world.get(bone).context("Source muzzle bone")?,
                    marker.translation,
                    true,
                ),
                None => marker.translation,
            };
            Some(rig::point(&placement, point, true))
        }
        _ => None,
    };
    let pivot = |bone: usize| rig::point(&object(bone), [0.; 3], true);
    Ok(Some(Gun {
        model: index,
        clip: name,
        turns,
        yaw_bone: model.bones[yaw].name.clone(),
        pitch_bone: model.bones[pitch].name.clone(),
        yaw_pivot: pivot(yaw),
        pitch_pivot: pivot(pitch),
        yaw_axis: axis(yaw, 2),
        pitch_axis: axis(pitch, 1),
        yaw: yaw_range,
        pitch: pitch_range,
        muzzle,
    }))
}

/// A native carrier's own aim, read from stock: the aim table, the bank ordinals of its two clips,
/// the controls each clip turns about which local axes, and the bones the gun is drawn on.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::halo_reach) struct Carrier {
    pub table: u32,
    pub row: (u8, u16, u16),
    pub stock_pitch: [f32; 2],
    pub yaw_turns: Vec<(usize, [f32; 3])>,
    pub pitch_turns: Vec<(usize, [f32; 3])>,
    pub yaw_bone: usize,
    pub pitch_bone: usize,
}

fn owner(
    native: &mut crate::tiger::reader::Reader,
    entity: &Payload,
    class: u32,
) -> Result<Option<(u32, Payload)>> {
    let mut found = Vec::new();
    for row in entity.array(16, 12, Some(0x8080_9C04))? {
        let tag = entity.u32(row)?;
        let value = native.tag(tag, Some(0x8080_9C36))?;
        if value.u32(value.pointer(24)? - 4)? == class {
            found.push((tag, (*value).clone()));
        }
    }
    ensure!(
        found.len() <= 1,
        "Native aim owner {class:08X} is ambiguous"
    );
    Ok(found.pop())
}

/// The bone a set of controls turns that carries the most of the skeleton below it.
fn widest(turns: &[(usize, [f32; 3])], controls: &Payload, parents: &[i32]) -> Result<usize> {
    let rows = controls.array(controls.pointer(24)? + 0xA0, 52, Some(0x8080_9076))?;
    let mut best = None;
    for &(slot, _) in turns {
        let bone =
            usize::try_from(controls.i16(*rows.get(slot).context("Aim control slot")? + 0x2C)?)?;
        let below = (0..parents.len())
            .filter(|&b| {
                let mut at = b as i32;
                while at >= 0 {
                    if at as usize == bone {
                        return true;
                    }
                    at = parents[at as usize];
                }
                false
            })
            .count();
        if best.is_none_or(|(_, count)| below > count) {
            best = Some((bone, below));
        }
    }
    Ok(best.context("Aim clip turns no control")?.0)
}

pub(in crate::halo_reach) fn parents(skeleton: &Payload) -> Result<Vec<i32>> {
    skeleton
        .array(skeleton.pointer(24)? + 0x80, 16, Some(0x8080_8A08))?
        .into_iter()
        .map(|row| Ok(skeleton.u32(row + 4)? as i32))
        .collect()
}

/// The stock aim of native entity `entity`, or `None` when its pose definition names no aim table.
pub(in crate::halo_reach) fn carrier(
    native: &mut crate::tiger::reader::Reader,
    entity: u32,
) -> Result<Option<Carrier>> {
    let entity = (*native.tag(entity, Some(0x8080_9C0F))?).clone();
    let Some((_, pose)) = owner(native, &entity, 0x8080_3640)? else {
        return Ok(None);
    };
    let (_, Some(table)) = aim::table_tag(&pose)? else {
        return Ok(None);
    };
    let row = aim::row(&*native.tag(table, Some(0x8080_348E))?)?;
    ensure!(row.mode == 2, "Native aim row does not play separate clips");
    let (yaw_turns, pitch_turns) = aim_turns(native, &entity, &row)?;
    let (_, controls) = owner(native, &entity, 0x8080_8F8F)?.context("Native aim controls")?;
    let (_, skeleton) = owner(native, &entity, 0x8080_8546)?.context("Native aim skeleton")?;
    let parents = parents(&skeleton)?;
    Ok(Some(Carrier {
        table,
        row: (row.mode, row.yaw_clip, row.pitch_clip),
        stock_pitch: row.pitch,
        yaw_bone: widest(&yaw_turns, &controls, &parents)?,
        pitch_bone: widest(&pitch_turns, &controls, &parents)?,
        yaw_turns,
        pitch_turns,
    }))
}

/// Where a translated gun turns and how far, in the native entity's object space and radians.
pub(in crate::halo_reach) struct Target {
    pub yaw_pivot: [f32; 3],
    pub pitch_pivot: [f32; 3],
    /// `None` aims in every direction through a closed chain.
    pub yaw: Option<[f32; 2]>,
    /// Native pitch bounds, positive down.
    pub pitch: [f32; 2],
}

/// The payloads a native aim chain lives in. Clips are in bank order.
pub(in crate::halo_reach) struct Rig<'a> {
    pub skeleton: &'a Payload,
    pub controls: &'a Payload,
    pub bank: &'a Payload,
    pub clips: &'a [Payload],
    pub table: &'a Payload,
}

/// The translated rig. `clips` holds every changed clip by bank ordinal, `sizes` every clip's
/// storage size in bank order for the supplemental layer table, and `objects` every bone's new
/// object bind.
pub(in crate::halo_reach) struct Aimed {
    pub skeleton: Payload,
    pub controls: Payload,
    pub clips: BTreeMap<usize, Payload>,
    pub sizes: Vec<u32>,
    pub bank: Option<Payload>,
    pub table: Payload,
    pub objects: Vec<[f32; 8]>,
    pub report: Value,
}

/// Translate a gun onto a native aim chain. The carrier's yaw and pitch bones move to the target's
/// pivots, every absolute clip follows the moved pivots, the aim clips are rewritten at the target's
/// coverage without the carrier's own translations, and a private aim table aims every descriptor
/// within the target's pitch bounds. The caller writes the payloads where `rig` came from and
/// points the pose definition's `+0x114` at the table.
pub(in crate::halo_reach) fn apply(rig: &Rig, carrier: &Carrier, target: &Target) -> Result<Aimed> {
    let bones = crate::tiger::animation::rig::bone_count(rig.skeleton)?;
    let parents = parents(rig.skeleton)?;
    let moved = pivot::move_bones(
        rig.skeleton,
        &BTreeMap::from([
            (carrier.yaw_bone, target.yaw_pivot),
            (carrier.pitch_bone, target.pitch_pivot),
        ]),
    )?;
    let (controls, slots) = pivot::shift_controls(rig.controls, &moved.local, &parents)?;
    let mut clips = BTreeMap::new();
    for (ordinal, payload) in rig.clips.iter().enumerate() {
        if let Some(shifted) = clip::shift_positions(payload, &slots)? {
            clips.insert(ordinal, shifted);
        }
    }
    let step = 5f32.to_radians();
    for (ordinal, turns, lane, vertices, closed) in [
        (
            carrier.row.1,
            &carrier.yaw_turns,
            0,
            match target.yaw {
                None => aim::sweep(
                    0,
                    [-std::f32::consts::PI, std::f32::consts::PI],
                    20f32.to_radians(),
                    true,
                )?,
                Some(bounds) => aim::sweep(0, bounds, step, false)?,
            },
            target.yaw.is_none(),
        ),
        (
            carrier.row.2,
            &carrier.pitch_turns,
            1,
            aim::sweep(1, target.pitch, step, false)?,
            false,
        ),
    ] {
        let ordinal = usize::from(ordinal);
        let template = clips
            .get(&ordinal)
            .or_else(|| rig.clips.get(ordinal))
            .context("Native aim clip ordinal")?
            .clone();
        clips.insert(
            ordinal,
            aim::clip(&template, &controls, bones, turns, lane, &vertices, closed)?,
        );
    }
    let sizes = rig
        .clips
        .iter()
        .enumerate()
        .map(|(ordinal, payload)| clip::size(clips.get(&ordinal).unwrap_or(payload)))
        .collect::<Result<Vec<_>>>()?;
    let table = aim::aliased(rig.table, aim::descriptors(rig.bank)?, target.pitch)?;
    let bank = aim::with_names(rig.bank, &[aim::AIM_YAW, aim::AIM_PITCH])?;
    let report = json!({
        "native_yaw_bone":carrier.yaw_bone,"native_pitch_bone":carrier.pitch_bone,
        "moved_pivots":{"yaw":target.yaw_pivot,"pitch":target.pitch_pivot},
        "local_pivot_changes":moved.local,"shifted_absolute_clips":clips.len().saturating_sub(2),
        "yaw_clip":carrier.row.1,"pitch_clip":carrier.row.2,
        "yaw_coverage":target.yaw.map_or(json!("full turn, closed chain of 19 vertices"),|b| json!(b)),
        "pitch_bounds_radians":target.pitch,"stock_pitch_bounds_radians":carrier.stock_pitch,
        "aimed_descriptors":aim::descriptors(rig.bank)?,"bank_names_added":bank.is_some(),
    });
    Ok(Aimed {
        skeleton: moved.skeleton,
        controls,
        clips,
        sizes,
        bank,
        table,
        objects: moved.objects,
        report,
    })
}

type AimTurns = Vec<(usize, [f32; 3])>;

fn aim_turns(
    native: &mut crate::tiger::reader::Reader,
    entity: &Payload,
    row: &aim::Row,
) -> Result<(AimTurns, AimTurns)> {
    let (_, lookup) = owner(native, entity, 0x8080_344B)?.context("Native aim lookup")?;
    let bank = native.tag(lookup.u32(lookup.pointer(24)? + 0x90)?, Some(0x8080_36F6))?;
    let clips = bank.array(8, 4, Some(0x8080_8F48))?;
    let clip = |ordinal: u16| -> Result<u32> {
        bank.u32(
            *clips
                .get(usize::from(ordinal))
                .context("Aim clip ordinal")?,
        )
    };
    let (yaw_tag, pitch_tag) = (clip(row.yaw_clip)?, clip(row.pitch_clip)?);
    let yaw_turns = aim::axes(&*native.tag(yaw_tag, Some(0x8080_8F49))?, 0).with_context(|| {
        format!(
            "Stock yaw aim clip {yaw_tag:08X} at ordinal {}",
            row.yaw_clip
        )
    })?;
    let pitch_turns =
        aim::axes(&*native.tag(pitch_tag, Some(0x8080_8F49))?, 1).with_context(|| {
            format!(
                "Stock pitch aim clip {pitch_tag:08X} at ordinal {}",
                row.pitch_clip
            )
        })?;
    Ok((yaw_turns, pitch_turns))
}
