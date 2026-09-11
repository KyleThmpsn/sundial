use super::{
    SqliteAccountDocument, SqliteAccountDocumentLoad, SqliteAccountError, document, writer,
};
use crate::test_support::TestDirectory;
use rusqlite::{Connection, params};
use std::{fs, path::Path};
use sundial_account::{
    AccountSettingKey, AccountSettingValue, AccountSettingsCommand, EquipmentSlot, ItemPlugs,
};

mod package;
mod safety;

pub(crate) fn default_resources() -> super::AccountDefaults {
    super::AccountDefaults {
        schema: super::contract::SCHEMA.to_owned(),
        rows: include_str!("fixtures/investment_defaults.sql").to_owned(),
        settings_schema: super::contract::SETTINGS_SCHEMA.to_owned(),
        settings_rows: include_str!("fixtures/account_settings_defaults.sql").to_owned(),
    }
}

pub(crate) fn create_fixture(path: &Path, inventory_flags: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch(super::contract::SCHEMA).unwrap();
    db.execute_batch(super::contract::SETTINGS_SCHEMA).unwrap();
    db.execute_batch(include_str!("fixtures/account_settings_defaults.sql"))
        .unwrap();
    db.execute(
        "INSERT INTO account VALUES (1, ?, 1)",
        [0x9EAA_3001_0010_0100_u64 as i64],
    )
    .unwrap();
    db.execute(
        "INSERT INTO characters VALUES (0, ?, 2, 1, 2, 40, 1, 0.0, 123, 0, 65535, -1, 4)",
        [0x9EAA_3002_0010_0100_u64 as i64],
    )
    .unwrap();
    db.execute_batch("INSERT INTO dismantle_rewards VALUES (0,20,3,16,1,1); INSERT INTO profile_items VALUES (0,5764607523034234881,10,25,2,1); UPDATE account_key_bindings SET primary_code=42 WHERE action=0; UPDATE account_display SET show_fps=1;").unwrap();
    for (
        location,
        position,
        soid,
        hash,
        level,
        quantity,
        serial,
        flags,
        policy,
        count,
        abilities,
    ) in [
        (
            0,
            0,
            0x4000_0000_0000_0001_i64,
            100,
            106,
            1,
            0,
            0,
            0,
            0,
            [4, 7, 10, 11, 2],
        ),
        (
            0,
            11,
            0x4000_0000_0000_0002_i64,
            200,
            106,
            1,
            1,
            1,
            1,
            2,
            [6, 8, 20, 21, 3],
        ),
        (
            1,
            0,
            0x4000_0000_0000_0003_i64,
            300,
            105,
            2,
            2,
            inventory_flags,
            0,
            0,
            [4, 7, 10, 11, 2],
        ),
    ] {
        db.execute(
            "INSERT INTO items VALUES (0,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,0)",
            params![
                location,
                position,
                soid,
                hash,
                level,
                quantity,
                serial,
                flags,
                policy,
                count,
                abilities[0],
                abilities[1],
                abilities[2],
                abilities[3],
                abilities[4]
            ],
        )
        .unwrap();
    }
    db.execute(
        "INSERT INTO sockets VALUES (?,0,77)",
        [0x4000_0000_0000_0002_i64],
    )
    .unwrap();
}

