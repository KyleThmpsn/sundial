//! Append local controls without renumbering a carrier's physics and attachment bones.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};

pub struct Bone {
    pub name: u32,
    /// Index in the complete, extended native skeleton.
    pub parent: usize,
    pub local: [f32; 8],
    pub object: [f32; 8],
    pub inverse: [f32; 8],
}

pub struct Extension {
    pub skeleton: Payload,
    pub controls: Payload,
    pub original_bones: usize,
    pub relocation: (usize, usize),
}

fn rows(p: &Payload, at: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.array(at, stride, Some(class))?
        .into_iter()
        .flat_map(|at| p.0[at..at + stride].iter().copied())
        .collect())
}

fn floats(values: &[f32; 8]) -> Result<Vec<u8>> {
    let norm = values[..4].iter().map(|v| v * v).sum::<f32>();
    ensure!(
        values.iter().all(|v| v.is_finite()) && (norm - 1.).abs() < 0.001 && values[7] > 0.,
        "Invalid appended rig transform"
    );
    Ok(values.iter().flat_map(|v| v.to_le_bytes()).collect())
}

/// Move typed interior pointers along with an expanded owner. The owner tag is
/// left intact for the normal private-package linker.
pub fn rebase_entity(
    entity: &mut [u8],
    original: &Payload,
    owner: u32,
    cut: usize,
    delta: usize,
) -> Result<()> {
    let snapshot = Payload(entity.to_vec());
    let primary = snapshot.array(16, 12, Some(0x80809C04))?;
    for at in crate::tiger::entity::owner_slots(&snapshot, original, owner)? {
        if !primary.contains(&at) {
            let target = usize::try_from(snapshot.u64(at + 8)?)?;
            if target >= cut {
                put(
                    entity,
                    at + 8,
                    &u64::try_from(
                        target
                            .checked_add(delta)
                            .context("Rig pointer relocation overflow")?,
                    )?
                    .to_le_bytes(),
                )?;
            }
        }
    }
    Ok(())
}

