//! Add diffuse and glow companions without duplicating a rigid surface's vertex or index data.
use super::Encoded;
use crate::{
    presentation::{append as append_array, put},
    tiger::{draws, payload::Payload},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn record(
    source: &Payload,
    row: usize,
    index: usize,
    binding: &Value,
    companion: Option<usize>,
) -> Result<([u8; 32], Value)> {
    let mut record = source.bytes::<32>(row)?;
    put(
        &mut record,
        draws::DRAW_INDEX,
        &u16::try_from(index)?.to_le_bytes(),
    )?;
    let mut patch = binding.clone();
    patch["offset"] = json!(index * 32);
    if let Some(stage) = companion {
        if stage == 7 {
            put(&mut record, 24, &21u16.to_le_bytes())?;
        }
        let name = patch["symbol"]
            .as_str()
            .context("Lighting source material name")?;
        let suffix = if stage == 4 { "baked" } else { "glow" };
        patch["symbol"] = json!(format!("{name}-{suffix}"));
    }
    Ok((record, patch))
}

/// Preserve the surface's activation group, detail category and index span. The model keeps one
/// ordered range per native stage and a fresh model-wide draw index for every resulting record.
pub(crate) fn append(
    encoded: &mut Encoded,
    lights: &BTreeSet<u32>,
    glows: &BTreeSet<u32>,
) -> Result<()> {
    if lights.is_empty() && glows.is_empty() {
        return Ok(());
    }
    let source = Payload(encoded.header.clone());
    let meshes = source.array(0x10, 136, Some(0x8080_7378))?;
    ensure!(
        meshes == [0xB0],
        "Lighting companions expect one authored rigid mesh"
    );
    let rows = source.array(0xC8, 32, Some(0x8080_737E))?;
    let ranges = (0..24)
        .map(|index| source.u16(0xD8 + index * 2).map(usize::from))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        ranges.first() == Some(&0)
            && ranges.last() == Some(&rows.len())
            && ranges.windows(2).all(|pair| pair[0] <= pair[1]),
        "Lighting companion source stage ranges differ"
    );
    let selected = lights
        .union(glows)
        .map(|key| format!("material-{key:08X}"))
        .collect::<BTreeSet<_>>();
    let mut bindings = BTreeMap::new();
    for patch in &encoded.patches {
        let offset = patch["offset"]
            .as_u64()
            .context("Rigid material patch offset")? as usize;
        if rows.contains(&offset) {
            ensure!(
                bindings.insert(offset, patch.clone()).is_none(),
                "Duplicate rigid material patch"
            );
        }
    }
    ensure!(
        bindings.len() == rows.len(),
        "Rigid draw has no material binding"
    );
    let opaque = rows[ranges[0]..ranges[1]]
        .iter()
        .filter(|&&row| {
            bindings[&row]["symbol"]
                .as_str()
                .is_some_and(|symbol| selected.contains(symbol))
        })
        .copied()
        .collect::<Vec<_>>();
    if opaque.is_empty() {
        return Ok(());
    }
    let mut records = Vec::new();
    let mut patches = Vec::<Value>::new();
    let mut new_ranges = vec![0u16];
    for stage in 0..23 {
        for (row, companion) in rows[ranges[stage]..ranges[stage + 1]]
            .iter()
            .copied()
            .map(|row| (row, false))
            .chain(
                opaque
                    .iter()
                    .copied()
                    .filter(|row| {
                        let keys = match stage {
                            4 => lights,
                            7 => glows,
                            _ => return false,
                        };
                        bindings[row]["symbol"].as_str().is_some_and(|symbol| {
                            keys.iter()
                                .any(|key| symbol == format!("material-{key:08X}"))
                        })
                    })
                    .map(|row| (row, true)),
            )
        {
            let (record, patch) = record(
                &source,
                row,
                records.len() / 32,
                &bindings[&row],
                companion.then_some(stage),
            )?;
            records.extend(record);
            patches.push(patch);
        }
        new_ranges.push(u16::try_from(records.len() / 32)?);
    }
    append_array(&mut encoded.header, 0xC8, 0x8080_737E, &records, 32)?;
    let header = Payload(encoded.header.clone());
    let first = *header
        .array(0xC8, 32, Some(0x8080_737E))?
        .first()
        .context("Empty lighting draw table")?;
    encoded.patches.retain(|patch| {
        !patch["offset"]
            .as_u64()
            .is_some_and(|offset| bindings.contains_key(&(offset as usize)))
    });
    for mut patch in patches {
        let relative = patch["offset"]
            .as_u64()
            .context("Lighting relative material offset")?;
        patch["offset"] = json!(first as u64 + relative);
        encoded.patches.push(patch);
    }
    for (index, range) in new_ranges.iter().enumerate() {
        put(&mut encoded.header, 0xD8 + index * 2, &range.to_le_bytes())?;
    }
    // The companion uses the same rigid vertex layout as its opaque source.
    for stage in [4, 7] {
        put(
            &mut encoded.header,
            0x108 + stage * 2,
            &source.u16(0x108)?.to_le_bytes(),
        )?;
    }
    draws::declare_draw_indices(&mut encoded.header, records.len() / 32)
}
