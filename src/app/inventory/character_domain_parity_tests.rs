//! Differential tests between legacy JSON character mutations and the neutral aggregate.

use serde_json::{Value, json};
use sundial_account as domain;

use crate::app::equipment::{
    equip_definition, legacy_document_tests as legacy_equipment, set_equipment_item_flags,
    set_equipment_item_level, set_equipment_item_plug, set_weapon_slot_empty,
};
use crate::persistence::json_account::JsonCharacterAdapter;

use super::legacy_character_tests as legacy_json;
use super::*;

fn document() -> Value {
    json!({
        "version": 8,
        "state": {
            "account": {
                "primary_soid": "0x9EAA300100100100",
                "profile_items": []
            },
            "characters": [
                {
                    "soid": "0x9EAA300200100100",
                    "class": 0,
                    "equipment": {
                        "kinetic": {
                            "instance_soid": "0x4000000000000001",
                            "definition_hash": 10,
                            "level": 106,
                            "quantity": 1,
                            "plugs": null,
                            "equipped_unknown": {"keep": true}
                        },
                        "energy": null
                    },
                    "inventory": [{
                        "instance_soid": "0x4000000000000002",
                        "definition_hash": 20,
                        "level": 105,
                        "quantity": 1,
                        "plugs": [1, null],
                        "flags": 1,
                        "stored_unknown": [1, 2]
                    }]
                },
                {
                    "soid": "0x9EAA300200100101",
                    "class": 1,
                    "equipment": {},
                    "inventory": []
                }
            ]
        }
    })
}

fn assert_both_unchanged(
    production: &Value,
    production_before: &Value,
    legacy: &Value,
    legacy_before: &Value,
) {
    assert_eq!(production, production_before);
    assert_eq!(legacy, legacy_before);
}

fn item_id(
    adapter: &JsonCharacterAdapter,
    character_index: usize,
    item_index: usize,
) -> domain::EntityId {
    adapter.state().characters()[character_index].inventory[item_index].id
}

fn character_entity_id(adapter: &JsonCharacterAdapter, character_index: usize) -> domain::EntityId {
    adapter.state().characters()[character_index].id
}

#[test]
fn production_inventory_cutover_matches_frozen_legacy_exactly() {
    let mut production = document();
    let mut legacy = production.clone();

    let location = InventoryItemLocation {
        character_index: 0,
        item_index: 0,
    };
    let action = InventoryItemAction::SetQuantity(2);
    assert_eq!(
        apply_inventory_item_action(&mut production, location, action.clone()),
        legacy_json::apply_inventory_item_action(&mut legacy, location, action)
    );
    assert_eq!(production, legacy);

    assert_eq!(
        swap_inventory_item_with_equipment(&mut production, location, "kinetic"),
        legacy_json::swap_inventory_item_with_equipment(&mut legacy, location, "kinetic")
    );
    assert_eq!(production, legacy);

    assert_eq!(
        move_equipment_item_to_inventory(&mut production, 0, "kinetic"),
        legacy_json::move_equipment_item_to_inventory(&mut legacy, 0, "kinetic")
    );
    assert_eq!(production, legacy);

    assert_eq!(
        move_inventory_item_to_character(&mut production, location, 1),
        legacy_json::move_inventory_item_to_character(&mut legacy, location, 1)
    );
    assert_eq!(production, legacy);

    let item = NewInventoryItem {
        definition_hash: 30,
        level: 106,
        quantity: 2,
    };
    assert_eq!(
        add_inventory_item(&mut production, 1, item),
        legacy_json::add_inventory_item(&mut legacy, 1, item)
    );
    assert_eq!(production, legacy);
}

