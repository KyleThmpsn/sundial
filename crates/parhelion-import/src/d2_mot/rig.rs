//! Resolve equipment runtime and skeleton data without installing it.
mod character;
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn attachment_rows(
    p: &Payload,
    resource: usize,
    content: u32,
    modern: bool,
) -> Result<Vec<usize>> {
    let (offset, stride, row_class) = if modern {
        (0x680, 0x128, 0x8080319B)
    } else {
        (0x588, 0xE8, 0x80803E72)
    };
    let mut selected = Vec::new();
    let mut family = Vec::new();
    for row in p.array(resource + offset, stride, Some(row_class))? {
        if p.u32(row)? == content {
            selected.push(row);
        } else if modern && p.u32(row + 0x28)? == content {
            family.push(row);
        }
    }
    // An exact variant selects its own attachment. Its key can also name the
    // family of sibling variants, whose animation profiles are not equivalent.
    if selected.is_empty() {
        selected = family;
    }
    // The embedded attachment precedes the variant table. An unmatched content
    // key uses it, including the empty-name selector on nonmodular weapons.
    if selected.is_empty() {
        Ok(vec![resource + offset - stride])
    } else {
        Ok(selected)
    }
}
fn patterns(r: &mut Reader, modern: bool) -> Result<Vec<(u32, u32)>> {
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
    rows.into_iter()
        .map(|row| Ok((table.u32(row + 4)?, table.u32(row + 16)?)))
        .collect()
}

