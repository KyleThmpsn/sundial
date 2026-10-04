//! Native control records and their allocation tree.
use super::{Range, Sequence};
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

pub(super) fn put(p: &mut Payload, at: usize, bytes: &[u8]) -> Result<()> {
    let end = at
        .checked_add(bytes.len())
        .context("sequence write overflow")?;
    p.0.get_mut(at..end)
        .context("sequence write outside payload")?
        .copy_from_slice(bytes);
    Ok(())
}

pub(super) fn relative(p: &mut Payload, field: usize, target: usize) -> Result<()> {
    let delta = i64::try_from(target)? - i64::try_from(field)?;
    put(p, field, &delta.to_le_bytes())
}

pub(super) fn object(p: &mut Payload, class: u32, size: usize) -> usize {
    // Native transform constants and bank vectors are read with aligned SIMD
    // loads. Keep both object starts and array rows on a vector boundary.
    let at = (p.0.len() + 19) & !15;
    p.0.resize(at + size, 0);
    p.0[at - 4..at].copy_from_slice(&class.to_le_bytes());
    at
}

pub(super) fn array(
    p: &mut Payload,
    field: usize,
    class: u32,
    rows: &[u8],
    stride: usize,
) -> Result<usize> {
    ensure!(
        stride > 0 && rows.len().is_multiple_of(stride),
        "sequence array stride differs"
    );
    let count = rows.len() / stride;
    put(p, field, &(count as u64).to_le_bytes())?;
    if count == 0 {
        put(p, field + 8, &0u64.to_le_bytes())?;
        return Ok(0);
    }
    let header = object(p, 0x80809FBD, 16 + rows.len());
    put(p, header, &(count as u64).to_le_bytes())?;
    put(p, header + 8, &class.to_le_bytes())?;
    put(p, header + 16, rows)?;
    relative(p, field + 8, header)?;
    Ok(header + 16)
}

pub(super) fn pair(
    p: &mut Payload,
    owner: u32,
    instance: usize,
    definition: usize,
    ic: u32,
    dc: u32,
) -> Result<()> {
    for (at, class, twin) in [(instance, dc, definition), (definition, ic, instance)] {
        put(p, at, &owner.to_le_bytes())?;
        put(p, at + 4, &class.to_le_bytes())?;
        put(p, at + 8, &(twin as u64).to_le_bytes())?;
    }
    Ok(())
}

/// The native common node base inserts a reciprocal object prefix and aligns
/// the kind separately. Source bit 5 is absent from corresponding native nodes.
pub(super) fn common(source: &Payload, from: usize, output: &mut Payload, to: usize) -> Result<()> {
    ensure!(
        source.u32(from + 8)? & !0x3F == 0,
        "unsupported sequence node flags"
    );
    // These are authored timing fields, including the start time at +10.
    // Their corresponding native common span preserves every float. Delays
    // may have nonzero start times, independently of their duration fields.
    for offset in [12, 16, 20, 24] {
        ensure!(
            source.f32(from + offset)?.is_finite(),
            "nonfinite sequence node timing"
        );
    }
    put(output, to + 16, &source.bytes::<28>(from)?)?;
    put(
        output,
        to + 24,
        &(source.u32(from + 8)? & !0x20).to_le_bytes(),
    )?;
    Ok(())
}

#[derive(Clone)]
pub(super) struct Allocation {
    pub name: u32,
    pub class: u32,
    pub count: usize,
    pub children: Vec<Allocation>,
}

impl Allocation {
    pub fn array(name: u32, class: u32, count: usize) -> Self {
        Self {
            name,
            class,
            count,
            children: vec![],
        }
    }

    pub fn node(class: u32) -> Self {
        Self {
            name: 0x811C9DC5,
            class: u32::MAX,
            count: 0,
            children: vec![Self::array(0x3988F7D4, class, 1)],
        }
    }

    pub fn write(rows: &[Self], output: &mut Payload, field: usize) -> Result<()> {
        ensure!(
            rows.len() <= 65535,
            "sequence allocation count exceeds capacity"
        );
        let start = array(output, field, 0x80808852, &vec![0; rows.len() * 40], 40)?;
        for (index, row) in rows.iter().enumerate() {
            let at = start + index * 40;
            put(output, at, &row.name.to_le_bytes())?;
            put(output, at + 16, &row.class.to_le_bytes())?;
            put(output, at + 20, &u32::try_from(row.count)?.to_le_bytes())?;
            Self::write(&row.children, output, at + 24)?;
        }
        Ok(())
    }
}

fn condition(
    source: &Option<Range>,
    output: &mut Payload,
    field: usize,
    inputs: &BTreeMap<u32, u32>,
    count: usize,
) -> Result<()> {
    match source {
        None => put(output, field, &0u64.to_le_bytes()),
        Some(range) => {
            let bytes = range.native(inputs, count)?;
            let at = object(output, super::NATIVE_RANGE_CLASS, bytes.len());
            put(output, at, &bytes)?;
            relative(output, field, at)
        }
    }
}

