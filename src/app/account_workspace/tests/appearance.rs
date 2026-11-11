//! Real-file appearance lifecycle, independent SQL readback and rollback.
use super::*;
use crate::character::Appearance;
use crate::persistence::sqlite_account::{snapshot, tests::versions};
use std::path::Path;

#[test]
fn appearance_save_reload_conflict_and_recovery_preserve_character_identity() {
    for case in versions::CASES {
        check_lifecycle(case);
    }
}

fn check_lifecycle(case: &str) {
    let directory = TestDirectory::new("appearance lifecycle");
    let settings = settings_path(&directory);
    let database = directory.0.join("data/investment.sqlite3");
    versions::fixture(&database, case);
    let db = Connection::open(&database).unwrap();
    db.execute_batch("ALTER TABLE characters ADD COLUMN future TEXT NOT NULL DEFAULT 'preserve';")
        .unwrap();
    let original = WorkspaceDocument::load(json!({"version":18}), &settings, false);
    let mut edited = original.clone();
    let header: [u8; 36] = [
        37, 0, 5, 0, 1, 0x91, 234, 2, 1, 3, 70, 2, 72, 3, 73, 3, 74, 3, 255, 255, 255, 255, 19, 3,
        199, 2, 0x92, 0x93, 197, 157, 28, 129, 1, 0x94, 0x95, 0x96,
    ];
    let appearance = Appearance::from_saved(0, 0, header).unwrap();
    if case == "v2" {
        assert!(
            edited
                .native_account_mut()
                .unwrap()
                .set_appearance(0, appearance)
                .is_err()
        );
        assert_eq!(edited, original);
        return;
    }
    let before = snapshot::read(&database).unwrap();
    edited
        .native_account_mut()
        .unwrap()
        .set_appearance(0, appearance.clone())
        .unwrap();
    let receipt = edited.save_account(|| Ok(())).unwrap();
    let v5 = case == "v5-fresh";
    let validity = if v5 { "1" } else { "appearance_header_valid" };
    let saved: (i64, i64, i64, Vec<u8>, i64, String) = db.query_row(
        &format!("SELECT soid,race,gender,appearance_header,{validity},future FROM characters WHERE slot=0"),
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
    ).unwrap();
    assert_eq!(
        saved,
        (
            0x9EAA_3002_0010_0100_u64 as i64,
            0,
            0,
            header.to_vec(),
            1,
            "preserve".into()
        )
    );
    capture_account(&db, &format!("appearance-saved-{case}.sqlite3"));
    let reloaded = WorkspaceDocument::load(json!({"version":18}), &settings, false);
    assert_eq!(
        reloaded.native_account().unwrap().appearance(0).unwrap(),
        Some(appearance)
    );
    assert_eq!(
        reloaded.native_account().unwrap().characters().characters()[0].inventory,
        original.native_account().unwrap().characters().characters()[0].inventory
    );
    let mut stale = reloaded.clone();
    let mut other = header;
    other[2] = 6;
    stale
        .native_account_mut()
        .unwrap()
        .set_appearance(0, Appearance::from_saved(0, 0, other).unwrap())
        .unwrap();
    db.execute_batch(if v5 {
        "UPDATE characters SET appearance_header=zeroblob(36)"
    } else {
        "UPDATE characters SET appearance_header_valid=0"
    })
    .unwrap();
    let external = snapshot::read(&database).unwrap();
    assert!(stale.save_account(|| Ok(())).is_err());
    assert_eq!(snapshot::read(&database).unwrap(), external);
    // Recovery refuses to overwrite an external change, then restores the complete original.
    assert!(edited.rollback_account_save(&receipt).is_err());
    if v5 {
        db.execute(
            "UPDATE characters SET appearance_header=?",
            [header.as_slice()],
        )
        .unwrap();
    } else {
        db.execute("UPDATE characters SET appearance_header_valid=1", [])
            .unwrap();
    }
    edited.rollback_account_save(&receipt).unwrap();
    assert_eq!(snapshot::read(&database).unwrap(), before);
    capture_account(&db, &format!("appearance-{case}.sqlite3"));
}

fn capture_account(db: &Connection, name: &str) {
    if let Some(output) = crate::test_support::artifacts(name) {
        db.backup(
            rusqlite::MAIN_DB,
            Path::new(&output),
            rusqlite::backup::NO_PROGRESS,
        )
        .unwrap();
    }
}
