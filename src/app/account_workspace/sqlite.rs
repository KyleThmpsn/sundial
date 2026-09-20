use std::collections::HashMap;

use crate::persistence::native_account::NativeAccountDocument;

use serde_json::json;
use sundial_account as domain;

use super::super::{ARMOR_SLOTS, WEAPON_SLOTS};
use super::{
    DismantleRewardAction, DismantleRewardLocation, DismantleRewardSnapshot, EquippedItemPlugs,
    EquippedItemSnapshot, EquippedPlugValue, InventoryError, InventoryItemAction,
    InventoryItemLocation, InventoryItemSnapshot, ItemPlugs, NewInventoryItem, ProfileItemAction,
    ProfileItemLocation, ProfileItemSnapshot,
};

pub(super) fn character_metadata<D: NativeAccountDocument>(
    document: &D,
    character_index: usize,
) -> Result<domain::CharacterMetadata, String> {
    character(document, character_index)?
        .metadata
        .ok_or_else(|| format!("Character {} metadata was not loaded", character_index + 1))
}

pub(super) fn class_armor_default_characters<D: NativeAccountDocument>(
    document: &D,
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

pub(super) fn apply_character_updates<D: NativeAccountDocument>(
    document: &mut D,
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
            D::character_capabilities(),
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

pub(super) fn inventory_item_abilities<D: NativeAccountDocument>(
    document: &D,
    location: InventoryItemLocation,
) -> Option<domain::CharacterAbilities> {
    let item = inventory_item(document, location).ok()?;
    document.persisted_item_abilities(item.id)
}

pub(super) fn apply_account_settings<D: NativeAccountDocument>(
    document: &mut D,
    commands: Vec<domain::AccountSettingsCommand>,
) -> Result<bool, String> {
    if commands.is_empty() {
        return Ok(false);
    }
    let before = document.settings().clone();
    document
        .settings_mut()
        .apply_all(D::settings_capabilities(), commands)
        .map_err(|error| error.to_string())?;
    Ok(document.settings() != &before)
}

pub(super) fn profile_items<D: NativeAccountDocument>(document: &D) -> Vec<ProfileItemSnapshot> {
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
        .collect()
}

pub(super) fn dismantle_rewards<D: NativeAccountDocument>(
    document: &D,
) -> Vec<DismantleRewardSnapshot> {
    document
        .profile()
        .dismantle_rewards()
        .iter()
        .enumerate()
        .map(|(index, reward)| DismantleRewardSnapshot {
            location: DismantleRewardLocation { index },
            definition_hash: reward.definition_hash.get(),
            quantity: reward.quantity,
            rarities: reward.rarities.clone(),
            gear_class: reward.gear_class,
            masterworked: reward.masterworked,
        })
        .collect()
}

pub(super) fn character_inventory<D: NativeAccountDocument>(
    document: &D,
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

pub(super) fn add_profile_item<D: NativeAccountDocument>(
    document: &mut D,
    definition_hash: u32,
    quantity: i32,
) -> Result<ProfileItemLocation, InventoryError> {
    let index = document.profile().profile_items().len();
    let item = domain::ProfileItem {
        id: document
            .next_entity_id()
            .map_err(app_inventory_error::<D>)?,
        definition_hash: domain::DefinitionHash::new(definition_hash),
        quantity,
        instance_soid: None,
    };
    document
        .profile_mut()
        .apply_profile_item(
            D::profile_capabilities(),
            domain::ProfileItemCommand::Add(item),
        )
        .map_err(domain_inventory_error::<D>)?;
    Ok(ProfileItemLocation { index })
}

pub(super) fn apply_profile_item_action<D: NativeAccountDocument>(
    document: &mut D,
    location: ProfileItemLocation,
    action: ProfileItemAction,
) -> Result<(), InventoryError> {
    let id = document
        .profile()
        .profile_items()
        .get(location.index)
        .map(|item| item.id)
        .ok_or_else(|| InventoryError::new(D::LABEL, "profile item index is out of range"))?;
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
        .apply_profile_item(D::profile_capabilities(), command)
        .map_err(domain_inventory_error::<D>)
}

pub(super) fn add_dismantle_reward<D: NativeAccountDocument>(
    document: &mut D,
    definition_hash: u32,
) -> Result<DismantleRewardLocation, InventoryError> {
    let index = document.profile().dismantle_rewards().len();
    let id = document
        .next_entity_id()
        .map_err(app_inventory_error::<D>)?;
    document
        .profile_mut()
        .apply_dismantle_reward(
            D::profile_capabilities(),
            domain::DismantleRewardCommand::AddForDefinition {
                id,
                definition_hash: domain::DefinitionHash::new(definition_hash),
            },
        )
        .map_err(domain_inventory_error::<D>)?;
    Ok(DismantleRewardLocation { index })
}

pub(super) fn apply_dismantle_reward_action<D: NativeAccountDocument>(
    document: &mut D,
    location: DismantleRewardLocation,
    action: DismantleRewardAction,
) -> Result<(), InventoryError> {
    let id = document
        .profile()
        .dismantle_rewards()
        .get(location.index)
        .map(|reward| reward.id)
        .ok_or_else(|| InventoryError::new(D::LABEL, "dismantle reward index is out of range"))?;
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
            rarities,
            gear_class,
            masterworked,
        }),
    };
    document
        .profile_mut()
        .apply_dismantle_reward(D::profile_capabilities(), command)
        .map_err(domain_inventory_error::<D>)
}