fn entities(
    r: &mut Reader,
    modern: bool,
    keys: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, BTreeSet<u32>>> {
    let mut entities = BTreeMap::<u32, BTreeSet<u32>>::new();
    for map_tag in r.classes(if modern { 0x8080978C } else { 0x80809780 }) {
        let Ok(map) = r.tag(map_tag, None) else {
            continue;
        };
        for row in map.array(8, if modern { 24 } else { 8 }, None)? {
            let key = map.u32(row)?;
            if !keys.contains(&key) {
                continue;
            }
            entities.entry(key).or_default().insert(if modern {
                r.ref64(&map, row + 8)?
            } else {
                map.u32(row + 4)?
            });
        }
    }
    Ok(entities)
}

fn runtime(r: &mut Reader, entity: u32, content: u32, modern: bool) -> Result<Value> {
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
                let mut skeleton = decode(&p, resource, class, tag, modern)?;
                skeleton["entity"] = json!(format!("{entity:08X}"));
                skeletons.push(skeleton);
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
    Ok(
        json!({"runtime_entity":format!("{entity:08X}"),"components":components,"skeletons":skeletons,"installed":false}),
    )
}

pub fn inspect(r: &mut Reader, item_tag: u32, modern: bool) -> Result<Value> {
    let item = r.tag(item_tag, None)?;
    let translation = item.pointer(if modern { 0x70 } else { 0x88 })?;
    let index = item.u16(translation + 0x58)? as usize;
    let (key, content) = *patterns(r, modern)?.get(index).context("pattern index")?;
    let entities = entities(r, modern, &BTreeSet::from([key]))?;
    let selected = entities.get(&key).context("runtime entity missing")?;
    ensure!(selected.len() == 1, "runtime entity is ambiguous");
    let mut report = runtime(r, *selected.first().unwrap(), content, modern)?;
    report["item_tag"] = json!(format!("{item_tag:08X}"));
    report["pattern_index"] = json!(index);
    report["pattern_key"] = json!(format!("{key:08X}"));
    report["content_key"] = json!(format!("{content:08X}"));
    write_json(&r.output.join("rig.json"), &report)?;
    r.finish()?;
    Ok(report)
}

fn attachments(p: &Payload, resource: usize, content: u32, modern: bool) -> Result<Vec<usize>> {
    Ok(attachment_rows(p, resource, content, modern)?
        .into_iter()
        .map(|row| row + if modern { 0xA0 } else { 0x78 })
        .collect())
}

/// Resolve the selected weapon's audio after the lightweight rig inspection.
/// Donor matching intentionally calls `inspect` so it does not decode sound
/// groups for every candidate.
pub fn inspect_with_audio(r: &mut Reader, item_tag: u32, modern: bool) -> Result<Value> {
    let mut report = inspect(r, item_tag, modern)?;
    super::audio::add_to_rig(r, &mut report, modern)?;
    write_json(&r.output.join("rig.json"), &report)?;
    r.finish()?;
    Ok(report)
}

fn append_runtime(target: &mut Vec<Value>, rows: &Value) -> Result<()> {
    for row in rows.as_array().context("runtime rig entries")? {
        if target.iter().any(|entry| entry["owner"] == row["owner"]) {
            continue;
        }
        let mut row = row.clone();
        row["shared_runtime"] = json!(true);
        target.push(row);
    }
    Ok(())
}

/// Gear can bind to an art-local skeleton, an equipment runtime or the character palette.
/// Keep entity membership so independent variants cannot borrow each other's local rig.
pub(crate) fn art(r: &mut Reader, report: &Value, modern: bool) -> Result<Value> {
    let item_tag = u32::from_str_radix(report["item_tag"].as_str().context("art item tag")?, 16)?;
    let item = r.tag(item_tag, None)?;
    let translation = item.pointer(if modern { 0x70 } else { 0x88 })?;
    // Ghosts and vehicles bind their art to a separate equipment runtime. Its FK hierarchy
    // supplies the mesh palette even when the art entity has no skeleton component of its own.
    let bucket = item.u8(if modern { 0x98 } else { 0xB8 })?;
    let runtime = if (3..=7).contains(&bucket) {
        Some(character::inspect(r, modern)?)
    } else if item.u16(translation + 0x58)? != u16::MAX {
        Some(inspect(r, item_tag, modern)?)
    } else {
        None
    };
    let entities = report["models"]
        .as_array()
        .context("art models")?
        .iter()
        .map(|model| {
            Ok(u32::from_str_radix(
                model["entity"].as_str().context("art entity")?,
                16,
            )?)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let mut components = Vec::new();
    let mut skeletons = Vec::new();
    for entity in entities {
        let payload = r.tag(entity, Some(if modern { 0x80809AD8 } else { 0x80809C0F }))?;
        for row in payload.array(if modern { 8 } else { 16 }, 12, None)? {
            let tag = payload.u32(row)?;
            let owner = r.tag(tag, Some(if modern { 0x80809B06 } else { 0x80809C36 }))?;
            let resource = owner.pointer(24)?;
            let class = owner.u32(resource.checked_sub(4).context("art resource")?)?;
            components.push(
                json!({"entity":format!("{entity:08X}"),"owner":format!("{tag:08X}"),
                "class":format!("{class:08X}"),"resource":resource}),
            );
            if matches!(class, 0x808081D6 | 0x808081DE | 0x8080853E | 0x80808546) {
                let mut skeleton = decode(&owner, resource, class, tag, modern)?;
                skeleton["entity"] = json!(format!("{entity:08X}"));
                skeletons.push(skeleton);
            }
        }
    }
    if let Some(runtime) = &runtime {
        append_runtime(&mut components, &runtime["components"])?;
        append_runtime(&mut skeletons, &runtime["skeletons"])?;
    }
    let mut result = runtime.unwrap_or_else(|| json!({}));
    result["item_tag"] = report["item_tag"].clone();
    result["components"] = json!(components);
    result["skeletons"] = json!(skeletons);
    result["installed"] = json!(false);
    result["runtime_kind"] = json!(if (3..=7).contains(&bucket) {
        "character"
    } else {
        "equipment"
    });
    if result["runtime_kind"] == "equipment" && result["content_key"].is_string() {
        super::audio::add_to_rig(r, &mut result, modern)?;
    }
    write_json(&r.output.join("rig.json"), &result)?;
    Ok(result)
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
            let expected = vec![start + reference];
            assert_eq!(attachments(&p, 0, 7, modern).unwrap(), expected);
            assert_eq!(
                attachments(&p, 0, 9, modern).unwrap(),
                vec![descriptor - stride + reference]
            );
            p.0[header + 8..header + 12].fill(0);
            assert!(attachments(&p, 0, 7, modern).is_err());
        }
    }
}
