use super::*;

#[test]
fn dawn_defaults_convert_to_sqlite_and_back() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("investment.sqlite3");
    let source: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/dawn-v6-42fc41e-defaults.json"
    ))
    .unwrap();
    from_json(&source, &super::super::tests::default_resources(), &path).unwrap();
    let super::super::SqliteAccountDocumentLoad::Loaded(document) =
        super::super::load_document(&path).unwrap()
    else {
        panic!("Account not loaded")
    };
    let mut notes = Vec::new();
    let result = to_json(&document, &source, &mut notes, &|_| true).unwrap();
    assert_eq!(result["state"]["characters"].as_array().unwrap().len(), 3);
    assert_eq!(
        result["state"]["account"]["primary_soid"],
        serde_json::json!(number(&source["state"]["account"]["primary_soid"], "primary").unwrap())
    );
    assert_eq!(
        result["server"]["entitlements"],
        source["server"]["entitlements"]
    );
    assert_materials(
        &result["state"]["account"]["profile_items"],
        &source["state"]["account"]["profile_items"],
    );
    assert_materials(
        &result["state"]["account"]["dismantle_rewards"],
        &source["state"]["account"]["dismantle_rewards"],
    );
    assert_eq!(
        result["state"]["account"]["settings"],
        source["state"]["account"]["settings"]
    );
    for (converted, original) in result["state"]["characters"]
        .as_array()
        .unwrap()
        .iter()
        .zip(source["state"]["characters"].as_array().unwrap())
    {
        assert_character(converted, original);
    }
}

fn assert_character(converted: &Value, original: &Value) {
    for field in [
        "soid",
        "race",
        "gender",
        "class",
        "movement_ability",
        "grenade_ability",
        "super_ability",
        "melee_ability",
        "class_ability",
        "level",
    ] {
        assert_eq!(
            number(&converted[field], field).unwrap(),
            number(&original[field], field).unwrap(),
            "{field}"
        );
    }
    for (slot, item) in original["equipment"].as_object().unwrap() {
        if item.is_null() {
            assert!(converted["equipment"][slot].is_null());
            continue;
        }
        let converted = &converted["equipment"][slot];
        for field in ["instance_soid", "definition_hash", "level", "quantity"] {
            assert_eq!(
                number(&converted[field], field).unwrap(),
                number(&item[field], field).unwrap(),
                "{slot}.{field}"
            );
        }
        assert_eq!(
            normalized_hashes(&converted["plugs"]),
            normalized_hashes(&item["plugs"])
        );
    }
}

fn assert_materials(actual: &Value, expected: &Value) {
    let actual = actual.as_array().unwrap();
    let expected = expected.as_array().unwrap();
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        for key in ["definition_hash", "quantity"] {
            assert_eq!(
                number(&actual[key], key).unwrap(),
                number(&expected[key], key).unwrap()
            );
        }
    }
}

fn normalized_hashes(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(normalized_hashes).collect()),
        Value::Null => Value::Null,
        value => serde_json::json!(number(value, "hash").unwrap()),
    }
}

#[test]
fn native_conversion_keeps_socket_lanes_and_reports_data_dawn_cannot_store() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("investment.sqlite3");
    super::super::tests::create_fixture(&path, 7);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("INSERT INTO character_stacks VALUES(0,0,25,10,0); INSERT INTO pending_rewards(character_slot,kind,definition_hash,quantity) VALUES(0,0,987,1); UPDATE items SET position=16 WHERE location=0 AND position=0; ALTER TABLE items ADD COLUMN future TEXT NOT NULL DEFAULT 'preserve'; CREATE TABLE future_data(value BLOB); INSERT INTO future_data VALUES(x'001122');").unwrap();
    let super::super::SqliteAccountDocumentLoad::Loaded(document) =
        super::super::load_document(&path).unwrap()
    else {
        panic!("Account not loaded")
    };
    let defaults: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/dawn-v6-42fc41e-defaults.json"
    ))
    .unwrap();
    let before = super::super::snapshot::read(&path).unwrap();
    let mut notes = Vec::new();
    let result = to_json(&document, &defaults, &mut notes, &|_| true).unwrap();
    let character = &result["state"]["characters"][0];
    // The fixture parks one item in native slot 16, which Dawn v6 has no equipment slot for. It
    // stays in the backup rather than moving into an inventory bucket the runtime cannot resolve.
    assert_eq!(character["inventory"].as_array().unwrap().len(), 1);
    assert_eq!(character["inventory"][0]["flags"], 3);
    assert_eq!(
        character["equipment"]["subclass"]["plugs"],
        serde_json::json!([77, null])
    );
    assert_eq!(
        result["state"]["account"]["dismantle_rewards"],
        serde_json::json!([])
    );
    for note in [
        "dismantle reward",
        "character material stacks",
        "pending rewards",
        "no matching slot",
        "marked as masterworked",
    ] {
        assert!(
            notes.iter().any(|text| text.contains(note)),
            "{note}: {notes:?}"
        );
    }
    document
        .write_conversion_copy(&directory.path().join("draft.sqlite3"))
        .unwrap();
    assert_eq!(
        super::super::snapshot::read(&directory.path().join("draft.before.sqlite3")).unwrap(),
        before
    );
    assert_eq!(super::super::snapshot::read(&path).unwrap(), before);
}

