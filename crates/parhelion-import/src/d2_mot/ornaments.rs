//! Source socket membership is the authority for ornament compatibility.
use crate::d2_mot::{
    assets::item,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn native(r: &mut Reader, hash: u32) -> Result<Value> {
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
            plugs.push(json!({"hash":items.u32(row)?,"tag":format!("{tag:08X}"),"art_indices":art,"category":category,"default":choice==initial}));
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

pub fn discover(r: &mut Reader, hash: u32) -> Result<Value> {
    discover_inner(r, hash, true)
}

pub(crate) fn gameplay_sockets(r: &mut Reader, hash: u32) -> Result<Value> {
    discover_inner(r, hash, false)
}

fn discover_inner(r: &mut Reader, hash: u32, labels: bool) -> Result<Value> {
    let (_, tag) = item::find(r, hash)?;
    let definition = r.tag(tag, Some(0x8080799D))?;
    let sockets = definition.pointer(0x60)?;
    ensure!(
        sockets >= 4 && definition.u32(sockets - 4)? == 0x808077C0,
        "unsupported socket resource"
    );
    let tables = r.classes(0x80807997);
    ensure!(tables.len() == 1, "ambiguous inventory table");
    let inventory = r.tag(tables[0], None)?;
    let items = inventory.array(8, 32, None)?;
    let tables = r.classes(0x808077CD);
    ensure!(tables.len() == 1, "ambiguous reusable plug-set table");
    let sets = r.tag(tables[0], None)?;
    let set_rows = sets.array(8, 24, Some(0x808077D3))?;
    let mut result = vec![];
    for (index, row) in definition
        .array(sockets, 0x60, Some(0x808077C3))?
        .into_iter()
        .enumerate()
    {
        let initial = definition.u32(row + 8)?;
        let mut choices = BTreeSet::new();
        if initial != u32::MAX {
            choices.insert(initial);
        }
        for entry in definition.array(row + 0x50, 0x58, Some(0x808077D5))? {
            choices.insert(definition.u32(entry + 0x20)?);
        }
        let mut set_ids = vec![];
        for offset in [0x14, 0x30] {
            let set_index = definition.u16(row + offset)?;
            if set_index == u16::MAX {
                continue;
            }
            let set = *set_rows
                .get(set_index as usize)
                .context("plug-set index outside table")?;
            set_ids.push(sets.u32(set)?);
            for entry in sets.array(set + 8, 0x58, Some(0x808077D5))? {
                choices.insert(sets.u32(entry + 0x20)?);
            }
        }
        let mut plugs = vec![];
        for choice in choices {
            let item_row = *items
                .get(choice as usize)
                .context("plug item index outside table")?;
            let plug_hash = inventory.u32(item_row)?;
            // Gameplay needs identities and categories, not localized ornament labels.
            // Some unused source choices reference missing strings in this build.
            let localized = if labels {
                crate::d2_mot::localization::item_name(r, plug_hash, choice as usize, 0)?
            } else {
                Value::Null
            };
            let plug_tag = r.ref64(&inventory, item_row + 16)?;
            let plug = r.tag(plug_tag, Some(0x8080799D))?;
            let resource = plug.pointer(0x40)?;
            ensure!(
                resource >= 4 && plug.u32(resource - 4)? == 0x808073A1,
                "unsupported plug resource for {plug_hash:08X}"
            );
            let translation = plug.pointer(0x70)?;
            let art = plug
                .array(translation, 4, None)?
                .into_iter()
                .map(|at| plug.u16(at + 2))
                .collect::<Result<Vec<_>>>()?;
            plugs.push(
                json!({"hash":plug_hash,"tag":format!("{plug_tag:08X}"),"index":choice,
                "category":plug.u32(resource)?,"default":choice==initial,"art_indices":art,
                "name":localized["name"],"localization":localized}),
            );
        }
        result.push(json!({"index":index,"socket_type_index":definition.u16(row)?,"plug_sets":set_ids,"choices":plugs}));
    }
    let report = json!({"source_weapon":hash,"source_tag":format!("{tag:08X}"),"sockets":result,
        "scope":"Source socket choices; appearance-bearing plugs require native ornament authoring before use."});
    write_json(&r.output.join("sockets.json"), &report)?;
    r.finish()?;
    Ok(report)
}
