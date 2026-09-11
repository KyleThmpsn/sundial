use super::*;
use sundial::investment::{preview_authored_account_cleanup, read_authored_account_source};

#[test]
fn native_account_journal_commits_recovers_and_refuses_concurrent_changes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let packages = root.join("packages");
    let backup = root.join("backup");
    fs::create_dir(&packages).unwrap();
    fs::create_dir(&backup).unwrap();
    fs::create_dir(root.join("data")).unwrap();
    fs::write(root.join("settings.json"), br#"{"version":18,"state":{}}"#).unwrap();
    let path = root.join("data/investment.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN").unwrap();
    for sql in [
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src/persistence/sqlite_account/fixtures/investment_schema.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src/persistence/sqlite_account/fixtures/account_settings_schema.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src/persistence/sqlite_account/fixtures/investment_defaults.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src/persistence/sqlite_account/fixtures/account_settings_defaults.sql"
        )),
    ] {
        db.execute_batch(sql).unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE extension (value TEXT); INSERT INTO extension VALUES('keep');").unwrap();
    let hash: u32 = db
        .query_row("SELECT definition_hash FROM items LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let proposal = preview_authored_account_cleanup(root, &BTreeSet::from([hash]), &[]).unwrap();
    assert_ne!(proposal.original_bytes, proposal.cleaned_bytes);
    let packages = fs::canonicalize(packages).unwrap();
    let record = prepare(&proposal, &packages, &backup).unwrap();
    commit(&proposal, &record, &packages, &backup).unwrap();
    let transaction = InstallTransactionRecord {
        schema: INSTALL_TRANSACTION_SCHEMA,
        state: InstallTransactionState::Pending,
        target_packages_directory: packages.clone(),
        backup_directory: backup.clone(),
        account_cleanup: Some(record.clone()),
        client_settings: None,
        artifacts: vec![],
    };
    verify_committed(&transaction).unwrap();
    recover_account(&transaction, || Ok(false)).unwrap();
    assert_eq!(
        read_authored_account_source(&path).unwrap(),
        proposal.original_bytes
    );
    commit(&proposal, &record, &packages, &backup).unwrap();
    db.execute("UPDATE extension SET value='outside'", [])
        .unwrap();
    let outside = read_authored_account_source(&path).unwrap();
    assert!(recover_account(&transaction, || Ok(false)).is_err());
    assert_eq!(read_authored_account_source(&path).unwrap(), outside);
}
