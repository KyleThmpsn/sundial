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
         INSERT INTO characters VALUES(0,'9EAA300100100101',0,0,0,1,50,1,1,1.0,308080871,1,6,7,10,15,2,93);
         INSERT INTO profile_items VALUES(0,'0000000000000000',3159615086,73595,0);
         INSERT INTO character_items VALUES
            ('9EAA300100100101',0,3,'4000000000000004',4070132608,106,1,3,0,1),
            ('9EAA300100100101',1,0,'400000000000001A',2715114534,106,1,16,0,0);
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
        .execute_batch("PRAGMA user_version=2")
        .unwrap();
    assert!(matches!(
        load(&path).unwrap(),
        DawnAccountDocumentLoad::Incompatible(DawnAccountIncompatibility::SchemaVersion {
            found: 2
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
