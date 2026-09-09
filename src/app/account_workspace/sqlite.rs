use std::collections::HashMap;

use serde_json::json;
use sundial_account as domain;

use super::super::{ARMOR_SLOTS, SLOTS, WEAPON_SLOTS};
use super::{
    DismantleGearClass, DismantleRarity, DismantleRewardAction, DismantleRewardLocation,
    DismantleRewardSnapshot, EquippedItemPlugs, EquippedItemSnapshot, EquippedPlugValue,
    InventoryError, InventoryItemAction, InventoryItemLocation, InventoryItemSnapshot, ItemPlugs,
    NewInventoryItem, ProfileItemAction, ProfileItemLocation, ProfileItemSnapshot,
    SqliteAccountDocument,
};

const SQLITE_PATH: &str = "state.sqlite3";

pub(super) fn character_metadata(
    document: &SqliteAccountDocument,
    character_index: usize,
) -> Result<domain::CharacterMetadata, String> {
    character(document, character_index)?
        .metadata
        .ok_or_else(|| format!("Character {} metadata was not loaded", character_index + 1))
}

pub(super) fn class_armor_default_characters(
    document: &SqliteAccountDocument,
) -> HashMap<u64, usize> {
    document
        .characters()
        .characters()
        .iter()
        .enumerate()
        .filter_map(|(index, character)| {
            character
                .metadata
                .map(|metadata| (u64::from(metadata.class_type), index))
        })
        .fold(HashMap::new(), |mut defaults, (class_type, index)| {
            defaults.entry(class_type).or_insert(index);
            defaults
        })
}

pub(super) fn apply_character_updates(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    updates: Vec<domain::CharacterMetadataUpdate>,
) -> Result<bool, String> {
    if updates.is_empty() {
        return Ok(false);
    }
    let abilities_changed = updates.iter().any(|update| {
        matches!(
            update,
            domain::CharacterMetadataUpdate::SetAbilities(_)
                | domain::CharacterMetadataUpdate::SetSuperAndMelee { .. }
        )
    });
    let character_id = character(document, character_index)?.id;
    let commands = updates
        .into_iter()
        .map(|update| domain::CharacterCommand::UpdateMetadata {
            character_id,
            update,
        })
        .collect();
    document
        .characters_mut()
        .apply(
            SqliteAccountDocument::character_capabilities(),
            domain::CharacterCommand::Batch(commands),
        )
        .map_err(|error| error.to_string())?;
    if abilities_changed {
        let persisted_selection = {
            let character = character(document, character_index)?;
            character.metadata.and_then(|metadata| {
                character
                    .equipment
                    .get(&domain::EquipmentSlot::new("subclass"))
                    .and_then(Option::as_ref)
                    .map(|item| (item.id, metadata.abilities))
            })
        };
        if let Some((item_id, abilities)) = persisted_selection {
            document.set_persisted_item_abilities(item_id, abilities);
        }
    }
    Ok(true)
}

pub(super) fn inventory_item_abilities(
    document: &SqliteAccountDocument,
    location: InventoryItemLocation,
) -> Option<domain::CharacterAbilities> {
    let item = inventory_item(document, location).ok()?;
    document.persisted_item_abilities(item.id)
}

pub(super) fn apply_account_settings(
    document: &mut SqliteAccountDocument,
    commands: Vec<domain::AccountSettingsCommand>,
) -> Result<bool, String> {
    if commands.is_empty() {
        return Ok(false);
    }
    let before = document.settings().clone();
    document
        .settings_mut()
        .apply_all(SqliteAccountDocument::settings_capabilities(), commands)
        .map_err(|error| error.to_string())?;
    Ok(document.settings() != &before)
}

pub(super) fn profile_items(document: &SqliteAccountDocument) -> Option<Vec<ProfileItemSnapshot>> {
    Some(
        document
            .profile()
            .profile_items()
            .iter()
            .enumerate()
            .map(|(index, item)| ProfileItemSnapshot {
                location: ProfileItemLocation { index },
                definition_hash: item.definition_hash.get(),
                quantity: item.quantity,
            })
            .collect(),
    )
}

