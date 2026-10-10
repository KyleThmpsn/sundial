use crate::app::change_review::collect_change_summaries;
use crate::app::tests::driver::*;
use crate::app::*;
use crate::test_support::TestDirectory;
use std::fs;

#[test]
fn settings_change_review_reports_nested_values_and_respects_its_limit() {
    let before = serde_json::json!({"state": {"characters": [{"class": 0, "race": 1}]}});
    let after = serde_json::json!({"state": {"characters": [{"class": 2, "race": 3}]}});

    let changes = collect_change_summaries(&before, &after, 10);
    assert_eq!(changes.len(), 2);
    assert!(changes[0].contains("/state/characters/0/class"));
    assert!(changes[1].contains("/state/characters/0/race"));

    let limited = collect_change_summaries(&before, &after, 1);
    assert_eq!(limited.len(), 1);
}

#[test]
fn sqlite_default_restore_preserves_only_inactive_json_account_domains() {
    let mut defaults = serde_json::json!({
        "version": 8,
        "state": {
            "account": {"settings": {"from": "defaults"}},
            "characters": [{"from": "defaults"}],
            "server": {"port": 1}
        }
    });
    let source = serde_json::json!({
        "version": 8,
        "state": {
            "account": {"settings": {"sentinel": true}},
            "characters": "stale but untouched",
            "server": {"port": 2}
        }
    });

    preserve_inactive_json_account_domains(&mut defaults, &source);

    assert_eq!(
        defaults.pointer("/state/account/settings/sentinel"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(
        defaults.pointer("/state/characters"),
        Some(&serde_json::json!("stale but untouched"))
    );
    assert_eq!(
        defaults.pointer("/state/server/port"),
        Some(&serde_json::json!(1))
    );
}

#[test]
fn sqlite_default_restore_does_not_create_missing_json_account_domains() {
    let mut defaults = serde_json::json!({
        "state": {"account": {}, "characters": [], "server": {}}
    });

    preserve_inactive_json_account_domains(&mut defaults, &serde_json::json!({"state": {}}));

    assert!(defaults.pointer("/state/account").is_none());
    assert!(defaults.pointer("/state/characters").is_none());
    assert!(defaults.pointer("/state/server").is_some());
}

#[test]
fn check_mode_accepts_compatible_sqlite_and_rejects_blocked_account_sources() {
    let compatible = TestDirectory::new("check-compatible-sqlite");
    let compatible_settings = compatible.0.join("settings.json");
    crate::persistence::sqlite_account::tests::create_fixture(
        &compatible.0.join("data").join("investment.sqlite3"),
        3,
    );
    let compatible_document = WorkspaceDocument::load(
        serde_json::json!({
            "version": 18,
            "state": {"account": "stale", "characters": "stale"}
        }),
        &compatible_settings,
        false,
    );
    assert_eq!(validate_for_check(&compatible_document), Ok(()));

    let blocked = TestDirectory::new("check-blocked-sqlite");
    let blocked_settings = blocked.0.join("settings.json");
    fs::create_dir_all(blocked.0.join("data")).unwrap();
    fs::write(
        blocked.0.join("data").join("investment.sqlite3"),
        b"not SQLite",
    )
    .unwrap();
    let blocked_document =
        WorkspaceDocument::load(serde_json::json!({"version": 18}), &blocked_settings, false);
    assert!(
        validate_for_check(&blocked_document)
            .unwrap_err()
            .contains("Account source is incompatible")
    );
}

#[test]
fn accepting_a_loaded_source_cannot_record_or_restore_the_previous_source() {
    let directory = TestDirectory::new("history-source-boundary");
    let mut app = app(directory.0.clone());
    let previous = app.document.clone();
    let entry = DocumentHistoryEntry {
        document: previous.clone(),
        label: "Old Account".into(),
    };
    app.undo_history.push(entry.clone());
    app.redo_history.push(entry);
    let next = WorkspaceDocument::json_only(
        serde_json::json!({"version": 8, "state": {"characters": [], "sentinel": "new account"}}),
    );
    app.replace_loaded_document(next.clone());
    assert!(app.undo_history.is_empty());
    assert!(app.redo_history.is_empty());
    app.undo();
    assert_eq!(app.document, next);
    assert!(!app.dirty);
    assert!(!app.settings_path.exists());
}

#[test]
fn disconnected_catalog_worker_releases_the_authoring_pause() {
    use crate::app::background_tasks::{CatalogTask, CatalogTaskKind};
    let directory = TestDirectory::new("catalog-worker-disconnect");
    let mut app = app(directory.0.clone());
    app.manifest.suspend_package_access();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.catalog_task = Some(CatalogTask {
        kind: CatalogTaskKind::Rebuild,
        receiver,
        progress: CatalogProgress {
            message: "Testing",
            completed: 0,
            total: 1,
        },
    });
    drop(sender);
    app.poll_catalog_task();
    assert!(app.catalog_task.is_none());
    assert!(!app.manifest.inspection_access().is_suspended());
    assert!(app.status_is_error);
}

#[test]
fn applying_stale_json_preserves_the_current_document_and_the_draft() {
    let directory = TestDirectory::new("json-draft-conflict");
    let mut app = app(directory.0.clone());
    let draft = app.raw_json.clone();
    app.document.json_mut()["state"]["sentinel"] = serde_json::json!(true);
    let current = app.document.clone();
    assert!(!app.apply_raw_json());
    assert_eq!(app.document, current);
    assert_eq!(app.raw_json, draft);
}
