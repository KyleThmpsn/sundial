//! Join appended bones to a native range list while retaining interleaved procedural ranges.
use super::*;
use std::collections::BTreeSet;

/// FK sections and procedural ranges as first bone and bone count, checked against the
/// skeleton's bone maps, bind poses and section schemas.
pub(super) struct Layout {
    pub sections: Vec<(usize, usize)>,
    pub procedural: Vec<(usize, usize)>,
}

impl Layout {
    pub fn read(p: &Payload) -> Result<Self> {
        let instance = p.pointer(16)?;
        let definition = p.pointer(24)?;
        let cut = definition
            .checked_sub(8)
            .context("FK definition has no class boundary")?;
        let sections = p.array(instance + 0x30, 64, Some(0x80808544))?;
        let schemas = p.array(definition + 0xD8, 24, Some(0x80808549))?;
        let procedural = p.array(instance + 0x40, 8, Some(0x80808A06))?;
        ensure!(
            !sections.is_empty()
                && sections.len() == schemas.len()
                && instance + usize::try_from(p.u64(0x48)?)? == definition
                && p.u32(definition + 0xE8)? as usize == procedural.len(),
            "FK range definitions do not match their instance"
        );
        let count = p.array(definition + 0x80, 16, Some(0x80808A08))?.len();
        let object = rows(p, definition + 0x90, 32, 0x80809F75)?;
        let section_map = rows(p, definition + 0xB0, 2, 0x80800006)?;
        let bone_map = rows(p, definition + 0xC0, 2, 0x80800006)?;
        ensure!(
            count > 0
                && object.len() == count * 32
                && section_map.len() == count * 2
                && bone_map.len() == count * 2,
            "FK range arrays have inconsistent sizes"
        );
        let maps = |bone: usize, selector: u16, offset: usize| {
            section_map[bone * 2..bone * 2 + 2] == selector.to_le_bytes()
                && bone_map[bone * 2..bone * 2 + 2] == (offset as u16).to_le_bytes()
        };
        let mut covered = BTreeSet::new();
        let mut layout = Self {
            sections: Vec::new(),
            procedural: Vec::new(),
        };
        for (index, (&section, &schema)) in sections.iter().zip(&schemas).enumerate() {
            let poses = p.array(section + 32, 32, Some(0x80809F75))?;
            let first = p.u32(section + 48)? as usize;
            ensure!(
                section + 64 <= cut
                    && !poses.is_empty()
                    && p.pointer(section + 16)? == instance
                    && p.u32(section + 4)? == 0x80808549
                    && p.u64(section + 8)? as usize == schema
                    && p.u32(schema + 4)? == 0x80808544
                    && p.u64(schema + 8)? as usize == section
                    && p.u32(schema + 16)? as usize == poses.len()
                    && p.u32(schema + 20)? == 0,
                "FK section does not match its schema"
            );
            for (offset, &pose) in poses.iter().enumerate() {
                let bone = first + offset;
                ensure!(
                    bone < count
                        && pose + 32 <= cut
                        && covered.insert(bone)
                        && maps(bone, index as u16, offset)
                        && p.0[pose..pose + 32] == object[bone * 32..bone * 32 + 32],
                    "FK section overlaps or differs from its bone map and bind pose"
                );
            }
            layout.sections.push((first, poses.len()));
        }
        for (index, &range) in procedural.iter().enumerate() {
            let first = usize::from(p.u16(range)?);
            let length = usize::from(p.u16(range + 4)?);
            ensure!(
                range + 8 <= cut
                    && length > 0
                    && p.u16(range + 2)? as usize == first
                    && p.u16(range + 6)? == 0,
                "Unsupported procedural bone range"
            );
            for offset in 0..length {
                let bone = first + offset;
                ensure!(
                    bone < count && covered.insert(bone) && maps(bone, !(index as u16), offset),
                    "Procedural range overlaps or differs from its bone map"
                );
            }
            layout.procedural.push((first, length));
        }
        ensure!(
            covered.len() == count,
            "FK ranges do not cover the skeleton"
        );
        Ok(layout)
    }

