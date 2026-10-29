//! Extend inherited values while preserving local programs and declaration ordinals.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

const NAMESPACE: u32 = 0x811c9dc5;
type Name = (u32, u32, u32);

/// Aim visibility. Stock optic banks `81532E86` and `81532F95` declare local `79FD498A` as parent
/// `43A361CE` under namespace `5F6317D5`. Shadowkeep reticles read it to appear once aiming
/// completes.
pub(crate) const AIM_VISIBILITY: u32 = 0x79fd498a;

/// The lookup namespace and parent channel a newly inherited local reads. Other locals take the
/// parent channel of their own name in the empty namespace.
fn parent(local: u32) -> (u32, u32) {
    if local == AIM_VISIBILITY {
        (0x5f6317d5, 0x43a361ce)
    } else {
        (NAMESPACE, local)
    }
}

fn lookup(original: &Payload, definition: usize, rows: &[usize]) -> Result<Vec<Name>> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for (slot, at) in original
        .array(definition + 0x108, 12, Some(0x808097a0))?
        .into_iter()
        .enumerate()
    {
        let name = (
            original.u32(at)?,
            original.u32(at + 4)?,
            original.u32(at + 8)?,
        );
        let row = *rows.get(name.2 as usize).context("Inherited declaration")?;
        ensure!(
            original.u16(row + 72)? as usize == slot && seen.insert(name.2),
            "Inherited channel slot differs from its name lookup"
        );
        result.push(name);
    }
    ensure!(
        result
            .windows(2)
            .all(|p| (p[0].0, p[0].1) < (p[1].0, p[1].1)),
        "Inherited channel names are not unique and sorted"
    );
    Ok(result)
}

fn span(names: &[Name]) -> Result<[u16; 2]> {
    default_span(names.iter().map(|name| name.0))
}

/// The first and last rows of the sorted inherited lookup in the default namespace, or `FFFF`
/// for both when it has none, as definition `+128/+12A` records them. Named namespaces sort
/// first, so the span starts after them, as in stock runtime bank `815226A0` (rows 7 to 23).
pub(crate) fn default_span(namespaces: impl IntoIterator<Item = u32>) -> Result<[u16; 2]> {
    let indices = namespaces
        .into_iter()
        .enumerate()
        .filter_map(|(i, namespace)| (namespace == NAMESPACE).then_some(i))
        .collect::<Vec<_>>();
    Ok(match (indices.first(), indices.last()) {
        (Some(&first), Some(&last)) => [u16::try_from(first)?, u16::try_from(last)?],
        _ => [u16::MAX; 2],
    })
}

fn mask(original: &Payload, field: usize, before: usize, after: usize) -> Result<Vec<u32>> {
    let rows = original.array(field, 4, Some(0x8080000b))?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(
        rows.len() == before.div_ceil(32),
        "Inherited channel dependency width differs"
    );
    let mut words = rows
        .into_iter()
        .map(|at| original.u32(at))
        .collect::<Result<Vec<_>>>()?;
    words.resize(after.div_ceil(32), 0);
    Ok(words)
}

fn write_mask(p: &mut Payload, field: usize, words: Vec<u32>) -> Result<()> {
    append(
        &mut p.0,
        field,
        0x8080000b,
        &words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
        4,
    )
}

fn declarations(
    original: &Payload,
    p: &mut Payload,
    definition: usize,
    rows: &[usize],
    order: &[u32],
    names: &[Name],
) -> Result<()> {
    let mut data = rows
        .iter()
        .flat_map(|&at| original.0[at..at + 112].iter().copied())
        .collect::<Vec<_>>();
    data.resize(order.len() * 112, 0);
    append(&mut p.0, definition + 0xd8, 0x808097a1, &data, 112)?;
    let written = p.array(definition + 0xd8, 112, Some(0x808097a1))?;
    for (ordinal, &row) in written.iter().enumerate() {
        let dependencies = if let Some(&old) = rows.get(ordinal) {
            // Bytecode, constants, program indices and interpolation resources
            // stay at their original addresses. No declaration ordinal changes.
            for field in [16, 32, 64, 88, 96] {
                if original.u64(old + field)? != 0 {
                    let target = original.pointer(old + field)?;
                    put(
                        &mut p.0,
                        row + field,
                        &(target as i64 - row as i64 - field as i64).to_le_bytes(),
                    )?;
                }
            }
            mask(original, old + 80, rows.len(), order.len())?
        } else {
            put(&mut p.0, row, &order[ordinal].to_le_bytes())?;
            put(&mut p.0, row + 104, &[0xff, 0xfe, 0xff, 0xff])?;
            let mut words = vec![0; order.len().div_ceil(32)];
            words[ordinal / 32] = 1 << (ordinal % 32);
            words
        };
        write_mask(p, row + 80, dependencies)?;
    }
    for (slot, &(_, _, ordinal)) in names.iter().enumerate() {
        put(
            &mut p.0,
            written[ordinal as usize] + 72,
            &u16::try_from(slot)?.to_le_bytes(),
        )?;
    }
    Ok(())
}

