use crate::game_settings::MAX_SUPPORTED_SCHEMA;
use serde_json::json;
use sundial_account::{DefinitionHash, InstanceSoid, ItemInstance, ItemPlugs};

use super::*;

#[test]
fn equipment_copy_uses_command_time_state_and_removes_absent_flags() {
    let mut document = document(8);
    let mut destination = document["state"]["characters"][0].clone();
    destination["soid"] = json!("0x9EAA300200100101");
    destination["inventory"] = json!([]);
    destination["equipment"]["kinetic"]["instance_soid"] = json!("0x4000000000000003");
    destination["equipment"]["kinetic"]["flags"] = json!(1);
    document["state"]["characters"]
        .as_array_mut()
        .unwrap()
        .push(destination);
    let adapter = JsonCharacterAdapter::load(&document).unwrap();
    let source_id = adapter.state().characters()[0].id;
    let destination_id = adapter.state().characters()[1].id;
    let slot = EquipmentSlot::new("kinetic");
    let (_, projected, _) = adapter
        .apply(
            &document,
            CharacterCommand::Batch(vec![
                CharacterCommand::UpdateEquipmentItem {
                    character_id: source_id,
                    slot: slot.clone(),
                    update: ItemUpdate::SetLevel(200),
                },
                CharacterCommand::CopyEquipmentItems {
                    source_character_id: source_id,
                    destination_character_id: destination_id,
                    slots: vec![slot.clone()],
                },
                CharacterCommand::UpdateEquipmentItem {
                    character_id: source_id,
                    slot,
                    update: ItemUpdate::SetLevel(300),
                },
            ]),
        )
        .unwrap();
    let destination = &projected["state"]["characters"][1]["equipment"]["kinetic"];
    assert_eq!(destination["level"], json!(200));
    assert!(destination.get("flags").is_none());
    assert_eq!(destination["instance_soid"], json!("0x4000000000000003"));
    assert_eq!(
        projected["state"]["characters"][0]["equipment"]["kinetic"]["level"],
        json!(300)
    );
}

fn document(version: u64) -> Value {
    json!({
        "version": version,
        "state": {
            "account": {"primary_soid": "0x9EAA300100100100"},
            "characters": [{
                "soid": "0x9EAA300200100100",
                "class": 0,
                "equipment": {
                    "kinetic": {
                        "instance_soid": "0x4000000000000001",
                        "definition_hash": 10,
                        "level": 106,
                        "quantity": 1,
                        "plugs": null,
                        "opaque": {"keep": true}
                    },
                    "energy": null
                },
                "inventory": [{
                    "instance_soid": "0x4000000000000002",
                    "definition_hash": 20,
                    "level": 106,
                    "quantity": 1,
                    "plugs": [1, null],
                    "future": true
                }]
            }]
        }
    })
}

#[test]
fn loading_and_projecting_without_a_command_is_lossless() {
    let document = document(8);
    let adapter = JsonCharacterAdapter::load(&document).unwrap();

    assert_eq!(adapter.project(&document).unwrap(), document);
}

#[test]
fn field_edits_preserve_other_representations_and_unknown_members() {
    let document = document(8);
    let adapter = JsonCharacterAdapter::load(&document).unwrap();
    let item_id = adapter.state().characters()[0].inventory[0].id;
    let (_, projected, _) = adapter
        .apply(
            &document,
            CharacterCommand::UpdateInventoryItem {
                item_id,
                update: ItemUpdate::SetQuantity(3),
            },
        )
        .unwrap();

    assert_eq!(
        projected.pointer("/state/characters/0/inventory/0"),
        Some(&json!({
            "instance_soid": "0x4000000000000002",
            "definition_hash": 20,
            "level": 106,
            "quantity": 3,
            "plugs": [1, null],
            "future": true
        }))
    );
}

