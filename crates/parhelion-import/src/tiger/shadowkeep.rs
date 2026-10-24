use crate::tiger::{
    payload::Payload,
    reader::{Reader, write_json},
};
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

struct Art {
    rows: Vec<Value>,
    placements: BTreeMap<u32, Vec<Value>>,
}

fn art_placements(
    item: &crate::tiger::payload::Payload,
    metadata: &crate::tiger::payload::Payload,
    selections: &[usize],
    rows: &[usize],
) -> Result<Art> {
    let mut art_rows = Vec::new();
    let mut all_placements = BTreeMap::<u32, Vec<Value>>::new();
    for (ordinal, &selection) in selections.iter().enumerate() {
        let index = item.u16(selection + 2)? as usize;
        let row = *rows.get(index).context("invalid art index")?;
        let singles = [metadata.u32(row + 8)?, metadata.u32(row + 12)?];
        let class = item.u8(selection)? as i8;
        for (position, &key) in singles.iter().enumerate() {
            all_placements
                .entry(key)
                .or_default()
                .push(json!({"art_row":ordinal,
                "art_index":index,"class":class,"single":position}));
        }
        let mut slots = Vec::new();
        for at in metadata.array(row + 16, 8, None)? {
            let resource = metadata.pointer(at)?;
            let selector = metadata.u64(resource)?;
            let assignments = metadata
                .array(resource + 8, 4, None)?
                .into_iter()
                .map(|at| metadata.u32(at))
                .collect::<Result<Vec<_>>>()?;
            for (position, &key) in assignments.iter().enumerate() {
                all_placements
                    .entry(key)
                    .or_default()
                    .push(json!({"art_row":ordinal,
                    "art_index":index,"class":class,"selector":selector,"position":position}));
            }
            slots.push(json!({"selector":selector,"assignments":assignments}));
        }
        art_rows.push(
            json!({"ordinal":ordinal,"class":class,"flags":item.u8(selection+1)?,
            "art_index":index,"singles":singles,"slots":slots}),
        );
    }
    Ok(Art {
        rows: art_rows,
        placements: all_placements,
    })
}

fn entity_models(r: &mut Reader, entity_tag: u32, entity: &Payload) -> Result<Vec<Value>> {
    let mut models = Vec::new();
    for component in entity.array(16, 12, Some(0x80809C04))? {
        let owner_tag = entity.u32(component)?;
        let owner = r.tag(owner_tag, Some(0x80809C36))?;
        let resource = owner.pointer(24)?;
        let resource_class =
            owner.u32(resource.checked_sub(4).context("invalid native resource")?)?;
        if !matches!(resource_class, 0x808072BD | 0x80807286) {
            continue;
        }
        let model_tag = owner.u32(resource + 0x1dc)?;
        let meshes = meshes(r, model_tag)?;
        models.push(json!({"entity":format!("{entity_tag:08X}"),"owner":format!("{owner_tag:08X}"),"model":format!("{model_tag:08X}"),"meshes":meshes,"cloth":resource_class == 0x80807286}));
    }
    Ok(models)
}

/// The definition tag of the installed item `hash`, from the investment globals' item table,
/// without model, animation or weapon-pattern assumptions.
pub(crate) fn item_tag(r: &mut Reader, hash: u32) -> Result<u32> {
    let tag = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("native globals")?
        .hash
        .0;
    let globals = r.tag(tag, None)?;
    let root = r.tag(globals.u32(16)?, Some(0x80807D84))?;
    let items = r.tag(root.u32(8 + 48 * 16)?, None)?;
    let mut matches = Vec::new();
    for row in items.array(8, 24, Some(0x80807BE8))? {
        if items.u32(row)? == hash {
            matches.push(items.u32(row + 16)?);
        }
    }
    ensure!(matches.len() == 1, "native item missing or ambiguous");
    Ok(matches[0])
}

/// The installed item's sockets: each one's type, the plugs its plug sets and its own entries
/// offer, and the curated fallback a randomized socket falls back to. The report is also written
/// to `native-sockets.json` in the reader's output.
pub fn sockets(r: &mut Reader, hash: u32) -> Result<Value> {
    let tag = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("globals")?
        .hash
        .0;
    let globals = r.tag(tag, None)?;
    let root = r.tag(globals.u32(16)?, None)?;
    let items = r.tag(root.u32(8 + 48 * 16)?, None)?;
    let rows = items.array(8, 24, Some(0x80807BE8))?;
    let sets = r.tag(root.u32(8 + 51 * 16)?, None)?;
    let set_rows = sets.array(8, 24, None)?;
    let row = *rows
        .iter()
        .find(|&&o| items.u32(o).ok() == Some(hash))
        .context("native item")?;
    let item = r.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
    if item.u64(0x68)? == 0 {
        let report = json!({"weapon":hash,"sockets":[]});
        write_json(&r.output.join("native-sockets.json"), &report)?;
        return Ok(report);
    }
    let mut sockets = vec![];
    for (index, row) in item
        .array(item.pointer(0x68)?, 0x50, Some(0x808077C4))?
        .into_iter()
        .enumerate()
    {
        let initial = item.u16(row + 2)?;
        let mut choices = BTreeSet::from([initial]);
        let mut fallback = None;
        for offset in [0x0C, 0x20] {
            let set_index = item.u16(row + offset)?;
            if set_index == u16::MAX {
                continue;
            }
            let set = *set_rows
                .get(set_index as usize)
                .context("native plug set")?;
            for entry in sets.array(set + 8, 0x20, Some(0x80802E03))? {
                let choice = sets.u16(entry)?;
                choices.insert(choice);
                if offset == 0x20
                    && fallback.is_none()
                    && choice != u16::MAX
                    && sets.u64(entry + 8)? == 0
                    && sets.u64(entry + 16)? == 0
                    && sets.f32(entry + 24)?.is_finite()
                    && sets.f32(entry + 24)? > 0.0
                {
                    fallback = Some(choice);
                }
            }
        }
        for entry in item.array(row + 0x40, 0x20, Some(0x80802E03))? {
            choices.insert(item.u16(entry)?);
        }
        let mut plugs = vec![];
        for choice in choices {
            if choice == u16::MAX {
                continue;
            }
            let row = *rows.get(choice as usize).context("native choice")?;
            let tag = items.u32(row + 16)?;
            let plug = r.tag(tag, None)?;
            let art = plug
                .array(plug.pointer(0x88)?, 4, Some(0x808077B5))?
                .into_iter()
                .map(|o| plug.u16(o + 2))
                .collect::<Result<Vec<_>>>()?;
            let categories = (0x100..plug.0.len().min(0x300).saturating_sub(8))
                .step_by(4)
                .filter(|&at| plug.u32(at).ok() == Some(0x808077E3))
                .map(|at| plug.u32(at + 4))
                .collect::<Result<Vec<_>>>()?;
            let category = (categories.len() == 1).then(|| categories[0]);
            plugs.push(json!({"hash":items.u32(row)?,"tag":format!("{tag:08X}"),"art_indices":art,"category":category,"item_kind":plug.u8(0xB8)?,"default":choice==initial}));
        }
        let curated_fallback = if initial == u16::MAX && item.u64(row + 0x40)? == 0 {
            fallback
                .map(|choice| items.u32(rows[choice as usize]))
                .transpose()?
        } else {
            None
        };
        sockets.push(json!({"index":index,"socket_type":item.u16(row)?,"choices":plugs,"curated_fallback":curated_fallback}));
    }
    let report = json!({"weapon":hash,"sockets":sockets});
    write_json(&r.output.join("native-sockets.json"), &report)?;
    r.finish()?;
    Ok(report)
}

