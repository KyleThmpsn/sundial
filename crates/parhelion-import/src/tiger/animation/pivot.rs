//! Move native FK bones to new pivots while keeping their orientations.
//!
//! A bone turns about its current position, which FK composes from its parent and its local
//! translation. Moving a pivot therefore changes the skeleton's object binds at definition
//! `+0x90` and their FK section pose copies, the inverse binds at `+0xA0`, the default local pose
//! of the bone's control at controls `+0xB0`, and the local translation every absolute clip stores
//! for that control. Descendants keep their local offsets and follow. Additive clips store deltas
//! and keep them.
use crate::{presentation::put, tiger::payload::Payload};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

type Vec3 = [f32; 3];

fn rotate(q: [f32; 4], v: Vec3) -> Vec3 {
    let [x, y, z, w] = q;
    let t = [
        2. * (y * v[2] - z * v[1]),
        2. * (z * v[0] - x * v[2]),
        2. * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

fn conjugate(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// A skeleton with moved pivots.
pub struct Moved {
    pub skeleton: Payload,
    /// Each moved bone's change of local translation, in its parent's frame.
    pub local: BTreeMap<usize, Vec3>,
    /// Every bone's new object bind: rotation, translation and scale.
    pub objects: Vec<[f32; 8]>,
}

fn transform(p: &Payload, at: usize) -> Result<[f32; 8]> {
    let mut out = [0.; 8];
    for (index, value) in out.iter_mut().enumerate() {
        *value = p.f32(at + index * 4)?;
    }
    Ok(out)
}

/// Move each bone in `targets` to its object-space position. Bones keep their rotations and unit
/// scale. Every other bone keeps its local offset from its parent.
pub fn move_bones(skeleton: &Payload, targets: &BTreeMap<usize, Vec3>) -> Result<Moved> {
    let base = skeleton.pointer(24)?;
    let instance = skeleton.pointer(16)?;
    ensure!(
        skeleton.u32(base - 4)? == 0x8080_8546 && skeleton.u32(instance - 4)? == 0x8080_8545,
        "Pivot moves require a native FK skeleton"
    );
    let hierarchy = skeleton.array(base + 0x80, 16, Some(0x8080_8A08))?;
    let objects = skeleton.array(base + 0x90, 32, Some(0x8080_9F75))?;
    let inverses = skeleton.array(base + 0xA0, 32, Some(0x8080_9F75))?;
    let sections = skeleton.array(base + 0xB0, 2, Some(0x8080_0006))?;
    let indices = skeleton.array(base + 0xC0, 2, Some(0x8080_0006))?;
    let fk = skeleton.array(instance + 0x30, 64, Some(0x8080_8544))?;
    let count = hierarchy.len();
    ensure!(
        count > 0
            && [objects.len(), inverses.len(), sections.len(), indices.len()]
                .iter()
                .all(|&n| n == count)
            && targets.keys().all(|&bone| bone < count),
        "Pivot skeleton arrays differ"
    );
    let old = objects
        .iter()
        .map(|&at| transform(skeleton, at))
        .collect::<Result<Vec<_>>>()?;
    let mut new = old.clone();
    let mut local = BTreeMap::new();
    for bone in 0..count {
        let parent = skeleton.u32(hierarchy[bone] + 4)? as i32;
        ensure!(
            parent < bone as i32,
            "Skeleton is not in parent-first order"
        );
        let moved = targets.get(&bone).copied();
        let follows = usize::try_from(parent)
            .ok()
            .is_some_and(|p| new[p] != old[p]);
        if moved.is_none() && !follows {
            continue;
        }
        ensure!(
            (old[bone][7] - 1.).abs() < 1e-4
                && usize::try_from(parent)
                    .ok()
                    .is_none_or(|p| (old[p][7] - 1.).abs() < 1e-4),
            "Pivot moves need unit bone scale"
        );
        let position = if parent < 0 {
            moved.unwrap_or([old[bone][4], old[bone][5], old[bone][6]])
        } else {
            let p = usize::try_from(parent)?;
            let rotation: [f32; 4] = old[p][..4].try_into()?;
            let offset = rotate(
                conjugate(rotation),
                sub(old[bone][4..7].try_into()?, old[p][4..7].try_into()?),
            );
            match moved {
                Some(target) => {
                    let wanted = rotate(conjugate(rotation), sub(target, new[p][4..7].try_into()?));
                    local.insert(bone, sub(wanted, offset));
                    target
                }
                None => add(new[p][4..7].try_into()?, rotate(rotation, offset)),
            }
        };
        if parent < 0 && moved.is_some() {
            local.insert(bone, sub(position, old[bone][4..7].try_into()?));
        }
        new[bone][4..7].copy_from_slice(&position);
    }
    let mut out = skeleton.clone();
    for bone in 0..count {
        if new[bone] == old[bone] {
            continue;
        }
        let rotation: [f32; 4] = old[bone][..4].try_into()?;
        let inverse = transform(skeleton, inverses[bone])?;
        let bound: Vec3 = old[bone][4..7].try_into()?;
        let expected = rotate(conjugate(rotation), bound.map(|v| -v));
        ensure!(
            (0..4).all(|i| (inverse[i] - conjugate(rotation)[i]).abs() < 1e-3
                || (inverse[i] + conjugate(rotation)[i]).abs() < 1e-3)
                && (0..3).all(|i| (inverse[4 + i] - expected[i]).abs() < 1e-3),
            "Bone {bone} inverse bind differs from its object bind"
        );
        let position: Vec3 = new[bone][4..7].try_into()?;
        let translation = rotate(conjugate(rotation), position.map(|v| -v));
        let section = skeleton.i16(sections[bone])?;
        // A procedural bone has no FK pose copy. Its controller poses it from the binds.
        let copy = if section >= 0 {
            let section = fk
                .get(usize::try_from(section)?)
                .context("Bone section is outside its skeleton")?;
            let poses = skeleton.array(section + 0x20, 32, Some(0x8080_9F75))?;
            let pose = *poses
                .get(usize::try_from(skeleton.i16(indices[bone])?)?)
                .context("Bone pose is outside its section")?;
            ensure!(
                transform(skeleton, pose)? == old[bone],
                "Bone {bone} FK pose differs from its object bind"
            );
            Some(pose)
        } else {
            None
        };
        for at in [Some(objects[bone]), copy].into_iter().flatten() {
            for (axis, value) in position.iter().enumerate() {
                put(&mut out.0, at + 16 + axis * 4, &value.to_le_bytes())?;
            }
        }
        for (axis, value) in translation.iter().enumerate() {
            put(
                &mut out.0,
                inverses[bone] + 16 + axis * 4,
                &value.to_le_bytes(),
            )?;
        }
    }
    Ok(Moved {
        skeleton: out,
        local,
        objects: new,
    })
}

/// Shift the default local pose of each control whose bone moved by its local change. Returns the
/// controls and each shifted slot's change. A moved bone must have its own direct control.
pub fn shift_controls(
    controls: &Payload,
    local: &BTreeMap<usize, Vec3>,
    parents: &[i32],
) -> Result<(Payload, BTreeMap<usize, Vec3>)> {
    let definition = controls.pointer(24)?;
    ensure!(
        controls.u32(definition - 4)? == 0x8080_8F8F,
        "Pivot moves require native rig controls"
    );
    let rows = controls.array(definition + 0xA0, 52, Some(0x8080_9076))?;
    let defaults = controls.array(definition + 0xB0, 32, None)?;
    ensure!(
        rows.len() == defaults.len(),
        "Control rows and default poses differ"
    );
    let mut out = controls.clone();
    let mut slots = BTreeMap::new();
    for (slot, &row) in rows.iter().enumerate() {
        let bone = controls.i16(row + 0x2C)?;
        let Some(delta) = usize::try_from(bone).ok().and_then(|bone| local.get(&bone)) else {
            continue;
        };
        let bone = usize::try_from(bone)?;
        ensure!(
            controls.u8(row + 8)? == 0
                && controls.u8(row + 0x30)? == 0
                && i32::from(controls.i16(row + 4)?)
                    == *parents.get(bone).context("Control bone")?,
            "Control of moved bone {bone} is not composed on its skeleton parent"
        );
        for (axis, value) in delta.iter().enumerate() {
            let at = defaults[slot] + 16 + axis * 4;
            put(&mut out.0, at, &(controls.f32(at)? + value).to_le_bytes())?;
        }
        ensure!(
            slots.insert(slot, *delta).is_none(),
            "A moved bone has two controls"
        );
    }
    Ok((out, slots))
}
