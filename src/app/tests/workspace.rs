use crate::app::*;
use crate::test_support::TestDirectory;
use std::fs;

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
