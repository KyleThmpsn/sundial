//! Preserve art placement when resolving source model entities.
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Every authored class row, including empty and alternative body placements.
pub fn rows(reader: &mut Reader, item: &Payload) -> Result<Vec<Value>> {
    let tags = reader.classes(0x808055CE);
    ensure!(tags.len() == 1, "ambiguous source art metadata");
    let tag = tags[0];
    let metadata = reader.tag(tag, Some(0x808055CE))?;
    let entries = metadata.array(8, 32, None)?;
    item.array(item.pointer(0x70)?, 4, Some(0x8080737D))?
        .into_iter()
        .enumerate()
        .map(|(ordinal, row)| {
            let index = item.u16(row + 2)? as usize;
            let at = *entries
                .get(index)
                .context("source art index outside metadata")?;
            let mut slots = Vec::new();
            for slot in metadata.array(at + 16, 8, None)? {
                let resource = metadata.pointer(slot)?;
                let keys = metadata
                    .array(resource + 8, 4, None)?
                    .into_iter()
                    .map(|at| metadata.u32(at))
                    .collect::<Result<Vec<_>>>()?;
                slots.push(json!({"selector":metadata.u64(resource)?,"assignments":keys}));
            }
            Ok(
                json!({"ordinal":ordinal,"class":item.u8(row)? as i8,"flags":item.u8(row+1)?,
                "art_index":index,"table":format!("{tag:08X}"),
                "singles":[metadata.u32(at+8)?,metadata.u32(at+12)?],"slots":slots}),
            )
        })
        .collect()
}

/// Resolve assignments without losing their selector, slot or class art row.
/// Multiple assignments can share one entity while occupying different slots.
pub fn read(reader: &mut Reader, item: &Payload) -> Result<Vec<Value>> {
    let mut placements = BTreeMap::<u32, Vec<Value>>::new();
    for row in rows(reader, item)? {
        for (position, key) in row["singles"]
            .as_array()
            .context("art singles")?
            .iter()
            .enumerate()
        {
            placements
                    .entry(u32::try_from(key.as_u64().context("art key")?)?)
                    .or_default()
                    .push(
                        json!({"table":row["table"],"art_index":row["art_index"],"art_row":row["ordinal"],"class":row["class"],"single":position}),
                    );
        }
        for slot in row["slots"].as_array().context("art slots")? {
            for (position, key) in slot["assignments"]
                .as_array()
                .context("art assignments")?
                .iter()
                .enumerate()
            {
                placements.entry(u32::try_from(key.as_u64().context("art key")?)?).or_default().push(
                        json!({"table":row["table"],"art_index":row["art_index"],"art_row":row["ordinal"],"class":row["class"],"selector":slot["selector"],"position":position}),
                    );
            }
        }
    }
    for sentinel in [0, u32::MAX, super::EMPTY] {
        placements.remove(&sentinel);
    }
    let mut parts = Vec::new();
    let mut seen = BTreeSet::new();
    for tag in reader.classes(0x80804F43) {
        let table = reader.tag(tag, None)?;
        for row in table.array(8, 8, None)? {
            let assignment = table.u32(row)?;
            let Some(placement) = placements.get(&assignment) else {
                continue;
            };
            let parent_tag = table.u32(row + 4)?;
            if !seen.insert((assignment, parent_tag)) {
                continue;
            }
            let parent = reader.tag(parent_tag, Some(0x80806FA3))?;
            let entity = reader.ref64(&parent, 8)?;
            let missing = [0, u32::MAX, super::EMPTY].contains(&entity);
            parts.push(json!({
                "assignment":format!("{assignment:08X}"),
                "parent":format!("{parent_tag:08X}"),
                "entity":format!("{entity:08X}"),
                "entity_class":if missing { Value::Null } else { json!(format!("{:08X}",reader.reference(entity)?)) },
                "missing":missing,
                "placements":placement,
            }));
        }
    }
    Ok(parts)
}
