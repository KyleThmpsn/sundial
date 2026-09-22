//! Resolve modern weapon runtime and skeleton data without installing it.
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn attachments(p: &Payload, resource: usize, content: u32, modern: bool) -> Result<Vec<usize>> {
    let (offset, stride, row_class, reference) = if modern {
        (0x680, 0x128, 0x8080319B, 0xA0)
    } else {
        (0x588, 0xE8, 0x80803E72, 0x78)
    };
    p.array(resource + offset, stride, Some(row_class))?
        .into_iter()
        .filter_map(|row| {
            let matches = (|| -> Result<bool> {
                Ok(p.u32(row)? == content || (modern && p.u32(row + 0x28)? == content))
            })();
            match matches {
                Ok(true) => Some(Ok(row + reference)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}
#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub fn inspect(r: &mut Reader, item_tag: u32, modern: bool) -> Result<Value> {
    let item = r.tag(item_tag, None)?;
    let translation = item.pointer(if modern { 0x70 } else { 0x88 })?;
    let index = item.u16(translation + 0x58)? as usize;
    let tables = if modern {
        r.classes(0x808052AA)
    } else {
        let tag = r
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|e| e.name == "investment_globals")
            .context("globals")?
            .hash
            .0;
        let globals = r.tag(tag, None)?;
        vec![globals.u32(16 + 70 * 16)?]
    };
    ensure!(tables.len() == 1, "ambiguous pattern tables");
    let table = r.tag(tables[0], None)?;
    let rows = table.array(8, 48, Some(if modern { 0x808052AE } else { 0x80805B7C }))?;
    let row = *rows.get(index).context("pattern index")?;
    let key = table.u32(row + 4)?;
    let content = table.u32(row + 16)?;
    let mut entities = BTreeSet::new();
    for map_tag in r.classes(if modern { 0x8080978C } else { 0x80809780 }) {
        let Ok(map) = r.tag(map_tag, None) else {
            continue;
        };
        for row in map.array(8, if modern { 24 } else { 8 }, None)? {
            if map.u32(row)? == key {
                entities.insert(if modern {
                    r.ref64(&map, row + 8)?
                } else {
                    map.u32(row + 4)?
                });
            }
        }
    }
    ensure!(entities.len() == 1, "runtime entity missing or ambiguous");
    let entity = *entities.first().unwrap();
    let mut pending = vec![entity];
    let mut visited = BTreeSet::new();
    let mut components = vec![];
    let mut skeletons = vec![];
    while let Some(entity) = pending.pop() {
        if !visited.insert(entity) {
            continue;
        }
        ensure!(visited.len() <= 16, "unexpected skeleton entity recursion");
        let e = r.tag(entity, Some(if modern { 0x80809AD8 } else { 0x80809C0F }))?;
        for row in e.array(if modern { 8 } else { 16 }, 12, None)? {
            let tag = e.u32(row)?;
            let p = r.tag(tag, Some(if modern { 0x80809B06 } else { 0x80809C36 }))?;
            let resource = p.pointer(24)?;
            let class = p.u32(resource - 4)?;
            components.push(json!({"entity":format!("{entity:08X}"),"owner":format!("{tag:08X}"),"class":format!("{class:08X}"),"resource":resource}));
            if [0x808081D6, 0x808081DE, 0x8080853E, 0x80808546].contains(&class) {
                skeletons.push(decode(&p, resource, class, tag, modern)?);
            }
            if class == if modern { 0x8080356E } else { 0x80804221 } {
                for reference in attachments(&p, resource, content, modern)? {
                    let target = if modern {
                        r.ref64(&p, reference)?
                    } else {
                        p.u32(reference)?
                    };
                    if ![0, u32::MAX, 0x811C9DC5].contains(&target) {
                        pending.push(target);
                    }
                }
            }
        }
    }
    let report = json!({"item_tag":format!("{item_tag:08X}"),"pattern_index":index,"pattern_key":format!("{key:08X}"),"content_key":format!("{content:08X}"),"runtime_entity":format!("{entity:08X}"),"components":components,"skeletons":skeletons,"installed":false});
    write_json(&r.output.join("rig.json"), &report)?;
    r.finish()?;
    Ok(report)
}

pub(crate) fn decode(
    p: &Payload,
    resource: usize,
    class: u32,
    tag: u32,
    modern: bool,
) -> Result<Value> {
    let base = if modern { 0x90 } else { 0x80 };
    let nodes = p.array(
        resource + base,
        16,
        Some(if modern { 0x80808642 } else { 0x80808A08 }),
    )?;
    let inverse_offset = base
        + if [0x808081DE, 0x80808546].contains(&class) {
            0x20
        } else {
            0x10
        };
    let inverse = p.array(resource + inverse_offset, 32, None)?;
    ensure!(
        nodes.len() == inverse.len(),
        "skeleton transform count mismatch"
    );
    let mut bones = vec![];
    for (index, (&row, &transform)) in nodes.iter().zip(&inverse).enumerate() {
        let parent = p.u32(row + 4)? as i32;
        ensure!(
            parent == -1 || (parent >= 0 && parent < index as i32),
            "skeleton not in parent-first order"
        );
        let values = (0..8)
            .map(|i| p.f32(transform + i * 4))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            values.iter().all(|x| x.is_finite()),
            "invalid skeleton transform"
        );
        bones.push(json!({"index":index,"name_hash":format!("{:08X}",p.u32(row)?),"parent":parent,"inverse_object_transform":values}));
    }
    Ok(json!({"owner":format!("{tag:08X}"),"class":format!("{class:08X}"),"bones":bones}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_person_attachment_rows_use_the_versioned_content_and_reference_fields() {
        for modern in [true, false] {
            let (descriptor, stride, class, reference) = if modern {
                (0x680, 0x128, 0x8080319Bu32, 0xA0)
            } else {
                (0x588, 0xE8, 0x80803E72, 0x78)
            };
            let header = 0x700;
            let start = header + 16;
            let mut p = Payload(vec![0; start + 3 * stride]);
            p.0[descriptor..descriptor + 8].copy_from_slice(&3u64.to_le_bytes());
            p.0[descriptor + 8..descriptor + 16]
                .copy_from_slice(&((header - descriptor - 8) as u64).to_le_bytes());
            p.0[header..header + 8].copy_from_slice(&3u64.to_le_bytes());
            p.0[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
            p.0[start..start + 4].copy_from_slice(&7u32.to_le_bytes());
            p.0[start + stride + 0x28..start + stride + 0x2C].copy_from_slice(&7u32.to_le_bytes());
            let expected = if modern {
                vec![start + reference, start + stride + reference]
            } else {
                vec![start + reference]
            };
            assert_eq!(attachments(&p, 0, 7, modern).unwrap(), expected);
            assert!(attachments(&p, 0, 9, modern).unwrap().is_empty());
            p.0[header + 8..header + 12].fill(0);
            assert!(attachments(&p, 0, 7, modern).is_err());
        }
    }
}
