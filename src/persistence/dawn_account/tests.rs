use super::*;
use rusqlite::Connection;
use std::path::Path;

/// Builds a database from Dawn's own schema with one account, one character and two items.
pub(crate) fn create_fixture(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch(contract::SCHEMA).unwrap();
    db.execute_batch(
        "UPDATE metadata SET value=2 WHERE key='account_revision';
         UPDATE metadata SET value=1 WHERE key='legacy_import_complete';
         INSERT INTO account VALUES(1,'9EAA300100100100');
         INSERT INTO allocators VALUES('item_instance','40000000000002AB'),
                                      ('profile_item_instance','500000000000001F');
         INSERT INTO characters VALUES(0,'9EAA300100100101',0,0,0,1,50,1,1,1.0,308080871,1,6,7,10,15,2,93,0);
         INSERT INTO profile_items VALUES(0,'500000000000000A',3159615086,73595,4);
         INSERT INTO character_items VALUES
            ('9EAA300100100101',0,3,'4000000000000004',4070132608,106,1,3,0,1,0),
            ('9EAA300100100101',1,0,'400000000000001A',2715114534,106,1,16,0,0,0);
         INSERT INTO item_sockets VALUES
            ('4000000000000004',0,3961599962),
            ('4000000000000004',1,NULL);
         INSERT INTO settings_values VALUES
            ('controls.mouseLookSensitivity',15,NULL),
            ('controls.adsSensitivityModifier',NULL,1.0),
            ('configured',1,NULL);
         INSERT INTO key_bindings VALUES(0,109,NULL),(3,60,NULL);",
    )
    .unwrap();
}

fn loaded(path: &Path) -> Box<DawnAccountDocument> {
    match load(path).unwrap() {
        DawnAccountDocumentLoad::Loaded(document) => document,
        other => panic!("expected a loaded database, got {other:?}"),
    }
}

#[test]
fn a_dawn_database_loads_into_storage_neutral_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let document = loaded(&path);

    assert_eq!(document.primary_soid().get(), 0x9EAA_3001_0010_0100);
    assert_eq!(document.account_revision(), 2);
    assert_eq!(document.next_item_soid(), 0x4000_0000_0000_02AB);
    assert_eq!(document.next_profile_item_soid(), 0x5000_0000_0000_001F);
    assert_eq!(document.profile().profile_items().len(), 1);

    let characters = document.characters().characters();
    assert_eq!(characters.len(), 1);
    let character = &characters[0];
    assert_eq!(character.soid.unwrap().get(), 0x9EAA_3001_0010_0101);
    let metadata = character.metadata.unwrap();
    assert_eq!(
        (metadata.race, metadata.gender, metadata.class_type),
        (0, 0, 1)
    );
    assert_eq!(metadata.abilities.melee, 15);

    // Equipment is sparse in Dawn, so the item at position 3 lands in the helmet slot with the
    // earlier slots left empty rather than shifting down.
    assert_eq!(character.equipment.len(), 1);
    let helmet = character
        .equipment
        .get(&EquipmentSlotKey::new("helmet"))
        .unwrap();
    let helmet = helmet.as_ref().unwrap();
    assert_eq!(helmet.definition_hash.get(), 4_070_132_608);
    assert_eq!(
        helmet.plugs,
        sundial_account::ItemPlugs::Authored(vec![
            Some(sundial_account::DefinitionHash::new(3_961_599_962)),
            None,
        ])
    );
    assert_eq!(character.inventory.len(), 1);
    assert_eq!(
        character.inventory[0].plugs,
        sundial_account::ItemPlugs::NativeDefaults
    );
}

use sundial_account::EquipmentSlot as EquipmentSlotKey;

#[test]
fn settings_and_key_bindings_become_storage_neutral_keys() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let document = loaded(&path);
    let values = document.settings().values();

    let sensitivity = sundial_account::AccountSettingKey::preference(
        sundial_account::AccountSettingGroup::Controls,
        "mouse_look_sensitivity",
    );
    assert_eq!(
        values.get(&sensitivity),
        Some(&sundial_account::AccountSettingValue::Unsigned(15))
    );
    let binding = sundial_account::AccountSettingKey::key_binding(
        "fire",
        sundial_account::KeyBindingSlot::Primary,
    );
    assert_eq!(
        values.get(&binding),
        Some(&sundial_account::AccountSettingValue::InputCode(109))
    );
    // "configured" carries no group, so it stays in the file rather than becoming a preference.
    assert!(
        values
            .keys()
            .all(|key| !format!("{key:?}").contains("configured"))
    );
}