#[test]
fn production_equipment_cutover_matches_frozen_legacy_exactly() {
    let defaults = vec![Some("0x00000001".to_owned()), None];
    let mut production = document();
    let mut legacy = production.clone();

    assert_eq!(
        set_equipment_item_level(&mut production, 0, "kinetic", 107),
        legacy_equipment::set_equipment_item_level(&mut legacy, 0, "kinetic", 107)
    );
    assert_eq!(production, legacy);

    assert_eq!(
        set_equipment_item_flags(&mut production, 0, "kinetic", Some(2)),
        legacy_equipment::set_equipment_item_flags(&mut legacy, 0, "kinetic", Some(2))
    );
    assert_eq!(production, legacy);

    *production
        .pointer_mut("/state/characters/0/equipment/kinetic/plugs")
        .unwrap() = json!([1, null]);
    *legacy
        .pointer_mut("/state/characters/0/equipment/kinetic/plugs")
        .unwrap() = json!([1, null]);
    assert_eq!(
        set_equipment_item_plug(&mut production, 0, "kinetic", 1, &defaults, Some(5)),
        legacy_equipment::set_equipment_item_plug(&mut legacy, 0, "kinetic", 1, &defaults, Some(5),)
    );
    assert_eq!(production, legacy);
    assert_eq!(
        production.pointer("/state/characters/0/equipment/kinetic/plugs/0"),
        Some(&Value::from(1))
    );

    assert_eq!(
        equip_definition(&mut production, 0, "kinetic", 30, &defaults),
        legacy_equipment::equip_definition(&mut legacy, 0, "kinetic", 30, &defaults)
    );
    assert_eq!(production, legacy);

    assert_eq!(
        equip_definition(&mut production, 0, "energy", 40, &defaults),
        legacy_equipment::equip_definition(&mut legacy, 0, "energy", 40, &defaults)
    );
    assert_eq!(production, legacy);

    assert_eq!(
        set_weapon_slot_empty(&mut production, 0, "kinetic"),
        legacy_equipment::set_weapon_slot_empty(&mut legacy, 0, "kinetic")
    );
    assert_eq!(production, legacy);
}

#[test]
fn production_equipment_failures_match_frozen_legacy_and_remain_atomic() {
    let mut production = document();
    let mut legacy = production.clone();
    let source = production.clone();
    assert_eq!(
        set_equipment_item_level(&mut production, 0, "kinetic", -1),
        legacy_equipment::set_equipment_item_level(&mut legacy, 0, "kinetic", -1)
    );
    assert_both_unchanged(&production, &source, &legacy, &source);

    let mut production = document();
    production
        .pointer_mut("/state/characters/0/equipment/kinetic")
        .and_then(Value::as_object_mut)
        .unwrap()
        .remove("plugs");
    let mut legacy = production.clone();
    let source = production.clone();
    assert_eq!(
        set_equipment_item_plug(&mut production, 0, "kinetic", 0, &[], Some(5)),
        legacy_equipment::set_equipment_item_plug(&mut legacy, 0, "kinetic", 0, &[], Some(5),)
    );
    assert_both_unchanged(&production, &source, &legacy, &source);

    let mut production = document();
    *production
        .pointer_mut("/state/characters/0/equipment/kinetic")
        .unwrap() = Value::String("malformed".into());
    let mut legacy = production.clone();
    let source = production.clone();
    assert_eq!(
        equip_definition(&mut production, 0, "kinetic", 30, &[]),
        legacy_equipment::equip_definition(&mut legacy, 0, "kinetic", 30, &[])
    );
    assert_both_unchanged(&production, &source, &legacy, &source);

    assert_eq!(
        set_weapon_slot_empty(&mut production, 0, "kinetic"),
        legacy_equipment::set_weapon_slot_empty(&mut legacy, 0, "kinetic")
    );
    assert_both_unchanged(&production, &source, &legacy, &source);

    let mut production = document();
    *production.get_mut("version").unwrap() = Value::from(3);
    let mut legacy = production.clone();
    let source = production.clone();
    assert_eq!(
        set_equipment_item_flags(&mut production, 0, "kinetic", Some(1)),
        legacy_equipment::set_equipment_item_flags(&mut legacy, 0, "kinetic", Some(1))
    );
    assert_both_unchanged(&production, &source, &legacy, &source);

    let mut production = document();
    let mut legacy = production.clone();
    let source = production.clone();
    assert_eq!(
        set_weapon_slot_empty(&mut production, 0, "helmet"),
        legacy_equipment::set_weapon_slot_empty(&mut legacy, 0, "helmet")
    );
    assert_both_unchanged(&production, &source, &legacy, &source);
}

