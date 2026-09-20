use super::*;
use crate::persistence::dawn_account::{DawnAccountDocumentLoad, load, tests::create_fixture};
use sundial_account::{
    AccountSettingGroup, AccountSettingKey, AccountSettingValue, AccountSettingsCommand,
    DefinitionHash, KeyBindingSlot, ProfileItem, ProfileItemCommand,
};

mod profile_stacks;
mod schema;

fn fixture() -> (tempfile::TempDir, DawnAccountDocument) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let document = loaded(&path);
    (directory, document)
}

#[test]
fn lowercase_native_identities_keep_all_carried_rows() {
    let (_directory, mut document) = fixture();
    let db = Connection::open(&document.path).unwrap();
    db.execute_batch("BEGIN;
        PRAGMA defer_foreign_keys=ON;
        UPDATE account SET primary_soid=lower(primary_soid);
        UPDATE characters SET soid=lower(soid),level=47;
        UPDATE character_items SET character_soid=lower(character_soid),instance_soid=lower(instance_soid),postmaster=1;
        UPDATE item_sockets SET instance_soid=lower(instance_soid);
        UPDATE profile_items SET instance_soid=lower(instance_soid);
        INSERT INTO item_rolls VALUES('400000000000001a',zeroblob(8),0,zeroblob(96));
        INSERT INTO missions VALUES('9eaa300100100101',123,456,2,7,9,0,100);
        COMMIT;").unwrap();
    document = loaded(&document.path);
    let before = snapshot::capture(&db).unwrap();
    save(&mut document).unwrap();
    db.execute(
        "UPDATE metadata SET value=2 WHERE key='account_revision'",
        [],
    )
    .unwrap();
    assert_eq!(snapshot::capture(&db).unwrap(), before);
}

fn loaded(path: &Path) -> DawnAccountDocument {
    let DawnAccountDocumentLoad::Loaded(document) = load(path).unwrap() else {
        panic!("Expected a Dawn account");
    };
    *document
}

fn edit_settings(document: &mut DawnAccountDocument) {
    let commands = [
        AccountSettingsCommand::Set {
            key: AccountSettingKey::preference(AccountSettingGroup::Display, "field_of_view"),
            value: AccountSettingValue::Unsigned(100),
        },
        AccountSettingsCommand::Set {
            key: AccountSettingKey::key_binding("fire", KeyBindingSlot::Primary),
            value: AccountSettingValue::InputCode(60),
        },
    ];
    document
        .settings_mut()
        .apply_all(DawnAccountDocument::settings_capabilities(), commands)
        .unwrap();
}

fn add_profile_item(document: &mut DawnAccountDocument) {
    let item = ProfileItem {
        id: document.next_entity_id().unwrap(),
        definition_hash: DefinitionHash::new(3159615086),
        quantity: 5,
        instance_soid: None,
    };
    document
        .profile_mut()
        .apply_profile_item(
            DawnAccountDocument::profile_capabilities(),
            ProfileItemCommand::Add(item),
        )
        .unwrap();
}

fn assert_reloaded_profile(document: &DawnAccountDocument) {
    // Entity IDs are session-local. SOIDs, hashes, and quantities are persisted.
    let rows = |document: &DawnAccountDocument| {
        document
            .profile()
            .profile_items()
            .iter()
            .map(|item| (item.instance_soid, item.definition_hash, item.quantity))
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&loaded(&document.path)), rows(document));
}

#[test]
fn settings_and_bindings_survive_saved_undo_and_redo() {
    let (_directory, mut edited) = fixture();
    let mut original = edited.clone();
    edit_settings(&mut edited);
    save(&mut edited).unwrap();
    assert_eq!(loaded(&edited.path).settings(), edited.settings());
    original.adopt_revision(&edited);
    save(&mut original).unwrap();
    assert_eq!(loaded(&original.path).settings(), original.settings());
    edited.adopt_revision(&original);
    save(&mut edited).unwrap();
    assert_eq!(loaded(&edited.path).settings(), edited.settings());
}

#[test]
fn rollback_rebases_settings_and_preserves_new_item_identities_on_retry() {
    let (_directory, mut document) = fixture();
    let original = document.clone();
    edit_settings(&mut document);
    add_profile_item(&mut document);
    let receipt = save(&mut document).unwrap();
    let items = document.profile().profile_items().to_vec();
    rollback_save(&document.path, &receipt).unwrap();
    assert_eq!(loaded(&document.path).snapshot, original.snapshot);
    document.adopt_revision(&original);
    save(&mut document).unwrap();
    assert_eq!(document.profile().profile_items(), items);
    assert_eq!(loaded(&document.path).settings(), document.settings());
    assert_reloaded_profile(&document);
}

