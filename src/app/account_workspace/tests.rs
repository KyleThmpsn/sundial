//! Account workspace routing and source-selection tests.

use crate::app::account_workspace as account;

#[cfg(feature = "sqlite-account")]
use std::fs;

#[cfg(feature = "sqlite-account")]
use rusqlite::Connection;
use serde_json::json;
#[cfg(feature = "sqlite-account")]
use sundial_account::{
    AccountSettingKey, AccountSettingValue, AccountSettingsCommand, CharacterAbilities,
    CharacterMetadataUpdate,
};

#[cfg(feature = "sqlite-account")]
use crate::catalog::{AbilityChoice, AbilityOptions, AttunementChoice, ItemDef};

#[cfg(feature = "sqlite-account")]
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
fn missing_database_keeps_existing_json_account_behavior() {
    let directory = TestDirectory::new("workspace-json-source");
    let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));

    assert_eq!(document.source_info().kind, AccountSourceKind::Json);
    assert_eq!(account::character_count(&document), 2);
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

#[cfg(not(feature = "sqlite-account"))]
#[test]
fn standard_build_ignores_retired_sqlite_sources_and_source_transitions() {
    let directory = TestDirectory::new("workspace-no-sqlite-feature");
    let settings_path = settings_path(&directory);
    let json_document = WorkspaceDocument::load(json_characters(2), &settings_path);
    assert_eq!(json_document.source_info().kind, AccountSourceKind::Json);

    std::fs::write(directory.0.join("state.sqlite3"), b"SQLite source").unwrap();
    assert_eq!(json_document.verify_account_source_unchanged(), Ok(()));

    let document = WorkspaceDocument::load(json_characters(2), &settings_path);
    assert_eq!(document.source_info().kind, AccountSourceKind::Json);
    assert!(document.account_editing_blocked().is_none());
    assert_eq!(account::character_count(&document), 2);
    assert!(!document.source_info().detail.contains("sqlite"));
    assert_eq!(
        std::fs::read(directory.0.join("state.sqlite3")).unwrap(),
        b"SQLite source"
    );
}

#[test]
fn json_source_materializes_schema_eight_preferences_through_schema_thirteen() {
    for version in [8, crate::game_settings::MAX_SUPPORTED_SCHEMA] {
        let directory = TestDirectory::new(&format!("workspace-json-normalization-{version}"));
        let document = WorkspaceDocument::load(
            json!({
                "version": version,
                "state": {"account": {"settings": {"display": {}}}}
            }),
            &settings_path(&directory),
        );

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

#[cfg(feature = "sqlite-account")]
#[test]
fn empty_and_uninitialized_databases_keep_json_account_behavior() {
    for name in ["zero-byte", "uninitialized"] {
        let directory = TestDirectory::new(name);
        let database_path = directory.0.join("state.sqlite3");
        if name == "zero-byte" {
            fs::File::create(&database_path).unwrap();
        } else {
            drop(Connection::open(&database_path).unwrap());
        }
        let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));

        assert_eq!(document.source_info().kind, AccountSourceKind::Json);
        assert_eq!(account::character_count(&document), 2);
    }
}

#[cfg(feature = "sqlite-account")]
#[test]
fn exact_pr88_database_is_authoritative_over_stale_json_account_data() {
    let directory = TestDirectory::new("workspace-sqlite-source");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("state.sqlite3"),
        3,
    );
    let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));

    assert_eq!(document.source_info().kind, AccountSourceKind::Sqlite);
    assert_eq!(account::character_count(&document), 1);
}

#[cfg(feature = "sqlite-account")]
#[test]
fn json_workspace_requires_reload_when_sqlite_becomes_authoritative() {
    let directory = TestDirectory::new("workspace-source-transition");
    let settings_path = settings_path(&directory);
    let document = WorkspaceDocument::load(json_characters(2), &settings_path);
    assert_eq!(document.verify_account_source_unchanged(), Ok(()));

    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("state.sqlite3"),
        3,
    );

    let error = document.verify_account_source_unchanged().unwrap_err();
    assert!(error.contains("became authoritative"));
    assert_eq!(document.source_info().kind, AccountSourceKind::Json);
}

#[cfg(feature = "sqlite-account")]
#[test]
fn missing_to_empty_database_transition_keeps_legacy_json_source() {
    let directory = TestDirectory::new("workspace-empty-transition");
    let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));
    fs::File::create(directory.0.join("state.sqlite3")).unwrap();

    assert_eq!(document.verify_account_source_unchanged(), Ok(()));
}

#[cfg(feature = "sqlite-account")]
#[test]
fn sqlite_source_does_not_materialize_stale_json_account_preferences() {
    let directory = TestDirectory::new("workspace-sqlite-no-json-normalization");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("state.sqlite3"),
        3,
    );
    let json = json!({
        "version": 8,
        "state": {"account": {"settings": {"display": {}}}}
    });
    let document = WorkspaceDocument::load(json.clone(), &settings_path(&directory));

    assert_eq!(document.source_info().kind, AccountSourceKind::Sqlite);
    assert_eq!(document.json(), &json);
}

