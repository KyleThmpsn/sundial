use std::{fs, path::Path};

use rusqlite::{Connection, params};
use serde_json::json;
use sundial_account::{
    AccountSettingKey, AccountSettingValue, DismantleGearClass, EquipmentSlot, ItemInstance,
    ItemPlugs, KeyBindingSlot, ProfileItemCommand,
};

use super::{
    SqliteAccountDocument, SqliteAccountDocumentLoad, SqliteAccountError,
    SqliteAccountIncompatibility, SqliteAccountLoad, SqliteAccountSnapshot, contract::PR_88_COMMIT,
    document, load, settings, writer,
};
use crate::persistence::json_account::{JsonCharacterAdapter, JsonProfileAdapter};
use crate::test_support::TestDirectory;

const SCHEMA_V1: &str = r#"
CREATE TABLE account_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    format_version INTEGER NOT NULL,
    primary_soid INTEGER NOT NULL,
    dismantle_reward_count INTEGER NOT NULL CHECK (dismantle_reward_count >= 0),
    profile_item_count INTEGER NOT NULL CHECK (profile_item_count >= 0),
    character_count INTEGER NOT NULL CHECK (character_count >= 0),
    settings_payload BLOB NOT NULL,
    updated_unix_seconds INTEGER NOT NULL
);
CREATE TABLE dismantle_rewards (
    account_id INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    definition_hash INTEGER NOT NULL CHECK (definition_hash >= 0),
    quantity INTEGER NOT NULL,
    tier_mask INTEGER NOT NULL CHECK (tier_mask BETWEEN 0 AND 255),
    class_mask INTEGER NOT NULL CHECK (class_mask BETWEEN 0 AND 255),
    masterwork INTEGER NOT NULL CHECK (masterwork BETWEEN 0 AND 255),
    PRIMARY KEY (account_id, position),
    FOREIGN KEY (account_id) REFERENCES account_state(singleton) ON DELETE CASCADE
);
CREATE TABLE profile_items (
    account_id INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    instance_soid INTEGER NOT NULL,
    definition_hash INTEGER NOT NULL CHECK (definition_hash >= 0),
    quantity INTEGER NOT NULL,
    mutation_serial INTEGER NOT NULL,
    PRIMARY KEY (account_id, position),
    FOREIGN KEY (account_id) REFERENCES account_state(singleton) ON DELETE CASCADE
);
CREATE TABLE characters (
    account_id INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    soid INTEGER NOT NULL,
    selected INTEGER NOT NULL CHECK (selected IN (0, 1)),
    race INTEGER NOT NULL CHECK (race BETWEEN 0 AND 255),
    gender INTEGER NOT NULL CHECK (gender BETWEEN 0 AND 255),
    character_class INTEGER NOT NULL CHECK (character_class BETWEEN 0 AND 255),
    level INTEGER NOT NULL CHECK (level BETWEEN 0 AND 255),
    accepted INTEGER NOT NULL CHECK (accepted IN (0, 1)),
    preview_available INTEGER NOT NULL CHECK (preview_available IN (0, 1)),
    appearance_value REAL NOT NULL,
    last_orbited_destination INTEGER NOT NULL CHECK (last_orbited_destination >= 0),
    content_bypass INTEGER NOT NULL CHECK (content_bypass IN (0, 1)),
    acquired_subclass_ability_mask INTEGER NOT NULL,
    inventory_count INTEGER NOT NULL CHECK (inventory_count >= 0),
    next_inventory_serial INTEGER NOT NULL CHECK (next_inventory_serial >= 0),
    PRIMARY KEY (account_id, position),
    FOREIGN KEY (account_id) REFERENCES account_state(singleton) ON DELETE CASCADE
);
CREATE TABLE character_items (
    account_id INTEGER NOT NULL,
    character_position INTEGER NOT NULL,
    location INTEGER NOT NULL CHECK (location IN (0, 1)),
    position INTEGER NOT NULL CHECK (position >= 0),
    instance_soid INTEGER NOT NULL,
    definition_hash INTEGER NOT NULL CHECK (definition_hash >= 0),
    item_level INTEGER NOT NULL,
    quantity INTEGER NOT NULL,
    mutation_serial INTEGER NOT NULL,
    flags INTEGER NOT NULL CHECK (flags >= 0),
    socket_policy INTEGER NOT NULL CHECK (socket_policy BETWEEN 0 AND 255),
    plug_count INTEGER NOT NULL CHECK (plug_count >= 0),
    movement_ability_entry INTEGER NOT NULL CHECK (movement_ability_entry BETWEEN 0 AND 255),
    grenade_ability_entry INTEGER NOT NULL CHECK (grenade_ability_entry BETWEEN 0 AND 255),
    super_ability_entry INTEGER NOT NULL CHECK (super_ability_entry BETWEEN 0 AND 255),
    melee_ability_entry INTEGER NOT NULL CHECK (melee_ability_entry BETWEEN 0 AND 255),
    class_ability_entry INTEGER NOT NULL CHECK (class_ability_entry BETWEEN 0 AND 255),
    PRIMARY KEY (account_id, character_position, location, position),
    FOREIGN KEY (account_id, character_position)
        REFERENCES characters(account_id, position) ON DELETE CASCADE
);
CREATE TABLE item_plugs (
    account_id INTEGER NOT NULL,
    character_position INTEGER NOT NULL,
    location INTEGER NOT NULL,
    item_position INTEGER NOT NULL,
    plug_position INTEGER NOT NULL CHECK (plug_position >= 0),
    definition_hash INTEGER CHECK (definition_hash >= 0),
    PRIMARY KEY (
        account_id,
        character_position,
        location,
        item_position,
        plug_position
    ),
    FOREIGN KEY (account_id, character_position, location, item_position)
        REFERENCES character_items(account_id, character_position, location, position)
        ON DELETE CASCADE
);
PRAGMA user_version = 1;
"#;