#[test]
fn production_equipment_patches_match_legacy_on_minimal_rows_and_versionless_documents() {
    let defaults = vec![Some("0x00000001".to_owned()), None];
    let mut production = json!({
        "version": 6,
        "state": {
            "characters": [{
                "equipment": {
                    "kinetic": {
                        "level": 200,
                        "flags": 2,
                        "plugs": [1, null],
                        "opaque": {"keep": true}
                    }
                }
            }]
        }
    });
    let mut legacy = production.clone();

    assert_eq!(
        set_equipment_item_level(&mut production, 0, "kinetic", 75),
        legacy_equipment::set_equipment_item_level(&mut legacy, 0, "kinetic", 75)
    );
    assert_eq!(
        set_equipment_item_flags(&mut production, 0, "kinetic", Some(3)),
        legacy_equipment::set_equipment_item_flags(&mut legacy, 0, "kinetic", Some(3))
    );
    assert_eq!(
        set_equipment_item_plug(&mut production, 0, "kinetic", 1, &defaults, Some(5)),
        legacy_equipment::set_equipment_item_plug(&mut legacy, 0, "kinetic", 1, &defaults, Some(5),)
    );
    assert_eq!(production, legacy);

    let mut production = document();
    production.as_object_mut().unwrap().remove("version");
    let mut legacy = production.clone();
    assert_eq!(
        set_weapon_slot_empty(&mut production, 0, "kinetic"),
        legacy_equipment::set_weapon_slot_empty(&mut legacy, 0, "kinetic")
    );
    assert_eq!(
        equip_definition(&mut production, 0, "kinetic", 30, &defaults),
        legacy_equipment::equip_definition(&mut legacy, 0, "kinetic", 30, &defaults)
    );
    assert_eq!(production, legacy);
}

#[test]
fn production_inventory_failures_match_frozen_legacy_and_remain_atomic() {
    let location = InventoryItemLocation {
        character_index: 0,
        item_index: 0,
    };

    let mut production = document();
    let mut legacy = production.clone();
    let production_before = production.clone();
    let legacy_before = legacy.clone();
    assert_eq!(
        apply_inventory_item_action(
            &mut production,
            location,
            InventoryItemAction::SetQuantity(0),
        ),
        legacy_json::apply_inventory_item_action(
            &mut legacy,
            location,
            InventoryItemAction::SetQuantity(0),
        )
    );
    assert_both_unchanged(&production, &production_before, &legacy, &legacy_before);

    let mut production = document();
    *production
        .pointer_mut("/state/characters/1/inventory")
        .unwrap() = Value::Array(
        (0..CHARACTER_INVENTORY_CAPACITY)
            .map(|index| {
                json!({
                    "instance_soid": format!("0x{:016X}", 0x5000_0000_0000_0000_u64 + index as u64),
                    "definition_hash": 40,
                    "level": 106,
                    "quantity": 1,
                    "plugs": null
                })
            })
            .collect(),
    );
    let mut legacy = production.clone();
    let production_before = production.clone();
    let legacy_before = legacy.clone();
    assert_eq!(
        move_inventory_item_to_character(&mut production, location, 1),
        legacy_json::move_inventory_item_to_character(&mut legacy, location, 1)
    );
    assert_both_unchanged(&production, &production_before, &legacy, &legacy_before);

    let mut production = document();
    let mut legacy = production.clone();
    let production_before = production.clone();
    let legacy_before = legacy.clone();
    assert_eq!(
        move_equipment_item_to_inventory(&mut production, 0, "energy"),
        legacy_json::move_equipment_item_to_inventory(&mut legacy, 0, "energy")
    );
    assert_both_unchanged(&production, &production_before, &legacy, &legacy_before);

    let mut production = document();
    production
        .pointer_mut("/state/characters/0")
        .and_then(Value::as_object_mut)
        .unwrap()
        .remove("inventory");
    *production
        .pointer_mut("/state/characters/0/equipment")
        .unwrap() = Value::String("malformed".into());
    let mut legacy = production.clone();
    let source = production.clone();
    assert_eq!(
        swap_inventory_item_with_equipment(&mut production, location, "kinetic"),
        legacy_json::swap_inventory_item_with_equipment(&mut legacy, location, "kinetic")
    );
    assert_both_unchanged(&production, &source, &legacy, &source);

    let mut production = document();
    *production
        .pointer_mut("/state/characters/0/inventory")
        .unwrap() = Value::String("destination malformed".into());
    *production
        .pointer_mut("/state/characters/1/inventory")
        .unwrap() = Value::String("source malformed".into());
    let mut legacy = production.clone();
    let source = production.clone();
    let reverse_location = InventoryItemLocation {
        character_index: 1,
        item_index: 0,
    };
    assert_eq!(
        move_inventory_item_to_character(&mut production, reverse_location, 0),
        legacy_json::move_inventory_item_to_character(&mut legacy, reverse_location, 0)
    );
    assert_both_unchanged(&production, &source, &legacy, &source);
}

