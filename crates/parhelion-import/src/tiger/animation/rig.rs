//! Append local controls without renumbering a carrier's physics and attachment bones.
mod section;
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

fn validate_map(
    original: &Payload,
    poses: &[usize],
    ranges: &[usize],
    object: &[u8],
    section_map: &[u8],
    bone_map: &[u8],
) -> Result<()> {
    let count = section_map.len() / 2;
    let fk_count = poses.len();
    let mut mapped = std::collections::BTreeSet::new();
    let mut procedural = std::collections::BTreeSet::new();
    for bone in 0..count {
        let section = u16::from_le_bytes(section_map[bone * 2..bone * 2 + 2].try_into()?);
        let index = u16::from_le_bytes(bone_map[bone * 2..bone * 2 + 2].try_into()?) as usize;
        if section == 0 {
            let &pose = poses
                .get(index)
                .context("FK bone maps outside its section")?;
            ensure!(
                mapped.insert(index)
                    && original.0[pose..pose + 32] == object[bone * 32..bone * 32 + 32],
                "FK section repeats a slot or differs from its object bind pose"
            );
        } else {
            let &range = ranges
                .get(index)
                .context("Procedural bone maps outside its range")?;
            // Preserve the checked one-bone procedural record without changing
            // its indices. More complex range layouts need their own adapter.
            ensure!(
                section == u16::MAX
                    && procedural.insert(index)
                    && original.u16(range)? as usize == bone
                    && original.u16(range + 2)? as usize == bone
                    && original.u16(range + 4)? == 1
                    && original.u16(range + 6)? == 0,
                "Unsupported procedural bone range"
            );
        }
    }
    ensure!(
        mapped.len() == fk_count && procedural.len() == ranges.len(),
        "Skeleton has unused FK or procedural slots"
    );
    Ok(())
}

fn skeleton(
    original: &Payload,
    tag: u32,
    bones: &[Bone],
) -> Result<(Payload, usize, usize, usize)> {
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
    let fk_count = poses.len();
    let mut hierarchy = rows(original, base + 0x80, 16, 0x80808A08)?;
    let count = hierarchy.len() / 16;
    let added = bones.len();
    ensure!(
        fk_count > 0 && count >= fk_count && added > 0 && count + added <= 256,
        "Extended rig exceeds native geometry palette limits"
    );
    let cut = poses.last().context("Empty FK poses")? + 32;
    let ranges = original.array(instance + 0x40, 8, Some(0x80808A06))?;
    ensure!(
        instance + usize::try_from(original.u64(0x48)?)? == base
            && original.u32(base + 0xE8)? as usize == ranges.len(),
        "FK instance span or procedural count differs"
    );
    if let (Some(&first), Some(&last)) = (ranges.first(), ranges.last()) {
        ensure!(
            first >= cut + 20
                && last + 16 == base
                && original.0[cut..first - 20].iter().all(|v| *v == 0)
                && original.u32(first - 20)? == 0x80809FBD
                && original.0[last + 8..base - 4].iter().all(|v| *v == 0),
            "FK procedural state has an unsupported placement"
        );
    } else {
        ensure!(
            cut + 8 == base,
            "FK instance has unsupported trailing state"
        );
    }
    let mut object = rows(original, base + 0x90, 32, 0x80809F75)?;
    let mut inverse = rows(original, base + 0xA0, 32, 0x80809F75)?;
    let mut section_map = rows(original, base + 0xB0, 2, 0x80800006)?;
    let mut bone_map = rows(original, base + 0xC0, 2, 0x80800006)?;
    ensure!(
        hierarchy.len() == count * 16
            && object.len() == count * 32
            && inverse.len() == count * 32
            && section_map.len() == count * 2
            && bone_map.len() == count * 2,
        "FK skeleton arrays have different bone counts"
    );
    validate_map(original, &poses, &ranges, &object, &section_map, &bone_map)?;
    let schemas = original.array(base + 0xD8, 24, Some(0x80808549))?;
    ensure!(
        schemas.len() == 1 && original.u32(schemas[0] + 16)? as usize == fk_count,
        "FK section schema differs"
    );
    let split = count != fk_count;
    ensure!(
        original.u32(section + 0x30)? == 0
            && original.u32(schemas[0] + 20)? == 0
            && (!split
                || (count == fk_count + 1
                    && ranges.len() == 1
                    && original.u16(ranges[0])? as usize == fk_count)),
        "FK extension requires a contiguous section and an optional trailing procedural bone"
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
        section_map.extend(i16::from(split).to_le_bytes());
        bone_map.extend((if split { offset } else { fk_count + offset } as i16).to_le_bytes());
    }
    // A section copies a contiguous global bone range. It cannot be extended
    // across a retained procedural bone, even if the lookup maps are compact.
    let extra_section = if split {
        Some(section::prepare(original, section, cut, &extra, count)?)
    } else {
        None
    };
    let insertion = extra_section
        .as_ref()
        .map_or(extra.as_slice(), |s| s.bytes.as_slice());
    let delta = insertion.len();
    let mut out = original.0.clone();
    out.splice(cut..cut, insertion.iter().copied());
    for field in [24, 0x38] {
        let target = original.pointer(field)?;
        ensure!(target >= cut, "FK header pointer is outside moved metadata");
        put(
            &mut out,
            field,
            &((target + delta - field) as i64).to_le_bytes(),
        )?;
    }
    if !ranges.is_empty() {
        let field = instance + 0x48;
        let target = original.pointer(field)?;
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
    if let Some(extra) = extra_section {
        extra.finish(&mut out, original, base + delta, schemas[0], instance)?;
    } else {
        put(
            &mut out,
            section + 0x20,
            &((fk_count + added) as u64).to_le_bytes(),
        )?;
        put(
            &mut out,
            poses[0] - 16,
            &((fk_count + added) as u64).to_le_bytes(),
        )?;
        put(
            &mut out,
            schemas[0] + delta + 16,
            &((fk_count + added) as u32).to_le_bytes(),
        )?;
    }
    for (field, class, stride, data) in [
        (0x80, 0x80808A08, 16, hierarchy),
        (0x90, 0x80809F75, 32, object),
        (0xA0, 0x80809F75, 32, inverse),
        (0xB0, 0x80800006, 2, section_map),
        (0xC0, 0x80800006, 2, bone_map),
    ] {
        append(&mut out, base + delta + field, class, &data, stride)?;
    }
    Ok((Payload(out), count, cut, delta))
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
    let (skeleton, count, cut, delta) = skeleton(original, tag, bones)?;
    Ok(Extension {
        skeleton,
        controls: controls(control, count, bones)?,
        original_bones: count,
        relocation: (cut, delta),
    })
}
