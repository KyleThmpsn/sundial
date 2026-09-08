//! Item identity and opaque state regressions for the optional SQLite account source.

use super::*;
use crate::app::inventory::{NewInventoryItem, set_inventory_locked_flag};
use crate::persistence::sqlite_account::SqliteAccountDocument;
use sundial_account::EquipmentSlot;

fn sqlite_document(document: &WorkspaceDocument) -> &SqliteAccountDocument {
    match &document.account {
        AccountDocument::Sqlite(sqlite) => sqlite,
        _ => panic!("fixture should select SQLite"),
    }
}

fn load_fixture(directory: &TestDirectory) -> WorkspaceDocument {
    WorkspaceDocument::load(
        json!({"version": 8, "state": {}}),
        &settings_path(directory),
    )
}

fn save_fixture(document: &mut WorkspaceDocument, directory: &TestDirectory) {
    let AccountDocument::Sqlite(sqlite) = &mut document.account else {
        panic!("fixture should select SQLite");
    };
    crate::persistence::sqlite_account::tests::save_fixture_document(
        sqlite,
        &directory.0.join("before-save.sqlite3"),
    );
}

fn equip_subclass(document: &mut WorkspaceDocument, item_index: usize) {
    super::super::super::equipment::equip_inventory_item(
        document,
        InventoryItemLocation {
            character_index: 0,
            item_index,
        },
        "subclass",
        &sqlite_subclass_item(),
        false,
    )
    .unwrap();
}

