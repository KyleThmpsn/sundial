use super::*;
use std::collections::BTreeMap;

#[test]
#[ignore = "requires SUNDIAL_TEST_INSTALL with the supported native packages"]
fn native_full_loadouts_fit_every_bucket_for_all_classes_and_supported_schemas() {
    let install = std::path::PathBuf::from(std::env::var_os("SUNDIAL_TEST_INSTALL").unwrap());
    let cache = crate::test_support::TestDirectory::new("inventory-native-sanity");
    let catalog =
        Catalog::load_or_scan_with_progress(&install, cache.0.join("catalog.json"), false, |_| {})
            .unwrap();
    for version in [6, 8, 16] {
        for class in 0..3 {
            let mut document = emote_document(version);
            document.json_mut()["state"]["characters"][0]["class"] = class.into();
            let original = document.clone();
            for _ in 0..16 {
                randomize_full_loadout(
                    &mut document,
                    &catalog,
                    0,
                    PlugSelectionMode::Supported,
                    false,
                    LoadoutOptions {
                        weapons: true,
                        armor: true,
                        equipment_flair: true,
                        subclass: true,
                        replace_held_inventory: true,
                        keep_locked_items: true,
                    },
                )
                .unwrap_or_else(|error| panic!("schema {version}, class {class}: {error}"));
                assert_native_placement(&document, &catalog, version, class);
                crate::app::account_validation::validate_new_account_catalog_issues(
                    &document, &original, &catalog, false,
                )
                .unwrap();
            }
        }
    }
}

fn assert_native_placement(
    document: &account::WorkspaceDocument,
    catalog: &Catalog,
    version: u64,
    class: i32,
) {
    let equipped = account::equipped_item_snapshots(document, 0).unwrap();
    assert_eq!(
        equipped.len(),
        document.equipment_slots().len(),
        "schema {version}, class {class}"
    );
    for item in &equipped {
        let definition = catalog.item(item.definition_hash.unwrap()).unwrap();
        assert_eq!(
            definition.bucket_hash, item.bucket_hash,
            "{}",
            definition.name
        );
        assert!(definition.class_type == 3 || definition.class_type == class as u64);
    }
    let held = account::character_inventory(document, 0).unwrap().unwrap();
    assert!(held.len() <= account::character_inventory_capacity(document));
    let mut counts = BTreeMap::<u8, (usize, usize, String)>::new();
    for hash in equipped
        .iter()
        .filter_map(|item| item.definition_hash)
        .chain(held.iter().map(|item| u64::from(item.definition_hash)))
    {
        let metadata = catalog.inventory_metadata(hash).unwrap();
        assert_eq!(metadata.scope, InventoryScope::Character);
        let capacity = usize::from(metadata.authored_row_capacity().unwrap());
        let entry = counts.entry(metadata.native_bucket_id).or_insert((
            0,
            capacity,
            metadata.bucket_label(),
        ));
        assert_eq!(entry.1, capacity);
        entry.0 += 1;
        assert!(
            entry.0 <= capacity,
            "schema {version}, class {class}: {} {}/{capacity}",
            entry.2,
            entry.0
        );
    }
    println!("schema {version}, class {class}: {counts:?}");
}