pub(crate) fn save_fixture_document(document: &mut SqliteAccountDocument, backup: &Path) {
    writer::save_for_test(document, backup.to_path_buf()).unwrap();
}
fn loaded(path: &Path) -> Box<SqliteAccountDocument> {
    match document::load(path).unwrap() {
        SqliteAccountDocumentLoad::Loaded(doc) => doc,
        other => panic!("expected loaded database, got {other:?}"),
    }
}
#[test]
fn shipped_upstream_defaults_load_and_round_trip_without_native_data_loss() {
    let dir = TestDirectory::new("official-defaults");
    let path = dir.0.join("investment.sqlite3");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("BEGIN").unwrap();
    for sql in [
        super::contract::SCHEMA,
        super::contract::SETTINGS_SCHEMA,
        include_str!("fixtures/investment_defaults.sql"),
        include_str!("fixtures/account_settings_defaults.sql"),
    ] {
        db.execute_batch(sql).unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    let before = document::database_revision(&db).unwrap();
    let mut doc = loaded(&path);
    assert_eq!(doc.characters().characters()[0].equipment.len(), 17);
    save_fixture_document(&mut doc, &dir.0.join("backup.sqlite3"));
    assert_eq!(document::database_revision(&db).unwrap(), before);
}
#[test]
fn sqlite_save_preserves_unknown_columns_seen_titles_and_other_tables() {
    let dir = TestDirectory::new("official-preserve");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 7);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE items ADD COLUMN future TEXT NOT NULL DEFAULT 'preserve'; UPDATE items SET seen=1; CREATE TABLE future_data (id INTEGER PRIMARY KEY, payload BLOB); INSERT INTO future_data VALUES(1,x'001122'); INSERT INTO pending_rewards(character_slot,kind,definition_hash,quantity) VALUES(0,0,987,1); INSERT INTO unlocks VALUES(-1,0,42,0,1);").unwrap();
    let mut doc = loaded(&path);
    let character = &doc.characters().characters()[0];
    let item_id = character.inventory[0].id;
    doc.characters_mut()
        .apply(
            SqliteAccountDocument::character_capabilities(),
            sundial_account::CharacterCommand::UpdateInventoryItem {
                item_id,
                update: sundial_account::ItemUpdate::SetQuantity(9),
            },
        )
        .unwrap();
    save_fixture_document(&mut doc, &dir.0.join("backup.sqlite3"));
    assert_eq!(
        db.query_row("SELECT quantity FROM items WHERE location=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        9
    );
    assert_eq!(
        db.query_row("SELECT future FROM items WHERE location=1", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "preserve"
    );
    assert_eq!(
        db.query_row("SELECT sum(seen) FROM items", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row("SELECT equipped_title FROM characters", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        65535
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM pending_rewards", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn null_socket_lanes_are_preserved() {
    let dir = TestDirectory::new("official-null-sockets");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    let mut doc = loaded(&path);
    assert_eq!(
        doc.characters().characters()[0].equipment[&EquipmentSlot::new("subclass")]
            .as_ref()
            .unwrap()
            .plugs,
        ItemPlugs::Authored(vec![Some(sundial_account::DefinitionHash::new(77)), None])
    );
    save_fixture_document(&mut doc, &dir.0.join("backup.sqlite3"));
    assert_eq!(
        db.query_row("SELECT slot FROM characters", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn external_wal_changes_in_unexposed_tables_reject_save() {
    let dir = TestDirectory::new("official-conflict");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    let mut doc = loaded(&path);
    db.execute("INSERT INTO unlocks VALUES(-1,0,42,0,1)", [])
        .unwrap();
    assert!(matches!(
        writer::save_for_test(&mut doc, dir.0.join("backup.sqlite3")),
        Err(SqliteAccountError::SourceChanged)
    ));
    assert!(!dir.0.join("backup.sqlite3").exists());
}
#[test]
fn normalized_preferences_save_without_resetting_reserved_native_values() {
    let dir = TestDirectory::new("official-preferences");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    let mut doc = loaded(&path);
    doc.settings_mut()
        .apply_all(
            SqliteAccountDocument::settings_capabilities(),
            [
                AccountSettingsCommand::Set {
                    key: AccountSettingKey::known_preference("field_of_view").unwrap(),
                    value: AccountSettingValue::Unsigned(155),
                },
                AccountSettingsCommand::Set {
                    key: AccountSettingKey::key_binding(
                        "fire",
                        sundial_account::KeyBindingSlot::Primary,
                    ),
                    value: AccountSettingValue::InputCode(0x473),
                },
                AccountSettingsCommand::Set {
                    key: AccountSettingKey::key_binding(
                        "fire",
                        sundial_account::KeyBindingSlot::Secondary,
                    ),
                    value: AccountSettingValue::Unassigned,
                },
            ],
        )
        .unwrap();
    save_fixture_document(&mut doc, &dir.0.join("backup.sqlite3"));
    assert_eq!(
        db.query_row(
            "SELECT primary_code,secondary_code FROM account_key_bindings WHERE action=0",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        )
        .unwrap(),
        (0x473, -1)
    );
    assert_eq!(
        db.query_row("SELECT field_of_view FROM account_display", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        155
    );
    assert_eq!(
        db.query_row("SELECT calibration_primary FROM account_display", [], |r| r
            .get::<_, f64>(0))
            .unwrap(),
        10000.0
    );
}