fn storage(
    original: &Payload,
    p: &mut Payload,
    allocation: &mut Payload,
    outputs: [usize; 2],
    inherited: [usize; 2],
) -> Result<()> {
    let instance = original.pointer(16)?;
    let records = allocation.array(0x20, 40, Some(0x80808852))?;
    for (field, class, name, counts, initial) in [
        (0x50, 0x80800090, 0xfc2f3d6f, outputs, 0),
        (0x90, 0x8080979c, 0x3a55a801, inherited, 0xff),
    ] {
        let rows = original.array(instance + field, 16, Some(class))?;
        ensure!(
            rows.len() == counts[0]
                && rows
                    .iter()
                    .all(|&at| original.0[at..at + 16].iter().all(|&v| v == initial)),
            "Inherited channel initial storage differs"
        );
        append(
            &mut p.0,
            instance + field,
            u64::from(class),
            &vec![initial; counts[1] * 16],
            16,
        )?;
        grow(allocation, &records, name, class, counts)?;
    }
    let before = original.array(instance + 0x40, 4, Some(0x8080000b))?.len();
    let mut pending = mask(original, instance + 0x40, outputs[0], outputs[1])?;
    if !pending.is_empty() {
        for ordinal in outputs[0]..outputs[1] {
            pending[ordinal / 32] |= 1 << (ordinal % 32);
        }
    }
    let after = pending.len();
    write_mask(p, instance + 0x40, pending)?;
    grow(
        allocation,
        &records,
        0x4a423762,
        0x8080000b,
        [before, after],
    )
}

fn grow(p: &mut Payload, rows: &[usize], name: u32, class: u32, counts: [usize; 2]) -> Result<()> {
    let selected = rows
        .iter()
        .copied()
        .filter(|&at| p.u32(at).ok() == Some(name))
        .collect::<Vec<_>>();
    let [row] = selected.as_slice() else {
        anyhow::bail!("Inherited channel allocation is absent or ambiguous");
    };
    ensure!(
        p.u32(row + 16)? == class
            && p.u32(row + 20)? as usize == counts[0]
            && p.u64(row + 24)? == 0,
        "Inherited channel allocation differs"
    );
    put(&mut p.0, row + 20, &u32::try_from(counts[1])?.to_le_bytes())
}

pub(crate) fn extend(
    original: &Payload,
    allocation: &Payload,
    requested: &[u32],
) -> Result<(Payload, Payload)> {
    let instance = original.pointer(16)?;
    let definition = original.pointer(24)?;
    ensure!(
        original.u32(instance - 4)? == 0x8080979f
            && original.u32(definition - 4)? == 0x80809790
            && original.u64(instance + 8)? == definition as u64,
        "Inherited channel bank roots differ"
    );
    let rows = original.array(definition + 0xd8, 112, Some(0x808097a1))?;
    let mut order = rows
        .iter()
        .map(|&at| original.u32(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        order.iter().copied().collect::<BTreeSet<_>>().len() == order.len(),
        "Channel names repeat"
    );
    let mut names = lookup(original, definition, &rows)?;
    let old_names = names.len();
    ensure!(
        span(&names)?
            == [
                original.u16(definition + 0x128)?,
                original.u16(definition + 0x12a)?
            ],
        "Inherited default namespace span differs"
    );
    for &name in requested {
        if !order.contains(&name) {
            let (namespace, parent) = parent(name);
            ensure!(
                !names
                    .iter()
                    .any(|row| (row.0, row.1) == (namespace, parent)),
                "Inherited channel lookup already names the parent of {name:08X}"
            );
            names.push((namespace, parent, u32::try_from(order.len())?));
            order.push(name);
        }
    }
    ensure!(
        order.len() > rows.len() && order.len() <= u16::MAX as usize,
        "Inherited channel extension has no supported additions"
    );
    names.sort_unstable();
    let mut p = original.clone();
    let mut allocation = allocation.clone();
    declarations(original, &mut p, definition, &rows, &order, &names)?;
    storage(
        original,
        &mut p,
        &mut allocation,
        [rows.len(), order.len()],
        [old_names, names.len()],
    )?;
    let bytes = names
        .iter()
        .flat_map(|&(a, b, c)| [a, b, c])
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    append(&mut p.0, definition + 0x108, 0x808097a0, &bytes, 12)?;
    for (field, before, after) in [
        (0x130, rows.len().div_ceil(32), order.len().div_ceil(32)),
        (0x134, rows.len(), order.len()),
        (0x140, old_names, names.len()),
    ] {
        ensure!(
            original.u32(definition + field)? as usize == before,
            "Inherited channel schema count differs"
        );
        put(
            &mut p.0,
            definition + field,
            &u32::try_from(after)?.to_le_bytes(),
        )?;
    }
    for (field, value) in [0x128, 0x12a].into_iter().zip(span(&names)?) {
        put(&mut p.0, definition + field, &value.to_le_bytes())?;
    }
    Ok((p, allocation))
}
