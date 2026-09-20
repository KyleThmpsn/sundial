use super::super::{
    DawnAccountDocumentLoad, load as load_document, save as save_document, tests::create_fixture,
};
use super::*;
use crate::catalog::{InventoryMetadata, InventoryScope, ItemStackability};

fn currency() -> InventoryMetadata {
    InventoryMetadata {
        scope: InventoryScope::Profile,
        native_bucket_id: 0,
        stackability: ItemStackability::Stackable,
        max_stack_size: Some(250000),
        bucket_capacity: Some(1),
    }
}
fn loaded(path: &std::path::Path) -> Box<DawnAccountDocument> {
    match load_document(path).unwrap() {
        DawnAccountDocumentLoad::Loaded(doc) => doc,
        other => panic!("{other:?}"),
    }
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("player-state.db");
    create_fixture(&path);
    (dir, path)
}

#[test]
fn runtime_fixed_width_reward_identities_round_trip_without_rewriting() {
    let (_dir, path) = fixture();
    let db = Connection::open(&path).unwrap();
    // Dawn's read_reward/offer_reward use parse_u64/bind_u64 for all five identities.
    // The SQL TEXT declaration is not a free-form text contract.
    db.execute_batch("INSERT INTO reward_debts(account_soid,character_soid,mission_hash,runtime_epoch,session_id,run_id,definition_hash,quantity) VALUES('9EAA300100100100','9EAA300100100101',123,'000000000000002A','FEDCBA9876543210','0000000000000000',3159615086,300000);").unwrap();
    let mut doc = loaded(&path);
    let expected = doc.reward_debts().to_vec();
    assert_eq!(expected[0].runtime_epoch, 42);
    assert_eq!(expected[0].session_id, 0xFEDC_BA98_7654_3210);
    assert_eq!(expected[0].run_id, 0);
    assert_eq!(expected[0].quantity, 300000);
    save_document(&mut doc).unwrap();
    assert_eq!(loaded(&path).reward_debts(), expected);
    db.execute(
        "UPDATE reward_debts SET session_id='not-a-native-identity'",
        [],
    )
    .unwrap();
    assert!(matches!(
        load_document(&path).unwrap(),
        DawnAccountDocumentLoad::Incompatible(_)
    ));
}

#[test]
fn currency_queue_edits_and_cancellation_preserve_history_and_missions() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    let before = doc.clone();
    doc.queue_currency(0, 3159615086, 500, &currency()).unwrap();
    assert!(doc.differs_from(&before));
    let first = doc.reward_debts()[0].clone();
    assert_eq!(first.mission_hash, EDITOR_MISSION);
    assert_eq!(first.session_id, EDITOR_SESSION);
    assert_eq!(first.run_id, first.id as u64);
    save_document(&mut doc).unwrap();
    let mut doc = loaded(&path);
    doc.set_debt_quantity(first.id, 1000, &currency()).unwrap();
    save_document(&mut doc).unwrap();
    doc.cancel_debt(first.id).unwrap();
    save_document(&mut doc).unwrap();
    let doc = loaded(&path);
    assert_eq!(doc.reward_debts().len(), 1);
    assert!(doc.reward_debts()[0].delivered);
    assert_eq!(doc.reward_debts()[0].credited, 0);
    assert_eq!(doc.reward_debts()[0].quantity, 1000);
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM missions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT value FROM metadata WHERE key='reward_epoch'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn queue_validation_rejects_non_currencies_bad_quantities_and_missing_characters() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    let before = doc.clone();
    for metadata in [
        InventoryMetadata {
            bucket_capacity: Some(50),
            ..currency()
        },
        InventoryMetadata {
            scope: InventoryScope::Character,
            ..currency()
        },
        InventoryMetadata {
            native_bucket_id: 14,
            ..currency()
        },
    ] {
        assert!(doc.queue_currency(0, 3159615086, 5, &metadata).is_err());
    }
    for quantity in [0, -1, 250001] {
        assert!(
            doc.queue_currency(0, 3159615086, quantity, &currency())
                .is_err()
        );
    }
    assert!(doc.queue_currency(9, 3159615086, 5, &currency()).is_err());
    assert!(doc.queue_currency(0, 0, 5, &currency()).is_err());
    assert!(!doc.differs_from(&before));
}

#[test]
fn queued_identity_respects_sqlite_sequence_and_does_not_recycle_deleted_ids() {
    let (_dir, path) = fixture();
    Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO sqlite_sequence(name,seq) VALUES('reward_debts',100)",
            [],
        )
        .unwrap();
    let mut doc = loaded(&path);
    doc.queue_currency(0, 3159615086, 5, &currency()).unwrap();
    assert_eq!(doc.reward_debts()[0].id, 101);
    save_document(&mut doc).unwrap();
    let mut doc = loaded(&path);
    doc.queue_currency(0, 3159615086, 5, &currency()).unwrap();
    assert_eq!(doc.reward_debts()[1].id, 102);
    save_document(&mut doc).unwrap();
}

#[test]
fn concurrent_delivery_cannot_be_edited_or_replayed() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    doc.queue_currency(0, 3159615086, 5, &currency()).unwrap();
    save_document(&mut doc).unwrap();
    doc.cancel_debt(1).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE reward_debts SET delivered=1,credited=5 WHERE debt_id=1",
            [],
        )
        .unwrap();
    assert!(
        save_document(&mut doc)
            .unwrap_err()
            .to_string()
            .contains("delivery changed")
    );
    let mut doc = loaded(&path);
    assert!(doc.cancel_debt(1).is_err());
    assert!(doc.set_debt_quantity(1, 10, &currency()).is_err());
    assert_eq!(doc.reward_debts()[0].credited, 5);
}

#[test]
fn save_undo_redo_cancels_without_deleting_and_can_restore_only_editor_cancellations() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    let mut original = doc.clone();
    doc.queue_currency(0, 3159615086, 5, &currency()).unwrap();
    save_document(&mut doc).unwrap();
    let mut queued = doc.clone();
    original.adopt_revision(&doc);
    save_document(&mut original).unwrap();
    assert!(loaded(&path).reward_debts()[0].delivered);
    queued.adopt_revision(&original);
    save_document(&mut queued).unwrap();
    assert!(!loaded(&path).reward_debts()[0].delivered);
    queued.cancel_debt(1).unwrap();
    save_document(&mut queued).unwrap();
    let mut reloaded = loaded(&path);
    reloaded.reward_debts[0].delivered = false;
    assert!(
        save_document(&mut reloaded).is_err(),
        "After reopening, finished history is immutable"
    );
}

#[test]
fn a_cancelled_new_reward_can_be_undone_after_its_first_save() {
    let (_dir, path) = fixture();
    let mut doc = loaded(&path);
    doc.queue_currency(0, 3159615086, 5, &currency()).unwrap();
    let mut pending = doc.clone();
    doc.cancel_debt(1).unwrap();
    save_document(&mut doc).unwrap();
    pending.adopt_revision(&doc);
    save_document(&mut pending).unwrap();
    assert!(!loaded(&path).reward_debts()[0].delivered);
}
