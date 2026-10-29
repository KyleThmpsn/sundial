//! A selected armor grant must see the actual imported movement key after persisted staging.
use super::*;
use parhelion_import::d1_roi::NativeOptions;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn prepare_armor(
    source: &Source,
    native: &Path,
    output: &Path,
    catalog: &InvestmentCatalog,
) -> Result<WeaponRecipe> {
    let armor = source.armor()?;
    let selected = armor
        .iter()
        .filter(|item| item.name.trim() == "Bones of Eao")
        .min_by_key(|item| item.hash)
        .context("Required source Bones of Eao armor")?;
    let donors = json!({"gear":catalog.gear_donors(ItemKind::Armor.bucket_hashes())
        .into_iter().map(|d|json!({"hash":d.hash,"name":d.name,"bucket_hash":d.bucket_hash,
        "class_type":catalog.item_class_type(d.hash),"collection_backed":d.collection_backed}))
        .collect::<Vec<_>>()});
    let folder = output.join("armor");
    let path = source.prepare_armor(
        selected.hash,
        native,
        &donors,
        &folder,
        &NativeOptions {
            xg_library: Some(configured("PARHELION_D1_XG")?),
            ..Default::default()
        },
        &mut |_| {},
    )?;
    let recipe = WeaponRecipe::load_json(&path)?;
    let saved = folder.join("persisted.parhelion.json");
    fs::write(&saved, recipe.to_json_pretty()?)?;
    let reloaded = WeaponRecipe::load_json(&saved)?;
    ensure!(recipe == reloaded, "Armor properties changed after reload");
    Ok(reloaded)
}

fn item_actions(reader: &mut Reader, recipe: &WeaponRecipe) -> Result<Vec<Payload>> {
    let globals = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|row| row.name == "investment_globals")
        .context("Globals")?
        .hash;
    let globals = reader.tag(globals.0, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    let items = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let item_rows = items.array(8, 24, Some(0x80807BE8))?;
    let identity = recipe.identity.item_hash.parse_u32()?;
    let selected = item_rows
        .iter()
        .copied()
        .filter(|&row| items.u32(row).ok() == Some(identity))
        .collect::<Vec<_>>();
    ensure!(
        selected.len() == 1,
        "Emitted armor identity is missing or repeated"
    );
    let item = reader.tag(items.u32(selected[0] + 16)?, Some(0x80807BEA))?;
    let sets = reader.tag(root.u32(8 + 51 * 16)?, None)?;
    let set_rows = sets.array(8, 24, None)?;
    let mut plugs = BTreeSet::new();
    for socket in item.array(item.pointer(0x68)?, 80, Some(0x808077C4))? {
        plugs.insert(item.u16(socket + 2)?);
        for row in item.array(socket + 0x40, 32, Some(0x80802E03))? {
            plugs.insert(item.u16(row)?);
        }
        for field in [12, 32] {
            let index = item.u16(socket + field)?;
            if index == u16::MAX {
                continue;
            }
            let at = *set_rows.get(usize::from(index)).context("Plug set")?;
            for row in sets.array(at + 8, 32, Some(0x80802E03))? {
                plugs.insert(sets.u16(row)?);
            }
        }
    }
    plugs.remove(&u16::MAX);
    let mut perks = BTreeSet::new();
    for index in plugs {
        let at = *item_rows.get(usize::from(index)).context("Plug row")?;
        let plug = reader.tag(items.u32(at + 16)?, Some(0x80807BEA))?;
        for row in plug.array(plug.pointer(0x70)? + 16, 24, Some(0x808077BC))? {
            perks.insert(plug.u16(row)?);
        }
    }
    // Stock presentation perks can have no runtime. The source count grant is private and
    // must be reachable through one of the item's actual plug/perk assignments.
    let finished = reader.tag(globals.u32(16 + 71 * 16)?, Some(0x80805C97))?;
    let finished_rows = finished.array(8, 24, Some(0x80805C9D))?;
    let runtime = reader.tag(0x80EC3F60, Some(0x80809780))?;
    let assigned = runtime.array(8, 8, None)?;
    let mut actions = Vec::new();
    for index in perks {
        let at = *finished_rows
            .get(usize::from(index))
            .context("Finished armor perk")?;
        let key = finished.u32(at + 4)?;
        let targets = assigned
            .iter()
            .copied()
            .filter(|&row| runtime.u32(row).ok() == Some(key))
            .collect::<Vec<_>>();
        ensure!(targets.len() <= 1, "Ambiguous armor runtime assignment");
        if let Some(&row) = targets.first() {
            actions.push((*reader.tag(runtime.u32(row + 4)?, Some(0x808040B5))?).clone());
        }
    }
    Ok(actions)
}