#[test]
fn missing_database_is_reported_without_creating_any_files() {
    let directory = TestDirectory::new("sqlite-missing");
    let path = directory.0.join("state.sqlite3");

    assert_eq!(load(&path).unwrap(), SqliteAccountLoad::Missing);
    assert!(!path.exists());
    assert!(!sidecar(&path, "-wal").exists());
    assert!(!sidecar(&path, "-shm").exists());
}

#[test]
fn empty_and_uninitialized_databases_are_reported_as_empty() {
    let directory = TestDirectory::new("sqlite-empty");
    let zero_byte = directory.0.join("zero-byte.sqlite3");
    fs::File::create(&zero_byte).unwrap();
    assert_eq!(load(&zero_byte).unwrap(), SqliteAccountLoad::Empty);
    assert_eq!(
        document::load(&zero_byte).unwrap(),
        SqliteAccountDocumentLoad::Empty
    );

    let uninitialized = directory.0.join("uninitialized.sqlite3");
    drop(Connection::open(&uninitialized).unwrap());
    assert_eq!(load(&uninitialized).unwrap(), SqliteAccountLoad::Empty);
    assert_eq!(
        document::load(&uninitialized).unwrap(),
        SqliteAccountDocumentLoad::Empty
    );
}

fn assert_pr88_profile(snapshot: &SqliteAccountSnapshot) {
    assert_eq!(snapshot.primary_soid().get(), 0x9EAA_3001_0010_0100);
    assert_eq!(snapshot.profile().profile_items().len(), 1);
    assert_eq!(
        snapshot.profile().profile_items()[0].definition_hash.get(),
        10
    );
    assert_eq!(snapshot.profile().profile_items()[0].quantity, 25);
    assert_eq!(snapshot.profile().dismantle_rewards().len(), 1);
    assert_eq!(snapshot.profile().dismantle_rewards()[0].rarities.len(), 1);
}

