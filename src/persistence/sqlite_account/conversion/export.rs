use super::*;
use serde_json::json;
use sundial_account::{
    AccountSettingKey, AccountSettingValue, Character, ItemInstance, ItemPlugs, KeyBindingSlot,
};

pub(crate) fn to_json(
    document: &SqliteAccountDocument,
    defaults: &Value,
    notes: &mut Vec<String>,
) -> Result<Value, String> {
    let mut output = defaults.clone();
    let templates = defaults["state"]["characters"]
        .as_array()
        .ok_or("Dawn defaults have no characters.")?;
    let mut characters = Vec::new();
    let mut moved = 0;
    let mut flags = 0;
    for (index, character) in document.characters().characters().iter().enumerate() {
        characters.push(export_character(
            document, index, character, templates, &mut moved, &mut flags,
        )?);
    }
    output["state"]["characters"] = json!(characters);
    output["state"]["account"]["primary_soid"] = json!(document.primary_soid());
    output["state"]["account"]["profile_items"] = json!(document.profile().profile_items().iter().map(|item| json!({"definition_hash":item.definition_hash.get(),"quantity":item.quantity})).collect::<Vec<_>>());
    let rewards = document.profile().dismantle_rewards();
    let transferable = rewards
        .iter()
        .filter(|row| {
            row.rarities.is_empty() && row.gear_class.is_none() && row.masterworked.is_none()
        })
        .take(8)
        .map(|row| json!({"definition_hash":row.definition_hash.get(),"quantity":row.quantity}))
        .collect::<Vec<_>>();
    note(
        notes,
        rewards.len() - transferable.len(),
        "dismantle reward rules cannot transfer to v6",
    );
    output["state"]["account"]["dismantle_rewards"] = json!(transferable);
    write_preferences(document, &mut output["state"]["account"]["settings"], notes)?;
    output["server"]["entitlements"] = document.entitlements().clone();
    write_progression(document, &mut output, notes)?;
    note(
        notes,
        moved,
        "equipped items will move to inventory because v6 has no matching slot",
    );
    note(
        notes,
        flags,
        "items will no longer be marked as masterworked",
    );
    let stacks = (0..characters.len())
        .map(|index| document.character_stacks(index).len())
        .sum();
    note(
        notes,
        stacks,
        "character material stacks will stay in the backup",
    );
    note(
        notes,
        document.pending_rewards().len(),
        "pending rewards will stay in the backup",
    );
    notes.push("Titles and item notification state stay in the backup. Some subclass unlock data has no v6 equivalent.".into());
    Ok(output)
}

fn export_character(
    document: &SqliteAccountDocument,
    index: usize,
    character: &Character,
    templates: &[Value],
    moved: &mut usize,
    flags: &mut usize,
) -> Result<Value, String> {
    let metadata = character
        .metadata
        .ok_or("Character metadata is unavailable.")?;
    let mut row = templates
        .iter()
        .find(|row| row["class"].as_u64() == Some(metadata.class_type.into()))
        .or_else(|| templates.first())
        .ok_or("Dawn defaults have no character template.")?
        .clone();
    row["soid"] = json!(
        character
            .soid
            .ok_or("Character SOID is unavailable.")?
            .get()
    );
    row["race"] = json!(metadata.race);
    row["gender"] = json!(metadata.gender);
    row["class"] = json!(metadata.class_type);
    for (key, value) in [
        ("movement_ability", metadata.abilities.movement),
        ("grenade_ability", metadata.abilities.grenade),
        ("super_ability", metadata.abilities.super_ability),
        ("melee_ability", metadata.abilities.melee),
        ("class_ability", metadata.abilities.class_ability),
    ] {
        row[key] = json!(value);
    }
    for key in [
        "preview_available",
        "appearance_value",
        "last_orbited_destination",
        "content_bypass",
        "level",
    ] {
        row[key] = document.runtime()["characters"][index][key].clone();
    }
    let equipment = row["equipment"]
        .as_object_mut()
        .ok_or("Dawn defaults have no equipment.")?;
    for value in equipment.values_mut() {
        *value = Value::Null;
    }
    let mut inventory = character
        .inventory
        .iter()
        .map(|item| export_item(item, flags))
        .collect::<Vec<_>>();
    for (slot, item) in &character.equipment {
        let Some(item) = item else { continue };
        let item = export_item(item, flags);
        if let Some(value) = equipment.get_mut(slot.as_str()) {
            *value = item;
        } else {
            inventory.push(item);
            *moved += 1;
        }
    }
    if inventory.len() > crate::account_contract::CHARACTER_INVENTORY_CAPACITY {
        return Err(format!(
            "Character {} needs {} inventory slots after conversion. Free space before converting.",
            index + 1,
            inventory.len()
        ));
    }
    row["inventory"] = json!(inventory);
    Ok(row)
}