#[test]
fn inventory_field_edits_match_legacy_json_exactly() {
    let actions = [
        InventoryItemAction::SetDefinitionHash(30),
        InventoryItemAction::SetLevel(107),
        InventoryItemAction::SetPlugs(ItemPlugs::Authored(vec![Some(4), None])),
        InventoryItemAction::SetFlags(Some(2)),
        InventoryItemAction::SetFlags(None),
    ];
    for action in actions {
        let mut legacy = document();
        let source = legacy.clone();
        let adapter = JsonCharacterAdapter::load_inventory_item(&source, 0).unwrap();
        let item_id = item_id(&adapter, 0, 0);
        let update = match action.clone() {
            InventoryItemAction::SetDefinitionHash(hash) => {
                domain::ItemUpdate::SetDefinitionHash(domain::DefinitionHash::new(hash))
            }
            InventoryItemAction::SetLevel(level) => domain::ItemUpdate::SetLevel(level),
            InventoryItemAction::SetQuantity(quantity) => domain::ItemUpdate::SetQuantity(quantity),
            InventoryItemAction::SetPlugs(plugs) => {
                domain::ItemUpdate::SetPlugs(domain_plugs(plugs))
            }
            InventoryItemAction::SetFlags(flags) => {
                domain::ItemUpdate::SetFlags(flags.map(u32::from))
            }
            InventoryItemAction::Remove => unreachable!(),
        };
        let (_, projected, _) = adapter
            .apply(
                &source,
                domain::CharacterCommand::UpdateInventoryItem { item_id, update },
            )
            .unwrap();
        legacy_json::apply_inventory_item_action(
            &mut legacy,
            InventoryItemLocation {
                character_index: 0,
                item_index: 0,
            },
            action,
        )
        .unwrap();

        assert_eq!(projected, legacy);
    }
}

#[test]
fn inventory_remove_matches_legacy_json_exactly() {
    let mut legacy = document();
    let source = legacy.clone();
    let adapter = JsonCharacterAdapter::load_inventory_item(&source, 0).unwrap();
    let item_id = item_id(&adapter, 0, 0);
    let (_, projected, _) = adapter
        .apply(
            &source,
            domain::CharacterCommand::RemoveInventoryItem { item_id },
        )
        .unwrap();
    legacy_json::apply_inventory_item_action(
        &mut legacy,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        InventoryItemAction::Remove,
    )
    .unwrap();
    assert_eq!(projected, legacy);
}