fn assert_pr88_character(snapshot: &SqliteAccountSnapshot) {
    let character = &snapshot.characters().characters()[0];
    assert_eq!(character.soid.unwrap().get(), 0x9EAA_3002_0010_0100);
    assert_eq!(character.inventory.len(), 1);
    assert_eq!(character.inventory[0].flags, Some(u32::MAX));
    assert_eq!(
        character.inventory[0].instance_soid.get(),
        0x4000_0000_0000_0003
    );
    assert_eq!(
        character.metadata.unwrap().abilities,
        sundial_account::CharacterAbilities {
            movement: 6,
            grenade: 8,
            super_ability: 20,
            melee: 21,
            class_ability: 3,
        }
    );
    let subclass = character.equipment[&EquipmentSlot::new("subclass")]
        .as_ref()
        .unwrap();
    assert_eq!(
        subclass.plugs,
        ItemPlugs::Authored(vec![Some(sundial_account::DefinitionHash::new(77)), None])
    );
}

fn assert_pr88_settings(snapshot: &SqliteAccountSnapshot) {
    assert_eq!(
        snapshot
            .settings()
            .values()
            .get(&AccountSettingKey::key_binding(
                "fire",
                KeyBindingSlot::Primary,
            )),
        Some(&AccountSettingValue::InputCode(42))
    );
}

#[test]
fn exact_pr88_snapshot_loads_losslessly_into_neutral_state() {
    let directory = TestDirectory::new("sqlite-pr88");
    let path = directory.0.join("state.sqlite3");
    create_fixture(&path, u32::MAX);
    let before = fs::read(&path).unwrap();

    let SqliteAccountLoad::Loaded(snapshot) = load(&path).unwrap() else {
        panic!("exact PR-88 fixture should load");
    };

    assert_eq!(PR_88_COMMIT, "5a5583ab0cc4244bca11974a928bdc1a0b49f4b7");
    assert_pr88_profile(&snapshot);
    assert_pr88_character(&snapshot);
    assert_pr88_settings(&snapshot);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!sidecar(&path, "-wal").exists());
    assert!(!sidecar(&path, "-shm").exists());
}

#[test]
fn wal_database_loads_without_modifying_the_main_database() {
    let directory = TestDirectory::new("sqlite-pr88-wal");
    let path = directory.0.join("state.sqlite3");
    create_fixture(&path, 3);
    let connection = Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    drop(connection);
    let before = fs::read(&path).unwrap();
    assert!(!sidecar(&path, "-wal").exists());
    assert!(!sidecar(&path, "-shm").exists());

    let loaded = document::load(&path).unwrap();

    assert!(matches!(loaded, SqliteAccountDocumentLoad::Loaded(_)));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn every_version_layer_is_probed_before_interpreting_rows() {
    let directory = TestDirectory::new("sqlite-versions");

    let schema_path = directory.0.join("newer-schema.sqlite3");
    let connection = Connection::open(&schema_path).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);
    let before = fs::read(&schema_path).unwrap();
    assert_eq!(
        load(&schema_path).unwrap(),
        SqliteAccountLoad::Incompatible(SqliteAccountIncompatibility::Schema {
            found: 2,
            supported: 1,
        })
    );
    assert_eq!(fs::read(&schema_path).unwrap(), before);

    let format_path = directory.0.join("newer-format.sqlite3");
    create_fixture(&format_path, 1);
    update(&format_path, "UPDATE account_state SET format_version = 2;");
    assert_eq!(
        load(&format_path).unwrap(),
        SqliteAccountLoad::Incompatible(SqliteAccountIncompatibility::AccountFormat {
            found: 2,
            supported: 1,
        })
    );

    let payload_path = directory.0.join("newer-payload.sqlite3");
    create_fixture(&payload_path, 1);
    let connection = Connection::open(&payload_path).unwrap();
    let mut payload = settings_payload();
    payload[..4].copy_from_slice(&2_u32.to_le_bytes());
    connection
        .execute("UPDATE account_state SET settings_payload = ?;", [payload])
        .unwrap();
    drop(connection);
    assert_eq!(
        load(&payload_path).unwrap(),
        SqliteAccountLoad::Incompatible(SqliteAccountIncompatibility::SettingsPayload {
            found: 2,
            supported: 1,
        })
    );
}

