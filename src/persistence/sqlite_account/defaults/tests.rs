use super::super::tests::{create_fixture, default_resources};
use super::*;
use crate::test_support::TestDirectory;

#[test]
fn reset_uses_installed_defaults_preserves_wal_backup_and_leaves_settings_untouched() {
    let directory = TestDirectory::new("account-default-reset");
    let path = directory.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let settings = directory.0.join("settings.json");
    std::fs::write(&settings, br#"{"version":18,"unknown":"keep"}"#).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE extension (value TEXT); INSERT INTO extension VALUES('preserve in recovery'); ALTER TABLE items ADD COLUMN extra TEXT; UPDATE items SET extra='custom';").unwrap();
    let before = package::read(&path).unwrap();
    let plan = ResetPlan::prepare(&path, &default_resources()).unwrap();
    let defaults = plan.after.clone();
    assert_eq!(package::read(&path).unwrap(), before);
    let receipt = plan
        .apply_with_backup(Some(directory.0.join("recovery.sqlite3")))
        .unwrap();
    assert_eq!(package::read(&path).unwrap(), defaults);
    assert_eq!(package::read(&receipt.safety_backup).unwrap(), before);
    assert_eq!(
        std::fs::read(settings).unwrap(),
        br#"{"version":18,"unknown":"keep"}"#
    );
    // Restoring the recovery snapshot must also recover opaque schema and WAL-only rows.
    package::restore(&path, &defaults, &receipt.safety_backup).unwrap();
    assert_eq!(package::read(&path).unwrap(), before);
}

#[test]
fn changes_after_confirmation_are_not_reset() {
    let directory = TestDirectory::new("account-reset-conflict");
    let path = directory.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let plan = ResetPlan::prepare(&path, &default_resources()).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; UPDATE account_display SET show_fps=0;")
        .unwrap();
    let changed = package::read(&path).unwrap();
    let backup = directory.0.join("recovery.sqlite3");
    assert!(matches!(
        plan.apply_with_backup(Some(backup.clone())),
        Err(SqliteAccountError::SourceChanged)
    ));
    assert_eq!(package::read(&path).unwrap(), changed);
    assert!(!backup.exists());
}

#[test]
fn invalid_defaults_and_unsupported_sources_are_never_reset() {
    let directory = TestDirectory::new("account-reset-invalid");
    let path = directory.0.join("investment.sqlite3");
    assert!(ResetPlan::prepare(&path, &default_resources()).is_err());
    assert!(!path.exists());
    create_fixture(&path, 3);
    let before = package::read(&path).unwrap();
    let mut defaults = default_resources();
    defaults
        .settings_rows
        .push_str("UPDATE account_audio SET migration_version=999;");
    assert!(ResetPlan::prepare(&path, &defaults).is_err());
    assert_eq!(package::read(&path).unwrap(), before);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA user_version=99").unwrap();
    let future = package::capture_path(&path).unwrap();
    assert!(ResetPlan::prepare(&path, &default_resources()).is_err());
    assert_eq!(package::capture_path(&path).unwrap(), future);
}

#[test]
fn backup_failure_and_commit_validation_failure_leave_the_account_intact() {
    let directory = TestDirectory::new("account-reset-rollback");
    let path = directory.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let before = package::read(&path).unwrap();
    let plan = ResetPlan::prepare(&path, &default_resources()).unwrap();
    assert!(plan.apply_with_backup(Some(directory.0.clone())).is_err());
    assert_eq!(package::read(&path).unwrap(), before);
    let mut plan = ResetPlan::prepare(&path, &default_resources()).unwrap();
    let mut invalid: serde_json::Value = serde_json::from_slice(&plan.after).unwrap();
    invalid["tables"]["account"]["rows"][0][1] = serde_json::json!({"Integer": 0});
    plan.after = serde_json::to_vec(&invalid).unwrap();
    let backup = directory.0.join("recovery.sqlite3");
    assert!(plan.apply_with_backup(Some(backup.clone())).is_err());
    assert_eq!(package::read(&path).unwrap(), before);
    assert_eq!(package::read(&backup).unwrap(), before);
}

#[test]
fn invalid_account_rows_can_be_reset_without_replacing_the_database_file() {
    let directory = TestDirectory::new("account-reset-invalid-rows");
    let path = directory.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("UPDATE account SET soid=0;").unwrap();
    assert!(package::read(&path).is_err());
    let before = package::capture_path(&path).unwrap();
    let plan = ResetPlan::prepare(&path, &default_resources()).unwrap();
    let expected = plan.after.clone();
    let receipt = plan
        .apply_with_backup(Some(directory.0.join("recovery.sqlite3")))
        .unwrap();
    assert_eq!(package::read(&path).unwrap(), expected);
    assert_eq!(
        package::capture_path(&receipt.safety_backup).unwrap(),
        before
    );
    assert_ne!(
        db.query_row("SELECT soid FROM account", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
