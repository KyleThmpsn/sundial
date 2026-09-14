use super::*;
use rusqlite::params;
use serde_json::json;
use std::collections::BTreeSet;

pub(crate) fn from_json(
    source: &Value,
    defaults: &AccountDefaults,
    path: &Path,
) -> Result<(), String> {
    if path.exists() {
        return Err("The staged database already exists.".into());
    }
    let mut db = Connection::open(path).map_err(err)?;
    db.execute_batch("PRAGMA foreign_keys=ON;").map_err(err)?;
    let tx = db.transaction().map_err(err)?;
    for sql in [
        &defaults.schema,
        &defaults.rows,
        &defaults.settings_schema,
        &defaults.settings_rows,
    ] {
        tx.execute_batch(sql).map_err(err)?;
    }
    super::super::reader::validate_schema(&tx).map_err(err)?;
    tx.execute_batch("DELETE FROM sockets; DELETE FROM items; DELETE FROM profile_items; DELETE FROM dismantle_rewards; DELETE FROM character_stacks; DELETE FROM pending_rewards; DELETE FROM unlocks; DELETE FROM family5; DELETE FROM characters; DELETE FROM account;").map_err(err)?;
    let account = &source["state"]["account"];
    let primary = number(&account["primary_soid"], "account SOID")?;
    tx.execute(
        "INSERT INTO account(id,soid,profile_setup_completed) VALUES(1,?,?)",
        params![
            primary as i64,
            account["profile_setup_completed"].as_bool().unwrap_or(true)
        ],
    )
    .map_err(err)?;
    let characters = source["state"]["characters"]
        .as_array()
        .ok_or("Missing characters.")?;
    let mut used = BTreeSet::from([primary]);
    for (slot, character) in characters.iter().enumerate() {
        write_character(&tx, slot, character, &mut used)?;
    }
    write_profile(&tx, account, &mut used)?;
    write_preferences(&tx, &account["settings"])?;
    super::super::entitlements::save(&tx, &source["server"]["entitlements"], &[]).map_err(err)?;
    write_progression(&tx, source, characters.len())?;
    super::super::validation::connection(&tx)?;
    tx.commit().map_err(err)?;
    super::super::snapshot::read(path)?;
    Ok(())
}

fn write_character(
    db: &Connection,
    slot: usize,
    row: &Value,
    used: &mut BTreeSet<u64>,
) -> Result<(), String> {
    let soid = number(&row["soid"], "character SOID")?;
    if !used.insert(soid) {
        return Err("Duplicate account or character SOID.".into());
    }
    let mut serial = 0_i64;
    db.execute("INSERT INTO characters(slot,soid,race,gender,class,level,preview_available,appearance_value,last_orbited_destination,content_bypass,equipped_title,acquired_subclass_mask,next_inventory_serial) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)", params![slot as i64, soid as i64, row["race"].as_i64(), row["gender"].as_i64(), row["class"].as_i64(), row["level"].as_i64().unwrap_or(50), row["preview_available"].as_bool().unwrap_or(true), row["appearance_value"].as_f64().unwrap_or(1.0), number(&row["last_orbited_destination"], "last destination")?, row["content_bypass"].as_bool().unwrap_or(false), 65535, -1, 0]).map_err(err)?;
    for (position, name) in super::super::contract::EQUIPMENT_SLOTS.iter().enumerate() {
        if let Some(item) = row["equipment"].get(*name).filter(|item| item.is_object()) {
            write_item(db, [slot, 0, position], item, row, serial, used)?;
            serial += 1;
        }
    }
    for (position, item) in row["inventory"]
        .as_array()
        .ok_or("Missing inventory.")?
        .iter()
        .enumerate()
    {
        write_item(db, [slot, 1, position], item, row, serial, used)?;
        serial += 1;
    }
    db.execute(
        "UPDATE characters SET next_inventory_serial=? WHERE slot=?",
        params![serial, slot as i64],
    )
    .map_err(err)?;
    Ok(())
}

fn write_item(
    db: &Connection,
    location: [usize; 3],
    item: &Value,
    character: &Value,
    serial: i64,
    used: &mut BTreeSet<u64>,
) -> Result<(), String> {
    let soid = number(&item["instance_soid"], "item SOID")?;
    if !used.insert(soid) {
        return Err("Duplicate item SOID.".into());
    }
    let plugs = item["plugs"].as_array();
    let [slot, location, position] = location;
    db.execute("INSERT INTO items(character_slot,location,position,instance_soid,definition_hash,level,quantity,mutation_serial,flags,socket_policy,plug_count,movement_ability,grenade_ability,super_ability,melee_ability,class_ability,seen) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,0)", params![slot as i64, location as i64, position as i64, soid as i64, number(&item["definition_hash"], "item definition")?, item["level"].as_i64().unwrap_or(0), item["quantity"].as_i64().unwrap_or(1), serial, item["flags"].as_u64().unwrap_or(0), i64::from(plugs.is_some()), plugs.map_or(0, Vec::len) as i64, character["movement_ability"].as_i64().unwrap_or(4), character["grenade_ability"].as_i64().unwrap_or(7), character["super_ability"].as_i64().unwrap_or(10), character["melee_ability"].as_i64().unwrap_or(11), character["class_ability"].as_i64().unwrap_or(2)]).map_err(err)?;
    for (lane, plug) in plugs.into_iter().flatten().enumerate() {
        if !plug.is_null() {
            db.execute(
                "INSERT INTO sockets(instance_soid,lane,plug_hash) VALUES(?,?,?)",
                params![soid as i64, lane as i64, number(plug, "socket")?],
            )
            .map_err(err)?;
        }
    }
    Ok(())
}