#[test]
fn same_version_layout_changes_and_count_mismatches_are_rejected() {
    let directory = TestDirectory::new("sqlite-invalid");
    let schema_path = directory.0.join("changed-schema.sqlite3");
    create_fixture(&schema_path, 1);
    update(&schema_path, "DROP TABLE item_plugs;");
    assert!(matches!(
        load(&schema_path),
        Err(SqliteAccountError::InvalidSchema(_))
    ));

    let count_path = directory.0.join("bad-count.sqlite3");
    create_fixture(&count_path, 1);
    update(&count_path, "UPDATE account_state SET character_count = 2;");
    let error = load(&count_path).unwrap_err().to_string();
    assert!(error.contains("root count is 2"), "{error}");
}

#[test]
fn positive_primary_soid_is_rejected_for_a_nonempty_account() {
    let directory = TestDirectory::new("sqlite-positive-primary-soid");
    let path = directory.0.join("state.sqlite3");
    create_fixture(&path, 1);
    update(
        &path,
        "UPDATE account_state SET primary_soid = 123 WHERE singleton = 1;",
    );

    let error = load(&path).unwrap_err().to_string();
    assert!(error.contains("account_state.primary_soid"), "{error}");
    assert!(error.contains("bit 63"), "{error}");
}

#[test]
fn sqlite_field_of_view_accepts_105_and_rejects_106() {
    let directory = TestDirectory::new("sqlite-field-of-view-range");
    let path = directory.0.join("state.sqlite3");
    let backup = directory.0.join("state-before.sqlite3");
    create_fixture(&path, 1);
    let key = AccountSettingKey::known_preference("field_of_view").unwrap();
    let mut document = loaded_document(&path);

    document
        .settings_mut()
        .apply_all(
            SqliteAccountDocument::settings_capabilities(),
            [sundial_account::AccountSettingsCommand::Set {
                key: key.clone(),
                value: AccountSettingValue::Unsigned(105),
            }],
        )
        .unwrap();
    writer::save_for_test(&mut document, backup).unwrap();

    let mut reloaded = loaded_document(&path);
    assert_eq!(
        reloaded.settings().values().get(&key),
        Some(&AccountSettingValue::Unsigned(105))
    );
    let before = reloaded.settings().clone();
    assert_eq!(
        reloaded.settings_mut().apply_all(
            SqliteAccountDocument::settings_capabilities(),
            [sundial_account::AccountSettingsCommand::Set {
                key,
                value: AccountSettingValue::Unsigned(106),
            }],
        ),
        Err(sundial_account::AccountError::InvalidAccountSettingValue)
    );
    assert_eq!(reloaded.settings(), &before);
}

#[test]
fn malformed_plug_prefix_is_rejected_instead_of_repaired() {
    let directory = TestDirectory::new("sqlite-plugs");
    let path = directory.0.join("state.sqlite3");
    create_fixture(&path, 1);
    update(
        &path,
        "DELETE FROM item_plugs WHERE character_position = 0 AND location = 0 \
         AND item_position = 11 AND plug_position = 0;",
    );

    let error = load(&path).unwrap_err().to_string();
    assert!(error.contains("item_plugs"), "{error}");
    assert!(error.contains("expected contiguous position 0"), "{error}");
}

#[test]
fn explicit_combined_class_mask_remains_distinct_from_no_filter() {
    let directory = TestDirectory::new("sqlite-class-mask");
    let path = directory.0.join("state.sqlite3");
    create_fixture(&path, 1);
    update(&path, "UPDATE dismantle_rewards SET class_mask = 3;");

    let SqliteAccountLoad::Loaded(snapshot) = load(&path).unwrap() else {
        panic!("valid combined class mask should load");
    };
    assert_eq!(
        snapshot.profile().dismantle_rewards()[0].gear_class,
        Some(DismantleGearClass::Both)
    );
}

