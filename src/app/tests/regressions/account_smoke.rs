//! Less-used account edit paths exercised against complete release fixtures.
use super::schema_smoke::{FIXTURES, with_document};
use super::*;
use crate::app::inventory::{
    DismantleGearClass, DismantleRarity, DismantleRewardAction, InventoryItemAction,
    InventoryItemLocation, ItemPlugs, NewInventoryItem, ProfileItemAction,
};
use serde_json::json;

fn item_path(location: InventoryItemLocation) -> String {
    format!(
        "/state/characters/{}/inventory/{}",
        location.character_index, location.item_index
    )
}

fn roundtrip(document: &Value) {
    let directory = TestDirectory::new("rare-account-roundtrip");
    let settings = directory.0.join("玩家 settings.json");
    let backups = directory.0.join("backup folder");
    std::fs::write(&settings, settings::encode_settings(document).unwrap()).unwrap();
    let receipt = settings::save_json_with_backup_root(&settings, document, &backups).unwrap();
    assert_eq!(settings::load_json(&settings).unwrap(), *document);
    assert_eq!(settings::load_json(&receipt.backup).unwrap(), *document);
    settings::validate_document(document).unwrap();
}

fn transfer_cycle(original: &Value, source: usize, flags: u8, plugs: ItemPlugs) {
    let mut document = original.clone();
    let added =
        inventory::add_inventory_item(&mut document, source, NewInventoryItem::single(42, 10))
            .unwrap();
    document.pointer_mut(&item_path(added)).unwrap()["future_item"] =
        json!({"text":"玩家 🌅","nested":[null,false,42]});
    for action in [
        InventoryItemAction::SetFlags(Some(flags)),
        InventoryItemAction::SetPlugs(plugs),
    ] {
        inventory::apply_inventory_item_action(&mut document, added, action).unwrap();
    }
    let item = document.pointer(&item_path(added)).unwrap().clone();
    let before = document.clone();
    assert!(inventory::move_inventory_item_to_character(&mut document, added, source).is_err());
    assert!(inventory::move_inventory_item_to_character(&mut document, added, usize::MAX).is_err());
    assert_eq!(document, before);
    let moved = inventory::move_inventory_item_to_character(&mut document, added, (source + 1) % 3)
        .unwrap();
    assert_eq!(document.pointer(&item_path(moved)), Some(&item));
    let restored =
        inventory::move_inventory_item_to_character(&mut document, moved, source).unwrap();
    assert_eq!(document.pointer(&item_path(restored)), Some(&item));
    let error = settings::validate_document(&document).unwrap_err();
    assert!(error.contains("future_item"), "{error}");
    // Unknown members survive movement, but shipped schemas reject newly authored item keys.
    // Exercise a writable snapshot separately without weakening that validation boundary.
    let mut writable = document.clone();
    writable
        .pointer_mut(&item_path(restored))
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("future_item");
    roundtrip(&writable);
    inventory::apply_inventory_item_action(
        &mut document,
        restored,
        InventoryItemAction::SetFlags(None),
    )
    .unwrap();
    inventory::apply_inventory_item_action(&mut document, restored, InventoryItemAction::Remove)
        .unwrap();
    assert_eq!(document, *original);
}

#[test]
fn real_schema_inventory_transfers_preserve_flags_plugs_unknown_fields_and_disk_roundtrip() {
    for fixture in FIXTURES {
        let original: Value = serde_json::from_str(fixture).unwrap();
        let flags = if original["version"].as_u64().unwrap() >= 13 {
            0..=7
        } else {
            0..=3
        };
        for source in 0..3 {
            for flag in flags.clone() {
                for plugs in [
                    ItemPlugs::NativeDefaults,
                    ItemPlugs::Authored(vec![Some(123), None, Some(456)]),
                    ItemPlugs::Authored(vec![None; 12]),
                ] {
                    transfer_cycle(&original, source, flag, plugs);
                }
            }
        }
    }
}