pub(super) fn check_armor(
    source: &Source,
    emitted: &mut Reader,
    recipe: &WeaponRecipe,
    movement_key: u32,
    before: &Payload,
) -> Result<Value> {
    let original = source.read(0x80A74698)?;
    let source_rows = original
        .array(original.pointer(24)? + 0xA0, 40, Some(0x80802A84))?
        .into_iter()
        .filter(|&row| original.u32(row).ok() == Some(0xA24C3BCC))
        .collect::<Vec<_>>();
    ensure!(
        source_rows.len() == 2,
        "Required source armor count group changed"
    );
    let bank = emitted.tag(0x80BC1D56, Some(0x80809C36))?;
    let all_rows = bank.array(bank.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let old_rows = before.array(before.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let mut found = BTreeMap::new();
    for action in item_actions(emitted, recipe)? {
        for field in action.array(0x38, 8, Some(0x808040AC))? {
            let at = action.pointer(field)?;
            if action.u8(at)? != 7 || action.u8(at + 2)? != 3 {
                continue;
            }
            ensure!(
                action.u8(at + 8)? == 0,
                "Armor count is not an Apply operation"
            );
            let key = action.u32(at + 4)?;
            ensure!(
                key != movement_key,
                "Distinct source groups share one identity"
            );
            let rows = all_rows
                .iter()
                .copied()
                .filter(|&row| bank.u32(row + 16).ok() == Some(key))
                .collect::<Vec<_>>();
            ensure!(
                rows.len() == 3,
                "Armor count group lost a source row or gate alternative"
            );
            ensure!(
                !old_rows
                    .iter()
                    .any(|&row| before.u32(row + 16).ok() == Some(key)),
                "Armor count aliases a stock key"
            );
            let mut readback = Vec::new();
            for ((source_row, gate), row) in [
                (source_rows[0], 0),
                (source_rows[1], 0x53722F50),
                (source_rows[1], movement_key),
            ]
            .into_iter()
            .zip(rows)
            {
                let source_body = original.pointer(source_row + 32)?;
                ensure!(
                    original.u32(source_body - 4)? == 0x8080209C,
                    "Source count class"
                );
                let body = bank.pointer(row + 48)?;
                ensure!(
                    bank.u32(body + 4)? == 0x808042A5
                        && bank.u32(body + 16)? == original.u32(source_body)?
                        && bank.u32(row + 20)? == original.u32(source_row + 4)?
                        && bank.u32(row + 24)? == original.u32(source_row + 8)?
                        && bank.u32(row + 32)? == original.u32(source_row + 16)?,
                    "Count value, selection or mode differs from source"
                );
                let actual_gate = if bank.u64(row + 40)? == 0 {
                    0
                } else {
                    let at = bank.pointer(row + 40)?;
                    ensure!(
                        bank.u32(at - 4)? == 0x8080454A && bank.u32(at + 4)? == 0,
                        "Count gate type or reserved word changed"
                    );
                    bank.u32(at)?
                };
                ensure!(
                    actual_gate == gate,
                    "Count gate lost stock or imported membership"
                );
                let source_gate = if original.u64(source_row + 24)? == 0 {
                    0
                } else {
                    let at = original.pointer(source_row + 24)?;
                    ensure!(original.u32(at - 4)? == 0x80802981, "Source gate class");
                    original.u32(at)?
                };
                ensure!(
                    source_gate == if gate == 0 { 0 } else { 0x53722F50 },
                    "Source conditional group no longer names Triple Jump"
                );
                let witnesses = old_rows
                    .iter()
                    .copied()
                    .filter(|&at| {
                        before.u32(at + 16).ok() == Some(0xA24C3BCC)
                            && before.u32(at + 20).ok() == original.u32(source_row + 4).ok()
                            && before.u32(at + 24).ok() == original.u32(source_row + 8).ok()
                            && before
                                .pointer(at + 48)
                                .ok()
                                .and_then(|body| before.u32(body + 4).ok())
                                == Some(0x808042A5)
                    })
                    .collect::<Vec<_>>();
                ensure!(witnesses.len() == 1, "Native count priority witness");
                ensure!(
                    bank.u32(row + 28)? == before.u32(witnesses[0] + 28)?,
                    "Count priority slot"
                );
                readback.push(json!({"key":key,"gate":gate,"value":bank.u32(body + 16)?,
                    "priority":bank.u32(row + 24)?,"handler":bank.u32(row + 28)?}));
            }
            ensure!(
                found.insert(key, readback).is_none(),
                "Count group has repeated activation"
            );
        }
    }
    ensure!(
        found.len() == 1,
        "Required armor count grant was not reachable from its item"
    );
    Ok(json!({"item":recipe.identity.item_hash,"groups":found,
        "movement_key":movement_key,"source_bank_sha256":sha(&original.0),
        "emitted_bank_sha256":sha(&bank.0),"gameplay_verified":false}))
}