#[test]
fn pr88_and_json_fixtures_produce_the_same_editable_domain_values() {
    let directory = TestDirectory::new("sqlite-json-parity");
    let path = directory.0.join("state.sqlite3");
    create_fixture(&path, 3);
    let SqliteAccountLoad::Loaded(sqlite) = load(&path).unwrap() else {
        panic!("valid SQLite fixture should load");
    };
    let document = json!({
        "version": 8,
        "state": {
            "account": {
                "primary_soid": "0x9EAA300100100100",
                "profile_items": [{"definition_hash": 10, "quantity": 25}],
                "dismantle_rewards": [{
                    "definition_hash": 20,
                    "quantity": 3,
                    "rarity": "legendary",
                    "class": "weapon",
                    "masterworked": true
                }]
            },
            "characters": [{
                "soid": "0x9EAA300200100100",
                "race": 2,
                "gender": 1,
                "class": 2,
                "movement_ability": 6,
                "grenade_ability": 8,
                "super_ability": 20,
                "melee_ability": 21,
                "class_ability": 3,
                "equipment": {
                    "kinetic": {
                        "instance_soid": "0x4000000000000001",
                        "definition_hash": 100,
                        "level": 106,
                        "quantity": 1,
                        "plugs": null,
                        "flags": 0
                    },
                    "subclass": {
                        "instance_soid": "0x4000000000000002",
                        "definition_hash": 200,
                        "level": 106,
                        "quantity": 1,
                        "plugs": [77, null],
                        "flags": 1
                    }
                },
                "inventory": [{
                    "instance_soid": "0x4000000000000003",
                    "definition_hash": 300,
                    "level": 105,
                    "quantity": 2,
                    "plugs": null,
                    "flags": 3
                }]
            }]
        }
    });
    let json_profile = JsonProfileAdapter::load(&document).unwrap();
    let json_characters = JsonCharacterAdapter::load(&document).unwrap();
    let json_metadata = JsonCharacterAdapter::load_character_metadata(&document, 0).unwrap();

    let sqlite_profile = sqlite.profile();
    assert_eq!(
        sqlite_profile
            .profile_items()
            .iter()
            .map(|item| (item.definition_hash, item.quantity))
            .collect::<Vec<_>>(),
        json_profile
            .state()
            .profile_items()
            .iter()
            .map(|item| (item.definition_hash, item.quantity))
            .collect::<Vec<_>>()
    );
    let sqlite_rewards = sqlite_profile.dismantle_rewards();
    let json_rewards = json_profile.state().dismantle_rewards();
    assert_eq!(sqlite_rewards.len(), json_rewards.len());
    assert_eq!(
        sqlite_rewards[0].definition_hash,
        json_rewards[0].definition_hash
    );
    assert_eq!(sqlite_rewards[0].quantity, json_rewards[0].quantity);
    assert_eq!(sqlite_rewards[0].rarities, json_rewards[0].rarities);
    assert_eq!(sqlite_rewards[0].gear_class, json_rewards[0].gear_class);
    assert_eq!(sqlite_rewards[0].masterworked, json_rewards[0].masterworked);

    let sqlite_character = &sqlite.characters().characters()[0];
    let json_character = &json_characters.state().characters()[0];
    assert_eq!(sqlite_character.soid, json_character.soid);
    assert_eq!(
        sqlite_character.metadata,
        json_metadata.state().characters()[0].metadata
    );
    assert_eq!(
        sqlite_character.inventory.len(),
        json_character.inventory.len()
    );
    assert_item_semantics(&sqlite_character.inventory[0], &json_character.inventory[0]);
    for slot in ["kinetic", "subclass"] {
        let slot = EquipmentSlot::new(slot);
        assert_item_semantics(
            sqlite_character.equipment[&slot].as_ref().unwrap(),
            json_character.equipment[&slot].as_ref().unwrap(),
        );
    }
}

