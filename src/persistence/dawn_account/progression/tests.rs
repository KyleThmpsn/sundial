use super::super::{DawnAccountDocumentLoad, load, save, tests::create_fixture};
use super::*;
use serde_json::json;
use std::path::Path;

fn loaded(path: &Path) -> Box<DawnAccountDocument> {
    match load(path).unwrap() {
        DawnAccountDocumentLoad::Loaded(doc) => doc,
        other => panic!("Expected Dawn database: {other:?}"),
    }
}

#[test]
#[ignore = "requires SUNDIAL_DAWN_ACCOUNT_DB and reads the selected database without writing"]
fn installed_dawn_progression_and_reward_ledger_are_readable() {
    let path = std::env::var_os("SUNDIAL_DAWN_ACCOUNT_DB").expect("SUNDIAL_DAWN_ACCOUNT_DB");
    let doc = loaded(Path::new(&path));
    for index in 0..doc.characters().characters().len() {
        crate::persistence::progression::validate(&doc.progression_view(index)).unwrap();
    }
    eprintln!(
        "Dawn schema 5: {} characters, {} progression rows, {} dismantle policies, {} reward debts",
        doc.characters().characters().len(),
        doc.progression.state.unlocks.len(),
        doc.profile().dismantle_rewards().len(),
        doc.reward_debts().len()
    );
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("player-state.db");
    create_fixture(&path);
    Connection::open(&path).unwrap().execute_batch(
        "INSERT INTO characters SELECT 1,'9EAA300100100102',last_selected,race,gender,2,level,accepted,preview_available,appearance,last_destination,content_bypass,movement_ability,grenade_ability,super_ability,melee_ability,class_ability,next_inventory_serial,vendor_campaigns FROM characters WHERE position=0;
         INSERT INTO durable_flags VALUES(0,'9EAA300100100100',10,2),(1,'9EAA300100100100',11,2),(2,'9EAA300100100101',12,2),(3,'9EAA300100100101',13,2),(3,'9EAA300100100102',14,2),(0,'9EAA300100100100',20,255);
         INSERT INTO durable_objectives VALUES(0,'9EAA300100100100',30,99),(3,'9EAA300100100101',31,-7),(3,'9EAA300100100102',31,42);
         INSERT INTO durable_progressions VALUES(0,'9EAA300100100100',38,0,1234),(0,'9EAA300100100100',38,2,7),(2,'9EAA300100100101',8,1,6),(2,'9EAA300100100102',8,1,9);
         INSERT INTO family5_flags VALUES(5,2),(60000,255);
         INSERT INTO family5_values VALUES(6,-123),(60000,17);"
    ).unwrap();
    (dir, path)
}

#[test]
fn dawn_reads_all_banks_and_character_soids_without_using_json_seed() {
    let (_dir, path) = fixture();
    let doc = loaded(&path);
    let view = doc.progression_view(0);
    let u = &view["state"]["unlocks"];
    assert_eq!(u["account_flag_runs"], json!([[10, 1]]));
    assert_eq!(u["profile_flag_runs"], json!([[11, 1]]));
    assert_eq!(u["character_flags"], json!([12]));
    assert_eq!(u["character_flag_runs"], json!([[13, 1]]));
    assert_eq!(u["objective_values"], json!([[30, 99]]));
    assert_eq!(u["character_objective_values"], json!([[31, -7]]));
    assert_eq!(u["account_progressions"], json!([[38, 1234, 0, 7]]));
    assert_eq!(u["character_progressions"], json!([[8, 0, 6, 0]]));
    assert_eq!(
        view["_native_progression"]["hidden_family_counts"],
        json!([1, 1])
    );
    assert_eq!(
        doc.progression_view(1)["_native_progression"]["character_slot"],
        1
    );
    assert_eq!(
        doc.progression_view(1)["state"]["unlocks"]["character_flag_runs"],
        json!([[14, 1]])
    );
}

#[test]
fn dawn_round_trips_edits_in_every_bank_and_preserves_other_character_and_hidden_rows() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    let original = doc.clone();
    let other = doc.progression_view(1);
    let mut view = doc.progression_view(0);
    let u = &mut view["state"]["unlocks"];
    u["account_flag_runs"] = json!([[100, 2]]);
    u["profile_flag_runs"] = json!([[110, 1]]);
    u["character_flags"] = json!([120]);
    u["character_flag_runs"] = json!([[130, 1]]);
    u["objective_values"] = json!([[140, 123]]);
    u["character_objective_values"] = json!([[150, 456]]);
    u["account_progressions"] = json!([[38, 2000, 20, 30]]);
    u["character_progressions"] = json!([[8, 40, 50, 60]]);
    view["state"]["investment"]["family5_flag_overrides"] = json!([[5, 1]]);
    view["state"]["investment"]["family5_value_overrides"] = json!([[6, 789]]);
    doc.apply_progression_view(0, &view).unwrap();
    assert!(doc.differs_from(&original));
    save(&mut doc).unwrap();
    let reloaded = loaded(&path);
    assert_eq!(reloaded.progression, doc.progression);
    assert_eq!(
        reloaded.progression_view(1)["state"]["unlocks"]["character_progressions"],
        other["state"]["unlocks"]["character_progressions"]
    );
    assert_eq!(
        reloaded.progression.state.unlocks.get(&(-1, 0, 20, 0)),
        Some(&255)
    );
    assert_eq!(
        reloaded.progression.state.family.get(&(0, 60000)),
        Some(&255)
    );
    assert_eq!(
        reloaded.progression.state.family.get(&(1, 60000)),
        Some(&17)
    );
    let before = reloaded.clone();
    doc.apply_progression_view(0, &doc.progression_view(0))
        .unwrap();
    assert!(!doc.differs_from(&before));
    save(&mut doc).unwrap();
    assert_eq!(loaded(&path).progression, before.progression);
}