fn skeleton(original: &Payload, tag: u32, bones: &[Bone]) -> Result<(Payload, usize, usize)> {
    let base = original.pointer(24)?;
    let instance = original.pointer(16)?;
    ensure!(
        original.u32(base - 4)? == 0x80808546 && original.u32(instance - 4)? == 0x80808545,
        "Animation extension requires a native FK skeleton"
    );
    let sections = original.array(instance + 0x30, 64, Some(0x80808544))?;
    ensure!(
        sections.len() == 1,
        "Animation extension requires one FK section"
    );
    let section = sections[0];
    let poses = original.array(section + 0x20, 32, Some(0x80809F75))?;
    let count = poses.len();
    let added = bones.len();
    ensure!(
        count > 0 && added > 0 && count + added <= 256,
        "Extended rig exceeds native geometry palette limits"
    );
    let cut = poses.last().context("Empty FK poses")? + 32;
    ensure!(
        cut + 8 == base && instance + usize::try_from(original.u64(0x48)?)? == base,
        "FK instance has trailing state that needs a separate relocation rule"
    );
    // Section ranges describe non-FK procedural slots on other rigs.
    ensure!(
        original.u64(instance + 0x40)? == 0,
        "Procedural FK ranges require a separate animation adapter"
    );
    let mut hierarchy = rows(original, base + 0x80, 16, 0x80808A08)?;
    let mut object = rows(original, base + 0x90, 32, 0x80809F75)?;
    let mut inverse = rows(original, base + 0xA0, 32, 0x80809F75)?;
    let mut section_map = rows(original, base + 0xB0, 2, 0x80800006)?;
    let mut bone_map = rows(original, base + 0xC0, 2, 0x80800006)?;
    ensure!(
        hierarchy.len() == count * 16
            && object.len() == count * 32
            && inverse.len() == count * 32
            && section_map == vec![0; count * 2]
            && bone_map
                == (0..count as u16)
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
        "FK section does not map directly to its complete skeleton"
    );
    ensure!(
        poses
            .iter()
            .enumerate()
            .all(|(i, &at)| original.0[at..at + 32] == object[i * 32..i * 32 + 32]),
        "FK instance differs from its object bind pose"
    );
    let schemas = original.array(base + 0xD8, 24, Some(0x80808549))?;
    ensure!(
        schemas.len() == 1 && original.u32(schemas[0] + 16)? as usize == count,
        "FK section schema differs"
    );
    let mut names = std::collections::BTreeSet::new();
    for row in hierarchy.chunks_exact(16) {
        names.insert(u32::from_le_bytes(row[..4].try_into()?));
    }
    let mut extra = Vec::new();
    for (offset, bone) in bones.iter().enumerate() {
        let index = count + offset;
        ensure!(
            bone.parent < index && names.insert(bone.name),
            "Appended FK bone has an invalid parent or repeated name"
        );
        let previous = hierarchy[bone.parent * 16 + 8..bone.parent * 16 + 12].to_vec();
        put(
            &mut hierarchy,
            bone.parent * 16 + 8,
            &(index as i32).to_le_bytes(),
        )?;
        hierarchy.extend(bone.name.to_le_bytes());
        hierarchy.extend((bone.parent as i32).to_le_bytes());
        hierarchy.extend((-1i32).to_le_bytes());
        hierarchy.extend(previous);
        let transform = floats(&bone.object)?;
        extra.extend(&transform);
        object.extend(transform);
        inverse.extend(floats(&bone.inverse)?);
        section_map.extend(0i16.to_le_bytes());
        bone_map.extend((index as i16).to_le_bytes());
    }
    let delta = extra.len();
    let mut out = original.0.clone();
    out.splice(cut..cut, extra);
    for field in [24, 0x38] {
        let target = original.pointer(field)?;
        ensure!(target >= cut, "FK header pointer is outside moved metadata");
        put(
            &mut out,
            field,
            &((target + delta - field) as i64).to_le_bytes(),
        )?;
    }
    for field in original.array(0x30, 16, Some(0x808091A4))? {
        let target = original.pointer(field)?;
        ensure!(
            target == instance && field >= cut,
            "FK instance relocation metadata differs"
        );
        put(
            &mut out,
            field + delta,
            &((target as i64) - (field + delta) as i64).to_le_bytes(),
        )?;
    }
    put(
        &mut out,
        0x48,
        &(original.u64(0x48)? + delta as u64).to_le_bytes(),
    )?;
    for at in (0..original.0.len().saturating_sub(15)).step_by(4) {
        if original.u32(at)? != tag {
            continue;
        }
        ensure!(
            original.u32(at + 4)? & 0xffff0000 == 0x80800000,
            "FK self reference is not typed"
        );
        let target = usize::try_from(original.u64(at + 8)?)?;
        ensure!(target < original.0.len(), "FK self reference exceeds owner");
        if target >= cut {
            put(
                &mut out,
                at + if at >= cut { delta } else { 0 } + 8,
                &((target + delta) as u64).to_le_bytes(),
            )?;
        }
    }
    put(
        &mut out,
        section + 0x20,
        &((count + added) as u64).to_le_bytes(),
    )?;
    put(
        &mut out,
        poses[0] - 16,
        &((count + added) as u64).to_le_bytes(),
    )?;
    put(
        &mut out,
        schemas[0] + delta + 16,
        &((count + added) as u32).to_le_bytes(),
    )?;
    for (field, class, stride, data) in [
        (0x80, 0x80808A08, 16, hierarchy),
        (0x90, 0x80809F75, 32, object),
        (0xA0, 0x80809F75, 32, inverse),
        (0xB0, 0x80800006, 2, section_map),
        (0xC0, 0x80800006, 2, bone_map),
    ] {
        append(&mut out, base + delta + field, class, &data, stride)?;
    }
    Ok((Payload(out), count, cut))
}

