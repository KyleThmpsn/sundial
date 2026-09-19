use super::*;
use crate::app::authoring_bridge::save_unlock_test_settings;
use crate::test_support::TestDirectory;
use serde_json::json;
use std::fs;
#[test]
fn native_authored_unlocks_cover_character_scopes_and_repeat_without_changes() {
    let directory = TestDirectory::new("authored-native-scopes");
    let path = directory.0.join("investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&path, 3);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("INSERT INTO characters SELECT 1,soid+1,race,gender,class,level,preview_available,appearance_value,last_orbited_destination,content_bypass,equipped_title,acquired_subclass_mask,next_inventory_serial FROM characters;").unwrap();
    let crate::persistence::sqlite_account::SqliteAccountDocumentLoad::Loaded(mut document) =
        crate::persistence::sqlite_account::load_document(&path).unwrap()
    else {
        panic!()
    };
    let unlocks = [(200, 1, 42), (201, 3, 43), (202, 6, 44)];
    assert_eq!(
        apply_native_authored_unlocks(&mut document, &unlocks).unwrap(),
        3
    );
    assert_eq!(
        apply_native_authored_unlocks(&mut document, &unlocks).unwrap(),
        0
    );
    crate::persistence::sqlite_account::tests::save_fixture_document(
        &mut document,
        &directory.0.join("backup.sqlite3"),
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM unlocks WHERE bank=0 AND slot=42 AND value=2",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM unlocks WHERE value=2 AND ((bank=4 AND slot=43) OR (bank=2 AND slot=44))",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        4
    );
}

#[test]
fn legacy_unlock_sync_ignores_an_existing_database() {
    let directory = TestDirectory::new("authored-unlock-legacy-with-database");
    let settings = directory.0.join("settings.json");
    let database = directory.0.join("data/investment.sqlite3");
    fs::write(
        &settings,
        serde_json::to_vec(&unlock_settings_for_test()).unwrap(),
    )
    .unwrap();
    crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
    let original_database = fs::read(&database).unwrap();
    let (saved_path, _, changed) = synchronize_authored_collection_unlocks_at(
        &settings,
        &[(200, 1, 42)],
        save_unlock_test_settings(&directory),
    )
    .unwrap();
    assert_eq!(saved_path, settings);
    assert_eq!(changed, 1);
    assert_eq!(fs::read(&database).unwrap(), original_database);
}

#[test]
fn sqlite_unlock_sync_updates_active_database_and_preserves_json() {
    let directory = TestDirectory::new("authored-unlock-sqlite");
    let settings = directory.0.join("settings.json");
    let database = directory.0.join("data").join("investment.sqlite3");
    let mut defaults = unlock_settings_for_test();
    defaults["version"] = json!(18);
    fs::write(&settings, serde_json::to_vec(&defaults).unwrap()).unwrap();
    crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
    let json_before = fs::read(&settings).unwrap();

    let (saved_path, backup, changed) = synchronize_authored_collection_unlocks_at(
        &settings,
        &[(200, 1, 42)],
        save_unlock_test_settings(&directory),
    )
    .unwrap();

    assert_eq!(saved_path, database);
    assert_eq!(changed, 1);
    assert!(backup.is_some_and(|path| path.is_file()));
    assert_eq!(fs::read(&settings).unwrap(), json_before);
    let db = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT value FROM unlocks WHERE character_slot=-1 AND bank=0 AND slot=42 AND lane=0",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        2
    );
    let saved = crate::persistence::json_document::load_workspace_json(&settings).unwrap();
    assert_eq!(
        saved.pointer("/state/unlocks/account_flag_runs"),
        Some(&json!([]))
    );
}

#[test]
fn already_acquired_authored_unlocks_do_not_rewrite_or_back_up_settings() {
    let directory = TestDirectory::new("authored-unlock-idempotent");
    let settings = directory.0.join("settings.json");
    let document = json!({
        "version": 8,
        "state": {"unlocks": {"account_flag_runs": [[42, 1]]}}
    });
    let encoded = serde_json::to_vec(&document).unwrap();
    fs::write(&settings, &encoded).unwrap();

    let (_, backup, changed) = synchronize_authored_collection_unlocks_at(
        &settings,
        &[(200, 1, 42)],
        save_unlock_test_settings(&directory),
    )
    .unwrap();

    assert_eq!(changed, 0);
    assert_eq!(backup, None);
    assert_eq!(fs::read(&settings).unwrap(), encoded);
    assert!(!directory.0.join("backups").exists());
}