#[test]
fn a_missing_or_unwritten_database_is_never_created() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    assert!(matches!(
        load(&path).unwrap(),
        DawnAccountDocumentLoad::Missing
    ));
    assert!(!path.exists(), "loading must not create the database");

    Connection::open(&path).unwrap();
    assert!(matches!(
        load(&path).unwrap(),
        DawnAccountDocumentLoad::Empty
    ));
}

#[test]
fn a_layout_dawn_would_refuse_is_reported_rather_than_loaded() {
    let directory = tempfile::tempdir().unwrap();
    for (label, sql) in [
        (
            "a fourth metadata row",
            "INSERT INTO metadata VALUES('sundial_edited',1)",
        ),
        (
            "an unknown allocator",
            "INSERT INTO allocators VALUES('other','4000000000000001')",
        ),
        (
            "a character position that does not start at zero",
            "UPDATE characters SET position=1",
        ),
        ("a class outside its range", "UPDATE characters SET class=7"),
        (
            "an inventory position that skips a row",
            "UPDATE character_items SET position=4 WHERE location=1",
        ),
        (
            "native plugs alongside stored socket lanes",
            "UPDATE character_items SET socket_policy=0 WHERE instance_soid='4000000000000004'",
        ),
    ] {
        let path = directory
            .path()
            .join(format!("{}.db", label.replace(' ', "-")));
        create_fixture(&path);
        Connection::open(&path).unwrap().execute_batch(sql).unwrap();
        match load(&path).unwrap() {
            DawnAccountDocumentLoad::Incompatible(problem) => {
                assert!(!problem.to_string().is_empty(), "{label}");
            }
            other => panic!("{label} should be reported as incompatible, got {other:?}"),
        }
    }
}

#[test]
fn a_newer_schema_is_surfaced_instead_of_being_read() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch("PRAGMA user_version=6")
        .unwrap();
    assert!(matches!(
        load(&path).unwrap(),
        DawnAccountDocumentLoad::Incompatible(DawnAccountIncompatibility::SchemaVersion {
            found: 6
        })
    ));
}

#[test]
fn the_database_sits_beside_the_settings_file() {
    let settings = Path::new("C:/Destiny2/bin/x64/Sunrise/settings.json");
    assert_eq!(
        crate::persistence::dawn_path(settings),
        Path::new("C:/Destiny2/bin/x64/Sunrise/player-state.db")
    );
}

#[test]
fn soids_round_trip_through_dawn_fixed_width_hexadecimal() {
    for value in [1_u64, 0x4000_0000_0000_0001, u64::MAX] {
        let text = contract::format_soid(value);
        assert_eq!(text.len(), contract::SOID_TEXT_LENGTH);
        assert_eq!(text, text.to_uppercase());
        assert_eq!(contract::parse_soid(&text), Some(value));
    }
    // Dawn requires exactly sixteen digits, so a short or long value is not a SOID.
    assert_eq!(contract::parse_soid("4000000000000"), None);
    assert_eq!(contract::parse_soid("00000000000000001"), None);
}