fn controls(original: &Payload, count: usize, bones: &[Bone]) -> Result<Payload> {
    let base = original.pointer(24)?;
    ensure!(
        original.u32(base - 4)? == 0x80808F8F,
        "Expected native rig controls"
    );
    ensure!(
        original.u64(base + 0xC0)? == 0 && original.u64(base + 0xF8)? == 0,
        "Procedural control tables require a separate animation adapter"
    );
    let mut groups = rows(original, base + 0x90, 8, 0x8080907F)?;
    let mut controls = rows(original, base + 0xA0, 52, 0x80809076)?;
    let mut defaults = rows(original, base + 0xB0, 32, 0x80809F75)?;
    let mut forward = rows(original, base + 0xD8, 2, 0x80800006)?;
    let mut reverse = rows(original, base + 0xE8, 2, 0x80800006)?;
    ensure!(
        controls.len() == count * 52
            && defaults.len() == count * 32
            && forward.len() == count * 2
            && reverse.len() == count * 2
            && !groups.is_empty(),
        "Rig controls require one slot per original FK bone"
    );
    let covered = groups
        .chunks_exact(8)
        .map(|r| u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize)
        .sum::<usize>();
    ensure!(
        covered == count,
        "Rig control groups do not cover their slots"
    );
    for (slot, row) in controls.chunks_exact(52).enumerate() {
        let bone = u16::from_le_bytes(row[44..46].try_into()?) as usize;
        ensure!(
            bone < count
                && u16::from_le_bytes(forward[bone * 2..bone * 2 + 2].try_into()?) as usize == slot
                && u16::from_le_bytes(reverse[slot * 2..slot * 2 + 2].try_into()?) as usize == bone,
            "Native control and bone mapping disagree"
        );
    }
    // Mode 0 composes a local input with its current parent pose. This operation
    // was executed through the original Shadowkeep F61420/F60700 consumers.
    let mut row = vec![0; 52];
    for field in [6, 12, 20, 26, 32, 40, 46] {
        put(&mut row, field, &(-1i16).to_le_bytes())?;
    }
    for field in [16, 36] {
        put(&mut row, field, &1f32.to_le_bytes())?;
    }
    row[22] = 255;
    row[42] = 255;
    row[49] = 255;
    let final_group = groups.len() - 4;
    let prior = u32::from_le_bytes(groups[final_group..].try_into()?);
    put(
        &mut groups,
        final_group,
        &(prior + bones.len() as u32).to_le_bytes(),
    )?;
    for (offset, bone) in bones.iter().enumerate() {
        put(&mut row, 0, &bone.name.to_le_bytes())?;
        for field in [4, 10, 24, 30] {
            put(&mut row, field, &(bone.parent as i16).to_le_bytes())?;
        }
        let index = (count + offset) as i16;
        put(&mut row, 44, &index.to_le_bytes())?;
        controls.extend(&row);
        defaults.extend(floats(&bone.local)?);
        forward.extend(index.to_le_bytes());
        reverse.extend(index.to_le_bytes());
    }
    let mut out = original.0.clone();
    for (field, class, stride, data) in [
        (0x90, 0x8080907F, 8, groups),
        (0xA0, 0x80809076, 52, controls),
        (0xB0, 0x80809F75, 32, defaults),
        (0xD8, 0x80800006, 2, forward),
        (0xE8, 0x80800006, 2, reverse),
    ] {
        append(&mut out, base + field, class, &data, stride)?;
    }
    Ok(Payload(out))
}

pub fn extend(
    original: &Payload,
    tag: u32,
    control: &Payload,
    bones: &[Bone],
) -> Result<Extension> {
    let (skeleton, count, cut) = skeleton(original, tag, bones)?;
    Ok(Extension {
        skeleton,
        controls: controls(control, count, bones)?,
        original_bones: count,
        relocation: (cut, bones.len() * 32),
    })
}
