use crate::d2_mot::reader::Reader;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
/// Read a native model with the buffers and materials a rendering carrier
/// needs, exporting every tag it touches through the reader.
fn meshes(r: &mut Reader, model_tag: u32) -> Result<Vec<Value>> {
    let model = r.tag(model_tag, Some(0x808073A5))?;
    let mut meshes = vec![];
    for mesh in model.array(16, 0x88, Some(0x80807378))? {
        let mut buffers = vec![];
        for offset in [0, 4, 8, 16] {
            let bt = model.u32(mesh + offset)?;
            if [0, u32::MAX].contains(&bt) {
                continue;
            }
            let header = r.tag(bt, None)?;
            r.tag(r.reference(bt)?, None)?;
            buffers.push(json!({"offset":offset,"header":format!("{bt:08X}"),"bytes":hex::encode(&header.0)}));
        }
        let parts = model.array(mesh + 24, 32, Some(0x8080737E))?;
        let materials = parts
            .iter()
            .map(|&p| model.u32(p))
            .collect::<Result<BTreeSet<_>>>()?;
        for &mat in &materials {
            if mat != u32::MAX {
                r.tag(mat, None)?;
            }
        }
        meshes.push(json!({"offset":mesh,"parts":parts.len(),"materials":materials.iter().map(|m|format!("{m:08X}")).collect::<Vec<_>>(),"buffers":buffers,"record":hex::encode(&model.0[mesh..mesh+0x88])}));
    }
    Ok(meshes)
}

/// Extract one native model to serve as an extra rendering template. This
/// supplies draw records and material shells only: the gameplay item and the
/// animation donor are untouched.
pub fn carrier_model(r: &mut Reader, model_tag: u32) -> Result<Value> {
    let meshes = meshes(r, model_tag)?;
    ensure!(!meshes.is_empty(), "native carrier model has no meshes");
    Ok(json!({"model":format!("{model_tag:08X}"),"meshes":meshes,"rendering_template_only":true}))
}

pub fn extract(r: &mut Reader, item_hash: u32) -> Result<Value> {
    let named = r
        .manager
        .lookup
        .named_tags
        .iter()
        .map(|e| (e.name.clone(), e.hash.0))
        .collect::<BTreeMap<_, _>>();
    let globals = r.tag(
        *named.get("investment_globals").context("missing globals")?,
        None,
    )?;
    let root = r.tag(globals.u32(16)?, Some(0x80807D84))?;
    let items = r.tag(root.u32(8 + 48 * 16)?, None)?;
    let mut matches = vec![];
    for row in items.array(8, 24, Some(0x80807BE8))? {
        if items.u32(row)? == item_hash {
            matches.push(row)
        }
    }
    ensure!(matches.len() == 1, "native item missing or ambiguous");
    let item = r.tag(items.u32(matches[0] + 16)?, Some(0x80807BEA))?;
    let translation = item.pointer(0x88)?;
    let indices = item
        .array(translation, 4, Some(0x808077B5))?
        .iter()
        .map(|&row| item.u16(row + 2))
        .collect::<Result<Vec<_>>>()?;
    let metadata = r.tag(globals.u32(16 + 66 * 16)?, Some(0x80805DF5))?;
    let rows = metadata.array(8, 32, Some(0x80805DFB))?;
    let mut keys = BTreeSet::new();
    // Where each assignment key sits in the art row: a direct single or a
    // selector slot position. Selector 0 is the body the imported model replaces.
    let mut placements = BTreeMap::new();
    for &i in &indices {
        let row = *rows.get(i as usize).context("invalid art index")?;
        for (position, offset) in [8usize, 12].into_iter().enumerate() {
            let key = metadata.u32(row + offset)?;
            keys.insert(key);
            placements
                .entry(key)
                .or_insert_with(|| json!({"single": position}));
        }
        for a in metadata.array(row + 16, 8, None)? {
            let res = metadata.pointer(a)?;
            let selector = metadata.u64(res)?;
            for (position, b) in metadata.array(res + 8, 4, None)?.into_iter().enumerate() {
                let key = metadata.u32(b)?;
                keys.insert(key);
                placements
                    .entry(key)
                    .or_insert_with(|| json!({"selector": selector, "position": position}));
            }
        }
    }
    for sentinel in [0, u32::MAX, 0x811C9DC5] {
        keys.remove(&sentinel);
    }
    let assets = r.tag(
        *named.get("investment_assets").context("missing assets")?,
        None,
    )?;
    let table = r.tag(assets.u32(0x20)?, Some(0x808056EA))?;
    let mut parents = vec![];
    let mut models = vec![];
    for row in table.array(8, 8, None)? {
        if !keys.contains(&table.u32(row)?) {
            continue;
        }
        let tag = table.u32(row + 4)?;
        let parent = r.tag(tag, None)?;
        let key = table.u32(row)?;
        parents.push(json!({"assignment":format!("{key:08X}"),"parent":format!("{tag:08X}"),"parent_class":format!("{:08X}",r.reference(tag)?),"parent_bytes":hex::encode(&parent.0),"placement":placements.get(&key).cloned().unwrap_or(Value::Null)}));
        let entity_tag = parent.u32(16)?;
        if entity_tag == u32::MAX {
            continue;
        }
        let entity = r.tag(entity_tag, Some(0x80809C0F))?;
        for component in entity.array(16, 12, Some(0x80809C04))? {
            let owner_tag = entity.u32(component)?;
            let owner = r.tag(owner_tag, Some(0x80809C36))?;
            let resource = owner.pointer(24)?;
            if owner.u32(resource.checked_sub(4).context("invalid native resource")?)? != 0x808072BD
            {
                continue;
            }
            let model_tag = owner.u32(resource + 0x1dc)?;
            let meshes = meshes(r, model_tag)?;
            models.push(json!({"entity":format!("{entity_tag:08X}"),"owner":format!("{owner_tag:08X}"),"model":format!("{model_tag:08X}"),"meshes":meshes}));
        }
    }
    ensure!(!models.is_empty(), "no native template models");
    for tag in [0x80EC270D, 0x80EC2713, 0x80EC270C, 0x80EC2710, 0x80EC271D] {
        r.tag(tag, Some(0x808071E8))?;
    }
    Ok(
        json!({"item":format!("{item_hash:08X}"),"item_tag":format!("{:08X}",items.u32(matches[0]+16)?),"art_indices":indices,"assignments":keys.iter().map(|k|format!("{k:08X}")).collect::<Vec<_>>(),"parents":parents,"models":models}),
    )
}
