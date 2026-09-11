//! Account workspace routing and source-selection tests.

mod sqlite_inventory;
mod sqlite_smoke;

use crate::app::account_workspace as account;

use std::fs;

use rusqlite::Connection;
use serde_json::json;
use sundial_account::{
    AccountSettingKey, AccountSettingValue, AccountSettingsCommand, CharacterAbilities,
    CharacterMetadataUpdate,
};

use crate::catalog::{AbilityChoice, AbilityOptions, AttunementChoice, ItemDef};

use super::super::inventory::{
    InventoryItemAction, InventoryItemLocation, ProfileItemAction, ProfileItemLocation,
};
use super::{AccountDocument, AccountSourceKind, WorkspaceDocument};
use crate::test_support::TestDirectory;

#[test]
fn json_workspace_exposes_neutral_character_metadata() {
    let document = WorkspaceDocument::json_only(json!({
        "version": 8,
        "state": {"characters": [{
            "race": 2,
            "gender": 1,
            "class": 2,
            "movement_ability": 6,
            "grenade_ability": 9,
            "super_ability": 20,
            "melee_ability": 21,
            "class_ability": 3
        }]}
    }));

    let metadata = account::character_metadata(&document, 0).unwrap();
    assert_eq!(metadata.class_type, 2);
    assert_eq!(metadata.abilities.super_ability, 20);
}

#[test]
fn blocked_account_operations_never_read_or_mutate_stale_json() {
    let original = json!({
        "version": 8,
        "state": {
            "characters": [{"equipment": {"kinetic": {"level": 75}}}],
            "account": {"profile_items": [], "settings": {}}
        }
    });
    let mut document = WorkspaceDocument::json_only(original.clone());
    document.account = AccountDocument::Blocked("Account unavailable".into());

    assert_eq!(account::character_count(&document), 0);
    assert!(!account::can_mutate_equipment(&document));
    assert!(!account::profile_items_editable(&document));
    assert!(account::character_metadata(&document, 0).is_err());
    assert!(account::equipped_item_snapshots(&document, 0).is_err());
    assert!(account::profile_items(&document).is_err());
    assert!(account::account_settings_map(&document).is_err());
    assert!(account::set_equipment_item_level(&mut document, 0, "kinetic", 80).is_err());
    assert!(account::add_profile_item(&mut document, 1, 1).is_err());
    assert!(account::apply_account_settings(&mut document, Vec::new()).is_err());
    assert_eq!(document.json(), &original);
}

#[test]
fn json_account_change_tracking_excludes_game_settings() {
    let original = WorkspaceDocument::json_only(json!({
        "state": {
            "account": {
                "profile_items": [],
                "settings": {"display": {"brightness": 3}}
            },
            "characters": []
        }
    }));

    let mut settings_only = original.clone();
    settings_only.json_mut()["state"]["account"]["settings"]["display"]["brightness"] = json!(4);
    assert!(!settings_only.json_account_changed_from(&original));

    let mut account_edit = original.clone();
    account_edit.json_mut()["state"]["account"]["profile_items"] = json!([{"definition_hash": 1}]);
    assert!(account_edit.json_account_changed_from(&original));

    let mut character_edit = original.clone();
    character_edit.json_mut()["state"]["characters"] = json!([{"soid": 1}]);
    assert!(character_edit.json_account_changed_from(&original));
}

#[test]
fn json_source_materializes_preferences_before_sqlite_schema() {
    for version in [8, 16, 17] {
        let directory = TestDirectory::new(&format!("workspace-json-normalization-{version}"));
        let document = WorkspaceDocument::load(
            json!({
                "version": version,
                "state": {"characters": [{}, {}], "account": {"settings": {"display": {}}}}
            }),
            &settings_path(&directory),
        );

        assert_eq!(document.source_info().kind, AccountSourceKind::Json);
        assert_eq!(account::character_count(&document), 2);

        assert_eq!(
            document
                .json()
                .pointer("/state/account/settings/key_binding_source"),
            Some(&json!("computer")),
            "schema {version}"
        );
        assert_eq!(
            document
                .json()
                .pointer("/state/account/settings/display/vertical_sync_interval"),
            Some(&json!(0)),
            "schema {version}"
        );
        assert_eq!(
            document
                .json()
                .pointer("/state/account/settings/display/field_of_view"),
            Some(&json!(85)),
            "schema {version}"
        );
    }
}

