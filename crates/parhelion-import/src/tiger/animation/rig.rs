//! Append local controls without renumbering a carrier's physics and attachment bones.
mod instance;
mod ranges;
mod section;
mod validation;
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
pub(crate) use instance::grow as grow_instance;
pub use ranges::Renumbering;
pub use validation::{validate_extension, validate_weapon_pair};

pub struct Bone {
    pub name: u32,
    /// Index in the complete native skeleton as the bones are given: the native bones first,
    /// then the appended ones in their order. The extension may renumber it.
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
    /// How the appended bones joined the native range list.
    pub layout: &'static str,
    /// Where every bone ended up. The appended bones are contiguous from `first_appended`.
    pub renumbering: Renumbering,
}

/// Native range lists hold this many FK sections and procedural ranges. Shadowkeep
/// `00577040` binds an equipment skeleton to a first-person skeleton through stack lists of
/// four range records, and a fifth record overwrites the function's stack cookie.
const NATIVE_RANGES: usize = 4;

/// Where appended bones join a skeleton's native range list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Placement {
    /// One new FK section after the trailing procedural range.
    Range,
    /// The range that ends at the last native bone, keeping the range count. A trailing FK
    /// section grows. Before a trailing procedural range, the appended bones join the FK
    /// section it follows and the range's bones move after them, still procedural, so the
    /// donor's procedural controls keep their bones and the appended bones have FK poses.
    Trailing,
}

impl Placement {
    /// The placement a skeleton requires. Native range lists alternate FK sections and
    /// procedural ranges from an FK section, so a trailing FK section grows, and a full list
    /// takes no further range. Equipment ranges bind one to one to the first-person ranges
    /// from the weapon root, so both skeletons of a weapon pair use `Trailing` when either
    /// requires it.
    pub fn required(skeleton: &Payload) -> Result<Self> {
        let instance = skeleton.pointer(16)?;
        let sections = skeleton.array(instance + 0x30, 64, Some(0x80808544))?.len();
        let procedural = skeleton.array(instance + 0x40, 8, Some(0x80808A06))?.len();
        Ok(
            if sections > procedural || sections + procedural >= NATIVE_RANGES {
                Self::Trailing
            } else {
                Self::Range
            },
        )
    }
}

