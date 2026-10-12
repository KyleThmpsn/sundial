//! Private interaction text reached through the driver's and physical body's tables.
use super::assembly::Assembly;
use crate::{
    presentation::{Graph, append, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn relative(bytes: &mut [u8], field: usize, target: usize) -> Result<()> {
    put(
        bytes,
        field,
        &(i64::try_from(target)? - i64::try_from(field)?).to_le_bytes(),
    )
}

fn locale(source: &Payload, values: &BTreeMap<usize, String>, count: usize) -> Result<Payload> {
    let parts = source.array(8, 32, Some(0x80809a90))?;
    let bytes = source.array_range(0x38, 1, Some(0x80800005))?;
    let combos = source.array(0x48, 16, Some(0x80809a8e))?;
    let wide = source.array_range(0x28, 2, Some(0x80800006))?;
    ensure!(
        !parts.is_empty() && combos.len() == count,
        "Vehicle locale table differs"
    );
    let first = parts[0];
    let end = parts.last().context("Vehicle locale parts")? + 32;
    for &combo in &combos {
        let start = source.pointer(combo)?;
        let size = usize::try_from(source.u64(combo + 8)?)?
            .checked_mul(32)
            .context("Vehicle string part count overflow")?;
        ensure!(
            start >= first
                && (start - first).is_multiple_of(32)
                && start.checked_add(size).is_some_and(|finish| finish <= end),
            "Vehicle string selects invalid parts"
        );
    }
    let mut text = source.0[bytes.clone()].to_vec();
    let mut replacements = Vec::new();
    for (&index, value) in values {
        let combo = *combos.get(index).context("Vehicle prompt index")?;
        let part = source.pointer(combo)?;
        ensure!(source.u64(combo + 8)? > 0, "Vehicle prompt is empty");
        let template = source.bytes::<32>(part)?;
        replacements.push((index, text.len(), template, value));
        text.extend_from_slice(value.as_bytes());
        text.push(0);
    }
    let mut output = source.clone();
    append(&mut output.0, 0x38, 0x80800005, &text, 1)?;
    let new_bytes = output.array_range(0x38, 1, Some(0x80800005))?.start;
    let mut records = source.0[first..end].to_vec();
    for (_, _, template, _) in &replacements {
        records.extend_from_slice(template);
    }
    append(&mut output.0, 8, 0x80809a90, &records, 32)?;
    let new_parts = output.array(8, 32, Some(0x80809a90))?;
    for (&old, &new) in parts.iter().zip(&new_parts) {
        ensure!(
            source.u64(old)? == 0,
            "Vehicle string part extension differs"
        );
        let target = source.pointer(old + 8)?;
        let length = usize::from(source.u16(old + 0x14)?);
        let target = if length == 0 {
            new_bytes
        } else if target >= bytes.start
            && target.checked_add(length).is_some_and(|v| v <= bytes.end)
        {
            new_bytes + target - bytes.start
        } else {
            ensure!(
                target >= wide.start
                    && target
                        .checked_add(length * 2)
                        .is_some_and(|v| v <= wide.end),
                "Vehicle string data is outside its declared arrays"
            );
            target
        };
        relative(&mut output.0, new + 8, target)?;
    }
    for &combo in &combos {
        let index = (source.pointer(combo)? - first) / 32;
        relative(&mut output.0, combo, new_parts[0] + index * 32)?;
    }
    for (offset, (index, start, _, value)) in replacements.iter().enumerate() {
        let part = new_parts[parts.len() + offset];
        ensure!(
            output.u64(part)? == 0,
            "Vehicle prompt part extension differs"
        );
        relative(&mut output.0, part + 8, new_bytes + start)?;
        put(
            &mut output.0,
            part + 0x14,
            &u16::try_from(value.len())?.to_le_bytes(),
        )?;
        put(
            &mut output.0,
            part + 0x16,
            &u16::try_from(value.chars().count())?.to_le_bytes(),
        )?;
        put(&mut output.0, part + 0x18, &0u16.to_le_bytes())?;
        relative(&mut output.0, combos[*index], part)?;
        put(&mut output.0, combos[*index] + 8, &1u64.to_le_bytes())?;
    }
    Ok(output)
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    name: &str,
) -> Result<Value> {
    ensure!(
        !name.trim().is_empty() && !name.contains(['\r', '\n', '\0']),
        "Invalid vehicle name"
    );
    let driver = assembly.owner(0x808042ad)?;
    let physical = assembly.owner(0x80803d4c)?;
    let mut tables = BTreeMap::<u32, BTreeMap<u32, String>>::new();
    let mut fields = Vec::new();
    for (owner, class, header, hashes, verb) in [
        (driver, 0x80803d7d, 0x100, &[0xf0][..], "Drive"),
        (physical, 0x80803d65, 0x170, &[0x160, 0x178][..], "Flip"),
    ] {
        let payload = &assembly.payloads[&owner];
        let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
        ensure!(
            payload.u32(definition - 4)? == class,
            "Vehicle prompt definition differs"
        );
        let table = payload.u32(definition + header)?;
        for &hash in hashes {
            let hash = payload.u32(definition + hash)?;
            let value = format!("{verb} {name}");
            let previous = tables.entry(table).or_default().insert(hash, value.clone());
            ensure!(
                previous.is_none_or(|old| old == value),
                "Conflicting vehicle prompts"
            );
        }
        fields.push((owner, definition + header, table));
    }
    let mut report = Vec::new();
    for (table, values) in tables {
        let mut header = (*native.tag(table, Some(0x80809a88))?).clone();
        let hashes = header.array(8, 4, Some(0x80800070))?;
        let mut selected = BTreeMap::new();
        for (hash, value) in &values {
            let found = hashes
                .iter()
                .enumerate()
                .filter_map(|(index, &row)| (header.u32(row).ok() == Some(*hash)).then_some(index))
                .collect::<Vec<_>>();
            ensure!(
                found.len() == 1,
                "Vehicle prompt hash is absent or ambiguous"
            );
            selected.insert(found[0], value.clone());
        }
        let symbol = format!("vehicle-prompts-{table:08X}");
        let mut patches = Vec::new();
        for language in 0..13 {
            let field = 24 + language * 4;
            let tag = header.u32(field)?;
            let source = native.tag(tag, Some(0x80809a8a))?;
            let output = locale(&source, &selected, hashes.len())?;
            let target = format!("{symbol}-{language}");
            graph.add(&target, tag, &output.0, None, Vec::new())?;
            put(&mut header.0, field, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":field,"symbol":target}));
        }
        graph.add(&symbol, table, &header.0, None, patches)?;
        for &(owner, offset, source) in &fields {
            if source == table {
                assembly.resource(owner, offset, symbol.clone())?;
            }
        }
        report.push(json!({"source_table":format!("{table:08X}"),"strings":values,"locales":13}));
    }
    Ok(json!({"tables":report,"scope":"Private vehicle interaction"}))
}