#[test]
fn empty_database_blocks_stale_json() {
    let directory = TestDirectory::new("workspace-empty-source");
    fs::create_dir_all(directory.0.join("data")).unwrap();
    fs::File::create(directory.0.join("data").join("investment.sqlite3")).unwrap();
    let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));

    assert_eq!(document.source_info().kind, AccountSourceKind::Blocked);
    assert_eq!(account::character_count(&document), 0);
}

#[test]
fn official_database_is_authoritative_and_preserves_inactive_json() {
    let directory = TestDirectory::new("workspace-sqlite-source");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("data").join("investment.sqlite3"),
        3,
    );
    let mut json = json_characters(2);
    json["state"]["account"] = json!({"settings": {"display": {}}});
    let document = WorkspaceDocument::load(json.clone(), &settings_path(&directory));

    assert_eq!(document.source_info().kind, AccountSourceKind::Sqlite);
    assert_eq!(account::character_count(&document), 1);
    assert_eq!(document.json(), &json);
}

#[test]
fn old_schemas_ignore_databases_before_and_after_loading() {
    for version in [6, 8, 13, 17] {
        for database_kind in [0, 1, 2] {
            let directory = TestDirectory::new("workspace-old-schema-database");
            let path = settings_path(&directory);
            let json = json!({"version": version, "state": {"characters": [{}, {}]}});
            let document = WorkspaceDocument::load(json.clone(), &path);
            let database = directory.0.join("data/investment.sqlite3");
            fs::create_dir_all(database.parent().unwrap()).unwrap();
            match database_kind {
                0 => fs::write(&database, b"").unwrap(),
                1 => fs::write(&database, b"not SQLite").unwrap(),
                _ => crate::persistence::sqlite_account::tests::create_fixture(&database, 3),
            }
            let original = fs::read(&database).unwrap();
            assert_eq!(document.verify_account_source_unchanged(), Ok(()));
            let loaded = WorkspaceDocument::load(json, &path);
            assert_eq!(loaded.source_info().kind, AccountSourceKind::Json);
            assert_eq!(account::character_count(&loaded), 2);
            assert_eq!(loaded.verify_account_source_unchanged(), Ok(()));
            assert_eq!(fs::read(&database).unwrap(), original);
        }
    }
}

#[test]
fn schema18_requires_a_database_even_when_json_contains_characters() {
    let directory = TestDirectory::new("workspace-v18-missing");
    let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));
    assert_eq!(document.source_info().kind, AccountSourceKind::Blocked);
    assert_eq!(account::character_count(&document), 0);
}

#[test]
fn sqlite_workspace_validation_ignores_stale_json_account_domains() {
    let directory = TestDirectory::new("workspace-sqlite-validation");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("data").join("investment.sqlite3"),
        3,
    );
    let stale_json = json!({
        "version": 18,
        "state": {
            "account": {"settings": "stale and invalid"},
            "characters": "stale and invalid"
        }
    });
    let sqlite_document = WorkspaceDocument::load(stale_json.clone(), &settings_path(&directory));
    assert_eq!(
        super::super::settings::validate_workspace_document(&sqlite_document),
        Ok(())
    );

    let json_directory = TestDirectory::new("workspace-json-validation");
    let mut stale_json = stale_json;
    stale_json["version"] = json!(8);
    let json_document = WorkspaceDocument::load(stale_json, &settings_path(&json_directory));
    assert!(super::super::settings::validate_workspace_document(&json_document).is_err());
}