#[test]
fn settings_payload_decode_encode_round_trip_is_exact() {
    let payload = settings_payload();
    let state = settings::decode(&payload, true).unwrap();

    assert_eq!(settings::encode(&state).unwrap(), payload);
}

#[test]
fn transactional_save_reloads_and_keeps_a_verified_pre_save_backup() {
    let directory = TestDirectory::new("sqlite-write");
    let path = directory.0.join("state.sqlite3");
    let backup = directory.0.join("state-before.sqlite3");
    create_fixture(&path, 3);
    let before = loaded_document(&path);
    let mut edited = before.clone();
    let item_id = edited.profile().profile_items()[0].id;
    edited
        .profile_mut()
        .apply_profile_item(
            SqliteAccountDocument::profile_capabilities(),
            ProfileItemCommand::SetQuantity {
                id: item_id,
                quantity: 26,
            },
        )
        .unwrap();

    let receipt = writer::save_for_test(&mut edited, backup.clone()).unwrap();

    assert_eq!(receipt.backup, backup);
    assert!(receipt.checkpoint_warning.is_none());
    let reloaded = loaded_document(&path);
    assert_eq!(reloaded, edited);
    assert_eq!(reloaded.profile().profile_items()[0].quantity, 26);
    let backed_up = loaded_document(&backup);
    assert_account_semantics(&backed_up, &before);
    assert_eq!(backed_up.profile().profile_items()[0].quantity, 25);
}

#[test]
fn externally_changed_source_rejects_save_without_mutating_the_document() {
    let directory = TestDirectory::new("sqlite-write-conflict");
    let path = directory.0.join("state.sqlite3");
    let backup = directory.0.join("must-not-exist.sqlite3");
    create_fixture(&path, 3);
    let mut document = loaded_document(&path);
    let before = document.clone();
    update(
        &path,
        "UPDATE profile_items SET quantity = 27 WHERE position = 0;",
    );

    assert!(matches!(
        writer::save_for_test(&mut document, backup.clone()),
        Err(SqliteAccountError::SourceChanged)
    ));
    assert_eq!(document, before);
    assert!(!backup.exists());
    assert_eq!(
        loaded_document(&path).profile().profile_items()[0].quantity,
        27
    );
}