#[test]
fn native_conversion_keeps_a_full_inventory_when_a_slot_has_no_target_equivalent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("investment.sqlite3");
    super::super::tests::create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    // Native slot 16 has no Dawn v6 equivalent. It used to move into the inventory, which could
    // push a full character past its capacity. Dropping it keeps the conversion available.
    db.execute(
        "UPDATE items SET position=16 WHERE location=0 AND position=0",
        [],
    )
    .unwrap();
    for position in 1..crate::account_contract::CHARACTER_INVENTORY_CAPACITY {
        db.execute("INSERT INTO items SELECT character_slot,location,?,instance_soid+?,definition_hash,level,quantity,mutation_serial,flags,socket_policy,plug_count,movement_ability,grenade_ability,super_ability,melee_ability,class_ability,seen FROM items WHERE location=1 AND position=0", rusqlite::params![position, position]).unwrap();
    }
    let super::super::SqliteAccountDocumentLoad::Loaded(document) =
        super::super::load_document(&path).unwrap()
    else {
        panic!("Account not loaded")
    };
    let defaults: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/dawn-v6-42fc41e-defaults.json"
    ))
    .unwrap();
    let mut notes = Vec::new();
    let result = to_json(&document, &defaults, &mut notes, &|_| true).unwrap();
    assert_eq!(
        result["state"]["characters"][0]["inventory"]
            .as_array()
            .unwrap()
            .len(),
        crate::account_contract::CHARACTER_INVENTORY_CAPACITY
    );
    assert!(
        notes.iter().any(|text| text.contains("no matching slot")),
        "{notes:?}"
    );
}

#[test]
fn native_conversion_drops_items_the_target_schema_does_not_define() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("investment.sqlite3");
    super::super::tests::create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    // The emote collection arrived with account schema 13. Dawn v6 has no bucket for it, and one
    // unresolvable item makes Dawn abandon the whole character loadout rather than skip the item.
    db.execute(
        "UPDATE items SET definition_hash=? WHERE location=1 AND position=0",
        [crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH],
    )
    .unwrap();
    let super::super::SqliteAccountDocumentLoad::Loaded(document) =
        super::super::load_document(&path).unwrap()
    else {
        panic!("Account not loaded")
    };
    let defaults: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/dawn-v6-42fc41e-defaults.json"
    ))
    .unwrap();
    assert_eq!(defaults["version"], 6);
    let mut notes = Vec::new();
    let result = to_json(&document, &defaults, &mut notes, &|_| true).unwrap();
    let inventory = result["state"]["characters"][0]["inventory"]
        .as_array()
        .unwrap();
    assert!(
        inventory.is_empty(),
        "schema 13 definition must not reach a v6 account: {inventory:?}"
    );
    assert!(
        notes.iter().any(|text| text.contains("cannot store")),
        "{notes:?}"
    );
}

#[test]
fn native_conversion_drops_items_the_installed_build_cannot_resolve() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("investment.sqlite3");
    super::super::tests::create_fixture(&path, 3);
    let super::super::SqliteAccountDocumentLoad::Loaded(document) =
        super::super::load_document(&path).unwrap()
    else {
        panic!("Account not loaded")
    };
    let defaults: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/dawn-v6-42fc41e-defaults.json"
    ))
    .unwrap();
    let mut notes = Vec::new();
    // Definition 300 is the fixture's only inventory item and the catalog cannot resolve it.
    let result = to_json(&document, &defaults, &mut notes, &|hash| hash != 300).unwrap();
    assert!(
        result["state"]["characters"][0]["inventory"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        notes.iter().any(|text| text.contains("cannot store")),
        "{notes:?}"
    );
}