#[test]
fn authored_unlock_sync_refuses_to_overwrite_a_newer_settings_document() {
    let directory = TestDirectory::new("authored-unlock-conflict");
    let settings = directory.0.join("settings.json");
    let original = unlock_settings_for_test();
    fs::write(&settings, serde_json::to_vec(&original).unwrap()).unwrap();
    let newer = json!({
        "version": 8,
        "state": {
            "unlocks": {"account_flag_runs": []},
            "changed_while_installing": true
        }
    });
    let newer_bytes = serde_json::to_vec(&newer).unwrap();
    fs::write(&settings, &newer_bytes).unwrap();

    let error = synchronize_loaded_authored_collection_unlocks(
        &settings,
        original,
        &[(200, 1, 42)],
        save_unlock_test_settings(&directory),
    )
    .unwrap_err();

    assert!(error.contains("changed outside Sundial"));
    assert_eq!(fs::read(&settings).unwrap(), newer_bytes);
    assert!(!directory.0.join("backups").exists());
}

#[test]
fn authored_unlock_sync_accepts_the_last_extended_account_flag_byte() {
    let directory = TestDirectory::new("authored-unlock-extension-boundary");
    let settings = directory.0.join("settings.json");
    fs::write(
        &settings,
        serde_json::to_vec(&unlock_settings_for_test()).unwrap(),
    )
    .unwrap();
    let slot = u16::try_from(crate::account_contract::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY - 1)
        .unwrap();

    let (_, _, changed) = synchronize_authored_collection_unlocks_at(
        &settings,
        &[(
            200,
            crate::account_contract::SHADOWKEEP_ACCOUNT_FLAG_BANK,
            slot,
        )],
        save_unlock_test_settings(&directory),
    )
    .unwrap();

    assert_eq!(changed, 1);
    let saved = crate::persistence::json_document::load_workspace_json(&settings).unwrap();
    assert_eq!(
        saved.pointer("/state/unlocks/account_flag_runs"),
        Some(&json!([[slot, 1]]))
    );
}

#[test]
fn authored_unlock_sync_rejects_the_first_byte_of_the_next_account_region() {
    let directory = TestDirectory::new("authored-unlock-extension-overflow");
    let settings = directory.0.join("settings.json");
    let original = unlock_settings_for_test();
    let original_bytes = serde_json::to_vec(&original).unwrap();
    fs::write(&settings, &original_bytes).unwrap();
    let slot =
        u16::try_from(crate::account_contract::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY).unwrap();

    let error = synchronize_authored_collection_unlocks_at(
        &settings,
        &[(
            200,
            crate::account_contract::SHADOWKEEP_ACCOUNT_FLAG_BANK,
            slot,
        )],
        save_unlock_test_settings(&directory),
    )
    .unwrap_err();

    assert!(error.contains("beyond the extended Shadowkeep account-flag region"));
    assert_eq!(fs::read(&settings).unwrap(), original_bytes);
    assert!(!directory.0.join("backups").exists());
}

fn unlock_settings_for_test() -> serde_json::Value {
    json!({
        "version": 8,
        "state": {"unlocks": {"account_flag_runs": []}}
    })
}

#[test]
fn authored_unlock_sync_preserves_v8_and_v13_documents_and_is_idempotent() {
    for version in [8, 13] {
        let directory = TestDirectory::new(&format!("authored-unlock-v{version}"));
        let settings = directory.0.join("settings.json");
        let original = json!({
            "version": version,
            "custom_setting": {"preserve": true},
            "state": {"unlocks": {
                "account_flag_runs": [[42, 1]],
                "character_flag_runs": [[61, 1]]
            }}
        });
        fs::write(&settings, serde_json::to_vec(&original).unwrap()).unwrap();
        let (_, backup, changed) = synchronize_authored_collection_unlocks_at(
            &settings,
            &[(21613, 1, 11923), (21614, 1, 11924)],
            save_unlock_test_settings(&directory),
        )
        .unwrap();
        assert_eq!(changed, 2);
        assert!(backup.unwrap().is_file());
        let saved = crate::persistence::json_document::load_workspace_json(&settings).unwrap();
        let mut expected = original;
        expected["state"]["unlocks"]["account_flag_runs"] = json!([[42, 1], [11923, 2]]);
        assert_eq!(saved, expected);
        let (_, backup, changed) = synchronize_authored_collection_unlocks_at(
            &settings,
            &[(21613, 1, 11923), (21614, 1, 11924)],
            save_unlock_test_settings(&directory),
        )
        .unwrap();
        assert_eq!(changed, 0);
        assert!(backup.is_none());
    }
}