/// The bones of a skeleton's procedural ranges, which native controllers pose instead of FK
/// sections. An appended FK bone cannot follow one as its child without breaking the
/// parent-first order every stock skeleton keeps, since the extension moves them last.
pub fn procedural_bones(skeleton: &Payload) -> Result<Vec<usize>> {
    let instance = skeleton.pointer(16)?;
    let mut bones = Vec::new();
    for range in skeleton.array(instance + 0x40, 8, Some(0x80808A06))? {
        let first = usize::from(skeleton.u16(range)?);
        let length = usize::from(skeleton.u16(range + 4)?);
        bones.extend(first..first + length);
    }
    Ok(bones)
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

/// Point every typed reference an entity still makes to a converted rig's stock owner at the
/// private copy. The component-owner retarget rewrites binding rows and descriptors, and
/// leaves the typed handles inside component data, which the relocation has already moved to
/// the private layout. The client resolves such a handle against the stock tag's bytes, where
/// the moved offset lands inside an array marker, and faults while loading the weapon. Every
/// remaining occurrence must be a typed handle into the private rig, as the relocation checked.
pub fn retarget_rebased_references(
    entity: &mut [u8],
    converted_len: usize,
    owner: u32,
    private: u32,
) -> Result<usize> {
    ensure!(
        owner != private,
        "Rig reference retarget needs two different tags"
    );
    let snapshot = Payload(entity.to_vec());
    let mut count = 0;
    for at in (0..entity.len().saturating_sub(15)).step_by(4) {
        if snapshot.u32(at)? != owner {
            continue;
        }
        ensure!(
            snapshot.u32(at + 4)? & 0xffff0000 == 0x80800000,
            "stale rig reference at {at:X} is not a typed resource pointer"
        );
        let target = usize::try_from(snapshot.u64(at + 8)?)?;
        ensure!(
            target
                .checked_add(16)
                .is_some_and(|end| end <= converted_len),
            "stale rig reference at {at:X} exceeds the private rig"
        );
        put(entity, at, &private.to_le_bytes())?;
        count += 1;
    }
    crate::tiger::entity::reject_stale_owner(&Payload(entity.to_vec()), owner)?;
    Ok(count)
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

#[allow(clippy::cognitive_complexity)]
fn skeleton(
    original: &Payload,
    tag: u32,
    bones: &[Bone],
    placement: Placement,
) -> Result<(Payload, usize, usize, usize, &'static str, Renumbering)> {
    let base = original.pointer(24)?;
    let instance = original.pointer(16)?;
    ensure!(
        original.u32(base - 4)? == 0x80808546 && original.u32(instance - 4)? == 0x80808545,
        "Animation extension requires a native FK skeleton"
    );
    ensure!(
        placement >= Placement::required(original)?,
        "Native range list cannot take another FK section"
    );
    let sections = original.array(instance + 0x30, 64, Some(0x80808544))?;
    let procedural = original.array(instance + 0x40, 8, Some(0x80808A06))?;
    if sections.len() != 1
        || procedural.len() > 1
        || procedural
            .iter()
            .any(|&at| original.u16(at + 4).ok() != Some(1))
        || (placement == Placement::Trailing && !procedural.is_empty())
    {
        return ranges::extend(original, tag, bones, placement);
    }
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
    let layout = if split {
        "appended FK section"
    } else {
        "grown FK section"
    };
    Ok((
        Payload(out),
        count,
        cut,
        delta,
        layout,
        Renumbering::appended(count, added),
    ))
}

/// The fields of a 52-byte control row that name bones: the parent references at 4, 10, 24
/// and 30, the controlled bone at 44, and the remaining signed slots, which hold `-1` or a
/// bone on native rows.
const CONTROL_BONE_FIELDS: [usize; 12] = [4, 6, 10, 12, 20, 24, 26, 30, 32, 40, 44, 46];

/// Renumbers the bone references of a control row in place. A value outside the skeleton's
/// bones is not a reference and stays.
pub(super) fn renumber_control(row: &mut [u8], renumbering: &Renumbering) -> Result<()> {
    for field in CONTROL_BONE_FIELDS {
        let old = i16::from_le_bytes(row[field..field + 2].try_into()?);
        if old < 0 || usize::try_from(old)? >= renumbering.map.len() {
            continue;
        }
        let new = i16::try_from(renumbering.index(i32::from(old))?)?;
        row[field..field + 2].copy_from_slice(&new.to_le_bytes());
    }
    Ok(())
}

fn controls(
    original: &Payload,
    count: usize,
    bones: &[Bone],
    renumbering: &Renumbering,
) -> Result<Payload> {
    let base = original.pointer(24)?;
    ensure!(
        original.u32(base - 4)? == 0x80808F8F,
        "Expected native rig controls"
    );
    ensure!(
        original.u64(base + 0xF8)? == 0,
        "Procedural control tables require a separate animation adapter"
    );
    let mut groups = rows(original, base + 0x90, 8, 0x8080907F)?;
    let mut controls = rows(original, base + 0xA0, 52, 0x80809076)?;
    let mut defaults = rows(original, base + 0xB0, 32, 0x80809F75)?;
    let mut forward = rows(original, base + 0xD8, 2, 0x80800006)?;
    let mut reverse = rows(original, base + 0xE8, 2, 0x80800006)?;
    let slots = controls.len() / 52;
    ensure!(
        slots > 0
            && defaults.len() == slots * 32
            && forward.len() == count * 2
            && reverse.len() == slots * 2
            && !groups.is_empty(),
        "Rig control and bone maps have inconsistent counts"
    );
    let covered = groups
        .chunks_exact(8)
        .map(|r| u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize)
        .sum::<usize>();
    ensure!(
        covered == slots,
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
    ensure!(
        renumbering.map.len() == count + bones.len(),
        "Control renumbering differs from the extended bone count"
    );
    // Native rows name bones by index, so moved bones are renumbered in every row and in
    // the slot to bone map, and the bone to slot map follows the bones into their new order.
    for existing in controls.chunks_exact_mut(52) {
        renumber_control(existing, renumbering)?;
    }
    for slot in reverse.chunks_exact_mut(2) {
        let old = i16::from_le_bytes(slot.try_into()?);
        slot.copy_from_slice(&i16::try_from(renumbering.index(i32::from(old))?)?.to_le_bytes());
    }
    for (offset, bone) in bones.iter().enumerate() {
        put(&mut row, 0, &bone.name.to_le_bytes())?;
        let parent = i16::try_from(renumbering.index(i32::try_from(bone.parent)?)?)?;
        for field in [4, 10, 24, 30] {
            put(&mut row, field, &parent.to_le_bytes())?;
        }
        let bone_index = i16::try_from(renumbering.map[count + offset])?;
        let slot_index = i16::try_from(slots + offset)?;
        put(&mut row, 44, &bone_index.to_le_bytes())?;
        controls.extend(&row);
        defaults.extend(floats(&bone.local)?);
        forward.extend(slot_index.to_le_bytes());
        reverse.extend(bone_index.to_le_bytes());
    }
    let forward = renumbering.permute(&forward, 2)?;
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
    placement: Placement,
) -> Result<Extension> {
    let (skeleton, count, cut, delta, layout, renumbering) =
        skeleton(original, tag, bones, placement)?;
    Ok(Extension {
        skeleton,
        controls: controls(control, count, bones, &renumbering)?,
        original_bones: count,
        relocation: (cut, delta),
        layout,
        renumbering,
    })
}
