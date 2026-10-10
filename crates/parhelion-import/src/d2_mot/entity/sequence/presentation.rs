//! Indexed particle choices and their native per-group runtime handles.
use super::{Bindings, controls::*, events};
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};

pub(crate) fn groups(source: &Payload) -> Result<Vec<Vec<usize>>> {
    let definition = source.pointer(24)?;
    let systems = source.array(definition + 0x2C0, 4, Some(0x80800014))?;
    source
        .array(definition + 0x280, 12, Some(0x80806D97))?
        .into_iter()
        .map(|row| {
            let count = usize::try_from(source.u32(row)?)?;
            let start = usize::try_from(source.u32(row + 4)?)?;
            ensure!(
                count > 0 && source.u32(row + 8)? == 0,
                "particle group shape differs"
            );
            Ok(systems
                .get(
                    start
                        ..start
                            .checked_add(count)
                            .context("particle group overflow")?,
                )
                .context("particle group exceeds its system table")?
                .to_vec())
        })
        .collect()
}

/// A null direct tag can select a group. It is not an absent particle event.
pub(crate) fn fields(source: &Payload, row: usize) -> Result<Vec<usize>> {
    let index = source.u32(row + 20)?;
    if source.u32(row + 16)? != u32::MAX {
        ensure!(
            index == u32::MAX,
            "particle row mixes direct and indexed systems"
        );
        return Ok(vec![row + 16]);
    }
    groups(source)?
        .get(usize::try_from(index)?)
        .cloned()
        .context("indexed particle row exceeds its group table")
}

fn copy(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    classes: [u32; 2],
    stride: usize,
) -> Result<usize> {
    let rows = source.array(from, stride, Some(classes[0]))?;
    let bytes = rows
        .iter()
        .flat_map(|&row| source.0[row..row + stride].iter().copied())
        .collect::<Vec<_>>();
    array(output, to, classes[1], &bytes, stride)?;
    Ok(rows.len())
}

pub(super) fn emit(
    source: &Payload,
    output: &mut Payload,
    bindings: &Bindings,
) -> Result<Option<Allocation>> {
    let sd = source.pointer(24)?;
    let nd = output.pointer(24)?;
    let groups = groups(source)?;
    ensure!(
        groups.len() <= u16::MAX as usize,
        "too many indexed particle groups"
    );
    ensure!(
        source.bytes::<32>(sd + 0x290)? == [0; 32]
            && source.bytes::<24>(sd + 0x2D0)? == [0; 24]
            && source.u64(sd + 0x2F8)? & !0x8000 == 0
            && source.u64(sd + 0x300)? == 0
            && source.u32(sd + 0x308)? as usize == groups.len(),
        "source indexed particle extensions require translation"
    );
    let si = usize::try_from(source.u64(sd + 0x278)?)?;
    let ni = usize::try_from(output.u64(nd + 0x1F8)?)?;
    ensure!(
        source.u32(sd + 0x274)? == 0x80806D92
            && source.u32(sd + 0x270)? == source.u32(source.pointer(16)?)?
            && source.u32(si)? == source.u32(sd + 0x270)?
            && source.u32(si + 4)? == 0x80806D93
            && source.u64(si + 8)? == (sd + 0x270) as u64
            && source.pointer(si + 16)? == source.pointer(16)?
            && source.bytes::<24>(si + 24)? == [0; 24]
            && source.pointer(sd + 0x2E8)? == sd + 0x270
            && output.u32(nd + 0x1F4)? == 0x808072C0
            && output.u32(ni + 4)? == 0x808072C1,
        "indexed particle runtime pair differs"
    );
    copy(
        source,
        sd + 0x280,
        output,
        nd + 0x200,
        [0x80806D97, 0x808072C4],
        12,
    )?;
    copy(
        source,
        sd + 0x2B0,
        output,
        nd + 0x230,
        [0x80806D98, 0x808072C5],
        8,
    )?;
    put(output, nd + 0x210, &[0; 32])?;
    let mut systems = Vec::new();
    for field in source.array(sd + 0x2C0, 4, Some(0x80800014))? {
        systems.extend(events::link(bindings, field, 0x80806920, 0x80806E28)?.to_le_bytes());
    }
    array(output, nd + 0x240, 0x80800014, &systems, 4)?;
    let runtime = source.array(si + 0x30, 8, Some(0x80806DB3))?;
    ensure!(
        runtime.len() == groups.len(),
        "indexed particle runtime count differs"
    );
    for row in runtime {
        ensure!(
            source.u64(row)? == u64::MAX,
            "indexed particle handles are initialized"
        );
    }
    array(
        output,
        ni + 0x20,
        0x80802EFB,
        &vec![0xFF; groups.len() * 8],
        8,
    )?;
    put(output, ni + 0x18, &[0; 8])?;
    put(output, nd + 0x260, &source.bytes::<8>(sd + 0x2F8)?)?;
    put(output, nd + 0x268, &source.bytes::<4>(sd + 0x308)?)?;
    Ok((!groups.is_empty()).then(|| {
        let mut allocation = Allocation::array(0x880CA149, u32::MAX, 0);
        allocation
            .children
            .push(Allocation::array(0xF52674D2, 0x80802EFB, groups.len()));
        allocation
    }))
}