pub(super) fn dismantle_rewards(
    document: &SqliteAccountDocument,
) -> Option<Vec<DismantleRewardSnapshot>> {
    Some(
        document
            .profile()
            .dismantle_rewards()
            .iter()
            .enumerate()
            .map(|(index, reward)| DismantleRewardSnapshot {
                location: DismantleRewardLocation { index },
                definition_hash: reward.definition_hash.get(),
                quantity: reward.quantity,
                rarities: reward.rarities.iter().copied().map(app_rarity).collect(),
                gear_class: reward.gear_class.map(app_gear_class),
                masterworked: reward.masterworked,
            })
            .collect(),
    )
}

pub(super) fn character_inventory(
    document: &SqliteAccountDocument,
    character_index: usize,
) -> Result<Option<Vec<InventoryItemSnapshot>>, InventoryError> {
    Ok(Some(
        character_inventory_ref(document, character_index)?
            .iter()
            .enumerate()
            .map(|(item_index, item)| inventory_snapshot(character_index, item_index, item))
            .collect(),
    ))
}

pub(super) fn add_profile_item(
    document: &mut SqliteAccountDocument,
    definition_hash: u32,
    quantity: i32,
) -> Result<ProfileItemLocation, InventoryError> {
    let index = document.profile().profile_items().len();
    let item = domain::ProfileItem {
        id: document.next_entity_id().map_err(sqlite_inventory_error)?,
        definition_hash: domain::DefinitionHash::new(definition_hash),
        quantity,
    };
    document
        .profile_mut()
        .apply_profile_item(
            SqliteAccountDocument::profile_capabilities(),
            domain::ProfileItemCommand::Add(item),
        )
        .map_err(domain_inventory_error)?;
    Ok(ProfileItemLocation { index })
}

pub(super) fn apply_profile_item_action(
    document: &mut SqliteAccountDocument,
    location: ProfileItemLocation,
    action: ProfileItemAction,
) -> Result<(), InventoryError> {
    let id = document
        .profile()
        .profile_items()
        .get(location.index)
        .map(|item| item.id)
        .ok_or_else(|| InventoryError::new(SQLITE_PATH, "profile item index is out of range"))?;
    let command = match action {
        ProfileItemAction::SetDefinitionHash(hash) => {
            domain::ProfileItemCommand::SetDefinitionHash {
                id,
                definition_hash: domain::DefinitionHash::new(hash),
            }
        }
        ProfileItemAction::SetQuantity(quantity) => {
            domain::ProfileItemCommand::SetQuantity { id, quantity }
        }
        ProfileItemAction::Remove => domain::ProfileItemCommand::Remove { id },
    };
    document
        .profile_mut()
        .apply_profile_item(SqliteAccountDocument::profile_capabilities(), command)
        .map_err(domain_inventory_error)
}

pub(super) fn add_dismantle_reward(
    document: &mut SqliteAccountDocument,
    definition_hash: u32,
) -> Result<DismantleRewardLocation, InventoryError> {
    let index = document.profile().dismantle_rewards().len();
    let id = document.next_entity_id().map_err(sqlite_inventory_error)?;
    document
        .profile_mut()
        .apply_dismantle_reward(
            SqliteAccountDocument::profile_capabilities(),
            domain::DismantleRewardCommand::AddForDefinition {
                id,
                definition_hash: domain::DefinitionHash::new(definition_hash),
            },
        )
        .map_err(domain_inventory_error)?;
    Ok(DismantleRewardLocation { index })
}