    /// Ranges in native list order. Shadowkeep `00577040` gives FK section `i` ordinal `2i`
    /// and procedural range `i` ordinal `2i + 1`, reading each record's first bone and count.
    pub fn native(&self) -> Result<Vec<(usize, usize)>> {
        let (sections, procedural) = (self.sections.len(), self.procedural.len());
        ensure!(
            sections == procedural || sections == procedural + 1,
            "Native range list does not alternate FK sections and procedural ranges"
        );
        Ok((0..sections + procedural)
            .map(|ordinal| {
                if ordinal % 2 == 0 {
                    self.sections[ordinal / 2]
                } else {
                    self.procedural[ordinal / 2]
                }
            })
            .collect())
    }
}

/// Where every bone of an extended skeleton ends up: old index to new index for the native
/// bones, then the appended bones in their order. Appending at the end leaves the native
/// bones in place. Inserting before the trailing procedural range moves that range's bones
/// after the appended ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renumbering {
    /// The new index of each old bone, then of each appended bone.
    pub map: Vec<usize>,
    /// The new index of the first appended bone. The appended bones are contiguous from it.
    pub first_appended: usize,
}

impl Renumbering {
    pub fn appended(count: usize, added: usize) -> Self {
        Self {
            map: (0..count + added).collect(),
            first_appended: count,
        }
    }

    /// The appended bones take the indices from `at`, and the native bones from `at` move
    /// after them.
    pub fn inserted(count: usize, added: usize, at: usize) -> Self {
        let map = (0..count + added)
            .map(|index| match index {
                index if index < at => index,
                index if index < count => index + added,
                index => at + index - count,
            })
            .collect();
        Self {
            map,
            first_appended: at,
        }
    }

    /// Puts `rows` of `stride` bytes, held in old bone order, into new bone order.
    pub fn permute(&self, rows: &[u8], stride: usize) -> Result<Vec<u8>> {
        ensure!(
            rows.len() == self.map.len() * stride,
            "Renumbered rows differ from the bone count"
        );
        let mut out = vec![0; rows.len()];
        for (old, &new) in self.map.iter().enumerate() {
            out[new * stride..(new + 1) * stride]
                .copy_from_slice(&rows[old * stride..(old + 1) * stride]);
        }
        Ok(out)
    }

    /// The new index of a bone reference, with `-1` kept as no bone.
    pub fn index(&self, old: i32) -> Result<i32> {
        if old < 0 {
            return Ok(old);
        }
        let new = *self
            .map
            .get(usize::try_from(old)?)
            .context("Bone reference is outside the extended skeleton")?;
        Ok(i32::try_from(new)?)
    }
}

/// The range that takes the appended bones.
#[derive(Clone, Copy)]
enum Target {
    /// One new FK section after the trailing procedural range.
    Range,
    /// The trailing FK section, which receives a larger pose array in the instance.
    Section(usize),
    /// The FK section before the trailing procedural range, which receives the appended bones
    /// as FK bones in a larger pose array. The range's bones move after them and remain
    /// procedural so the donor's procedural controls retain their original targets.
    Insert { section: usize, procedural: usize },
}

impl Target {
    fn layout(self) -> &'static str {
        match self {
            Self::Range => "appended FK section",
            Self::Section(_) => "grown trailing FK section",
            Self::Insert { .. } => "FK bones inserted before the trailing procedural range",
        }
    }
}