#[test]
fn no_op_projection_preserves_sparse_bytes_and_lanes() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    let original = doc.clone();
    doc.apply_progression_view(0, &doc.progression_view(0))
        .unwrap();
    assert!(!doc.differs_from(&original));
    save(&mut doc).unwrap();
    assert_eq!(loaded(&path).progression, original.progression);
}

#[test]
fn invalid_edits_and_unimplemented_delivery_are_atomic() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    let original = doc.clone();
    let mut view = doc.progression_view(0);
    view["state"]["unlocks"]["account_flag_runs"] = json!([[12300, 1]]);
    assert!(doc.apply_progression_view(0, &view).is_err());
    let mut view = doc.progression_view(0);
    view["_progression_rewards"] = json!([]);
    assert!(
        doc.apply_progression_view(0, &view)
            .unwrap_err()
            .contains("Dawn progression reward claims")
    );
    assert!(
        doc.apply_progression_view(99, &doc.progression_view(0))
            .is_err()
    );
    assert!(!doc.differs_from(&original));
}

#[test]
fn concurrent_sparse_write_is_refused_even_without_a_revision_change() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    Connection::open(&path)
        .unwrap()
        .execute("UPDATE durable_objectives SET value=100 WHERE scope=0", [])
        .unwrap();
    assert!(
        save(&mut doc)
            .unwrap_err()
            .to_string()
            .contains("progression changed")
    );
    assert_eq!(loaded(&path).account_revision(), 2);
    assert_eq!(
        loaded(&path).progression.state.unlocks.get(&(-1, 3, 30, 0)),
        Some(&100)
    );
}

#[test]
fn hidden_override_rows_count_toward_capacity() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    let before = doc.clone();
    let mut view = doc.progression_view(0);
    view["state"]["investment"]["family5_flag_overrides"] =
        json!((0..100).map(|i| [i, 2]).collect::<Vec<_>>());
    assert!(doc.apply_progression_view(0, &view).is_err());
    assert!(!doc.differs_from(&before));
}

#[test]
fn invalid_dawn_scopes_owners_and_ranges_fail_closed() {
    for sql in [
        "INSERT INTO durable_flags VALUES(9,'9EAA300100100100',1,2)",
        "INSERT INTO durable_flags VALUES(2,'9EAA300100100199',1,2)",
        "INSERT INTO durable_objectives VALUES(1,'9EAA300100100100',1,2)",
        "INSERT INTO durable_progressions VALUES(0,'9EAA300100100100',1,3,1)",
        "INSERT INTO durable_flags VALUES(0,'9EAA300100100100',12300,2)",
    ] {
        let (_dir, path) = fixture();
        Connection::open(&path).unwrap().execute(sql, []).unwrap();
        assert!(
            matches!(
                load(&path).unwrap(),
                DawnAccountDocumentLoad::Incompatible(_)
            ),
            "{sql}"
        );
    }
}

#[test]
fn restored_backup_can_be_rebased_and_saved_again() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    let original = doc.clone();
    let mut view = doc.progression_view(0);
    view["state"]["unlocks"]["objective_values"] = json!([[30, 500]]);
    doc.apply_progression_view(0, &view).unwrap();
    let receipt = save(&mut doc).unwrap();
    crate::persistence::dawn_account::rollback_save(&path, &receipt).unwrap();
    doc.adopt_revision(&original);
    save(&mut doc).unwrap();
    assert_eq!(
        loaded(&path).progression_view(0)["state"]["unlocks"]["objective_values"],
        json!([[30, 500]])
    );
}

#[test]
fn writes_preserve_owner_text_and_unknown_columns() {
    let (_dir, path) = fixture();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE durable_objectives ADD COLUMN future TEXT NOT NULL DEFAULT 'keep'; UPDATE durable_objectives SET owner_soid=lower(owner_soid) WHERE scope=0;").unwrap();
    let mut doc = loaded(&path);
    let mut view = doc.progression_view(0);
    view["state"]["unlocks"]["objective_values"] = json!([[30, 500]]);
    doc.apply_progression_view(0, &view).unwrap();
    save(&mut doc).unwrap();
    let row: (String, i32, String) = db
        .query_row(
            "SELECT owner_soid,value,future FROM durable_objectives WHERE scope=0 AND slot=30",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, ("9eaa300100100100".into(), 500, "keep".into()));
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM durable_objectives WHERE scope=0",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn a_failed_progression_write_rolls_back_the_entire_transaction() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    let original = doc.clone();
    let mut view = doc.progression_view(0);
    view["state"]["unlocks"]["account_flag_runs"] = json!([[100, 1]]);
    view["state"]["investment"]["family5_value_overrides"] = json!([[6, 200]]);
    doc.apply_progression_view(0, &view).unwrap();
    // The value edit collides only after the earlier progression writes have run.
    Connection::open(&path).unwrap().execute_batch("CREATE UNIQUE INDEX reject_override ON family5_values((CASE WHEN value=200 THEN 17 ELSE value END))").unwrap();
    let error = save(&mut doc).unwrap_err().to_string();
    assert!(error.contains("UNIQUE constraint failed"), "{error}");
    let after = loaded(&path);
    assert_eq!(after.progression, original.progression);
    assert_eq!(after.account_revision(), original.account_revision());
    assert_eq!(after.characters(), original.characters());
}