/// The resource an investment record's pointer at `at` names, checked against its class word.
pub(crate) fn resource(p: &Payload, at: usize, class: u32) -> Result<usize> {
    ensure!(p.u64(at)? != 0, "missing investment resource at {at:X}");
    let target = p.pointer(at)?;
    ensure!(
        target >= 4 && p.u32(target - 4)? == class,
        "unexpected investment resource at {at:X}"
    );
    Ok(target)
}

/// Shadowkeep stat definition indices by stat hash, from the installed investment globals.
pub(crate) fn stat_definitions(r: &mut Reader) -> Result<BTreeMap<u32, u16>> {
    let tag = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("native globals")?
        .hash
        .0;
    let globals = r.tag(tag, None)?;
    let root = r.tag(globals.u32(16)?, Some(0x80807D84))?;
    let stats = r.tag(root.u32(8 + 95 * 16)?, None)?;
    let mut definitions = BTreeMap::new();
    for (index, at) in stats
        .array(8, 32, Some(0x80807D09))?
        .into_iter()
        .enumerate()
    {
        ensure!(
            definitions
                .insert(stats.u32(at)?, u16::try_from(index)?)
                .is_none(),
            "duplicate native stat hash"
        );
    }
    Ok(definitions)
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
    let selections = item.array(translation, 4, Some(0x808077B5))?;
    let indices = selections
        .iter()
        .map(|&row| item.u16(row + 2))
        .collect::<Result<Vec<_>>>()?;
    let metadata = r.tag(globals.u32(16 + 66 * 16)?, Some(0x80805DF5))?;
    let rows = metadata.array(8, 32, Some(0x80805DFB))?;
    let mut keys = BTreeSet::new();
    // Where each assignment key sits in the art row: a direct single or a
    // selector slot position. Selector 0 is the body the imported model replaces.
    let mut placements = BTreeMap::new();
    let Art {
        rows: art_rows,
        placements: all_placements,
    } = art_placements(&item, &metadata, &selections, &rows)?;
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
        let entity_tag = parent.u32(16)?;
        let entity = (entity_tag != u32::MAX)
            .then(|| r.tag(entity_tag, Some(0x80809C0F)))
            .transpose()?;
        // The part that carries the weapon's marker set (grip, sights, fire points) is the
        // one the weapon is placed by, so the body host is chosen from these.
        let mut marker_set = false;
        if let Some(entity) = &entity {
            for component in entity.array(16, 12, Some(0x80809C04))? {
                let owner = r.tag(entity.u32(component)?, Some(0x80809C36))?;
                let data = owner.pointer(0x10)?;
                marker_set |=
                    data >= 4 && owner.u32(data - 4)? == crate::tiger::markers::NATIVE_COMPONENT;
            }
        }
        parents.push(json!({"assignment":format!("{key:08X}"),"parent":format!("{tag:08X}"),"entity":format!("{entity_tag:08X}"),"parent_class":format!("{:08X}",r.reference(tag)?),"parent_bytes":hex::encode(&parent.0),"placement":placements.get(&key).cloned().unwrap_or(Value::Null),"placements":all_placements.get(&key),"marker_set":marker_set}));
        let Some(entity) = entity else {
            continue;
        };
        models.extend(entity_models(r, entity_tag, &entity)?);
    }
    ensure!(!models.is_empty(), "no native template models");
    for tag in [0x80EC270D, 0x80EC2713, 0x80EC270C, 0x80EC2710, 0x80EC271D] {
        r.tag(tag, Some(0x808071E8))?;
    }
    Ok(
        json!({"item":format!("{item_hash:08X}"),"item_tag":format!("{:08X}",items.u32(matches[0]+16)?),"art_indices":indices,"art_rows":art_rows,"assignments":keys.iter().map(|k|format!("{k:08X}")).collect::<Vec<_>>(),"parents":parents,"models":models}),
    )
}