fn export_item(item: &ItemInstance, flags: &mut usize) -> Value {
    let original_flags = item.flags.unwrap_or_default();
    *flags += usize::from(original_flags & !3 != 0);
    let mut row = json!({"instance_soid":item.instance_soid.get(),"definition_hash":item.definition_hash.get(),"level":item.level,"quantity":item.quantity,"flags":original_flags & 3,"plugs":null});
    if let ItemPlugs::Authored(plugs) = &item.plugs {
        row["plugs"] = json!(
            plugs
                .iter()
                .map(|plug| plug.map(|hash| hash.get()))
                .collect::<Vec<_>>()
        );
    }
    row
}

fn write_preferences(
    document: &SqliteAccountDocument,
    target: &mut Value,
    notes: &mut Vec<String>,
) -> Result<(), String> {
    let mut missing = 0;
    for (key, value) in document.settings().values() {
        match key {
            AccountSettingKey::Preference { group, name } => {
                let group = crate::persistence::json_account::setting_group_name(*group);
                let parent = match group {
                    Some(group) => &mut target[group],
                    None => &mut *target,
                };
                let Some(destination) = parent.get_mut(name.as_ref()) else {
                    missing += 1;
                    continue;
                };
                *destination = preference_value(value)?;
            }
            AccountSettingKey::KeyBinding { action, slot } => {
                let field = match slot {
                    KeyBindingSlot::Primary => "primary",
                    KeyBindingSlot::Secondary => "secondary",
                };
                let value = match value {
                    AccountSettingValue::Unassigned => Value::Null,
                    AccountSettingValue::InputCode(code) => {
                        match crate::game_settings::native_input_name(u64::from(*code)) {
                            Some(name) => json!(name),
                            None => {
                                notes.push(format!("Key binding {action} {field} has no v6 name and will use the Dawn default."));
                                continue;
                            }
                        }
                    }
                    _ => return Err("Invalid native key binding.".into()),
                };
                target["key_bindings"][action.as_ref()][field] = value;
            }
        }
    }
    note(
        notes,
        missing,
        "account preferences are absent from Dawn's defaults and will stay in the backup",
    );
    Ok(())
}

fn preference_value(value: &AccountSettingValue) -> Result<Value, String> {
    match value {
        AccountSettingValue::Boolean(value) => Ok(json!(value)),
        AccountSettingValue::Unsigned(value) => Ok(json!(value)),
        AccountSettingValue::Decimal(value) => Ok(json!(value.get())),
        AccountSettingValue::Text(value) => Ok(json!(value)),
        _ => Err("Invalid native account preference.".into()),
    }
}

fn write_progression(
    document: &SqliteAccountDocument,
    output: &mut Value,
    notes: &mut Vec<String>,
) -> Result<(), String> {
    let first = document.progression_view(0);
    for index in 1..document.characters().characters().len() {
        if document.progression_view(index)["state"] != first["state"] {
            notes.push("Dawn shares character progression. Character 1's progression will be used for all characters.".into());
            break;
        }
    }
    for parent in ["unlocks", "investment"] {
        for (key, value) in output["state"][parent]
            .as_object_mut()
            .ok_or("Missing Dawn progression defaults.")?
        {
            if let Some(source) = first["state"][parent].get(key) {
                *value = source.clone();
            }
        }
    }
    notes.push("Progression values without a v6 field stay in the SQLite backup. Check subclass abilities and progression in game.".into());
    Ok(())
}

fn note(notes: &mut Vec<String>, count: usize, text: &str) {
    if count != 0 {
        notes.push(format!("{count} {text}."));
    }
}
