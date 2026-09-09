use super::*;

#[test]
fn guided_recovery_restores_schema_and_data_atomically_and_preserves_a_safety_snapshot() {
    let dir = TestDirectory::new("sqlite-guided-recovery");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute("INSERT INTO pending_rewards(character_slot,kind,definition_hash,quantity) VALUES(0,0,300,1)", []).unwrap();
    let original = super::super::package::read(&path).unwrap();
    let mut doc = loaded(&path);
    let receipt = writer::save_for_test(&mut doc, dir.0.join("backup.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA user_version=99; CREATE TABLE future_data (value TEXT); INSERT INTO future_data VALUES('outside');").unwrap();
    let outside = super::super::package::capture_path(&path).unwrap();
    let restored = writer::restore_backup_safely(&path, &receipt.backup).unwrap();
    assert_eq!(super::super::package::read(&path).unwrap(), original);
    assert_eq!(
        super::super::package::capture_path(&restored.safety_backup).unwrap(),
        outside
    );
    db.execute("UPDATE account SET profile_setup_completed=0", [])
        .unwrap();
    let newer = super::super::package::capture_path(&path).unwrap();
    assert!(super::super::package::restore(&path, &original, &receipt.backup).is_err());
    assert_eq!(super::super::package::capture_path(&path).unwrap(), newer);
}

#[test]
fn unknown_cascading_relations_are_never_silently_discarded() {
    let dir = TestDirectory::new("sqlite-unknown-relation");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE extension (item INTEGER REFERENCES items(instance_soid) ON DELETE CASCADE, value TEXT); INSERT INTO extension SELECT instance_soid,'keep' FROM items WHERE location=1;").unwrap();
    let before = super::super::package::read(&path).unwrap();
    let mut doc = loaded(&path);
    assert!(writer::save_for_test(&mut doc, dir.0.join("backup.sqlite3")).is_err());
    assert_eq!(super::super::package::read(&path).unwrap(), before);
}

#[test]
fn missing_corrupt_old_future_and_wrong_schema_never_become_writable() {
    let dir = TestDirectory::new("sqlite-format-safety");
    let path = dir.0.join("investment.sqlite3");
    assert!(matches!(
        document::load(&path).unwrap(),
        SqliteAccountDocumentLoad::Missing
    ));
    assert!(!path.exists());
    fs::write(&path, b"not a database").unwrap();
    assert!(document::load(&path).is_err());
    fs::remove_file(&path).unwrap();
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    for sql in [
        "PRAGMA user_version=1",
        "PRAGMA user_version=3",
        "PRAGMA user_version=2; PRAGMA application_id=42",
        "PRAGMA application_id=1397902921; ALTER TABLE account_display RENAME COLUMN show_fps TO incompatible",
    ] {
        db.execute_batch(sql).unwrap();
        assert!(
            !matches!(
                document::load(&path),
                Ok(SqliteAccountDocumentLoad::Loaded(_))
            ),
            "{sql}"
        );
    }
}

#[test]
fn malformed_native_items_sockets_and_identities_are_rejected() {
    for sql in [
        "PRAGMA ignore_check_constraints=ON; UPDATE items SET flags=8 WHERE location=1",
        "UPDATE items SET instance_soid=(SELECT soid FROM account) WHERE location=1",
        "INSERT INTO sockets VALUES(4611686018427387906,2,99)",
        "UPDATE items SET position=2 WHERE location=1",
        "UPDATE profile_items SET position=5",
        "UPDATE dismantle_rewards SET position=5",
        "BEGIN; PRAGMA defer_foreign_keys=ON; UPDATE characters SET slot=2; UPDATE items SET character_slot=2; COMMIT",
        "UPDATE account_key_bindings SET primary_code=65535 WHERE action=0",
        "UPDATE account_key_bindings SET primary_code=116 WHERE action=0",
        "UPDATE account_key_bindings SET primary_code=768 WHERE action=0",
        "UPDATE account_display SET calibration_primary=1234",
        "UPDATE account_display SET calibration_alpha=1",
        "UPDATE account_audio SET migration_version=7",
        "UPDATE account_interface SET reserved_text_mode=1",
        "UPDATE account_interface SET subtitle_options_entry=1",
        "INSERT INTO character_stacks VALUES(0,1,42,1,0)",
        "INSERT INTO character_stacks VALUES(0,0,42,1,0),(0,1,42,1,0)",
        "INSERT INTO character_stacks VALUES(0,0,2166136261,1,0)",
        "INSERT INTO character_stacks VALUES(0,0,42,2147483648,0)",
        "INSERT INTO character_stacks VALUES(0,0,42,1,2147483648)",
        "INSERT INTO entitlements VALUES(0,'same',1),(1,'same',1)",
        "INSERT INTO entitlements VALUES(0,'not-an-application-id',2)",
        "INSERT INTO entitlements VALUES(0,char(10),1)",
        "INSERT INTO family5 VALUES(0,1,7,2)",
        "INSERT INTO entitlements VALUES(1,'test',1)",
        "INSERT INTO unlocks VALUES(-1,0,42,1,1)",
        "INSERT INTO unlocks VALUES(-1,1,512,0,1)",
        "CREATE TRIGGER unexpected AFTER UPDATE ON account BEGIN UPDATE account SET profile_setup_completed=0; END",
    ] {
        let dir = TestDirectory::new("sqlite-invalid-native");
        let path = dir.0.join("investment.sqlite3");
        create_fixture(&path, 3);
        Connection::open(&path).unwrap().execute_batch(sql).unwrap();
        assert!(document::load(&path).is_err(), "{sql}");
    }
}

#[test]
fn family_override_removal_keeps_native_positions_dense_and_preserves_unexposed_values() {
    let dir = TestDirectory::new("sqlite-family-positions");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("INSERT INTO family5 VALUES(0,0,10,2),(0,1,11,2),(0,2,65000,255); ALTER TABLE family5 ADD COLUMN extra TEXT NOT NULL DEFAULT 'keep';").unwrap();
    let mut doc = loaded(&path);
    let mut view = doc.progression_view(0);
    assert_eq!(
        view["_native_progression"]["hidden_family_counts"],
        serde_json::json!([1, 0])
    );
    assert_eq!(
        view["_native_progression"]["family"],
        serde_json::json!([[0, 10, 2], [0, 11, 2], [0, 65000, 255]])
    );
    view["state"]["investment"]["family5_flag_overrides"] = serde_json::json!([[11, 2]]);
    doc.apply_progression_view(0, &view).unwrap();
    save_fixture_document(&mut doc, &dir.0.join("backup.sqlite3"));
    assert_eq!(
        db.query_row("SELECT position FROM family5 WHERE slot=11", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT value FROM family5 WHERE slot=65000", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        255
    );
    assert_eq!(
        db.query_row("SELECT extra FROM family5 WHERE slot=65000", [], |r| r
            .get::<_, String>(
            0
        ))
        .unwrap(),
        "keep"
    );
    loaded(&path);
}

#[test]
fn native_hidden_overrides_cannot_overflow_the_runtime_list() {
    let dir = TestDirectory::new("sqlite-hidden-capacity");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<99) INSERT INTO family5 SELECT 0,x,64000+x,255 FROM n;").unwrap();
    let mut doc = loaded(&path);
    let before = doc.progression_view(0);
    let mut after = before.clone();
    after["state"]["investment"]["family5_flag_overrides"] = serde_json::json!([[10, 2]]);
    assert!(doc.apply_progression_view(0, &after).is_err());
    assert_eq!(doc.progression_view(0), before);
}

#[test]
fn deleted_source_is_not_recreated_and_invalid_backup_is_not_applied() {
    let dir = TestDirectory::new("sqlite-missing-save");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let mut doc = loaded(&path);
    let bad = dir.0.join("invalid.sqlite3");
    fs::write(&bad, b"invalid").unwrap();
    let before = super::super::package::read(&path).unwrap();
    assert!(writer::restore_backup_safely(&path, &bad).is_err());
    assert_eq!(super::super::package::read(&path).unwrap(), before);
    fs::remove_file(&path).unwrap();
    assert!(writer::save_for_test(&mut doc, dir.0.join("backup.sqlite3")).is_err());
    assert!(!path.exists());
}

#[test]
fn coordinated_rollback_restores_all_domains_and_refuses_outside_writes() {
    let dir = TestDirectory::new("sqlite-rollback");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE extension (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO extension VALUES(1,'keep');").unwrap();
    let before = super::super::package::read(&path).unwrap();
    let mut doc = loaded(&path);
    let mut runtime = doc.runtime().clone();
    runtime["account"]["profile_setup_completed"] = serde_json::json!(false);
    doc.set_runtime(runtime);
    let receipt = writer::save_for_test(&mut doc, dir.0.join("first.sqlite3")).unwrap();
    assert_ne!(super::super::package::read(&path).unwrap(), before);
    writer::rollback_save(&path, &receipt).unwrap();
    assert_eq!(super::super::package::read(&path).unwrap(), before);
    assert_eq!(
        super::super::package::read(&receipt.backup).unwrap(),
        before
    );
    let mut doc = loaded(&path);
    let receipt = writer::save_for_test(&mut doc, dir.0.join("second.sqlite3")).unwrap();
    db.execute("UPDATE extension SET value='outside'", [])
        .unwrap();
    let outside = super::super::package::read(&path).unwrap();
    assert!(writer::rollback_save(&path, &receipt).is_err());
    assert_eq!(super::super::package::read(&path).unwrap(), outside);
}

#[test]
fn save_then_undo_restores_deleted_rows_with_unknown_columns() {
    let dir = TestDirectory::new("sqlite-undo-native");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE items ADD COLUMN extra TEXT NOT NULL DEFAULT 'default'; UPDATE items SET extra='original',seen=1; ALTER TABLE entitlements ADD COLUMN extra TEXT NOT NULL DEFAULT 'default'; INSERT INTO entitlements VALUES(0,'123',2,'original');").unwrap();
    let mut original = loaded(&path);
    let mut edited = original.clone();
    let item_id = edited.characters().characters()[0].inventory[0].id;
    edited
        .characters_mut()
        .apply(
            SqliteAccountDocument::character_capabilities(),
            sundial_account::CharacterCommand::RemoveInventoryItem { item_id },
        )
        .unwrap();
    edited.set_entitlements(serde_json::json!([]));
    save_fixture_document(&mut edited, &dir.0.join("first.sqlite3"));
    original.adopt_revision_from(&edited);
    save_fixture_document(&mut original, &dir.0.join("undo.sqlite3"));
    assert_eq!(
        db.query_row("SELECT extra FROM items WHERE location=1", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "original"
    );
    assert_eq!(
        db.query_row("SELECT extra FROM entitlements", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "original"
    );
}