#[test]
fn sqlite_facade_mutates_every_account_domain_without_touching_json() {
    let directory = TestDirectory::new("workspace-sqlite-mutations");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("data").join("investment.sqlite3"),
        3,
    );
    let mut document = WorkspaceDocument::load(
        json!({
            "version": 18,
            "state": {
                "account": {"settings": {"sentinel": true}},
                "characters": [{"sentinel": true}]
            }
        }),
        &settings_path(&directory),
    );
    let persisted = document.clone();
    let json_before = document.json().clone();

    account::apply_profile_item_action(
        &mut document,
        ProfileItemLocation { index: 0 },
        ProfileItemAction::SetQuantity(26),
    )
    .unwrap();
    account::apply_inventory_item_action(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        InventoryItemAction::SetQuantity(3),
    )
    .unwrap();
    account::set_equipment_item_level(&mut document, 0, "kinetic", 104).unwrap();
    account::apply_character_updates(
        &mut document,
        0,
        vec![CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
            movement: 5,
            grenade: 8,
            super_ability: 20,
            melee: 21,
            class_ability: 3,
        })],
    )
    .unwrap();
    account::apply_account_settings(
        &mut document,
        vec![AccountSettingsCommand::Set {
            key: AccountSettingKey::known_preference("show_fps").unwrap(),
            value: AccountSettingValue::Boolean(false),
        }],
    )
    .unwrap();

    assert_eq!(
        account::profile_items(&document).unwrap().unwrap()[0].quantity,
        26
    );
    assert_eq!(
        account::character_inventory(&document, 0).unwrap().unwrap()[0].quantity,
        3
    );
    assert_eq!(
        account::equipped_item_snapshots(&document, 0)
            .unwrap()
            .into_iter()
            .find(|item| item.slot == "kinetic")
            .unwrap()
            .level,
        Some(104)
    );
    assert_eq!(
        account::character_metadata(&document, 0)
            .unwrap()
            .abilities
            .movement,
        5
    );
    let settings = serde_json::Value::Object(account::account_settings_map(&document).unwrap());
    assert_eq!(
        settings.pointer("/display/show_fps"),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(document.json(), &json_before);
    assert!(document.account_changed_from(&persisted));
    assert!(!document.json_changed_from(&persisted));
    let summaries = document.account_change_summaries(&persisted, 20);
    assert!(
        summaries
            .iter()
            .any(|summary| summary.contains("/profile_items/") && summary.contains("/quantity"))
    );
    assert!(
        summaries
            .iter()
            .any(|summary| summary.contains("/inventory/") && summary.contains("/quantity"))
    );
    assert!(
        summaries
            .iter()
            .any(|summary| summary.contains("/equipment/kinetic/power"))
    );
    assert!(
        summaries
            .iter()
            .any(|summary| summary.contains("/metadata"))
    );
    assert!(
        summaries
            .iter()
            .any(|summary| summary.contains("/account_settings/display/show_fps"))
    );
}