pub(super) fn apply_dismantle_reward_action(
    document: &mut SqliteAccountDocument,
    location: DismantleRewardLocation,
    action: DismantleRewardAction,
) -> Result<(), InventoryError> {
    let id = document
        .profile()
        .dismantle_rewards()
        .get(location.index)
        .map(|reward| reward.id)
        .ok_or_else(|| {
            InventoryError::new(SQLITE_PATH, "dismantle reward index is out of range")
        })?;
    let command = match action {
        DismantleRewardAction::Remove => domain::DismantleRewardCommand::Remove { id },
        DismantleRewardAction::SetPolicy {
            definition_hash,
            quantity,
            rarities,
            gear_class,
            masterworked,
        } => domain::DismantleRewardCommand::SetPolicy(domain::DismantleReward {
            id,
            definition_hash: domain::DefinitionHash::new(definition_hash),
            quantity,
            rarities: rarities.into_iter().map(domain_rarity).collect(),
            gear_class: gear_class.map(domain_gear_class),
            masterworked,
        }),
    };
    document
        .profile_mut()
        .apply_dismantle_reward(SqliteAccountDocument::profile_capabilities(), command)
        .map_err(domain_inventory_error)
}

pub(super) fn add_inventory_item(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    item: NewInventoryItem,
) -> Result<InventoryItemLocation, InventoryError> {
    let character = character(document, character_index).map_err(app_inventory_error)?;
    let character_id = character.id;
    let item_index = character.inventory.len();
    let entity_id = document.next_entity_id().map_err(sqlite_inventory_error)?;
    let first_soid =
        domain::InstanceSoid::try_from_u64(super::super::inventory::GENERATED_INSTANCE_SOID_START)
            .expect("the generated SOID start is nonzero");
    let instance_soid = document
        .characters()
        .next_available_instance_soid(first_soid)
        .map_err(domain_inventory_error)?;
    document
        .characters_mut()
        .apply(
            SqliteAccountDocument::character_capabilities(),
            domain::CharacterCommand::AddInventoryItem {
                character_id,
                item: domain::ItemInstance {
                    id: entity_id,
                    instance_soid,
                    definition_hash: domain::DefinitionHash::new(item.definition_hash),
                    level: item.level,
                    quantity: item.quantity,
                    plugs: domain::ItemPlugs::NativeDefaults,
                    flags: None,
                },
            },
        )
        .map_err(domain_inventory_error)?;
    Ok(InventoryItemLocation {
        character_index,
        item_index,
    })
}

pub(super) fn apply_inventory_item_action(
    document: &mut SqliteAccountDocument,
    location: InventoryItemLocation,
    action: InventoryItemAction,
) -> Result<(), InventoryError> {
    let item = inventory_item(document, location)?;
    let item_id = item.id;
    let current_flags = item.flags;
    let command = match action {
        InventoryItemAction::Remove => domain::CharacterCommand::RemoveInventoryItem { item_id },
        action => domain::CharacterCommand::UpdateInventoryItem {
            item_id,
            update: domain_item_update(action, current_flags),
        },
    };
    document
        .characters_mut()
        .apply(SqliteAccountDocument::character_capabilities(), command)
        .map_err(domain_inventory_error)?;
    Ok(())
}