#[test]
fn swaps_move_original_raw_rows_without_rebuilding_them() {
    let document = document(8);
    let adapter = JsonCharacterAdapter::load(&document).unwrap();
    let item_id = adapter.state().characters()[0].inventory[0].id;
    let (_, projected, result) = adapter
        .apply(
            &document,
            CharacterCommand::SwapInventoryItemWithEquipment {
                item_id,
                slot: EquipmentSlot::new("kinetic"),
            },
        )
        .unwrap();

    assert_eq!(
        result,
        CharacterCommandResult::EquipmentSwapped { replaced: true }
    );
    assert_eq!(
        projected.pointer("/state/characters/0/equipment/kinetic/future"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        projected.pointer("/state/characters/0/inventory/0/opaque/keep"),
        Some(&Value::Bool(true))
    );
}

#[test]
fn socket_edits_preserve_unedited_raw_plugs_and_extend_with_nulls() {
    for version in [6, 8, 13] {
        let mut document = document(version);
        document["state"]["characters"][0]["inventory"][0]["plugs"] = json!([1, "0x00000002"]);
        let adapter = JsonCharacterAdapter::load(&document).unwrap();
        let item_id = adapter.state().characters()[0].inventory[0].id;
        let (_, projected, _) = adapter
            .apply(
                &document,
                CharacterCommand::UpdateInventoryItem {
                    item_id,
                    update: ItemUpdate::SetPlug {
                        index: 3,
                        plug: Some(DefinitionHash::new(42)),
                        default_plugs: vec![],
                    },
                },
            )
            .unwrap();
        assert_eq!(
            projected["state"]["characters"][0]["inventory"][0]["plugs"],
            json!([1, "0x00000002", null, "0x0000002A"])
        );
        assert_eq!(
            projected["state"]["characters"][0]["inventory"][0]["future"],
            true
        );
    }
}

#[test]
fn full_plug_replacement_is_not_lost_after_a_socket_edit() {
    for version in [6, 8, 13] {
        let document = document(version);
        let adapter = JsonCharacterAdapter::load(&document).unwrap();
        let item_id = adapter.state().characters()[0].inventory[0].id;
        let command = |update| CharacterCommand::UpdateInventoryItem { item_id, update };
        let (_, projected, _) = adapter
            .apply(
                &document,
                CharacterCommand::Batch(vec![
                    command(ItemUpdate::SetPlugs(ItemPlugs::Authored(vec![
                        Some(DefinitionHash::new(42)),
                        Some(DefinitionHash::new(43)),
                    ]))),
                    command(ItemUpdate::SetPlug {
                        index: 1,
                        plug: Some(DefinitionHash::new(44)),
                        default_plugs: vec![],
                    }),
                ]),
            )
            .unwrap();
        assert_eq!(
            projected["state"]["characters"][0]["inventory"][0]["plugs"],
            json!(["0x0000002A", "0x0000002C"])
        );
    }
}

#[test]
fn batch_projection_tracks_items_across_intermediate_states() {
    let document = document(8);
    let adapter = JsonCharacterAdapter::load(&document).unwrap();
    let character_id = adapter.state().characters()[0].id;
    let item_id = adapter.state().characters()[0].inventory[0].id;
    let (_, projected, result) = adapter
        .apply(
            &document,
            CharacterCommand::Batch(vec![
                CharacterCommand::SwapInventoryItemWithEquipment {
                    item_id,
                    slot: EquipmentSlot::new("kinetic"),
                },
                CharacterCommand::UpdateEquipmentItem {
                    character_id,
                    slot: EquipmentSlot::new("kinetic"),
                    update: ItemUpdate::SetLevel(107),
                },
            ]),
        )
        .unwrap();

    assert_eq!(
        result,
        CharacterCommandResult::Batch(vec![
            CharacterCommandResult::EquipmentSwapped { replaced: true },
            CharacterCommandResult::None,
        ])
    );
    assert_eq!(
        projected.pointer("/state/characters/0/equipment/kinetic/level"),
        Some(&json!(107))
    );
    assert_eq!(
        projected.pointer("/state/characters/0/equipment/kinetic/future"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        projected.pointer("/state/characters/0/inventory/0/opaque/keep"),
        Some(&Value::Bool(true))
    );
}

#[test]
fn future_unknown_equipment_slots_remain_opaque() {
    let mut document = document(MAX_SUPPORTED_SCHEMA + 1);
    document
        .pointer_mut("/state/characters/0/equipment")
        .and_then(Value::as_object_mut)
        .unwrap()
        .insert("future_slot".into(), json!({"future_layout": true}));
    let adapter = JsonCharacterAdapter::load(&document).unwrap();
    let item_id = adapter.state().characters()[0].inventory[0].id;
    let (_, projected, _) = adapter
        .apply(
            &document,
            CharacterCommand::UpdateInventoryItem {
                item_id,
                update: ItemUpdate::SetLevel(107),
            },
        )
        .unwrap();

    assert_eq!(
        projected.pointer("/state/characters/0/equipment/future_slot"),
        document.pointer("/state/characters/0/equipment/future_slot")
    );
}

#[test]
fn invalid_commands_leave_adapter_and_document_untouched() {
    let document = document(8);
    let original_document = document.clone();
    let adapter = JsonCharacterAdapter::load(&document).unwrap();
    let before = adapter.clone();
    let item_id = adapter.state().characters()[0].inventory[0].id;

    assert!(
        adapter
            .apply(
                &document,
                CharacterCommand::UpdateInventoryItem {
                    item_id,
                    update: ItemUpdate::SetQuantity(0),
                }
            )
            .is_err()
    );
    assert_eq!(adapter, before);
    assert_eq!(document, original_document);
}

#[test]
fn new_items_use_canonical_known_fields() {
    let document = document(8);
    let adapter = JsonCharacterAdapter::load(&document).unwrap();
    let character_id = adapter.state().characters()[0].id;
    let item_id = adapter.next_entity_id();
    let (_, projected, _) = adapter
        .apply(
            &document,
            CharacterCommand::AddInventoryItem {
                character_id,
                item: ItemInstance {
                    id: item_id,
                    instance_soid: InstanceSoid::try_from_u64(0x4000_0000_0000_0003).unwrap(),
                    definition_hash: DefinitionHash::new(30),
                    level: 106,
                    quantity: 2,
                    plugs: ItemPlugs::NativeDefaults,
                    flags: None,
                },
            },
        )
        .unwrap();

    assert_eq!(
        projected.pointer("/state/characters/0/inventory/1"),
        Some(&json!({
            "instance_soid": "0x4000000000000003",
            "definition_hash": "0x0000001E",
            "level": 106,
            "quantity": 2,
            "plugs": null
        }))
    );
}