#[test]
fn sqlite_subclass_abilities_follow_the_owned_item_across_an_equip_swap() {
    let directory = TestDirectory::new("workspace-sqlite-subclass-abilities");
    let database_path = directory.0.join("data").join("investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database_path, 3);
    set_fixture_inventory_subclass_abilities(&database_path, [6, 8, 20, 21, 3]);
    let mut document = WorkspaceDocument::load(
        json!({"version": 18, "state": {}}),
        &settings_path(&directory),
    );
    let outgoing_selection = CharacterAbilities {
        movement: 5,
        grenade: 7,
        super_ability: 10,
        melee: 11,
        class_ability: 2,
    };

    let outgoing_item_id = match &document.account {
        AccountDocument::Sqlite(sqlite) => {
            sqlite.characters().characters()[0].equipment
                [&sundial_account::EquipmentSlot::new("subclass")]
                .as_ref()
                .unwrap()
                .id
        }
        _ => panic!("fixture should select SQLite"),
    };
    account::apply_character_updates(
        &mut document,
        0,
        vec![CharacterMetadataUpdate::SetAbilities(outgoing_selection)],
    )
    .unwrap();
    match &document.account {
        AccountDocument::Sqlite(sqlite) => assert_eq!(
            sqlite.persisted_item_abilities(outgoing_item_id),
            Some(outgoing_selection)
        ),
        _ => panic!("fixture should select SQLite"),
    }

    let incoming_selection = CharacterAbilities {
        movement: 6,
        grenade: 8,
        super_ability: 20,
        melee: 21,
        class_ability: 3,
    };
    let replaced = super::super::equipment::equip_inventory_item(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        "subclass",
        &sqlite_subclass_item(),
        false,
    )
    .unwrap();

    assert!(replaced);
    assert_eq!(
        account::character_metadata(&document, 0).unwrap().abilities,
        incoming_selection
    );
    match &document.account {
        AccountDocument::Sqlite(sqlite) => {
            let character = &sqlite.characters().characters()[0];
            let equipped = character.equipment[&sundial_account::EquipmentSlot::new("subclass")]
                .as_ref()
                .unwrap();
            assert_eq!(
                sqlite.persisted_item_abilities(equipped.id),
                Some(incoming_selection)
            );
            assert_eq!(
                sqlite.persisted_item_abilities(character.inventory[0].id),
                Some(outgoing_selection)
            );
        }
        _ => panic!("fixture should select SQLite"),
    }
}

#[test]
fn sqlite_subclass_equip_replaces_an_invalid_persisted_selection_with_defaults() {
    let directory = TestDirectory::new("workspace-sqlite-invalid-subclass-abilities");
    let database_path = directory.0.join("data").join("investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database_path, 3);
    set_fixture_inventory_subclass_abilities(&database_path, [63, 63, 63, 63, 63]);
    let mut document = WorkspaceDocument::load(
        json!({"version": 18, "state": {}}),
        &settings_path(&directory),
    );

    super::super::equipment::equip_inventory_item(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        "subclass",
        &sqlite_subclass_item(),
        false,
    )
    .unwrap();

    let defaults = CharacterAbilities {
        movement: 5,
        grenade: 7,
        super_ability: 10,
        melee: 11,
        class_ability: 2,
    };
    assert_eq!(
        account::character_metadata(&document, 0).unwrap().abilities,
        defaults
    );
    match &document.account {
        AccountDocument::Sqlite(sqlite) => {
            let equipped = sqlite.characters().characters()[0].equipment
                [&sundial_account::EquipmentSlot::new("subclass")]
                .as_ref()
                .unwrap();
            assert_eq!(sqlite.persisted_item_abilities(equipped.id), Some(defaults));
        }
        _ => panic!("fixture should select SQLite"),
    }
}

#[test]
fn corrupt_or_incompatible_database_blocks_stale_json_fallback() {
    let corrupt = TestDirectory::new("workspace-corrupt-source");
    fs::create_dir_all(corrupt.0.join("data")).unwrap();
    fs::write(
        corrupt.0.join("data").join("investment.sqlite3"),
        b"not a SQLite database",
    )
    .unwrap();
    let corrupt_document = WorkspaceDocument::load(json_characters(2), &settings_path(&corrupt));
    assert_eq!(
        corrupt_document.source_info().kind,
        AccountSourceKind::Blocked
    );
    assert_eq!(account::character_count(&corrupt_document), 0);

    let incompatible = TestDirectory::new("workspace-incompatible-source");
    fs::create_dir_all(incompatible.0.join("data")).unwrap();
    let connection =
        Connection::open(incompatible.0.join("data").join("investment.sqlite3")).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);
    let incompatible_document =
        WorkspaceDocument::load(json_characters(2), &settings_path(&incompatible));
    assert_eq!(
        incompatible_document.source_info().kind,
        AccountSourceKind::Blocked
    );
    assert_eq!(account::character_count(&incompatible_document), 0);
}

fn set_fixture_inventory_subclass_abilities(database_path: &std::path::Path, abilities: [u8; 5]) {
    let connection = Connection::open(database_path).unwrap();
    connection
        .execute(
            "UPDATE items SET definition_hash = 201, quantity = 1, \
             movement_ability = ?, grenade_ability = ?, \
             super_ability = ?, melee_ability = ?, class_ability = ? \
             WHERE character_slot = 0 AND location = 1 \
             AND position = 0;",
            abilities,
        )
        .unwrap();
}

fn sqlite_subclass_item() -> ItemDef {
    let choice = |entry| AbilityChoice {
        entry,
        name: format!("Entry {entry}"),
    };
    ItemDef {
        hash: 201,
        name: "Stored subclass".to_owned(),
        type_name: "Warlock Subclass".to_owned(),
        bucket_hash: 3_284_755_031,
        class_type: 2,
        default_plugs: Vec::new(),
        sockets: Vec::new(),
        abilities: AbilityOptions {
            movement: vec![choice(5), choice(6)],
            grenade: vec![choice(7), choice(8)],
            super_ability: vec![choice(10), choice(20)],
            melee: vec![choice(11), choice(21)],
            class_ability: vec![choice(2), choice(3)],
            attunements: vec![
                AttunementChoice {
                    name: "Default path".to_owned(),
                    super_abilities: vec![choice(10)],
                    melee: choice(11),
                    perks: Vec::new(),
                },
                AttunementChoice {
                    name: "Alternate path".to_owned(),
                    super_abilities: vec![choice(20)],
                    melee: choice(21),
                    perks: Vec::new(),
                },
            ],
        },
    }
}

fn settings_path(directory: &TestDirectory) -> std::path::PathBuf {
    directory.0.join("settings.json")
}

fn json_characters(count: usize) -> serde_json::Value {
    json!({"version": 18, "state": {"characters": vec![json!({}); count]}})
}

#[test]
fn official_sqlite_equipment_contract_is_available_for_schema18() {
    let directory = TestDirectory::new("workspace-sqlite-v13-gates");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("data/investment.sqlite3"),
        3,
    );
    let mut document = WorkspaceDocument::load(json!({"version":18}), &settings_path(&directory));
    assert!(document.supports_v13_account());
    assert_eq!(document.equipment_slots().len(), 17);
    assert_eq!(
        crate::persistence::sqlite_account::SqliteAccountDocument::character_capabilities()
            .item_flag_mask,
        7
    );
    account::set_equipment_item_flags(&mut document, 0, "kinetic", Some(4)).unwrap();
    account::equip_definition(&mut document, 0, "artifact", 42, &[]).unwrap();
    assert!(
        account::equipped_item_snapshots(&document, 0)
            .unwrap()
            .iter()
            .any(|item| item.slot == "artifact" && item.definition_hash == Some(42))
    );
    let AccountDocument::Sqlite(native) = &mut document.account else {
        panic!("expected SQLite")
    };
    crate::persistence::sqlite_account::tests::save_fixture_document(
        native,
        &directory.0.join("artifact-backup.sqlite3"),
    );
    let reopened = WorkspaceDocument::load(json!({"version":18}), &settings_path(&directory));
    assert!(
        account::equipped_item_snapshots(&reopened, 0)
            .unwrap()
            .iter()
            .any(|item| item.slot == "artifact" && item.definition_hash == Some(42))
    );
}