fn write_profile(db: &Connection, account: &Value, used: &mut BTreeSet<u64>) -> Result<(), String> {
    let mut soid = 0x5000_0000_0000_0001_u64;
    for (position, item) in account["profile_items"]
        .as_array()
        .ok_or("Missing profile items.")?
        .iter()
        .enumerate()
    {
        while !used.insert(soid) {
            soid = soid.checked_add(1).ok_or("Item SOIDs exhausted.")?;
        }
        db.execute("INSERT INTO profile_items(position,instance_soid,definition_hash,quantity,mutation_serial,seen) VALUES(?,?,?,?,?,0)", params![position as i64, soid as i64, number(&item["definition_hash"], "material")?, item["quantity"].as_i64(), position as i64]).map_err(err)?;
    }
    for (position, item) in account["dismantle_rewards"]
        .as_array()
        .ok_or("Missing dismantle rewards.")?
        .iter()
        .enumerate()
    {
        // v6 rewards have no filters. Do not invent restrictions during conversion.
        db.execute("INSERT INTO dismantle_rewards(position,definition_hash,quantity,tier_mask,class_mask,masterwork) VALUES(?,?,?,0,0,0)", params![position as i64, number(&item["definition_hash"], "dismantle reward")?, item["quantity"].as_i64()]).map_err(err)?;
    }
    Ok(())
}

fn write_preferences(db: &Connection, source: &Value) -> Result<(), String> {
    for (group, table) in [
        ("", "account_preferences"),
        ("controls", "account_controls"),
        ("audio", "account_audio"),
        ("display", "account_display"),
        ("interface", "account_interface"),
        ("social", "account_social"),
    ] {
        let values = if group.is_empty() {
            source
        } else {
            &source[group]
        };
        let stmt = db.prepare(&format!("SELECT * FROM {table}")).map_err(err)?;
        let columns = stmt
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for name in columns.into_iter().filter(|name| name != "id") {
            let Some(value) = values.get(&name) else {
                continue;
            };
            let value = if name == "key_binding_source" {
                rusqlite::types::Value::Integer(match value.as_str() {
                    Some("account") => 0,
                    Some("computer") => 1,
                    _ => return Err("Invalid key binding source.".into()),
                })
            } else {
                sql_value(value)?
            };
            db.execute(
                &format!("UPDATE {table} SET \"{name}\"=? WHERE id=1"),
                [value],
            )
            .map_err(err)?;
        }
    }
    for (index, action) in sundial_account::KEY_BINDING_ACTIONS.iter().enumerate() {
        for (field, column) in [("primary", "primary_code"), ("secondary", "secondary_code")] {
            let Some(value) = source["key_bindings"][*action].get(field) else {
                continue;
            };
            let code = if value.is_null()
                || value
                    .as_str()
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case("unused"))
            {
                -1
            } else {
                i64::from(
                    crate::game_settings::named_input_code(
                        value.as_str().ok_or("Invalid key binding.")?,
                    )
                    .ok_or("Key binding has no native code.")?,
                )
            };
            db.execute(
                &format!("UPDATE account_key_bindings SET {column}=? WHERE action=?"),
                params![code, index as i64],
            )
            .map_err(err)?;
        }
    }
    Ok(())
}

fn sql_value(value: &Value) -> Result<rusqlite::types::Value, String> {
    use rusqlite::types::Value as Sql;
    match value {
        Value::Bool(value) => Ok(Sql::Integer(i64::from(*value))),
        Value::Number(value) => value
            .as_i64()
            .map(Sql::Integer)
            .or_else(|| value.as_f64().map(Sql::Real))
            .ok_or("Invalid account setting.".into()),
        _ => Err("Invalid account setting.".into()),
    }
}

fn write_progression(db: &Connection, source: &Value, count: usize) -> Result<(), String> {
    let mut progression = super::super::progression::Progression::load(db).map_err(err)?;
    for index in 0..count {
        let mut view = progression.view(index);
        for parent in ["unlocks", "investment"] {
            for (key, value) in view["state"][parent]
                .as_object_mut()
                .ok_or("Invalid progression.")?
            {
                *value = source["state"][parent]
                    .get(key)
                    .cloned()
                    .unwrap_or(json!([]));
            }
        }
        progression.apply(index, &view).map_err(err)?;
    }
    progression.save(db).map_err(err)
}