#[test]
fn inventory_equipment_swaps_match_legacy_json_exactly() {
    let slot = "energy";
    let mut legacy = document();
    let source = legacy.clone();
    let adapter = JsonCharacterAdapter::load_inventory_equipment_slot(&source, 0, slot).unwrap();
    let item_id = item_id(&adapter, 0, 0);
    let (_, projected, result) = adapter
        .apply(
            &source,
            domain::CharacterCommand::SwapInventoryItemWithEquipment {
                item_id,
                slot: domain::EquipmentSlot::new(slot),
            },
        )
        .unwrap();
    let replaced = legacy_json::swap_inventory_item_with_equipment(
        &mut legacy,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        slot,
    )
    .unwrap();

    assert_eq!(
        result,
        domain::CharacterCommandResult::EquipmentSwapped { replaced }
    );
    assert_eq!(projected, legacy);
}

#[test]
fn scoped_inventory_edits_leave_malformed_sibling_characters_opaque() {
    let mut legacy = document();
    legacy
        .pointer_mut("/state/characters/0")
        .and_then(Value::as_object_mut)
        .unwrap()
        .remove("soid");
    *legacy.pointer_mut("/state/characters/1/inventory").unwrap() =
        Value::String("opaque inventory".into());
    *legacy.pointer_mut("/state/characters/1/equipment").unwrap() =
        Value::String("opaque equipment".into());
    let source = legacy.clone();
    assert!(JsonCharacterAdapter::load(&source).is_err());
    let adapter = JsonCharacterAdapter::load_inventory_item(&source, 0).unwrap();
    let item_id = item_id(&adapter, 0, 0);
    let (_, projected, _) = adapter
        .apply(
            &source,
            domain::CharacterCommand::UpdateInventoryItem {
                item_id,
                update: domain::ItemUpdate::SetQuantity(2),
            },
        )
        .unwrap();
    legacy_json::apply_inventory_item_action(
        &mut legacy,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        InventoryItemAction::SetQuantity(2),
    )
    .unwrap();

    assert_eq!(projected, legacy);
}

#[test]
fn scoped_moves_ignore_unrelated_malformed_characters() {
    let mut legacy = document();
    legacy
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .unwrap()
        .push(json!({"inventory": "opaque", "equipment": 7}));
    let source = legacy.clone();
    let adapter = JsonCharacterAdapter::load_inventory_move(&source, 0, 1).unwrap();
    let item_id = item_id(&adapter, 0, 0);
    let destination_character_id = character_entity_id(&adapter, 1);
    let (_, projected, _) = adapter
        .apply(
            &source,
            domain::CharacterCommand::MoveInventoryItem {
                item_id,
                destination_character_id,
            },
        )
        .unwrap();
    legacy_json::move_inventory_item_to_character(
        &mut legacy,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        1,
    )
    .unwrap();

    assert_eq!(projected, legacy);
}

#[test]
fn scoped_equipment_operations_ignore_unowned_malformed_rows() {
    let mut legacy = document();
    legacy
        .pointer_mut("/state/characters/0/equipment")
        .and_then(Value::as_object_mut)
        .unwrap()
        .insert("heavy".into(), Value::String("opaque".into()));
    let source = legacy.clone();
    let adapter =
        JsonCharacterAdapter::load_inventory_equipment_slot(&source, 0, "kinetic").unwrap();
    let item_id = item_id(&adapter, 0, 0);
    let (_, projected, _) = adapter
        .apply(
            &source,
            domain::CharacterCommand::SwapInventoryItemWithEquipment {
                item_id,
                slot: domain::EquipmentSlot::new("kinetic"),
            },
        )
        .unwrap();
    legacy_json::swap_inventory_item_with_equipment(
        &mut legacy,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        "kinetic",
    )
    .unwrap();
    assert_eq!(projected, legacy);

    let mut legacy = document();
    *legacy.pointer_mut("/state/characters/0/inventory").unwrap() = Value::String("opaque".into());
    let source = legacy.clone();
    let adapter = JsonCharacterAdapter::load_equipment_slot(&source, 0, "kinetic").unwrap();
    let character_id = character_entity_id(&adapter, 0);
    let (_, projected, _) = adapter
        .apply(
            &source,
            domain::CharacterCommand::UpdateEquipmentItem {
                character_id,
                slot: domain::EquipmentSlot::new("kinetic"),
                update: domain::ItemUpdate::SetLevel(107),
            },
        )
        .unwrap();
    legacy_equipment::set_equipment_item_level(&mut legacy, 0, "kinetic", 107).unwrap();
    assert_eq!(projected, legacy);
}