#[test]
fn newly_added_profile_stacks_keep_zero_identities_without_consuming_allocator() {
    let (_directory, mut document) = fixture();
    let first = document.next_profile_item_soid();
    add_profile_item(&mut document);
    add_profile_item(&mut document);
    save(&mut document).unwrap();
    let items = document.profile().profile_items().to_vec();
    assert_eq!(items[1].instance_soid, None);
    assert_eq!(items[2].instance_soid, None);
    assert_eq!(document.next_profile_item_soid(), first);
    let next = document.next_profile_item_soid();
    save(&mut document).unwrap();
    assert_eq!(document.profile().profile_items(), items);
    assert_eq!(document.next_profile_item_soid(), next);
    assert_reloaded_profile(&document);
}

#[test]
fn failed_commit_does_not_assign_identities_or_advance_the_document() {
    let (_directory, mut document) = fixture();
    let db = Connection::open(&document.path).unwrap();
    // A second profile row violates this index after the graph rewrite has started.
    db.execute_batch("CREATE UNIQUE INDEX reject_second_profile_row ON profile_items((1))")
        .unwrap();
    add_profile_item(&mut document);
    let before = document.clone();
    let stored = snapshot::capture(&db).unwrap();
    assert!(save(&mut document).is_err());
    assert_eq!(document, before);
    assert_eq!(snapshot::capture(&db).unwrap(), stored);
}

#[test]
fn rollback_refuses_versioned_unversioned_and_schema_changes() {
    for sql in [
        "UPDATE metadata SET value=value+1 WHERE key='account_revision'",
        "UPDATE settings_values SET integer_value=99 WHERE key='pc.fieldOfViewAdjustment'",
        "INSERT INTO family5_values VALUES(3,7)",
        "INSERT INTO reward_debts VALUES(NULL,'9EAA300100100100','9EAA300100100101',1,'epoch','session','run',3159615086,5,1,0)",
        "CREATE TABLE runtime_extension(value BLOB)",
    ] {
        let (_directory, mut document) = fixture();
        edit_settings(&mut document);
        let receipt = save(&mut document).unwrap();
        let db = Connection::open(&document.path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL").unwrap();
        db.execute_batch(sql).unwrap();
        let newer = snapshot::capture(&db).unwrap();
        let error = rollback_save(&document.path, &receipt).unwrap_err();
        assert!(
            error.to_string().contains("changed after saving"),
            "{error}"
        );
        assert_eq!(snapshot::capture(&db).unwrap(), newer);
        assert!(receipt.backup.is_file());
    }
}

#[test]
fn rollback_restores_wal_rows_foreign_keys_and_extension_sequences_exactly() {
    let (_directory, mut document) = fixture();
    let db = Connection::open(&document.path).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
        CREATE TABLE z_extension(id INTEGER PRIMARY KEY AUTOINCREMENT, value BLOB);
        INSERT INTO z_extension VALUES(17,X'0011');
        INSERT INTO z_extension VALUES(35,NULL);
        DELETE FROM z_extension WHERE id=35;
        INSERT INTO item_rolls VALUES('4000000000000004',zeroblob(8),0,zeroblob(96));",
    )
    .unwrap();
    document = loaded(&document.path);
    let before = snapshot::capture(&db).unwrap();
    edit_settings(&mut document);
    let receipt = save(&mut document).unwrap();
    rollback_save(&document.path, &receipt).unwrap();
    assert_eq!(snapshot::capture(&db).unwrap(), before);
}

#[test]
fn rollback_failure_is_atomic() {
    let (_directory, document) = fixture();
    let db = Connection::open(&document.path).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_restore BEFORE INSERT ON metadata
        WHEN NEW.key='account_revision' AND NEW.value=2
        BEGIN SELECT RAISE(ABORT,'injected rollback failure'); END;",
    )
    .unwrap();
    // Exercise rollback failure independently of save's new unknown-trigger guard.
    let before = snapshot::capture(&db).unwrap();
    db.execute(
        "UPDATE metadata SET value=3 WHERE key='account_revision'",
        [],
    )
    .unwrap();
    let committed = snapshot::capture(&db).unwrap();
    let receipt = DawnSaveReceipt {
        backup: PathBuf::new(),
        revision: 3,
        before,
        committed: committed.clone(),
    };
    assert!(rollback_save(&document.path, &receipt).is_err());
    assert_eq!(snapshot::capture(&db).unwrap(), committed);
}
