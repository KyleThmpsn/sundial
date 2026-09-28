//! Preserve art placement when resolving source model entities.
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Resolve assignments without losing their selector, slot or class art row.
/// Multiple assignments can share one entity while occupying different slots.
pub fn read(reader: &mut Reader, item: &Payload) -> Result<Vec<Value>> {
    let mut indices = BTreeSet::new();
    for row in item.array(item.pointer(0x70)?, 4, None)? {
        indices.insert(item.u16(row + 2)? as usize);
    }
    let mut placements = BTreeMap::<u32, Vec<Value>>::new();
    for tag in reader.classes(0x808055CE) {
        let table = reader.tag(tag, None)?;
        let rows = table.array(8, 32, None)?;
        for &index in &indices {
            let Some(&row) = rows.get(index) else {
                continue;
            };
            for (position, offset) in [8, 12].into_iter().enumerate() {
                placements
                    .entry(table.u32(row + offset)?)
                    .or_default()
                    .push(
                        json!({"table":format!("{tag:08X}"),"art_index":index,"single":position}),
                    );
            }
            for slot in table.array(row + 16, 8, None)? {
                let resource = table.pointer(slot)?;
                let selector = table.u64(resource)?;
                for (position, at) in table.array(resource + 8, 4, None)?.into_iter().enumerate() {
                    placements.entry(table.u32(at)?).or_default().push(
                        json!({"table":format!("{tag:08X}"),"art_index":index,"selector":selector,"position":position}),
                    );
                }
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