#[test]
fn add_uses_identity_only_equipment_scanning_and_tolerates_existing_duplicate_soids() {
    let mut legacy = document();
    legacy
        .pointer_mut("/state/characters/0/equipment")
        .and_then(Value::as_object_mut)
        .unwrap()
        .insert(
            "heavy".into(),
            json!({
                "instance_soid": format_instance_soid(GENERATED_INSTANCE_SOID_START + 2),
                "opaque": true
            }),
        );
    *legacy
        .pointer_mut("/state/characters/0/inventory/0/instance_soid")
        .unwrap() = Value::String(format_instance_soid(GENERATED_INSTANCE_SOID_START));
    let source = legacy.clone();
    assert!(JsonCharacterAdapter::load(&source).is_err());
    let adapter = JsonCharacterAdapter::load_for_inventory_add(&source).unwrap();
    let character_id = character_entity_id(&adapter, 1);
    let instance_soid = adapter
        .state()
        .next_available_instance_soid(
            domain::InstanceSoid::try_from_u64(GENERATED_INSTANCE_SOID_START).unwrap(),
        )
        .unwrap();
    let (_, projected, _) = adapter
        .apply(
            &source,
            domain::CharacterCommand::AddInventoryItem {
                character_id,
                item: domain::ItemInstance {
                    id: adapter.next_entity_id(),
                    instance_soid,
                    definition_hash: domain::DefinitionHash::new(30),
                    level: 106,
                    quantity: 1,
                    plugs: domain::ItemPlugs::NativeDefaults,
                    flags: None,
                },
            },
        )
        .unwrap();
    legacy_json::add_inventory_item(&mut legacy, 1, NewInventoryItem::single(30, 106)).unwrap();

    assert_eq!(projected, legacy);
    assert_eq!(instance_soid.get(), GENERATED_INSTANCE_SOID_START + 1);
}

#[test]
fn future_unknown_equipment_layouts_remain_opaque_in_scoped_edits() {
    let mut legacy = document();
    legacy["version"] = Value::from(crate::game_settings::MAX_SUPPORTED_SCHEMA + 1);
    legacy
        .pointer_mut("/state/characters/0/equipment")
        .and_then(Value::as_object_mut)
        .unwrap()
        .insert("future_slot".into(), json!({"not_an_item": true}));
    let source = legacy.clone();
    let adapter = JsonCharacterAdapter::load_inventory_item(&source, 0).unwrap();
    let item_id = item_id(&adapter, 0, 0);
    let (_, projected, _) = adapter
        .apply(
            &source,
            domain::CharacterCommand::UpdateInventoryItem {
                item_id,
                update: domain::ItemUpdate::SetLevel(108),
            },
        )
        .unwrap();
    legacy_json::apply_inventory_item_action(
        &mut legacy,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        InventoryItemAction::SetLevel(108),
    )
    .unwrap();

    assert_eq!(projected, legacy);
}

fn domain_plugs(plugs: ItemPlugs) -> domain::ItemPlugs {
    match plugs {
        ItemPlugs::NativeDefaults => domain::ItemPlugs::NativeDefaults,
        ItemPlugs::Authored(plugs) => domain::ItemPlugs::Authored(
            plugs
                .into_iter()
                .map(|plug| plug.map(domain::DefinitionHash::new))
                .collect(),
        ),
    }
}