/// A save replaces the whole account graph, and `item_rolls` cascades away with the items it
/// belongs to, so anything this build does not model has to survive being written over. Without
/// this, opening an account and saving it would quietly take every roll with it.
#[test]
fn a_save_keeps_the_rows_this_build_does_not_model() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let kept = "4000000000000004";
    let entropy = vec![1_u8, 2, 3, 4, 5, 6, 7, 8];
    let owned = vec![9_u8; 96];
    {
        let db = Connection::open(&path).unwrap();
        db.execute(
            "INSERT INTO item_rolls(instance_soid,entropy,lane_mask,owned_rows) VALUES(?1,?2,?3,?4)",
            rusqlite::params![kept, entropy, 42_i64, owned],
        )
        .unwrap();
        db.execute(
            "UPDATE character_items SET postmaster=1 WHERE instance_soid=?1",
            rusqlite::params![kept],
        )
        .unwrap();
        db.execute("UPDATE characters SET vendor_campaigns=5", [])
            .unwrap();
    }

    let mut document = loaded(&path);
    super::writer::save(&mut document).unwrap();

    let db = Connection::open(&path).unwrap();
    let roll: (Vec<u8>, i64, Vec<u8>) = db
        .query_row(
            "SELECT entropy,lane_mask,owned_rows FROM item_rolls WHERE instance_soid=?1",
            rusqlite::params![kept],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the roll must survive the save that rewrote its item");
    assert_eq!(roll, (entropy, 42, owned));
    let postmaster: i64 = db
        .query_row(
            "SELECT postmaster FROM character_items WHERE instance_soid=?1",
            rusqlite::params![kept],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(postmaster, 1);
    let campaigns: i64 = db
        .query_row("SELECT vendor_campaigns FROM characters", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(campaigns, 5);
}

/// Loading an account and saving it back with no edits must leave the database as it was found.
///
/// The writer rebuilds the account graph from a storage-neutral model that holds a fraction of what
/// Dawn keeps, so every column it does not model has to survive the round trip. Before the carried
/// rows existed this rewrote character levels to 50, blanked profile-item identities, reset every
/// mutation serial and dropped all item rolls. Only account_revision may move.
#[test]
fn a_save_with_no_edits_returns_the_database_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    // Values a naive writer would overwrite with literals.
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "UPDATE characters SET last_selected=1,level=27,accepted=0,preview_available=0,
                appearance=0.25,last_destination=771,content_bypass=0,next_inventory_serial=41,
                vendor_campaigns=3;
             UPDATE character_items SET mutation_serial=9 WHERE instance_soid='4000000000000004';
             UPDATE character_items SET postmaster=1 WHERE instance_soid='400000000000001A';
             UPDATE profile_items SET instance_soid='500000000000000E',mutation_serial=13;
             INSERT INTO item_rolls(instance_soid,entropy,lane_mask,owned_rows)
                VALUES('4000000000000004',X'0102030405060708',7,zeroblob(96));
             INSERT INTO vendor_unlocks VALUES('9EAA300100100100',0,5,9,1);",
        )
        .unwrap();
    }
    let before = snapshot_tables(&path);

    let mut document = loaded(&path);
    super::writer::save(&mut document).unwrap();

    let after = snapshot_tables(&path);
    for (table, rows) in &before {
        assert_eq!(
            after.get(table),
            Some(rows),
            "{table} changed across a save that made no edits"
        );
    }
}

/// Every row of every table except metadata, whose account_revision legitimately advances.
fn snapshot_tables(path: &Path) -> std::collections::BTreeMap<String, Vec<String>> {
    let db = Connection::open(path).unwrap();
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut captured = std::collections::BTreeMap::new();
    for table in tables {
        if table == "metadata" {
            continue;
        }
        let columns: Vec<String> = db
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let selected = columns
            .iter()
            .map(|column| format!("quote({column})"))
            .collect::<Vec<_>>()
            .join("||','||");
        let rows: Vec<String> = db
            .prepare(&format!("SELECT {selected} FROM {table} ORDER BY 1"))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        captured.insert(table, rows);
    }
    captured
}

/// A settings edit has to reach the database, land in the column the key was read from, and leave
/// every key this build does not model exactly where Dawn put it.
///
/// Before the write path existed, the edit was accepted, reported saved and the revision advanced,
/// while the value never left memory.
#[test]
fn a_settings_edit_is_written_to_the_column_it_was_read_from() {
    use sundial_account::{
        AccountSettingGroup, AccountSettingKey, AccountSettingValue, AccountSettingsCommand,
        FiniteF64,
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let mut document = loaded(&path);

    let integer_key =
        AccountSettingKey::preference(AccountSettingGroup::Controls, "mouse_look_sensitivity");
    let real_key =
        AccountSettingKey::preference(AccountSettingGroup::Controls, "ads_sensitivity_modifier");
    document
        .settings_mut()
        .apply_all(
            DawnAccountDocument::settings_capabilities(),
            [
                AccountSettingsCommand::Set {
                    key: integer_key,
                    value: AccountSettingValue::Unsigned(22),
                },
                AccountSettingsCommand::Set {
                    key: real_key,
                    value: AccountSettingValue::Decimal(FiniteF64::new(1.25).unwrap()),
                },
            ],
        )
        .unwrap();
    super::writer::save(&mut document).unwrap();

    let db = Connection::open(&path).unwrap();
    let read = |key: &str| -> (Option<i64>, Option<f64>) {
        db.query_row(
            "SELECT integer_value,real_value FROM settings_values WHERE key=?1",
            rusqlite::params![key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    };
    // Each value goes back to its own column, and the other stays NULL.
    assert_eq!(read("controls.mouseLookSensitivity"), (Some(22), None));
    assert_eq!(read("controls.adsSensitivityModifier"), (None, Some(1.25)));
    // An ungrouped switch Dawn keeps and Sundial does not model is untouched.
    assert_eq!(read("configured"), (Some(1), None));

    // And it survives a reload, which is what the user sees next time.
    let reopened = loaded(&path);
    assert_eq!(
        reopened
            .settings()
            .values()
            .get(&AccountSettingKey::preference(
                AccountSettingGroup::Controls,
                "mouse_look_sensitivity"
            )),
        Some(&AccountSettingValue::Unsigned(22))
    );
}