#[cfg(feature = "sqlite-account")]
#[test]
fn sqlite_workspace_validation_ignores_stale_json_account_domains() {
    let directory = TestDirectory::new("workspace-sqlite-validation");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("state.sqlite3"),
        3,
    );
    let stale_json = json!({
        "version": 8,
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
    let json_document = WorkspaceDocument::load(stale_json, &settings_path(&json_directory));
    assert!(super::super::settings::validate_workspace_document(&json_document).is_err());
}

#[cfg(feature = "sqlite-account")]
#[test]
fn sqlite_facade_mutates_every_account_domain_without_touching_json() {
    let directory = TestDirectory::new("workspace-sqlite-mutations");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("state.sqlite3"),
        3,
    );
    let mut document = WorkspaceDocument::load(
        json!({
            "version": 8,
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

#[cfg(feature = "sqlite-account")]
#[test]
fn sqlite_subclass_abilities_follow_the_owned_item_across_an_equip_swap() {
    let directory = TestDirectory::new("workspace-sqlite-subclass-abilities");
    let database_path = directory.0.join("state.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database_path, 3);
    set_fixture_inventory_subclass_abilities(&database_path, [6, 8, 20, 21, 3]);
    let mut document = WorkspaceDocument::load(
        json!({"version": 8, "state": {}}),
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

#[cfg(feature = "sqlite-account")]
#[test]
fn sqlite_subclass_equip_replaces_an_invalid_persisted_selection_with_defaults() {
    let directory = TestDirectory::new("workspace-sqlite-invalid-subclass-abilities");
    let database_path = directory.0.join("state.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database_path, 3);
    set_fixture_inventory_subclass_abilities(&database_path, [63, 63, 63, 63, 63]);
    let mut document = WorkspaceDocument::load(
        json!({"version": 8, "state": {}}),
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

#[cfg(feature = "sqlite-account")]
#[test]
fn corrupt_or_incompatible_database_blocks_stale_json_fallback() {
    let corrupt = TestDirectory::new("workspace-corrupt-source");
    fs::write(corrupt.0.join("state.sqlite3"), b"not a SQLite database").unwrap();
    let corrupt_document = WorkspaceDocument::load(json_characters(2), &settings_path(&corrupt));
    assert_eq!(
        corrupt_document.source_info().kind,
        AccountSourceKind::Blocked
    );
    assert_eq!(account::character_count(&corrupt_document), 0);

    let incompatible = TestDirectory::new("workspace-incompatible-source");
    let connection = Connection::open(incompatible.0.join("state.sqlite3")).unwrap();
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

#[cfg(feature = "sqlite-account")]
fn set_fixture_inventory_subclass_abilities(database_path: &std::path::Path, abilities: [u8; 5]) {
    let connection = Connection::open(database_path).unwrap();
    connection
        .execute(
            "UPDATE character_items SET definition_hash = 201, quantity = 1, \
             movement_ability_entry = ?, grenade_ability_entry = ?, \
             super_ability_entry = ?, melee_ability_entry = ?, class_ability_entry = ? \
             WHERE account_id = 1 AND character_position = 0 AND location = 1 \
             AND position = 0;",
            abilities,
        )
        .unwrap();
}

#[cfg(feature = "sqlite-account")]
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
    json!({"state": {"characters": vec![json!({}); count]}})
}

#[cfg(feature = "sqlite-account")]
#[test]
fn v13_json_does_not_upgrade_the_sqlite_equipment_contract() {
    let directory = TestDirectory::new("workspace-sqlite-v13-gates");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("state.sqlite3"),
        3,
    );
    let mut document = WorkspaceDocument::load(json!({"version":13}), &settings_path(&directory));
    assert!(!document.supports_v13_account());
    assert_eq!(document.equipment_slots().len(), 16);
    assert!(
        document
            .equipment_slots()
            .iter()
            .all(|(slot, _, _)| *slot != "artifact")
    );
    // PR88 already supports opaque u32 flags, independent of the JSON settings version.
    assert_eq!(
        crate::persistence::sqlite_account::SqliteAccountDocument::character_capabilities()
            .item_flag_mask,
        u32::MAX
    );
    account::set_equipment_item_flags(&mut document, 0, "kinetic", Some(4)).unwrap();
    let original = document.clone();
    assert!(account::equip_definition(&mut document, 0, "artifact", 42, &[]).is_err());
    assert!(
        account::equip_definition(
            &mut document,
            0,
            "emote",
            crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH,
            &[]
        )
        .is_err()
    );
    assert_eq!(document, original);
}
