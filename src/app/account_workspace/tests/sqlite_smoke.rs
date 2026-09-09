//! Real-file smoke coverage for coordinated saves and retries.
use super::*;
use crate::app::workspace_save::{
    WorkspaceSaveError, WorkspaceSaveReceipt, save_changed_sources_with_json,
};
use crate::persistence::sqlite_account::package;

// Use the same coordinator and real writers with an injected closed game for disposable files.
fn save_changed_sources(
    document: &mut WorkspaceDocument,
    persisted: &WorkspaceDocument,
    path: &std::path::Path,
    json_changed: bool,
    account_changed: bool,
) -> Result<WorkspaceSaveReceipt, WorkspaceSaveError> {
    save_changed_sources_with_json(
        document,
        persisted,
        path,
        json_changed,
        account_changed,
        |path, value, expected, normalize| {
            crate::app::settings::save_test_json_checked(
                path,
                value,
                expected,
                normalize,
                &path.parent().unwrap().join("json-backups"),
            )
        },
    )
}

#[test]
fn sqlite_smoke_mixed_save_conflict_rollback_retry_and_reload() {
    let directory = TestDirectory::new("sqlite smoke 玩家");
    let settings = settings_path(&directory);
    let database = directory.0.join("data/investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
    let db = Connection::open(&database).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; INSERT INTO character_stacks VALUES(0,0,0,1,0); ALTER TABLE profile_items ADD COLUMN future TEXT NOT NULL DEFAULT 'first'; INSERT INTO profile_items VALUES(1,5764607523034234882,11,9,3,0,'second');").unwrap();
    db.execute("INSERT INTO unlocks VALUES(-1,0,46,0,1)", [])
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/sunrise-v18-169fd29-defaults.json"
    ))
    .unwrap();
    let encoded = serde_json::to_vec(&json).unwrap();
    fs::write(&settings, &encoded).unwrap();
    let persisted = WorkspaceDocument::load(json, &settings);
    assert_eq!(persisted.source_info().kind, AccountSourceKind::Sqlite);
    assert_eq!(
        persisted.progression_view(0)["state"]["unlocks"]["account_flag_runs"],
        json!([])
    );
    let before = package::read(&database).unwrap();
    let mut edited = persisted.clone();
    account::apply_profile_item_action(
        &mut edited,
        ProfileItemLocation { index: 0 },
        ProfileItemAction::Remove,
    )
    .unwrap();
    let mut runtime = edited.runtime_view();
    runtime["state"]["account"]["profile_setup_completed"] = json!(false);
    runtime["server"]["entitlements"] = json!([{"name":"quote\"\\", "owned":"handle"}]);
    edited.apply_runtime_view(runtime).unwrap();
    let mut progression = edited.progression_view(0);
    for (field, value) in [
        ("account_flag_runs", json!([[42, 1]])),
        ("profile_flag_runs", json!([[43, 1]])),
        ("character_flags", json!([44])),
        ("character_flag_runs", json!([[45, 1]])),
    ] {
        progression["state"]["unlocks"][field] = value;
    }
    edited.apply_progression_view(0, progression).unwrap();
    edited.json_mut()["future_smoke"] = json!({"keep":"玩家"});

    let external = b"{\"version\":18,\"outside_edit\":true}";
    fs::write(&settings, external).unwrap();
    let error = save_changed_sources(&mut edited, &persisted, &settings, true, true)
        .err()
        .unwrap();
    assert_eq!(error.sqlite_rollback, Some(Ok(())), "{}", error.message);
    assert_eq!(package::read(&database).unwrap(), before);
    assert_eq!(fs::read(&settings).unwrap(), external);
    assert!(edited.account_changed_from(&persisted));

    fs::write(&settings, &encoded).unwrap();
    let receipt = save_changed_sources(&mut edited, &persisted, &settings, true, true).unwrap();
    assert!(receipt.json.is_some());
    assert!(receipt.sqlite.is_some());
    let reopened = WorkspaceDocument::load(
        serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap(),
        &settings,
    );
    assert_eq!(reopened.json(), edited.json());
    assert_saved_domains(&db, &reopened);
    let before_repeat = package::read(&database).unwrap();
    let mut repeat = reopened.clone();
    save_changed_sources(&mut repeat, &reopened, &settings, false, true).unwrap();
    assert_eq!(package::read(&database).unwrap(), before_repeat);
    if let Some(path) = std::env::var_os("SUNDIAL_SQLITE_SMOKE_EXPORT") {
        db.backup(rusqlite::MAIN_DB, std::path::Path::new(&path), None)
            .unwrap();
    }
}

fn assert_saved_domains(db: &Connection, reopened: &WorkspaceDocument) {
    assert_eq!(
        account::profile_items(reopened).unwrap().unwrap()[0].definition_hash,
        11
    );
    assert_eq!(
        reopened.runtime_view()["state"]["account"]["profile_setup_completed"],
        false
    );
    let native: (i64, String) = db
        .query_row(
            "SELECT seen,future FROM profile_items WHERE position=0",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(native, (0, "second".into()));
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM unlocks WHERE bank IN (0,1,2,4) AND value=2",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        4
    );
    assert_eq!(
        db.query_row(
            "SELECT value FROM unlocks WHERE bank=0 AND slot=46",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}