pub(super) fn add_inventory_item<D: NativeAccountDocument>(
    document: &mut D,
    character_index: usize,
    item: NewInventoryItem,
) -> Result<InventoryItemLocation, InventoryError> {
    let character = character(document, character_index).map_err(app_inventory_error::<D>)?;
    let character_id = character.id;
    let item_index = character.inventory.len();
    let entity_id = document
        .next_entity_id()
        .map_err(app_inventory_error::<D>)?;
    let instance_soid = document
        .next_item_identity()
        .map_err(app_inventory_error::<D>)?;
    document
        .characters_mut()
        .apply(
            D::character_capabilities(),
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
        .map_err(domain_inventory_error::<D>)?;
    document.observe_item_identity(instance_soid);
    Ok(InventoryItemLocation {
        character_index,
        item_index,
    })
}

pub(super) fn apply_inventory_item_action<D: NativeAccountDocument>(
    document: &mut D,
    location: InventoryItemLocation,
    action: InventoryItemAction,
) -> Result<(), InventoryError> {
    let item = inventory_item(document, location)?;
    let item_id = item.id;
    let command = match action {
        InventoryItemAction::Remove => domain::CharacterCommand::RemoveInventoryItem { item_id },
        action => domain::CharacterCommand::UpdateInventoryItem {
            item_id,
            update: domain_item_update(action),
        },
    };
    document
        .characters_mut()
        .apply(D::character_capabilities(), command)
        .map_err(domain_inventory_error::<D>)?;
    Ok(())
}

pub(super) fn remove_character_inventory_items<D: NativeAccountDocument>(
    document: &mut D,
    character_index: usize,
    item_indices: impl IntoIterator<Item = usize>,
) -> Result<usize, InventoryError> {
    let character = character(document, character_index).map_err(app_inventory_error::<D>)?;
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
                    InventoryError::new(D::LABEL, "inventory item index is out of range")
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let removed = commands.len();
    document
        .characters_mut()
        .apply(
            D::character_capabilities(),
            domain::CharacterCommand::Batch(commands),
        )
        .map_err(domain_inventory_error::<D>)?;
    Ok(removed)
}

pub(super) fn swap_inventory_item_with_equipment<D: NativeAccountDocument>(
    document: &mut D,
    location: InventoryItemLocation,
    slot: &str,
) -> Result<bool, InventoryError> {
    let item_id = inventory_item(document, location)?.id;
    let result = document
        .characters_mut()
        .apply(
            D::character_capabilities(),
            domain::CharacterCommand::SwapInventoryItemWithEquipment {
                item_id,
                slot: domain::EquipmentSlot::new(slot),
            },
        )
        .map_err(domain_inventory_error::<D>)?;
    match result {
        domain::CharacterCommandResult::EquipmentSwapped { replaced } => Ok(replaced),
        _ => Err(InventoryError::new(
            D::LABEL,
            "SQLite inventory swap returned an unexpected result",
        )),
    }
}

pub(super) fn move_inventory_item_to_character<D: NativeAccountDocument>(
    document: &mut D,
    location: InventoryItemLocation,
    destination_character_index: usize,
) -> Result<InventoryItemLocation, InventoryError> {
    let item_id = inventory_item(document, location)?.id;
    let destination =
        character(document, destination_character_index).map_err(app_inventory_error::<D>)?;
    let destination_character_id = destination.id;
    let item_index = destination.inventory.len();
    document
        .characters_mut()
        .apply(
            D::character_capabilities(),
            domain::CharacterCommand::MoveInventoryItem {
                item_id,
                destination_character_id,
            },
        )
        .map_err(domain_inventory_error::<D>)?;
    Ok(InventoryItemLocation {
        character_index: destination_character_index,
        item_index,
    })
}

pub(super) fn move_equipment_item_to_inventory<D: NativeAccountDocument>(
    document: &mut D,
    character_index: usize,
    slot: &str,
) -> Result<(), InventoryError> {
    let character_id = character(document, character_index)
        .map_err(app_inventory_error::<D>)?
        .id;
    document
        .characters_mut()
        .apply(
            D::character_capabilities(),
            domain::CharacterCommand::MoveEquipmentItemToInventory {
                character_id,
                slot: domain::EquipmentSlot::new(slot),
            },
        )
        .map_err(domain_inventory_error::<D>)?;
    Ok(())
}

pub(super) fn equipped_item_snapshots<D: NativeAccountDocument>(
    document: &D,
    character_index: usize,
) -> Result<Vec<EquippedItemSnapshot>, String> {
    let character = character(document, character_index)?;
    Ok(crate::account_contract::ALL_EQUIPMENT_SLOTS
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

pub(super) fn equip_definition<D: NativeAccountDocument>(
    document: &mut D,
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
    let mut created_identity = None;
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
        let instance_soid = document.next_item_identity()?;
        created_identity = Some(instance_soid);
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
    apply_character_command(document, command)?;
    if let Some(identity) = created_identity {
        document.observe_item_identity(identity);
    }
    Ok(())
}

pub(super) fn set_equipment_item_level<D: NativeAccountDocument>(
    document: &mut D,
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

pub(super) fn set_equipment_item_flags<D: NativeAccountDocument>(
    document: &mut D,
    character_index: usize,
    slot: &str,
    flags: Option<u8>,
) -> Result<(), String> {
    update_equipment(
        document,
        character_index,
        slot,
        domain::ItemUpdate::SetFlags(flags.map(u32::from)),
    )
}

pub(super) fn set_equipment_item_plug<D: NativeAccountDocument>(
    document: &mut D,
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

pub(super) fn set_weapon_slot_empty<D: NativeAccountDocument>(
    document: &mut D,
    character_index: usize,
    slot: &str,
) -> Result<(), String> {
    if !WEAPON_SLOTS.contains(&slot) {
        return Err(format!(
            "Only weapon slots can be set to empty. {slot} was not changed"
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

pub(super) fn restore_class_armor<D: NativeAccountDocument>(
    document: &mut D,
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

fn character<D: NativeAccountDocument>(
    document: &D,
    index: usize,
) -> Result<&domain::Character, String> {
    document
        .characters()
        .characters()
        .get(index)
        .ok_or_else(|| format!("Character {} does not exist", index + 1))
}

fn character_inventory_ref<D: NativeAccountDocument>(
    document: &D,
    character_index: usize,
) -> Result<&[domain::ItemInstance], InventoryError> {
    character(document, character_index)
        .map(|character| character.inventory.as_slice())
        .map_err(app_inventory_error::<D>)
}

fn inventory_item<D: NativeAccountDocument>(
    document: &D,
    location: InventoryItemLocation,
) -> Result<&domain::ItemInstance, InventoryError> {
    character_inventory_ref(document, location.character_index)?
        .get(location.item_index)
        .ok_or_else(|| InventoryError::new(D::LABEL, "inventory item index is out of range"))
}

fn apply_character_command<D: NativeAccountDocument>(
    document: &mut D,
    command: domain::CharacterCommand,
) -> Result<(), String> {
    document
        .characters_mut()
        .apply(D::character_capabilities(), command)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn update_equipment<D: NativeAccountDocument>(
    document: &mut D,
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

fn inferred_item_level<D: NativeAccountDocument>(document: &D, character_index: usize) -> i32 {
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

fn domain_item_update(action: InventoryItemAction) -> domain::ItemUpdate {
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
        InventoryItemAction::SetFlags(flags) => domain::ItemUpdate::SetFlags(flags.map(u32::from)),
        InventoryItemAction::Remove => unreachable!("remove actions are handled separately"),
    }
}

fn app_inventory_error<D: NativeAccountDocument>(error: String) -> InventoryError {
    InventoryError::new(D::LABEL, error)
}

fn domain_inventory_error<D: NativeAccountDocument>(error: domain::AccountError) -> InventoryError {
    InventoryError::new(D::LABEL, error.to_string())
}