#[test]
fn new_subclass_abilities_survive_swaps_before_save_and_reload_while_stored() {
    let directory = TestDirectory::new("sqlite-new-subclass-abilities");
    let database_path = directory.0.join("state.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database_path, 3);
    set_fixture_inventory_subclass_abilities(&database_path, [5, 7, 10, 11, 2]);
    let mut document = load_fixture(&directory);
    let location =
        account::add_inventory_item(&mut document, 0, NewInventoryItem::single(201, 106)).unwrap();
    let new_soid = account::character_inventory(&document, 0).unwrap().unwrap()
        [location.item_index]
        .instance_soid;
    let selection = CharacterAbilities {
        movement: 6,
        grenade: 8,
        super_ability: 20,
        melee: 21,
        class_ability: 3,
    };
    equip_subclass(&mut document, location.item_index);
    account::apply_character_updates(
        &mut document,
        0,
        vec![CharacterMetadataUpdate::SetAbilities(selection)],
    )
    .unwrap();

    equip_subclass(&mut document, 0);
    equip_subclass(&mut document, 0);
    assert_eq!(
        account::character_metadata(&document, 0).unwrap().abilities,
        selection
    );

    equip_subclass(&mut document, 0);
    save_fixture(&mut document, &directory);
    let mut reloaded = load_fixture(&directory);
    let location = account::character_inventory(&reloaded, 0)
        .unwrap()
        .unwrap()
        .into_iter()
        .find(|item| item.instance_soid == new_soid)
        .unwrap()
        .location;
    assert_eq!(
        account::persisted_inventory_item_abilities(&reloaded, location),
        Some(selection)
    );
    equip_subclass(&mut reloaded, location.item_index);
    assert_eq!(
        account::character_metadata(&reloaded, 0).unwrap().abilities,
        selection
    );
}

#[test]
fn replacing_the_last_loaded_item_does_not_reuse_its_persistence_state() {
    let directory = TestDirectory::new("sqlite-replaced-item-identity");
    let database_path = directory.0.join("state.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database_path, 3);
    set_fixture_inventory_subclass_abilities(&database_path, [6, 8, 20, 21, 3]);
    let mut document = load_fixture(&directory);
    let removed_id = sqlite_document(&document).characters().characters()[0].inventory[0].id;
    account::remove_character_inventory_items(&mut document, 0, [0]).unwrap();
    let location =
        account::add_inventory_item(&mut document, 0, NewInventoryItem::single(201, 106)).unwrap();
    let new_id = sqlite_document(&document).characters().characters()[0].inventory[0].id;
    assert_ne!(new_id, removed_id);
    assert_eq!(
        account::persisted_inventory_item_abilities(&document, location),
        None
    );

    save_fixture(&mut document, &directory);
    let connection = Connection::open(database_path).unwrap();
    let serial: i64 = connection
        .query_row(
            "SELECT mutation_serial FROM character_items WHERE location = 1 AND position = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serial, 4,
        "a new item must receive the next character serial"
    );
}

#[test]
fn removing_a_new_subclass_does_not_leak_its_unsaved_ability_selection() {
    let directory = TestDirectory::new("sqlite-removed-new-subclass");
    let database_path = directory.0.join("state.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database_path, 3);
    set_fixture_inventory_subclass_abilities(&database_path, [5, 7, 10, 11, 2]);
    let mut document = load_fixture(&directory);
    let location =
        account::add_inventory_item(&mut document, 0, NewInventoryItem::single(201, 106)).unwrap();
    equip_subclass(&mut document, location.item_index);
    let removed_id = sqlite_document(&document).characters().characters()[0].equipment
        [&EquipmentSlot::new("subclass")]
        .as_ref()
        .unwrap()
        .id;
    equip_subclass(&mut document, 0);
    account::remove_character_inventory_items(&mut document, 0, [0]).unwrap();

    let replacement =
        account::add_inventory_item(&mut document, 0, NewInventoryItem::single(201, 106)).unwrap();
    let new_id = sqlite_document(&document).characters().characters()[0].inventory
        [replacement.item_index]
        .id;
    assert_ne!(new_id, removed_id);
    assert_eq!(
        account::persisted_inventory_item_abilities(&document, replacement),
        None
    );
}

#[test]
fn sqlite_lock_controls_preserve_upper_flags_for_stored_and_equipped_items() {
    const FLAGS: u32 = 0xABCD_EF01;
    let directory = TestDirectory::new("sqlite-opaque-item-flags");
    let database_path = directory.0.join("state.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database_path, FLAGS);
    Connection::open(&database_path)
        .unwrap()
        .execute(
            "UPDATE character_items SET flags = ? WHERE location = 0 AND position = 0",
            [i64::from(FLAGS)],
        )
        .unwrap();
    let mut document = load_fixture(&directory);
    let location = InventoryItemLocation {
        character_index: 0,
        item_index: 0,
    };

    for locked in [false, true] {
        let inventory = account::character_inventory(&document, 0).unwrap().unwrap();
        let equipment = account::equipped_item_snapshots(&document, 0).unwrap();
        let kinetic = equipment
            .iter()
            .find(|item| item.slot == "kinetic")
            .unwrap();
        assert_eq!(inventory[0].flags, Some(u8::from(!locked)));
        assert_eq!(kinetic.flags, Some(u8::from(!locked)));
        assert!(kinetic.issues.is_empty());
        account::apply_inventory_item_action(
            &mut document,
            location,
            InventoryItemAction::SetFlags(set_inventory_locked_flag(inventory[0].flags, locked)),
        )
        .unwrap();
        account::set_equipment_item_flags(
            &mut document,
            0,
            "kinetic",
            set_inventory_locked_flag(kinetic.flags, locked),
        )
        .unwrap();

        let character = &sqlite_document(&document).characters().characters()[0];
        let expected = Some((FLAGS & !1) | u32::from(locked));
        assert_eq!(character.inventory[0].flags, expected);
        assert_eq!(
            character.equipment[&EquipmentSlot::new("kinetic")]
                .as_ref()
                .unwrap()
                .flags,
            expected
        );
    }
    save_fixture(&mut document, &directory);
    let reloaded = load_fixture(&directory);
    let character = &sqlite_document(&reloaded).characters().characters()[0];
    assert_eq!(character.inventory[0].flags, Some(FLAGS));
    assert_eq!(
        character.equipment[&EquipmentSlot::new("kinetic")]
            .as_ref()
            .unwrap()
            .flags,
        Some(FLAGS)
    );
}