pub(super) fn write(
    source: &Payload,
    sequence: &Sequence,
    output: &mut Payload,
    inputs: &BTreeMap<u32, u32>,
    input_count: usize,
) -> Result<Allocation> {
    let root = output.pointer(16)?;
    let definition = output.pointer(24)?;
    let owner = output.u32(root)?;
    let count = sequence.controls.len();
    ensure!(count <= i16::MAX as usize, "too many sequence controls");
    let ir = array(output, root + 0xA0, 0x808093E5, &vec![0; count * 48], 48)?;
    let dr = array(
        output,
        definition + 0x158,
        0x808093E6,
        &vec![0; count * 24],
        24,
    )?;
    let mut allocation = Allocation::array(0xC78E66C7, 0x808093E5, count);
    for (index, control) in sequence.controls.iter().enumerate() {
        let (dc, ic, size, kind) = match control.class {
            0x808091E3 => (0x808093D7, 0x808093D6, 0x58, 1),
            0x808091D9 => (0x808093CD, 0x808093CC, 0x68, 6),
            0x808091E5 => (0x808093D9, 0x808093D8, 0x58, 0),
            0x808091E1 => (0x808093D5, 0x808093D4, 0x68, 2),
            class => anyhow::bail!("unsupported sequence control {class:08X}"),
        };
        let from = control.offset;
        ensure!(
            source.u16(from + 28)? == kind && source.u8(from + 31)? == 0xFF,
            "source control kind or extension differs"
        );
        ensure!(
            source.u8(from + 30)? <= 1,
            "unsupported sequence control mode"
        );
        let instance_row = ir + index * 48;
        let definition_row = dr + index * 24;
        pair(
            output,
            owner,
            instance_row,
            definition_row,
            0x808093E5,
            0x808093E6,
        )?;
        relative(output, instance_row + 16, root)?;
        let instance = object(output, ic, 0x60);
        let target = object(output, dc, size);
        pair(output, owner, instance, target, ic, dc)?;
        if kind == 2 {
            // The weighted flow has no selected child before first entry.
            // All inspected native 93D4 envelopes use this sentinel.
            put(output, instance + 0x50, &u32::MAX.to_le_bytes())?;
        }
        relative(output, instance + 16, instance_row)?;
        relative(output, instance_row + 32, instance)?;
        relative(output, definition_row + 16, target)?;
        common(source, from, output, target)?;
        put(output, target + 0x30, &source.bytes::<3>(from + 28)?)?;
        let mut children = Vec::with_capacity(control.children.len() * 4);
        for child in &control.children {
            children.extend(u16::from(child.event).to_le_bytes());
            children.extend(u16::try_from(child.index)?.to_le_bytes());
        }
        array(output, target + 0x38, 0x808093FB, &children, 4)?;
        for slot in 0..2 {
            condition(
                &control.conditions[slot],
                output,
                target + 0x48 + slot * 8,
                inputs,
                input_count,
            )?;
        }
        if kind == 6 {
            let input = source.u32(from + 0x48)?;
            let named = sequence
                .inputs
                .get(input as usize)
                .context("sequence control input outside source table")?;
            ensure!(
                source.u32(from + 0x4C)? == named.name,
                "sequence control input name differs"
            );
            ensure!(
                source.u64(from + 0x50)? == 0,
                "unsupported sequence control extension"
            );
            let range = Range {
                offset: from + 0x40,
                input,
                lower: source.f32(from + 0x40)?,
                upper: source.f32(from + 0x44)?,
            }
            .native(inputs, input_count)?;
            put(output, target + 0x58, &range[4..])?;
            put(output, target + 0x64, &0u32.to_le_bytes())?;
        } else if kind == 2 {
            ensure!(
                source.u8(from + 30)? == 0,
                "weighted sequence control mode requires translation"
            );
            let weights = source.array(from + 0x40, 4, Some(0x8080000F))?;
            ensure!(
                !weights.is_empty() && weights.len() == control.children.len(),
                "sequence weight count differs from children"
            );
            let mut total = 0.0f32;
            let mut bytes = Vec::with_capacity(weights.len() * 4);
            for row in weights {
                let value = source.f32(row)?;
                ensure!(
                    value.is_finite() && value >= 0.0,
                    "invalid sequence flow weight"
                );
                total += value;
                bytes.extend_from_slice(&source.bytes::<4>(row)?);
            }
            ensure!(
                total.is_finite() && total > 0.0,
                "invalid sequence flow weight total"
            );
            array(output, target + 0x58, 0x8080000F, &bytes, 4)?;
        }
        allocation.children.push(Allocation::node(ic));
    }
    let state = vec![0xFF; count * 2];
    array(output, root + 0xE8, 0x8080000A, &state, 2)?;
    put(
        output,
        definition + 0x198,
        &u32::try_from(count)?.to_le_bytes(),
    )?;
    Ok(allocation)
}
