use std::num::NonZeroU64;

use super::*;

const CAPABILITIES: CharacterCapabilities = CharacterCapabilities {
    metadata_writable: true,
    inventory_writable: true,
    equipment_writable: true,
    equipment_flags_writable: true,
    inventory_capacity: Some(3),
    enforce_loaded_inventory_capacity: true,
    max_item_plugs: 4,
    item_flag_mask: 3,
    enforce_unique_instance_soids: true,
};

fn id(value: u64) -> EntityId {
    EntityId::new(NonZeroU64::new(value).unwrap())
}

fn soid(value: u64) -> InstanceSoid {
    InstanceSoid::try_from_u64(value).unwrap()
}

fn item(id_value: u64, soid_value: u64, definition_hash: u32) -> ItemInstance {
    ItemInstance {
        id: id(id_value),
        instance_soid: soid(soid_value),
        definition_hash: DefinitionHash::new(definition_hash),
        level: 106,
        quantity: 1,
        plugs: ItemPlugs::NativeDefaults,
        flags: None,
    }
}

fn character(id_value: u64, soid_value: u64, inventory: Vec<ItemInstance>) -> Character {
    Character {
        id: id(id_value),
        soid: Some(soid(soid_value)),
        metadata: None,
        inventory,
        equipment: BTreeMap::new(),
    }
}

fn state() -> CharacterState {
    CharacterState::try_new(
        CAPABILITIES,
        vec![soid(1)],
        vec![
            character(10, 2, vec![item(20, 3, 100)]),
            character(11, 4, Vec::new()),
        ],
    )
    .unwrap()
}

fn metadata() -> CharacterMetadata {
    CharacterMetadata {
        race: 0,
        gender: 0,
        class_type: 0,
        abilities: CharacterAbilities {
            movement: 4,
            grenade: 7,
            super_ability: 10,
            melee: 11,
            class_ability: 2,
        },
    }
}

#[test]
fn metadata_batches_validate_atomically() {
    let mut state = state();
    state.characters[0].metadata = Some(metadata());
    let before = state.clone();

    let error = state
        .apply(
            CAPABILITIES,
            CharacterCommand::Batch(vec![
                CharacterCommand::UpdateMetadata {
                    character_id: id(10),
                    update: CharacterMetadataUpdate::SetAppearanceAndClass {
                        race: 2,
                        gender: 1,
                        class_type: 2,
                    },
                },
                CharacterCommand::UpdateMetadata {
                    character_id: id(10),
                    update: CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
                        movement: 64,
                        ..metadata().abilities
                    }),
                },
            ]),
        )
        .unwrap_err();

    assert_eq!(error, AccountError::InvalidCharacterMetadata);
    assert_eq!(state, before);
}

#[test]
fn metadata_commands_require_loaded_writable_metadata() {
    let mut state = state();
    let command = CharacterCommand::UpdateMetadata {
        character_id: id(10),
        update: CharacterMetadataUpdate::SetSuperAndMelee {
            super_ability: 20,
            melee: 21,
        },
    };
    assert_eq!(
        state.apply(CAPABILITIES, command.clone()),
        Err(AccountError::CharacterMetadataNotLoaded)
    );

    state.characters[0].metadata = Some(metadata());
    let mut read_only = CAPABILITIES;
    read_only.metadata_writable = false;
    let before = state.clone();
    assert_eq!(
        state.apply(read_only, command),
        Err(AccountError::CharacterMetadataReadOnly)
    );
    assert_eq!(state, before);
}

#[test]
fn equipment_copies_preserve_destination_identity() {
    let mut state = state();
    let slot = EquipmentSlot::new("helmet");
    state.characters[0]
        .equipment
        .insert(slot.clone(), Some(item(21, 5, 101)));
    state.characters[1]
        .equipment
        .insert(slot.clone(), Some(item(22, 6, 202)));

    state
        .apply(
            CAPABILITIES,
            CharacterCommand::CopyEquipmentItems {
                source_character_id: id(10),
                destination_character_id: id(11),
                slots: vec![slot.clone()],
            },
        )
        .unwrap();

    let copied = state.characters[1].equipment[&slot].as_ref().unwrap();
    assert_eq!(copied.id, id(22));
    assert_eq!(copied.instance_soid, soid(6));
    assert_eq!(copied.definition_hash, DefinitionHash::new(101));
}

#[test]
fn loaded_state_rejects_duplicate_soids_across_every_location() {
    let error = CharacterState::try_new(
        CAPABILITIES,
        vec![soid(1)],
        vec![character(10, 2, vec![item(20, 1, 100)])],
    )
    .unwrap_err();

    assert_eq!(error, AccountError::DuplicateInstanceSoid(1));
}

