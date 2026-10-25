//! Source socket membership is the authority for ornament compatibility.
use crate::d2_mot::{
    assets::item,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn discover(r: &mut Reader, hash: u32) -> Result<Value> {
    discover_inner(r, hash, true)
}

pub fn gameplay_sockets(r: &mut Reader, hash: u32) -> Result<Value> {
    discover_inner(r, hash, false)
}

fn discover_inner(r: &mut Reader, hash: u32, labels: bool) -> Result<Value> {
    let (_, tag) = item::find(r, hash)?;
    let definition = r.tag(tag, Some(0x8080799D))?;
    if definition.u64(0x60)? == 0 {
        return Ok(json!({"weapon":hash,"sockets":[]}));
    }
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
        // The plugs the socket offers freely, in source order: its initial plug, its own entries
        // and then its reusable set (+0x14). Its randomized set (+0x30) holds the rolled ones.
        let mut curated = Vec::new();
        let mut randomized = BTreeSet::new();
        if initial != u32::MAX {
            choices.insert(initial);
            curated.push(initial);
        }
        for entry in definition.array(row + 0x50, 0x58, Some(0x808077D5))? {
            let choice = definition.u32(entry + 0x20)?;
            choices.insert(choice);
            if !curated.contains(&choice) {
                curated.push(choice);
            }
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
                let choice = sets.u32(entry + 0x20)?;
                choices.insert(choice);
                if offset == 0x30 {
                    randomized.insert(choice);
                } else if !curated.contains(&choice) {
                    curated.push(choice);
                }
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
                "curated":curated.iter().position(|curated| *curated == choice),
                "randomized":randomized.contains(&choice),
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