#[test]
fn real_schema_profile_items_preserve_unknown_rows_and_reject_invalid_edits_atomically() {
    for fixture in FIXTURES {
        let mut document: Value = serde_json::from_str(fixture).unwrap();
        document["state"]["account"]["profile_items"][0]["future"] = json!({"keep":[false,null]});
        let original = document.clone();
        let row = inventory::add_profile_item(&mut document, 42, 1).unwrap();
        for quantity in [1, 999_999, i32::MAX] {
            inventory::apply_profile_item_action(
                &mut document,
                row,
                ProfileItemAction::SetQuantity(quantity),
            )
            .unwrap();
            let before = document.clone();
            for invalid in [0, -1, i32::MIN] {
                assert!(
                    inventory::apply_profile_item_action(
                        &mut document,
                        row,
                        ProfileItemAction::SetQuantity(invalid)
                    )
                    .is_err()
                );
                assert_eq!(document, before);
            }
            roundtrip(&document);
        }
        inventory::apply_profile_item_action(&mut document, row, ProfileItemAction::Remove)
            .unwrap();
        assert_eq!(document, original);
    }
}

#[test]
fn real_schema_dismantle_rules_honor_legacy_gates_and_preserve_other_account_data() {
    for fixture in FIXTURES {
        let mut document: Value = serde_json::from_str(fixture).unwrap();
        let original = document.clone();
        let row = inventory::add_dismantle_reward(&mut document, 42).unwrap();
        let before = document.clone();
        let filtered = DismantleRewardAction::SetPolicy {
            definition_hash: 42,
            quantity: 7,
            rarities: vec![DismantleRarity::Rare, DismantleRarity::Exotic],
            gear_class: Some(DismantleGearClass::Weapon),
            masterworked: Some(true),
        };
        let result = inventory::apply_dismantle_reward_action(&mut document, row, filtered);
        if original["version"].as_u64().unwrap() < 8 {
            assert!(result.is_err());
            assert_eq!(document, before);
        } else {
            result.unwrap();
        }
        roundtrip(&document);
        inventory::apply_dismantle_reward_action(&mut document, row, DismantleRewardAction::Remove)
            .unwrap();
        assert_eq!(document, original);
    }
}

#[test]
fn full_account_history_undo_redo_and_branching_preserve_schema_and_unknown_data() {
    for fixture in FIXTURES {
        let directory = TestDirectory::new("schema-history-smoke");
        let mut original: Value = serde_json::from_str(fixture).unwrap();
        original["future_extension"] = json!({"keep":"玩家 🌅"});
        let mut app = with_document(directory.0.clone(), original);
        let initial = app.document.clone();
        for index in 0..DOCUMENT_HISTORY_LIMIT + 9 {
            let before = app.document.clone();
            app.document.json_mut()["steam"]["user"]["persona_name"] =
                json!(format!("Smoke {index}"));
            app.dirty = true;
            app.record_document_change(before);
        }
        let final_document = app.document.clone();
        assert_eq!(app.undo_history.len(), DOCUMENT_HISTORY_LIMIT);
        for _ in 0..DOCUMENT_HISTORY_LIMIT {
            app.undo();
        }
        let oldest = app.document.clone();
        app.undo();
        assert_eq!(app.document, oldest);
        for _ in 0..DOCUMENT_HISTORY_LIMIT {
            app.redo();
        }
        assert_eq!(app.document, final_document);
        app.undo();
        // The frame following an undo consumes its history-suppression marker.
        app.record_document_change(app.document.clone());
        let before = app.document.clone();
        app.document.json_mut()["steam"]["user"]["persona_name"] = json!("Branched edit");
        app.record_document_change(before);
        assert!(app.redo_history.is_empty());
        assert_eq!(app.document.json()["version"], initial.json()["version"]);
        assert_eq!(app.document.json()["state"], initial.json()["state"]);
        assert_eq!(
            app.document.json()["future_extension"],
            initial.json()["future_extension"]
        );
        assert!(!app.settings_path.exists());
    }
}
