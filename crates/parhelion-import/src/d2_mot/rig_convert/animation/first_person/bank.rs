//! State choices address descriptors, independently of the bank's clip array.
use super::*;
use crate::d2_mot::{payload::Payload, rig_convert::write_array};

pub(super) struct Clip {
    pub tag: u32,
    pub frames: u16,
    pub mode: u16,
}

pub(super) fn modes(bank: &Payload, clips: &[(u32, u32)]) -> Result<BTreeMap<u32, u16>> {
    let mut result = BTreeMap::new();
    for row in bank.array(0x68, 32, Some(0x80809002))? {
        let index = usize::from(bank.u16(row + 24)?);
        let tag = clips
            .get(index)
            .context("animation descriptor clip index")?
            .0;
        let mode = bank.u16(row + 26)?;
        if let Some(previous) = result.insert(tag, mode) {
            ensure!(
                previous == mode,
                "clip descriptors disagree about playback mode"
            );
        }
    }
    Ok(result)
}

pub(super) fn convert(
    reader: &mut Reader,
    source: &Payload,
    native: &Payload,
    native_clips: &[(u32, u32)],
    converted: &BTreeMap<u32, Clip>,
) -> Result<(Payload, BTreeMap<u32, u32>, Value)> {
    let mut output = native.clone();
    let mut tags = native_clips.iter().map(|c| c.0).collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut names = BTreeMap::new();
    for row in native.array(0x68, 32, Some(0x80809002))? {
        // Nonempty inline tracks need pointer relocation and an explicit converter.
        ensure!(
            native.bytes::<16>(row)? == [0; 16],
            "animation descriptor has inline tracks"
        );
        let name = native.u32(row + 16)?;
        let mut bytes = native.bytes::<32>(row)?;
        let clip_tag = native_clips
            .get(usize::from(native.u16(row + 24)?))
            .context("native descriptor clip")?
            .0;
        if let Some(clip) = converted.values().find(|clip| clip.tag == clip_tag) {
            ensure!(clip.frames > 0, "converted clip has no frames");
            bytes[20..24].copy_from_slice(&(f32::from(clip.frames - 1) / 30.0).to_le_bytes());
        }
        // Names can alias different clips. A state addresses a descriptor
        // ordinal, so preserve both rows and match by name AND clip identity.
        if let Some(&previous) = names.get(&(name, clip_tag)) {
            ensure!(
                rows[previous] == bytes,
                "ambiguous native animation descriptor"
            );
        } else {
            names.insert((name, clip_tag), rows.len());
        }
        rows.push(bytes);
    }
    let original = rows.len();
    // Base poses can be addressed by clip name without a state descriptor.
    // Keep these converted source clips reachable in the same private bank.
    for clip in converted.values() {
        if !tags.contains(&clip.tag) {
            tags.push(clip.tag);
        }
    }
    let mut mapping = BTreeMap::new();
    let source_slots = source.array(8, 16, Some(0x80808BDF))?;
    for (ordinal, at) in source
        .array(0x58, 48, Some(0x80808BDE))?
        .into_iter()
        .enumerate()
    {
        let name = source.u32(at + 16)?;
        if source.bytes::<16>(at)? != [0; 16] || source.f32(at + 20)? != 1.0 {
            continue;
        }
        let tag = reader.ref64(source, at + 24)?;
        let source_slot = source_slots
            .get(usize::try_from(source.u32(at + 40)?)?)
            .context("source descriptor clip index")?;
        ensure!(
            reader.ref64(source, *source_slot)? == tag,
            "source descriptor and clip slot disagree"
        );
        let Some(clip) = converted.get(&tag) else {
            continue;
        };
        ensure!(clip.frames > 0, "animation descriptor clip has no frames");
        let index = if let Some(index) = tags.iter().position(|tag| *tag == clip.tag) {
            index
        } else {
            tags.push(clip.tag);
            tags.len() - 1
        };
        let row_index = if let Some(index) = names.get(&(name, clip.tag)) {
            *index
        } else {
            let index = rows.len();
            let mut row = [0; 32];
            row[16..20].copy_from_slice(&name.to_le_bytes());
            row[26..28].copy_from_slice(&clip.mode.to_le_bytes());
            rows.push(row);
            names.insert((name, clip.tag), index);
            index
        };
        let row = &mut rows[row_index];
        row[20..24].copy_from_slice(&(f32::from(clip.frames - 1) / 30.0).to_le_bytes());
        row[24..26].copy_from_slice(&u16::try_from(index)?.to_le_bytes());
        mapping.insert(u32::try_from(ordinal)?, u32::try_from(row_index)?);
    }
    let auxiliary = native.array(24, 4, Some(0x80800007))?;
    ensure!(
        auxiliary.len() == native_clips.len(),
        "native bank auxiliary index count differs"
    );
    let mut auxiliary = auxiliary
        .iter()
        .map(|&p| native.u32(p))
        .collect::<Result<Vec<_>>>()?;
    auxiliary.resize(tags.len(), u32::MAX);
    write_array(
        &mut output.0,
        8,
        0x80808F48,
        tags.len(),
        &tags
            .iter()
            .flat_map(|t| t.to_le_bytes())
            .collect::<Vec<_>>(),
    )?;
    write_array(
        &mut output.0,
        24,
        0x80800007,
        auxiliary.len(),
        &auxiliary
            .iter()
            .flat_map(|t| t.to_le_bytes())
            .collect::<Vec<_>>(),
    )?;
    write_array(
        &mut output.0,
        0x68,
        0x80809002,
        rows.len(),
        &rows.iter().flatten().copied().collect::<Vec<_>>(),
    )?;
    let size = output.0.len() as u64;
    output.0[..8].copy_from_slice(&size.to_le_bytes());
    let report = json!({"native_descriptors":original,"descriptors":rows.len(),"source_descriptors":mapping.len(),"clips":tags.len(),"added_descriptors":rows.len()-original});
    Ok((output, mapping, report))
}
