use super::*;

fn stacks() -> (tempfile::TempDir, DawnAccountDocument) {
    let (temp, doc) = fixture();
    let db = Connection::open(&doc.path).unwrap();
    db.execute_batch(
        "DELETE FROM profile_items;
        INSERT INTO profile_items VALUES(0,'0000000000000000',101,10,7);
        INSERT INTO profile_items VALUES(1,'0000000000000000',102,20,19);
        INSERT INTO profile_items VALUES(2,'500000000000000a',103,30,11);",
    )
    .unwrap();
    (temp, loaded(&doc.path))
}

fn rows(doc: &DawnAccountDocument) -> Vec<(String, u32, i32, i64)> {
    Connection::open(&doc.path).unwrap()
        .prepare("SELECT instance_soid,definition_hash,quantity,mutation_serial FROM profile_items ORDER BY position").unwrap()
        .query_map([], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap()
        .collect::<Result<_,_>>().unwrap()
}

#[test]
fn zero_identity_stacks_keep_independent_serials_on_settings_save_and_reload() {
    let (_temp, mut doc) = stacks();
    let before = rows(&doc);
    let allocator = doc.next_profile_item_soid();
    edit_settings(&mut doc);
    save(&mut doc).unwrap();
    assert_eq!(rows(&doc), before);
    assert_eq!(doc.next_profile_item_soid(), allocator);
    let mut reloaded = loaded(&doc.path);
    save(&mut reloaded).unwrap();
    assert_eq!(rows(&doc), before);
}

#[test]
fn removing_first_stack_does_not_reassign_later_serials_and_undo_restores_rows() {
    let (_temp, mut doc) = stacks();
    let mut original = doc.clone();
    let before = rows(&doc);
    let id = doc.profile().profile_items()[0].id;
    doc.profile_mut()
        .apply_profile_item(
            DawnAccountDocument::profile_capabilities(),
            ProfileItemCommand::Remove { id },
        )
        .unwrap();
    save(&mut doc).unwrap();
    assert_eq!(rows(&doc), before[1..]);
    save(&mut doc).unwrap();
    assert_eq!(rows(&doc), before[1..]);
    original.adopt_revision(&doc);
    save(&mut original).unwrap();
    let restored = rows(&original);
    assert_eq!(&restored[1..], &before[1..]);
    assert_eq!(
        (&restored[0].0, restored[0].1, restored[0].2),
        (&before[0].0, before[0].1, before[0].2)
    );
    assert!(restored[0].3 >= before[0].3);
}

#[test]
fn editing_one_zero_identity_stack_updates_only_its_serial() {
    let (_temp, mut doc) = stacks();
    let before = rows(&doc);
    let id = doc.profile().profile_items()[0].id;
    doc.profile_mut()
        .apply_profile_item(
            DawnAccountDocument::profile_capabilities(),
            ProfileItemCommand::SetQuantity { id, quantity: 50 },
        )
        .unwrap();
    save(&mut doc).unwrap();
    let after = rows(&doc);
    assert_eq!(after[0], ("0000000000000000".into(), 101, 50, 20));
    assert_eq!(&after[1..], &before[1..]);
    save(&mut doc).unwrap();
    assert_eq!(rows(&doc), after);
}

#[test]
fn malformed_profile_identity_is_incompatible_not_repaired() {
    let (_temp, doc) = stacks();
    Connection::open(&doc.path)
        .unwrap()
        .execute(
            "UPDATE profile_items SET instance_soid='oops' WHERE position=0",
            [],
        )
        .unwrap();
    assert!(matches!(
        load(&doc.path).unwrap(),
        DawnAccountDocumentLoad::Incompatible(_)
    ));
}

#[test]
fn exhausted_profile_serial_refuses_edit_without_partial_writes() {
    let (_temp, doc) = stacks();
    let db = Connection::open(&doc.path).unwrap();
    db.execute(
        "UPDATE profile_items SET mutation_serial=2147483647 WHERE position=1",
        [],
    )
    .unwrap();
    let mut doc = loaded(&doc.path);
    let id = doc.profile().profile_items()[0].id;
    doc.profile_mut()
        .apply_profile_item(
            DawnAccountDocument::profile_capabilities(),
            ProfileItemCommand::SetQuantity { id, quantity: 99 },
        )
        .unwrap();
    let original = doc.clone();
    let before = snapshot::capture(&db).unwrap();
    assert!(
        save(&mut doc)
            .unwrap_err()
            .to_string()
            .contains("no mutation serial")
    );
    assert_eq!(doc, original);
    assert_eq!(snapshot::capture(&db).unwrap(), before);
}

#[test]
fn zero_identity_postmaster_recovery_updates_only_the_matching_stack() {
    let (_temp, doc) = stacks();
    let db = Connection::open(&doc.path).unwrap();
    db.execute_batch(
        "UPDATE character_items SET definition_hash=101,quantity=3,postmaster=1 WHERE location=1",
    )
    .unwrap();
    let mut doc = loaded(&doc.path);
    doc.recover_postmaster(0, 0x400000000000001A, 3, |_| {
        Some(crate::catalog::InventoryMetadata {
            scope: crate::catalog::InventoryScope::Profile,
            native_bucket_id: 15,
            stackability: crate::catalog::ItemStackability::Stackable,
            max_stack_size: Some(999),
            bucket_capacity: Some(100),
        })
    })
    .unwrap();
    save(&mut doc).unwrap();
    assert_eq!(
        rows(&doc),
        vec![
            ("0000000000000000".into(), 101, 13, 20),
            ("0000000000000000".into(), 102, 20, 19),
            ("500000000000000a".into(), 103, 30, 11)
        ]
    );
}
