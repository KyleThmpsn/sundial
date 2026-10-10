mod baseline;

use std::collections::{HashMap, HashSet};

use super::*;
use crate::catalog::{AbilityOptions, InventoryScope, ItemStackability};

#[derive(Default)]
struct FakeCatalog {
    items: HashMap<u64, ItemDef>,
    inventory: HashMap<u64, InventoryMetadata>,
    plugs: HashSet<u64>,
}

impl AccountCatalog for FakeCatalog {
    fn item(&self, hash: u64) -> Option<&ItemDef> {
        self.items.get(&hash)
    }

    fn inventory_metadata(&self, hash: u64) -> Option<&InventoryMetadata> {
        self.inventory.get(&hash)
    }

    fn contains_plug(&self, hash: u64) -> bool {
        self.plugs.contains(&hash)
    }
}

fn definition(hash: u64, bucket_hash: u64, class_type: u64) -> ItemDef {
    ItemDef {
        hash,
        name: format!("Item {hash}"),
        type_name: String::new(),
        bucket_hash,
        class_type,
        default_plugs: Vec::new(),
        sockets: Vec::new(),
        abilities: AbilityOptions::default(),
    }
}

fn character_metadata(max_stack_size: u32) -> InventoryMetadata {
    InventoryMetadata {
        scope: InventoryScope::Character,
        native_bucket_id: 0,
        stackability: ItemStackability::Instanced,
        max_stack_size: Some(max_stack_size),
        bucket_capacity: Some(10),
    }
}

fn profile_metadata(max_stack_size: u32) -> InventoryMetadata {
    InventoryMetadata {
        scope: InventoryScope::Profile,
        native_bucket_id: 0,
        stackability: ItemStackability::Stackable,
        max_stack_size: Some(max_stack_size),
        bucket_capacity: Some(10),
    }
}

#[test]
fn character_reference_checks_item_class_stack_and_equipped_bucket() {
    let hash = 10;
    let expected_bucket = 20;
    let mut catalog = FakeCatalog::default();
    catalog
        .items
        .insert(hash, definition(hash, expected_bucket, 0));
    catalog.inventory.insert(hash, character_metadata(1));

    let mut issues = Vec::new();
    validate_character_item(
        &catalog,
        CharacterItemReference {
            context: "test item",
            hash,
            quantity: 1,
            class_type: Some(0),
            expected_bucket: Some(expected_bucket),
        },
        false,
        &mut issues,
    );
    assert!(issues.is_empty());

    validate_character_item(
        &catalog,
        CharacterItemReference {
            context: "bad item",
            hash,
            quantity: 2,
            class_type: Some(2),
            expected_bucket: Some(30),
        },
        false,
        &mut issues,
    );
    assert!(issues.iter().any(|issue| issue.contains("quantity 2")));
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("character is class 2"))
    );
    assert!(issues.iter().any(|issue| issue.contains("slot bucket")));
}

#[test]
fn profile_only_inventory_definitions_are_valid_catalog_references() {
    let hash = 5;
    let mut catalog = FakeCatalog::default();
    catalog.inventory.insert(hash, profile_metadata(999));
    let mut issues = Vec::new();

    validate_profile_item(&catalog, "profile-only item", hash, 1, &mut issues);

    assert!(issues.is_empty());
}

#[test]
fn plug_references_use_the_plug_catalog_instead_of_the_equipment_catalog() {
    let plug_hash = 20;
    let item_hash = 30;
    let mut catalog = FakeCatalog::default();
    catalog.plugs.insert(plug_hash);
    catalog.items.insert(item_hash, definition(item_hash, 0, 3));
    let mut issues = Vec::new();

    validate_plug(&catalog, "valid plug", 0, plug_hash, &mut issues);
    assert!(issues.is_empty());

    validate_plug(&catalog, "item used as plug", 0, item_hash, &mut issues);
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("item used as plug"))
    );
}

#[test]
fn character_and_plug_hashes_must_exist() {
    let catalog = FakeCatalog::default();
    let mut issues = Vec::new();
    validate_profile_item(&catalog, "missing profile item", 5, 1, &mut issues);
    validate_character_item(
        &catalog,
        CharacterItemReference {
            context: "missing item",
            hash: 10,
            quantity: 1,
            class_type: Some(0),
            expected_bucket: None,
        },
        false,
        &mut issues,
    );
    validate_plug(&catalog, "test item", 0, 20, &mut issues);

    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("missing profile item"))
    );
    assert!(issues.iter().any(|issue| issue.contains("missing item")));
    assert!(issues.iter().any(|issue| issue.contains("plug 1")));
}

#[test]
fn authored_socket_count_must_match_installed_shape_for_equipped_and_stored_items() {
    let mut catalog = FakeCatalog::default();
    let mut item = definition(10, 0, 3);
    item.default_plugs = vec![None; 4];
    catalog.items.insert(10, item);
    for count in [0, 3, 4, 5] {
        let mut stored_issues = Vec::new();
        validate_inventory_plugs(
            &catalog,
            "stored item",
            10,
            &ItemPlugs::Authored(vec![None; count]),
            &mut stored_issues,
        );
        let mut equipped_issues = Vec::new();
        validate_equipped_plugs(
            &catalog,
            "equipped item",
            10,
            &EquippedItemPlugs::Authored(vec![EquippedPlugValue::Empty; count]),
            &mut equipped_issues,
        );
        assert_eq!(stored_issues.is_empty(), count == 4);
        assert_eq!(equipped_issues.is_empty(), count == 4);
    }
    let mut issues = Vec::new();
    validate_inventory_plugs(
        &catalog,
        "stored item",
        10,
        &ItemPlugs::NativeDefaults,
        &mut issues,
    );
    validate_equipped_plugs(
        &catalog,
        "equipped item",
        10,
        &EquippedItemPlugs::NativeDefaults,
        &mut issues,
    );
    assert!(issues.is_empty());

    catalog.items.get_mut(&10).unwrap().default_plugs.clear();
    validate_inventory_plugs(
        &catalog,
        "socketless item",
        10,
        &ItemPlugs::Authored(vec![]),
        &mut issues,
    );
    assert!(issues.is_empty());
    validate_inventory_plugs(
        &catalog,
        "socketless item",
        10,
        &ItemPlugs::Authored(vec![None]),
        &mut issues,
    );
    assert!(issues.iter().any(|issue| issue.contains("requires 0")));
}
