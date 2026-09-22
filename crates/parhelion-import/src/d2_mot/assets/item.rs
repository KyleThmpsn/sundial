//! Shared inventory-art lookup; no weapon-pattern or combat assumptions.
use crate::d2_mot::reader::Reader;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;
pub fn find(r: &mut Reader, hash: u32) -> Result<(usize, u32)> {
    let mut matches = vec![];
    for t in r.classes(0x80807997) {
        let p = r.tag(t, None)?;
        for (i, row) in p.array(8, 32, None)?.into_iter().enumerate() {
            if p.u32(row)? == hash {
                matches.push((i, r.ref64(&p, row + 16)?));
            }
        }
    }
    ensure!(matches.len() == 1, "item {hash:08X} missing or ambiguous");
    Ok(matches[0])
}
pub fn presentation(r: &mut Reader, hash: u32) -> Result<Value> {
    let (index, tag) = find(r, hash)?;
    let localized = crate::d2_mot::localization::item_name(r, hash, index, 0)?;
    let icon = match crate::d2_mot::icon::export(r, hash, index) {
        Ok((texture, w, h)) => {
            json!({"path":"item-icon.png","texture":format!("{texture:08X}"),"size":[w,h]})
        }
        Err(error) => json!({"status":"blocked","reason":format!("{error:#}")}),
    };
    let item = r.tag(tag, Some(0x8080799D))?;
    let root = item.pointer(0x70)?;
    let mut art = BTreeSet::new();
    for row in item.array(root, 4, None)? {
        art.insert(item.u16(row + 2)? as usize);
    }
    let mut keys = BTreeSet::new();
    for t in r.classes(0x808055CE) {
        let p = r.tag(t, None)?;
        let rows = p.array(8, 32, None)?;
        for &i in &art {
            let row = *rows.get(i).context("art index outside table")?;
            keys.insert(p.u32(row + 8)?);
            keys.insert(p.u32(row + 12)?);
            for a in p.array(row + 16, 8, None)? {
                let resource = p.pointer(a)?;
                for b in p.array(resource + 8, 4, None)? {
                    keys.insert(p.u32(b)?);
                }
            }
        }
    }
    keys.retain(|v| ![0, u32::MAX, 0x811C9DC5].contains(v));
    let mut entities = BTreeSet::new();
    for t in r.classes(0x80804F43) {
        let p = r.tag(t, None)?;
        for row in p.array(8, 8, None)? {
            if keys.contains(&p.u32(row)?) {
                let parent = r.tag(p.u32(row + 4)?, Some(0x80806FA3))?;
                let entity = r.ref64(&parent, 8)?;
                if ![0, u32::MAX, 0x811C9DC5].contains(&entity) {
                    entities.insert(entity);
                }
            }
        }
    }
    Ok(
        json!({"item_hash":hash,"item_tag":format!("{tag:08X}"),"item_index":index,"name":localized["name"],"localization":localized,"icon":icon,"art_indices":art,"assignment_keys":keys,"entities":entities}),
    )
}