pub(super) fn remove_character_inventory_items(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    item_indices: impl IntoIterator<Item = usize>,
) -> Result<usize, InventoryError> {
    let character = character(document, character_index).map_err(app_inventory_error)?;
    let mut indices = item_indices.into_iter().collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    let commands = indices
        .iter()
        .map(|&index| {
            character
                .inventory
                .get(index)
                .map(|item| domain::CharacterCommand::RemoveInventoryItem { item_id: item.id })
                .ok_or_else(|| {
                    InventoryError::new(SQLITE_PATH, "inventory item index is out of range")
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let removed = commands.len();
    document
        .characters_mut()
        .apply(
            SqliteAccountDocument::character_capabilities(),
            domain::CharacterCommand::Batch(commands),
        )
        .map_err(domain_inventory_error)?;
    Ok(removed)
}

pub(super) fn swap_inventory_item_with_equipment(
    document: &mut SqliteAccountDocument,
    location: InventoryItemLocation,
    slot: &str,
) -> Result<bool, InventoryError> {
    let item_id = inventory_item(document, location)?.id;
    let result = document
        .characters_mut()
        .apply(
            SqliteAccountDocument::character_capabilities(),
            domain::CharacterCommand::SwapInventoryItemWithEquipment {
                item_id,
                slot: domain::EquipmentSlot::new(slot),
            },
        )
        .map_err(domain_inventory_error)?;
    match result {
        domain::CharacterCommandResult::EquipmentSwapped { replaced } => Ok(replaced),
        _ => Err(InventoryError::new(
            SQLITE_PATH,
            "SQLite inventory swap returned an unexpected result",
        )),
    }
}

pub(super) fn move_inventory_item_to_character(
    document: &mut SqliteAccountDocument,
    location: InventoryItemLocation,
    destination_character_index: usize,
) -> Result<InventoryItemLocation, InventoryError> {
    let item_id = inventory_item(document, location)?.id;
    let destination =
        character(document, destination_character_index).map_err(app_inventory_error)?;
    let destination_character_id = destination.id;
    let item_index = destination.inventory.len();
    document
        .characters_mut()
        .apply(
            SqliteAccountDocument::character_capabilities(),
            domain::CharacterCommand::MoveInventoryItem {
                item_id,
                destination_character_id,
            },
        )
        .map_err(domain_inventory_error)?;
    Ok(InventoryItemLocation {
        character_index: destination_character_index,
        item_index,
    })
}

pub(super) fn move_equipment_item_to_inventory(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    slot: &str,
) -> Result<(), InventoryError> {
    let character_id = character(document, character_index)
        .map_err(app_inventory_error)?
        .id;
    document
        .characters_mut()
        .apply(
            SqliteAccountDocument::character_capabilities(),
            domain::CharacterCommand::MoveEquipmentItemToInventory {
                character_id,
                slot: domain::EquipmentSlot::new(slot),
            },
        )
        .map_err(domain_inventory_error)?;
    Ok(())
}

pub(super) fn equipped_item_snapshots(
    document: &SqliteAccountDocument,
    character_index: usize,
) -> Result<Vec<EquippedItemSnapshot>, String> {
    let character = character(document, character_index)?;
    Ok(SLOTS
        .iter()
        .filter_map(|&(slot, slot_label, bucket_hash)| {
            character
                .equipment
                .get(&domain::EquipmentSlot::new(slot))
                .and_then(Option::as_ref)
                .map(|item| equipment_snapshot(slot, slot_label, bucket_hash, item))
        })
        .collect())
}

pub(super) fn equip_definition(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    slot: &str,
    definition_hash: u64,
    default_plugs: &[Option<String>],
) -> Result<(), String> {
    let definition_hash = u32::try_from(definition_hash)
        .map(domain::DefinitionHash::new)
        .map_err(|_| "Equipment definition hash must fit in 32 bits".to_owned())?;
    let plugs = parse_default_plugs(default_plugs)?;
    let character = character(document, character_index)?;
    let character_id = character.id;
    let slot_key = domain::EquipmentSlot::new(slot);
    let command = if character
        .equipment
        .get(&slot_key)
        .and_then(Option::as_ref)
        .is_some()
    {
        domain::CharacterCommand::UpdateEquipmentItem {
            character_id,
            slot: slot_key,
            update: domain::ItemUpdate::SetDefinitionAndPlugs {
                definition_hash,
                plugs,
            },
        }
    } else {
        let entity_id = document
            .next_entity_id()
            .map_err(|error| error.to_string())?;
        let first_soid = domain::InstanceSoid::try_from_u64(
            super::super::inventory::GENERATED_INSTANCE_SOID_START,
        )
        .expect("the generated SOID start is nonzero");
        let instance_soid = document
            .characters()
            .next_available_instance_soid(first_soid)
            .map_err(|error| error.to_string())?;
        domain::CharacterCommand::SetEquipmentItem {
            character_id,
            slot: slot_key,
            item: Some(domain::ItemInstance {
                id: entity_id,
                instance_soid,
                definition_hash,
                level: inferred_item_level(document, character_index),
                quantity: 1,
                plugs,
                flags: None,
            }),
        }
    };
    apply_character_command(document, command)
}

pub(super) fn set_equipment_item_level(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    slot: &str,
    level: i64,
) -> Result<(), String> {
    let level = i32::try_from(level)
        .ok()
        .filter(|level| *level >= 0)
        .ok_or_else(|| "Equipment level must be a non-negative signed 32-bit integer".to_owned())?;
    update_equipment(
        document,
        character_index,
        slot,
        domain::ItemUpdate::SetLevel(level),
    )
}

pub(super) fn set_equipment_item_flags(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    slot: &str,
    flags: Option<u8>,
) -> Result<(), String> {
    let current_flags = character(document, character_index)?
        .equipment
        .get(&domain::EquipmentSlot::new(slot))
        .and_then(Option::as_ref)
        .and_then(|item| item.flags);
    update_equipment(
        document,
        character_index,
        slot,
        domain::ItemUpdate::SetFlags(merge_editor_flags(current_flags, flags)),
    )
}

pub(super) fn set_equipment_item_plug(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    slot: &str,
    socket_index: usize,
    default_plugs: &[Option<String>],
    hash: Option<u64>,
) -> Result<(), String> {
    let plug = hash
        .map(|hash| {
            u32::try_from(hash)
                .map(domain::DefinitionHash::new)
                .map_err(|_| "Equipment plug hash must fit in 32 bits".to_owned())
        })
        .transpose()?;
    let defaults = match parse_default_plugs(default_plugs)? {
        domain::ItemPlugs::Authored(values) => values,
        domain::ItemPlugs::NativeDefaults => unreachable!(),
    };
    update_equipment(
        document,
        character_index,
        slot,
        domain::ItemUpdate::SetPlug {
            index: socket_index,
            plug,
            default_plugs: defaults,
        },
    )
}

pub(super) fn set_weapon_slot_empty(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    slot: &str,
) -> Result<(), String> {
    if !WEAPON_SLOTS.contains(&slot) {
        return Err(format!(
            "Only weapon slots can be set to empty; {slot} was not changed"
        ));
    }
    let character_id = character(document, character_index)?.id;
    apply_character_command(
        document,
        domain::CharacterCommand::SetEquipmentItem {
            character_id,
            slot: domain::EquipmentSlot::new(slot),
            item: None,
        },
    )
}

pub(super) fn restore_class_armor(
    document: &mut SqliteAccountDocument,
    source_character_index: usize,
    destination_character_index: usize,
) -> Result<bool, String> {
    let source_character_id = character(document, source_character_index)?.id;
    let destination_character_id = character(document, destination_character_index)?.id;
    let before = document.characters().clone();
    apply_character_command(
        document,
        domain::CharacterCommand::CopyEquipmentItems {
            source_character_id,
            destination_character_id,
            slots: ARMOR_SLOTS
                .iter()
                .map(|slot| domain::EquipmentSlot::new(*slot))
                .collect(),
        },
    )?;
    Ok(document.characters() != &before)
}

fn character(document: &SqliteAccountDocument, index: usize) -> Result<&domain::Character, String> {
    document
        .characters()
        .characters()
        .get(index)
        .ok_or_else(|| format!("Character {} does not exist", index + 1))
}

fn character_inventory_ref(
    document: &SqliteAccountDocument,
    character_index: usize,
) -> Result<&[domain::ItemInstance], InventoryError> {
    character(document, character_index)
        .map(|character| character.inventory.as_slice())
        .map_err(app_inventory_error)
}

fn inventory_item(
    document: &SqliteAccountDocument,
    location: InventoryItemLocation,
) -> Result<&domain::ItemInstance, InventoryError> {
    character_inventory_ref(document, location.character_index)?
        .get(location.item_index)
        .ok_or_else(|| InventoryError::new(SQLITE_PATH, "inventory item index is out of range"))
}

fn apply_character_command(
    document: &mut SqliteAccountDocument,
    command: domain::CharacterCommand,
) -> Result<(), String> {
    document
        .characters_mut()
        .apply(SqliteAccountDocument::character_capabilities(), command)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn update_equipment(
    document: &mut SqliteAccountDocument,
    character_index: usize,
    slot: &str,
    update: domain::ItemUpdate,
) -> Result<(), String> {
    let character = character(document, character_index)?;
    let character_id = character.id;
    if character
        .equipment
        .get(&domain::EquipmentSlot::new(slot))
        .and_then(Option::as_ref)
        .is_none()
    {
        return Err(format!("The {slot} slot is empty"));
    }
    apply_character_command(
        document,
        domain::CharacterCommand::UpdateEquipmentItem {
            character_id,
            slot: domain::EquipmentSlot::new(slot),
            update,
        },
    )
}

fn inferred_item_level(document: &SqliteAccountDocument, character_index: usize) -> i32 {
    character(document, character_index)
        .ok()
        .into_iter()
        .flat_map(|character| character.equipment.values().flatten())
        .map(|item| item.level)
        .filter(|level| *level > 0)
        .max()
        .unwrap_or(106)
}

fn parse_default_plugs(values: &[Option<String>]) -> Result<domain::ItemPlugs, String> {
    values
        .iter()
        .map(|value| {
            value
                .as_deref()
                .map(|value| {
                    crate::hash::parse_hash_hex(value)
                        .and_then(|hash| u32::try_from(hash).ok())
                        .map(domain::DefinitionHash::new)
                        .ok_or_else(|| format!("Invalid equipment default plug hash: {value}"))
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()
        .map(domain::ItemPlugs::Authored)
}

fn inventory_snapshot(
    character_index: usize,
    item_index: usize,
    item: &domain::ItemInstance,
) -> InventoryItemSnapshot {
    InventoryItemSnapshot {
        location: InventoryItemLocation {
            character_index,
            item_index,
        },
        instance_soid: item.instance_soid.get(),
        definition_hash: item.definition_hash.get(),
        level: item.level,
        quantity: item.quantity,
        plugs: app_item_plugs(&item.plugs),
        flags: editor_flags(item.flags),
    }
}

fn equipment_snapshot(
    slot: &'static str,
    slot_label: &'static str,
    bucket_hash: u64,
    item: &domain::ItemInstance,
) -> EquippedItemSnapshot {
    let plugs = match &item.plugs {
        domain::ItemPlugs::NativeDefaults => EquippedItemPlugs::NativeDefaults,
        domain::ItemPlugs::Authored(values) => EquippedItemPlugs::Authored(
            values
                .iter()
                .map(|value| {
                    value.map_or(EquippedPlugValue::Empty, |hash| {
                        EquippedPlugValue::Hash(u64::from(hash.get()))
                    })
                })
                .collect(),
        ),
    };
    let flags = editor_flags(item.flags);
    let raw_item_text = json!({
        "instance_soid": format!("0x{:016X}", item.instance_soid.get()),
        "definition_hash": format!("0x{:08X}", item.definition_hash.get()),
        "level": item.level,
        "quantity": item.quantity,
        "flags": item.flags,
    })
    .to_string();
    EquippedItemSnapshot {
        slot,
        slot_label,
        bucket_hash,
        raw_item_text,
        definition_hash: Some(u64::from(item.definition_hash.get())),
        definition_text: crate::hash::format_hash_hex(u64::from(item.definition_hash.get())),
        instance_soid: Some(item.instance_soid.get()),
        instance_soid_text: format!("0x{:016X}", item.instance_soid.get()),
        level: Some(i64::from(item.level)),
        quantity: Some(i64::from(item.quantity)),
        flags,
        plugs,
        issues: Vec::new(),
    }
}

fn app_item_plugs(plugs: &domain::ItemPlugs) -> ItemPlugs {
    match plugs {
        domain::ItemPlugs::NativeDefaults => ItemPlugs::NativeDefaults,
        domain::ItemPlugs::Authored(values) => ItemPlugs::Authored(
            values
                .iter()
                .map(|value| value.map(domain::DefinitionHash::get))
                .collect(),
        ),
    }
}

fn editor_flags(flags: Option<u32>) -> Option<u8> {
    flags.map(|flags| (flags & u32::from(u8::MAX)) as u8)
}

fn merge_editor_flags(current: Option<u32>, edited: Option<u8>) -> Option<u32> {
    let opaque = current.unwrap_or_default() & !u32::from(u8::MAX);
    match edited {
        Some(flags) => Some(opaque | u32::from(flags)),
        None => (opaque != 0).then_some(opaque),
    }
}

fn domain_item_update(
    action: InventoryItemAction,
    current_flags: Option<u32>,
) -> domain::ItemUpdate {
    match action {
        InventoryItemAction::SetDefinitionHash(hash) => {
            domain::ItemUpdate::SetDefinitionHash(domain::DefinitionHash::new(hash))
        }
        InventoryItemAction::SetLevel(level) => domain::ItemUpdate::SetLevel(level),
        InventoryItemAction::SetQuantity(quantity) => domain::ItemUpdate::SetQuantity(quantity),
        InventoryItemAction::SetPlugs(plugs) => domain::ItemUpdate::SetPlugs(match plugs {
            ItemPlugs::NativeDefaults => domain::ItemPlugs::NativeDefaults,
            ItemPlugs::Authored(values) => domain::ItemPlugs::Authored(
                values
                    .into_iter()
                    .map(|value| value.map(domain::DefinitionHash::new))
                    .collect(),
            ),
        }),
        InventoryItemAction::SetFlags(flags) => {
            domain::ItemUpdate::SetFlags(merge_editor_flags(current_flags, flags))
        }
        InventoryItemAction::Remove => unreachable!("remove actions are handled separately"),
    }
}

const fn app_rarity(value: domain::DismantleRarity) -> DismantleRarity {
    match value {
        domain::DismantleRarity::Common => DismantleRarity::Common,
        domain::DismantleRarity::Uncommon => DismantleRarity::Uncommon,
        domain::DismantleRarity::Rare => DismantleRarity::Rare,
        domain::DismantleRarity::Legendary => DismantleRarity::Legendary,
        domain::DismantleRarity::Exotic => DismantleRarity::Exotic,
    }
}

const fn domain_rarity(value: DismantleRarity) -> domain::DismantleRarity {
    match value {
        DismantleRarity::Common => domain::DismantleRarity::Common,
        DismantleRarity::Uncommon => domain::DismantleRarity::Uncommon,
        DismantleRarity::Rare => domain::DismantleRarity::Rare,
        DismantleRarity::Legendary => domain::DismantleRarity::Legendary,
        DismantleRarity::Exotic => domain::DismantleRarity::Exotic,
    }
}

const fn app_gear_class(value: domain::DismantleGearClass) -> DismantleGearClass {
    match value {
        domain::DismantleGearClass::Weapon => DismantleGearClass::Weapon,
        domain::DismantleGearClass::Armor => DismantleGearClass::Armor,
        domain::DismantleGearClass::Both => DismantleGearClass::Both,
    }
}

const fn domain_gear_class(value: DismantleGearClass) -> domain::DismantleGearClass {
    match value {
        DismantleGearClass::Weapon => domain::DismantleGearClass::Weapon,
        DismantleGearClass::Armor => domain::DismantleGearClass::Armor,
        DismantleGearClass::Both => domain::DismantleGearClass::Both,
    }
}

fn app_inventory_error(error: String) -> InventoryError {
    InventoryError::new(SQLITE_PATH, error)
}

fn domain_inventory_error(error: domain::AccountError) -> InventoryError {
    InventoryError::new(SQLITE_PATH, error.to_string())
}

fn sqlite_inventory_error(
    error: crate::persistence::sqlite_account::SqliteAccountError,
) -> InventoryError {
    InventoryError::new(SQLITE_PATH, error.to_string())
}