#[test]
fn invalid_and_over_capacity_adds_are_atomic() {
    let mut state = state();
    let before = state.clone();
    let invalid = ItemInstance {
        quantity: 0,
        ..item(21, 5, 101)
    };
    assert_eq!(
        state.apply(
            CAPABILITIES,
            CharacterCommand::AddInventoryItem {
                character_id: id(10),
                item: invalid,
            }
        ),
        Err(AccountError::InvalidQuantity)
    );
    assert_eq!(state, before);

    state
        .apply(
            CAPABILITIES,
            CharacterCommand::AddInventoryItem {
                character_id: id(10),
                item: item(21, 5, 101),
            },
        )
        .unwrap();
    state
        .apply(
            CAPABILITIES,
            CharacterCommand::AddInventoryItem {
                character_id: id(10),
                item: item(22, 6, 102),
            },
        )
        .unwrap();
    let before = state.clone();
    assert_eq!(
        state.apply(
            CAPABILITIES,
            CharacterCommand::AddInventoryItem {
                character_id: id(10),
                item: item(23, 7, 103),
            }
        ),
        Err(AccountError::CapacityExceeded {
            entity: EntityKind::ItemInstance,
            capacity: 3,
        })
    );
    assert_eq!(state, before);
}

#[test]
fn failed_unequip_is_atomic_when_inventory_is_full() {
    let mut state = state();
    state.characters[0].inventory.push(item(21, 5, 101));
    state.characters[0].inventory.push(item(22, 6, 102));
    let slot = EquipmentSlot::new("heavy");
    state.characters[0]
        .equipment
        .insert(slot.clone(), Some(item(23, 7, 103)));
    let before = state.clone();

    assert_eq!(
        state.apply(
            CAPABILITIES,
            CharacterCommand::MoveEquipmentItemToInventory {
                character_id: id(10),
                slot,
            }
        ),
        Err(AccountError::CapacityExceeded {
            entity: EntityKind::ItemInstance,
            capacity: 3,
        })
    );
    assert_eq!(state, before);
}

#[test]
fn item_edits_validate_plugs_flags_and_permissions_atomically() {
    let mut state = state();
    let before = state.clone();
    assert_eq!(
        state.apply(
            CAPABILITIES,
            CharacterCommand::UpdateInventoryItem {
                item_id: id(20),
                update: ItemUpdate::SetPlugs(ItemPlugs::Authored(vec![None; 5])),
            }
        ),
        Err(AccountError::TooManyItemPlugs { maximum: 4 })
    );
    assert_eq!(state, before);

    let mut read_only_flags = CAPABILITIES;
    read_only_flags.equipment_flags_writable = false;
    let slot = EquipmentSlot::new("helmet");
    state.characters[0]
        .equipment
        .insert(slot.clone(), Some(item(21, 5, 101)));
    let before = state.clone();
    assert_eq!(
        state.apply(
            read_only_flags,
            CharacterCommand::UpdateEquipmentItem {
                character_id: id(10),
                slot,
                update: ItemUpdate::SetFlags(Some(1)),
            }
        ),
        Err(AccountError::EquipmentFlagsReadOnly)
    );
    assert_eq!(state, before);
}

#[test]
fn single_plug_updates_materialize_defaults_and_validate_the_index() {
    let mut state = state();
    state
        .apply(
            CAPABILITIES,
            CharacterCommand::UpdateInventoryItem {
                item_id: id(20),
                update: ItemUpdate::SetPlug {
                    index: 1,
                    plug: Some(DefinitionHash::new(22)),
                    default_plugs: vec![Some(DefinitionHash::new(11)), None],
                },
            },
        )
        .unwrap();
    assert_eq!(
        state.characters()[0].inventory[0].plugs,
        ItemPlugs::Authored(vec![
            Some(DefinitionHash::new(11)),
            Some(DefinitionHash::new(22)),
        ])
    );

    let before = state.clone();
    assert_eq!(
        state.apply(
            CAPABILITIES,
            CharacterCommand::UpdateInventoryItem {
                item_id: id(20),
                update: ItemUpdate::SetPlug {
                    index: CAPABILITIES.max_item_plugs,
                    plug: None,
                    default_plugs: Vec::new(),
                },
            },
        ),
        Err(AccountError::TooManyItemPlugs { maximum: 4 })
    );
    assert_eq!(state, before);
}

#[test]
fn allocation_scans_reserved_character_inventory_and_equipment_soids() {
    let mut state = state();
    state.characters[0]
        .equipment
        .insert(EquipmentSlot::new("kinetic"), Some(item(21, 5, 101)));

    assert_eq!(
        state.next_available_instance_soid(soid(1)).unwrap(),
        soid(6)
    );
}

#[test]
fn read_only_commands_leave_state_untouched() {
    let mut state = state();
    let before = state.clone();
    let mut capabilities = CAPABILITIES;
    capabilities.inventory_writable = false;

    assert_eq!(
        state.apply(
            capabilities,
            CharacterCommand::RemoveInventoryItem { item_id: id(20) }
        ),
        Err(AccountError::InventoryReadOnly)
    );
    assert_eq!(state, before);
}