#[test]
fn deleted_source_is_not_recreated_by_save() {
    let directory = TestDirectory::new("sqlite-save-deleted-source");
    let path = directory.0.join("state.sqlite3");
    let backup = directory.0.join("backup.sqlite3");
    create_fixture(&path, 3);
    let mut document = loaded_document(&path);
    fs::remove_file(&path).unwrap();

    let error = match writer::save_for_test(&mut document, backup.clone()) {
        Ok(_) => panic!("saving a deleted source must fail"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("open for writing"));
    assert!(!path.exists());
    assert!(!backup.exists());
}

#[test]
fn verified_backup_restores_the_pre_save_database() {
    let directory = TestDirectory::new("sqlite-restore");
    let path = directory.0.join("state.sqlite3");
    let backup = directory.0.join("state-before.sqlite3");
    create_fixture(&path, 3);
    let mut document = loaded_document(&path);
    let item_id = document.profile().profile_items()[0].id;
    document
        .profile_mut()
        .apply_profile_item(
            SqliteAccountDocument::profile_capabilities(),
            ProfileItemCommand::SetQuantity {
                id: item_id,
                quantity: 26,
            },
        )
        .unwrap();
    writer::save_for_test(&mut document, backup.clone()).unwrap();
    assert_eq!(
        loaded_document(&path).profile().profile_items()[0].quantity,
        26
    );

    writer::restore_backup(&path, &backup).unwrap();

    assert_eq!(
        loaded_document(&path).profile().profile_items()[0].quantity,
        25
    );
}

#[test]
fn guided_restore_preserves_the_current_database_before_replacement() {
    let directory = TestDirectory::new("sqlite-guided-restore");
    let path = directory.0.join("state.sqlite3");
    let selected_backup = directory.0.join("selected.sqlite3");
    let safety_backup = directory.0.join("recovery.sqlite3");
    create_fixture(&path, 3);
    let mut document = loaded_document(&path);
    let item_id = document.profile().profile_items()[0].id;
    document
        .profile_mut()
        .apply_profile_item(
            SqliteAccountDocument::profile_capabilities(),
            ProfileItemCommand::SetQuantity {
                id: item_id,
                quantity: 26,
            },
        )
        .unwrap();
    writer::save_for_test(&mut document, selected_backup.clone()).unwrap();

    let receipt =
        writer::restore_backup_safely_for_test(&path, &selected_backup, safety_backup.clone())
            .unwrap();

    assert_eq!(receipt.safety_backup, safety_backup);
    assert_eq!(
        loaded_document(&path).profile().profile_items()[0].quantity,
        25
    );
    assert_eq!(
        loaded_document(&receipt.safety_backup)
            .profile()
            .profile_items()[0]
            .quantity,
        26
    );
}

#[test]
fn guided_restore_can_recover_an_incompatible_but_healthy_database() {
    let directory = TestDirectory::new("sqlite-guided-incompatible-restore");
    let path = directory.0.join("state.sqlite3");
    let selected_backup = directory.0.join("selected.sqlite3");
    let safety_backup = directory.0.join("recovery.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);
    create_fixture(&selected_backup, 3);

    writer::restore_backup_safely_for_test(&path, &selected_backup, safety_backup.clone()).unwrap();

    assert_eq!(loaded_document(&path).schema_version(), 1);
    let recovery =
        Connection::open_with_flags(safety_backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert_eq!(
        recovery
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn guided_restore_rejects_an_invalid_selection_before_backing_up_or_writing() {
    let directory = TestDirectory::new("sqlite-guided-invalid-selection");
    let path = directory.0.join("state.sqlite3");
    let invalid_backup = directory.0.join("invalid.sqlite3");
    let safety_backup = directory.0.join("must-not-exist.sqlite3");
    create_fixture(&path, 3);
    let before = fs::read(&path).unwrap();
    fs::write(&invalid_backup, b"not a SQLite database").unwrap();

    assert!(
        writer::restore_backup_safely_for_test(&path, &invalid_backup, safety_backup.clone())
            .is_err()
    );

    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!safety_backup.exists());
}

fn loaded_document(path: &Path) -> SqliteAccountDocument {
    let SqliteAccountDocumentLoad::Loaded(document) = document::load(path).unwrap() else {
        panic!("expected a compatible SQLite account document");
    };
    *document
}

fn assert_account_semantics(left: &SqliteAccountDocument, right: &SqliteAccountDocument) {
    assert_eq!(left.primary_soid(), right.primary_soid());
    assert_eq!(left.profile(), right.profile());
    assert_eq!(left.characters(), right.characters());
    assert_eq!(left.settings(), right.settings());
}

fn assert_item_semantics(left: &ItemInstance, right: &ItemInstance) {
    assert_eq!(left.instance_soid, right.instance_soid);
    assert_eq!(left.definition_hash, right.definition_hash);
    assert_eq!(left.level, right.level);
    assert_eq!(left.quantity, right.quantity);
    assert_eq!(left.plugs, right.plugs);
    assert_eq!(left.flags, right.flags);
}

pub(crate) fn create_fixture(path: &Path, inventory_flags: u32) {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(SCHEMA_V1).unwrap();
    connection
        .execute(
            "INSERT INTO account_state VALUES (1, 1, ?, 1, 1, 1, ?, 0);",
            params![sql_u64(0x9EAA_3001_0010_0100), settings_payload()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO dismantle_rewards VALUES (1, 0, 20, 3, 16, 1, 1);",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO profile_items VALUES (1, 0, ?, 10, 25, 2);",
            [sql_u64(0x5000_0000_0000_0001)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO characters VALUES (1, 0, ?, 1, 2, 1, 2, 40, 1, 1, 0.0, \
             123, 0, ?, 1, 4);",
            params![sql_u64(0x9EAA_3002_0010_0100), sql_u64(u64::MAX)],
        )
        .unwrap();
    insert_item(
        &connection,
        0,
        0,
        0x4000_0000_0000_0001,
        100,
        106,
        1,
        0,
        0,
        0,
        [4, 7, 10, 11, 2],
    );
    insert_item(
        &connection,
        0,
        11,
        0x4000_0000_0000_0002,
        200,
        106,
        1,
        1,
        1,
        2,
        [6, 8, 20, 21, 3],
    );
    insert_item(
        &connection,
        1,
        0,
        0x4000_0000_0000_0003,
        300,
        105,
        2,
        2,
        inventory_flags,
        0,
        [4, 7, 10, 11, 2],
    );
    connection
        .execute("INSERT INTO item_plugs VALUES (1, 0, 0, 11, 0, 77);", [])
        .unwrap();
    connection
        .execute("INSERT INTO item_plugs VALUES (1, 0, 0, 11, 1, NULL);", [])
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn insert_item(
    connection: &Connection,
    location: i64,
    position: i64,
    instance_soid: u64,
    definition_hash: u32,
    level: i32,
    quantity: i32,
    mutation_serial: i32,
    flags: u32,
    plug_count: usize,
    abilities: [u8; 5],
) {
    let socket_policy = i64::from(plug_count != 0);
    connection
        .execute(
            "INSERT INTO character_items VALUES (1, 0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
            params![
                location,
                position,
                sql_u64(instance_soid),
                definition_hash,
                level,
                quantity,
                mutation_serial,
                flags,
                socket_policy,
                plug_count,
                abilities[0],
                abilities[1],
                abilities[2],
                abilities[3],
                abilities[4],
            ],
        )
        .unwrap();
}

fn settings_payload() -> Vec<u8> {
    let mut writer = PayloadWriter::default();
    writer.u32(1);
    writer.i8(0);
    writer.i8(0);
    writer.i8(5);
    for value in [false, false, true, false, false] {
        writer.boolean(value);
    }
    writer.i32(50);
    for value in [false, false, false, true] {
        writer.boolean(value);
    }
    writer.f32(1.0);
    writer.i8(0);

    for value in [0, 0, 0, 8, 5] {
        writer.i8(value);
    }
    writer.boolean(false);
    for value in [10, 10, 10] {
        writer.i8(value);
    }

    writer.i8(3);
    writer.boolean(true);
    writer.i8(0);
    writer.u8(1);
    writer.i32(85);
    writer.f32(10_000.0);
    writer.f32(0.0);

    for value in [0, 0, 0, 0] {
        writer.i8(value);
    }
    writer.boolean(true);
    for value in [0, 0, 0, 0, 0, 0, 0, 0, 0] {
        writer.i8(value);
    }

    writer.boolean(false);
    writer.i8(0);
    for value in [true, false, true, true] {
        writer.boolean(value);
    }
    for value in [0, 0, 0, 0, 0] {
        writer.i8(value);
    }
    writer.u8(0);
    writer.boolean(true);
    for index in 0..60 {
        writer.optional_u16((index == 0).then_some(42));
        writer.optional_u16(None);
    }
    writer.boolean(true);
    assert_eq!(writer.0.len(), 438);
    writer.0
}

#[derive(Default)]
struct PayloadWriter(Vec<u8>);

impl PayloadWriter {
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn boolean(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.0.extend(value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.extend(value.to_le_bytes());
    }

    fn i8(&mut self, value: i8) {
        self.0.extend(value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.0.extend(value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    fn optional_u16(&mut self, value: Option<u16>) {
        self.boolean(value.is_some());
        self.u16(value.unwrap_or(0));
    }
}

fn update(path: &Path, sql: &str) {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(sql).unwrap();
}

fn sql_u64(value: u64) -> i64 {
    value as i64
}

fn sidecar(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    value.into()
}