#[test]
fn v18_runtime_and_progression_edits_use_native_domains_and_preserve_inactive_json() {
    let directory = TestDirectory::new("workspace-v18-domains");
    let path = directory.0.join("data/investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("INSERT INTO characters SELECT 1,soid+1,race,gender,class,level,preview_available,appearance_value,last_orbited_destination,content_bypass,equipped_title,acquired_subclass_mask,next_inventory_serial FROM characters; INSERT INTO unlocks VALUES(0,2,42,0,2); INSERT INTO unlocks VALUES(1,2,50,0,2);").unwrap();
    let json = json!({"version":18,"state":{"account":{"opaque":true},"characters":["legacy"],"unlocks":{"opaque":true}},"server":{"entitlements":["legacy"]}});
    let mut document = WorkspaceDocument::load(json.clone(), &settings_path(&directory));
    let original = document.clone();
    let mut runtime = document.runtime_view();
    runtime["server"]["entitlements"] = json!([{"name":"123","owned":"application"}]);
    runtime["state"]["account"]["profile_setup_completed"] = json!(false);
    runtime["state"]["characters"][1]["content_bypass"] = json!(true);
    document.apply_runtime_view(runtime).unwrap();
    let mut progression = document.progression_view(1);
    progression["state"]["unlocks"]["character_flags"] = json!([51]);
    document.apply_progression_view(1, progression).unwrap();
    assert_eq!(document.json(), &json);
    let summaries = super::sqlite_change_summaries(
        match &original.account {
            AccountDocument::Sqlite(v) => v,
            _ => panic!(),
        },
        match &document.account {
            AccountDocument::Sqlite(v) => v,
            _ => panic!(),
        },
        50,
    );
    for domain in ["runtime", "entitlements", "progression"] {
        assert!(
            summaries.iter().any(|s| s.contains(domain)),
            "{domain}: {summaries:?}"
        );
    }
    let AccountDocument::Sqlite(native) = &mut document.account else {
        panic!()
    };
    crate::persistence::sqlite_account::tests::save_fixture_document(
        native,
        &directory.0.join("backup.sqlite3"),
    );
    assert!(
        !db.query_row("SELECT profile_setup_completed FROM account", [], |r| r
            .get::<_, bool>(0))
            .unwrap()
    );
    assert!(
        db.query_row(
            "SELECT content_bypass FROM characters WHERE slot=1",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert_eq!(
        db.query_row(
            "SELECT slot FROM unlocks WHERE character_slot=0 AND bank=2",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        42
    );
    assert_eq!(
        db.query_row(
            "SELECT slot FROM unlocks WHERE character_slot=1 AND bank=2",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        51
    );
    assert_eq!(
        db.query_row("SELECT ownership FROM entitlements", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        2
    );
}

#[test]
fn native_runtime_drafts_remain_editable_but_invalid_values_cannot_be_saved() {
    let directory = TestDirectory::new("sqlite-runtime-draft");
    let path = directory.0.join("data/investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&path, 3);
    let mut document = WorkspaceDocument::load(json!({"version":18}), &settings_path(&directory));
    let before = crate::persistence::sqlite_account::package::read(&path).unwrap();
    let mut draft = document.runtime_view();
    draft["server"]["entitlements"] = json!([{"name":"","owned":"handle"}]);
    draft["state"]["characters"][0]["last_orbited_destination"] = json!("0x");
    document.apply_runtime_view(draft).unwrap();
    assert!(document.save_sqlite().is_err());
    assert_eq!(
        crate::persistence::sqlite_account::package::read(&path).unwrap(),
        before
    );
    let mut draft = document.runtime_view();
    draft["server"]["entitlements"][0]["name"] = json!("test");
    draft["state"]["characters"][0]["last_orbited_destination"] = json!("0x1234");
    document.apply_runtime_view(draft).unwrap();
    let AccountDocument::Sqlite(native) = &mut document.account else {
        panic!()
    };
    crate::persistence::sqlite_account::tests::save_fixture_document(
        native,
        &directory.0.join("backup.sqlite3"),
    );
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT last_orbited_destination FROM characters", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0x1234
    );
}