/// Makes the instance's procedural ranges, holding `rows`, its last array again, as native
/// skeletons keep them, and zeroes the array's former place at `old`. Shadowkeep reads the words
/// after the last procedural row as tags up to a zero word (`exe+0xA40300` in the 2.9.2 client):
/// the stale second row a shrunk array left there faulted the client at boot.
fn trail_procedural(out: &mut Vec<u8>, instance: usize, old: &[usize], rows: &[u8]) -> Result<()> {
    if let (Some(&first), Some(&last)) = (old.first(), old.last()) {
        out.get_mut(first - 20..last + 8)
            .context("Procedural range array lies outside the instance")?
            .fill(0);
    }
    append(out, instance + 0x40, 0x80808A06, rows, 8)
}

fn relative(bytes: &mut [u8], field: usize, target: usize) -> Result<()> {
    put(bytes, field, &(target as i64 - field as i64).to_le_bytes())
}

fn rebase(out: &mut [u8], original: &Payload, tag: u32, cut: usize, delta: usize) -> Result<()> {
    let instance = original.pointer(16)?;
    for field in [24, 0x38] {
        let target = original.pointer(field)?;
        ensure!(target >= cut, "FK metadata header points into the instance");
        relative(out, field, target + delta)?;
    }
    for field in original.array(0x30, 16, Some(0x808091A4))? {
        ensure!(
            field >= cut && original.pointer(field)? == instance,
            "FK relocation footer differs"
        );
        relative(out, field + delta, instance)?;
    }
    put(
        out,
        0x48,
        &(original.u64(0x48)? + delta as u64).to_le_bytes(),
    )?;
    for at in (0..original.0.len().saturating_sub(15)).step_by(4) {
        if original.u32(at)? != tag {
            continue;
        }
        let target = usize::try_from(original.u64(at + 8)?)?;
        ensure!(
            original.u32(at + 4)? & 0xffff0000 == 0x80800000 && target < original.0.len(),
            "FK self reference is not a valid typed interior reference"
        );
        if target >= cut {
            put(
                out,
                at + if at >= cut { delta } else { 0 } + 8,
                &u64::try_from(target + delta)?.to_le_bytes(),
            )?;
        }
    }
    Ok(())
}

/// Renumbers the parent, first child and next sibling words of hierarchy rows held in new
/// bone order, whose values still name old bones.
fn relink(hierarchy: &mut [u8], renumbering: &Renumbering) -> Result<()> {
    for row in hierarchy.chunks_exact_mut(16) {
        for field in [4, 8, 12] {
            let old = i32::from_le_bytes(row[field..field + 4].try_into()?);
            row[field..field + 4].copy_from_slice(&renumbering.index(old)?.to_le_bytes());
        }
    }
    Ok(())
}

pub(super) fn extend(
    original: &Payload,
    tag: u32,
    bones: &[Bone],
    placement: Placement,
) -> Result<(Payload, usize, usize, usize, &'static str, Renumbering)> {
    let layout = Layout::read(original)?;
    let instance = original.pointer(16)?;
    let definition = original.pointer(24)?;
    let cut = definition - 8;
    let sections = original.array(instance + 0x30, 64, Some(0x80808544))?;
    let schemas = original.array(definition + 0xD8, 24, Some(0x80808549))?;
    let procedural = original.array(instance + 0x40, 8, Some(0x80808A06))?;
    let mut hierarchy = rows(original, definition + 0x80, 16, 0x80808A08)?;
    let count = hierarchy.len() / 16;
    let mut object = rows(original, definition + 0x90, 32, 0x80809F75)?;
    let mut inverse = rows(original, definition + 0xA0, 32, 0x80809F75)?;
    let mut section_map = rows(original, definition + 0xB0, 2, 0x80800006)?;
    let mut bone_map = rows(original, definition + 0xC0, 2, 0x80800006)?;
    ensure!(
        !bones.is_empty() && count + bones.len() <= 256 && inverse.len() == count * 32,
        "Extended FK range arrays have inconsistent sizes"
    );
    // Complete coverage leaves exactly one range ending at the last native bone.
    let ends = |&(first, length): &(usize, usize)| first + length == count;
    let trailing = match (
        layout.sections.iter().position(ends),
        layout.procedural.iter().position(ends),
    ) {
        (Some(index), None) => Target::Section(index),
        (None, Some(index)) => {
            // The appended bones join the FK section the trailing procedural range follows,
            // so both must be the last of their kind in native list order.
            let (first, _) = layout.procedural[index];
            let section = layout
                .sections
                .iter()
                .position(|&(section_first, length)| section_first + length == first)
                .context("Trailing procedural range does not follow an FK section")?;
            ensure!(
                section + 1 == layout.sections.len() && index + 1 == layout.procedural.len(),
                "Trailing procedural range is not the last range of its kind"
            );
            Target::Insert {
                section,
                procedural: index,
            }
        }
        _ => anyhow::bail!("FK ranges have no single trailing range"),
    };
    // A new section takes the next ordinal, so it must follow the last procedural range.
    let target = match (placement, trailing) {
        (Placement::Trailing, trailing) => trailing,
        (Placement::Range, Target::Insert { .. })
            if sections.len() == procedural.len()
                && sections.len() + procedural.len() < NATIVE_RANGES =>
        {
            Target::Range
        }
        _ => anyhow::bail!("Native range list cannot take another FK section"),
    };
    let (selector, start) = match target {
        Target::Range => (sections.len() as u16, 0),
        Target::Section(index) => (index as u16, layout.sections[index].1),
        Target::Insert { section, .. } => (u16::try_from(section)?, layout.sections[section].1),
    };
    let renumbering = match target {
        Target::Insert {
            procedural: index, ..
        } => Renumbering::inserted(count, bones.len(), layout.procedural[index].0),
        _ => Renumbering::appended(count, bones.len()),
    };
    let mut names = hierarchy
        .chunks_exact(16)
        .map(|row| u32::from_le_bytes(row[..4].try_into().unwrap()))
        .collect::<BTreeSet<_>>();
    let mut extra = Vec::new();
    // The appended rows are built in old bone order first, with the parent given in that
    // order, and every array is put into new bone order at the end.
    for (offset, bone) in bones.iter().enumerate() {
        let index = count + offset;
        ensure!(
            bone.parent < index && names.insert(bone.name),
            "Appended FK bone has an invalid parent or repeated name"
        );
        ensure!(
            renumbering.map[bone.parent] < renumbering.map[index],
            "Appended FK bone would follow a moved procedural parent"
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
        let pose = floats(&bone.object)?;
        extra.extend(&pose);
        object.extend(pose);
        inverse.extend(floats(&bone.inverse)?);
        section_map.extend(selector.to_le_bytes());
        bone_map.extend(u16::try_from(start + offset)?.to_le_bytes());
    }
    let (mut out, delta) = match target {
        Target::Range => {
            let mut out = original.0[..cut].to_vec();
            let section_bytes = sections
                .iter()
                .chain(sections.last())
                .flat_map(|&at| original.0[at..at + 64].iter().copied())
                .collect::<Vec<_>>();
            append(&mut out, instance + 0x30, 0x80808544, &section_bytes, 64)?;
            let new_sections = Payload(out.clone()).array(instance + 0x30, 64, Some(0x80808544))?;
            for (&old, &new) in sections.iter().zip(&new_sections) {
                relative(&mut out, new + 0x28, original.pointer(old + 0x28)?)?;
            }
            for &new in &new_sections {
                relative(&mut out, new + 16, instance)?;
            }
            let last = *new_sections.last().context("Appended FK section missing")?;
            append(&mut out, last + 32, 0x80809F75, &extra, 32)?;
            put(&mut out, last + 48, &u32::try_from(count)?.to_le_bytes())?;
            let ranges = rows(original, instance + 0x40, 8, 0x80808A06)?;
            trail_procedural(&mut out, instance, &procedural, &ranges)?;
            let delta = out.len() - cut;
            out.extend(&original.0[cut..]);
            rebase(&mut out, original, tag, cut, delta)?;
            let schema_bytes = schemas
                .iter()
                .chain(schemas.last())
                .flat_map(|&at| original.0[at..at + 24].iter().copied())
                .collect::<Vec<_>>();
            append(
                &mut out,
                definition + delta + 0xD8,
                0x80808549,
                &schema_bytes,
                24,
            )?;
            let new_schemas =
                Payload(out.clone()).array(definition + delta + 0xD8, 24, Some(0x80808549))?;
            for (&section, &schema) in new_sections.iter().zip(&new_schemas) {
                put(&mut out, section + 8, &u64::try_from(schema)?.to_le_bytes())?;
                put(&mut out, schema + 8, &u64::try_from(section)?.to_le_bytes())?;
            }
            let last = *new_schemas.last().context("Appended FK schema missing")?;
            put(
                &mut out,
                last + 16,
                &u32::try_from(bones.len())?.to_le_bytes(),
            )?;
            (out, delta)
        }
        Target::Section(index) => {
            // The larger pose array stays inside the copied instance image.
            let mut out = original.0[..cut].to_vec();
            let mut poses = rows(original, sections[index] + 32, 32, 0x80809F75)?;
            poses.extend(&extra);
            append(&mut out, sections[index] + 32, 0x80809F75, &poses, 32)?;
            let ranges = rows(original, instance + 0x40, 8, 0x80808A06)?;
            trail_procedural(&mut out, instance, &procedural, &ranges)?;
            let delta = out.len() - cut;
            out.extend(&original.0[cut..]);
            rebase(&mut out, original, tag, cut, delta)?;
            put(
                &mut out,
                schemas[index] + delta + 16,
                &u32::try_from(poses.len() / 32)?.to_le_bytes(),
            )?;
            (out, delta)
        }
        Target::Insert {
            section,
            procedural: index,
        } => {
            // The section's larger pose array, with the appended bones after its own, stays
            // inside the copied instance image. The trailing range keeps its record and its
            // length and starts after the appended bones.
            let mut out = original.0[..cut].to_vec();
            let mut poses = rows(original, sections[section] + 32, 32, 0x80809F75)?;
            poses.extend(&extra);
            append(&mut out, sections[section] + 32, 0x80809F75, &poses, 32)?;
            let mut ranges = rows(original, instance + 0x40, 8, 0x80808A06)?;
            let first = u16::try_from(layout.procedural[index].0 + bones.len())?;
            put(&mut ranges, index * 8, &first.to_le_bytes())?;
            put(&mut ranges, index * 8 + 2, &first.to_le_bytes())?;
            trail_procedural(&mut out, instance, &procedural, &ranges)?;
            let delta = out.len() - cut;
            out.extend(&original.0[cut..]);
            rebase(&mut out, original, tag, cut, delta)?;
            put(
                &mut out,
                schemas[section] + delta + 16,
                &u32::try_from(poses.len() / 32)?.to_le_bytes(),
            )?;
            (out, delta)
        }
    };
    let mut hierarchy = renumbering.permute(&hierarchy, 16)?;
    relink(&mut hierarchy, &renumbering)?;
    for (field, class, stride, bytes) in [
        (0x80, 0x80808A08, 16, hierarchy),
        (0x90, 0x80809F75, 32, renumbering.permute(&object, 32)?),
        (0xA0, 0x80809F75, 32, renumbering.permute(&inverse, 32)?),
        (0xB0, 0x80800006, 2, renumbering.permute(&section_map, 2)?),
        (0xC0, 0x80800006, 2, renumbering.permute(&bone_map, 2)?),
    ] {
        append(&mut out, definition + delta + field, class, &bytes, stride)?;
    }
    Ok((
        Payload(out),
        count,
        cut,
        delta,
        target.layout(),
        renumbering,
    ))
}
