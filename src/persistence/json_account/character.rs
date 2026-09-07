//! Lossless JSON adapter for the character equipment/inventory aggregate.
//!
//! `loading` selects and decodes only the fields required by a command. This module
//! records changes against stable entity IDs; `projection` writes those changes
//! into the original JSON rows, preserving untouched representations and fields.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

use serde_json::{Map, Value};
use sundial_account::{
    CharacterCapabilities, CharacterCommand, CharacterCommandResult, CharacterMetadataUpdate,
    CharacterState, EntityId, EquipmentSlot, ItemUpdate,
};

use super::JsonAccountError;

mod loading;
mod projection;
#[cfg(test)]
mod tests;

type JsonCharacterResult<T> = Result<T, JsonAccountError>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ItemFieldChanges {
    definition_hash: bool,
    level: bool,
    quantity: bool,
    plugs: bool,
    flags: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MetadataFieldChanges {
    race: bool,
    gender: bool,
    class_type: bool,
    movement: bool,
    grenade: bool,
    super_ability: bool,
    melee: bool,
    class_ability: bool,
}

impl MetadataFieldChanges {
    const APPEARANCE_AND_CLASS: Self = Self {
        race: true,
        gender: true,
        class_type: true,
        movement: false,
        grenade: false,
        super_ability: false,
        melee: false,
        class_ability: false,
    };

    const ABILITIES: Self = Self {
        race: false,
        gender: false,
        class_type: false,
        movement: true,
        grenade: true,
        super_ability: true,
        melee: true,
        class_ability: true,
    };

    const SUPER_AND_MELEE: Self = Self {
        race: false,
        gender: false,
        class_type: false,
        movement: false,
        grenade: false,
        super_ability: true,
        melee: true,
        class_ability: false,
    };

    fn merge(&mut self, other: Self) {
        self.race |= other.race;
        self.gender |= other.gender;
        self.class_type |= other.class_type;
        self.movement |= other.movement;
        self.grenade |= other.grenade;
        self.super_ability |= other.super_ability;
        self.melee |= other.melee;
        self.class_ability |= other.class_ability;
    }
}

impl ItemFieldChanges {
    const ALL: Self = Self {
        definition_hash: true,
        level: true,
        quantity: true,
        plugs: true,
        flags: true,
    };

    fn for_update(update: &ItemUpdate) -> Self {
        let mut changes = Self::default();
        match update {
            ItemUpdate::SetDefinitionHash(_) => changes.definition_hash = true,
            ItemUpdate::SetDefinitionAndPlugs { .. } => {
                changes.definition_hash = true;
                changes.plugs = true;
            }
            ItemUpdate::SetLevel(_) => changes.level = true,
            ItemUpdate::SetQuantity(_) => changes.quantity = true,
            ItemUpdate::SetPlugs(_) | ItemUpdate::SetPlug { .. } => changes.plugs = true,
            ItemUpdate::SetFlags(_) => changes.flags = true,
        }
        changes
    }

    fn merge(&mut self, other: Self) {
        self.definition_hash |= other.definition_hash;
        self.level |= other.level;
        self.quantity |= other.quantity;
        self.plugs |= other.plugs;
        self.flags |= other.flags;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EquipmentOrigin {
    Null,
    Item(EntityId),
}

/// A full plug replacement dominates later per-socket edits in the same projection.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PlugProjection {
    Replace,
    Patch(BTreeSet<usize>),
}

impl PlugProjection {
    fn record(&mut self, index: Option<usize>) {
        match (self, index) {
            (projection, None) => *projection = Self::Replace,
            (Self::Patch(indices), Some(index)) => {
                indices.insert(index);
            }
            (Self::Replace, Some(_)) => {}
        }
    }
}

/// A loaded character aggregate with adapter-owned raw item sidecars.
///
/// Commands mutate storage-neutral state. Projection moves original rows by stable entity ID and
/// rewrites only fields explicitly changed by a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JsonCharacterAdapter {
    source_schema_version: Option<u64>,
    capabilities: CharacterCapabilities,
    state: CharacterState,
    next_entity_id: NonZeroU64,
    character_indices: BTreeMap<EntityId, usize>,
    inventory_origins: BTreeMap<EntityId, Vec<EntityId>>,
    equipment_origins: BTreeMap<(EntityId, EquipmentSlot), EquipmentOrigin>,
    item_rows: BTreeMap<EntityId, Map<String, Value>>,
    metadata_changes: BTreeMap<EntityId, MetadataFieldChanges>,
    /// Source row as it existed when each copy command ran, including prior batch edits.
    item_copies: BTreeMap<EntityId, Map<String, Value>>,
    item_changes: BTreeMap<EntityId, ItemFieldChanges>,
    plug_projections: BTreeMap<EntityId, PlugProjection>,
}

impl JsonCharacterAdapter {
    pub(crate) const fn state(&self) -> &CharacterState {
        &self.state
    }

    pub(crate) fn character_id_at_index(&self, character_index: usize) -> Option<EntityId> {
        self.character_indices
            .iter()
            .find_map(|(id, index)| (*index == character_index).then_some(*id))
    }

    pub(crate) const fn next_entity_id(&self) -> EntityId {
        EntityId::new(self.next_entity_id)
    }

    pub(crate) fn apply(
        &self,
        document: &Value,
        command: CharacterCommand,
    ) -> JsonCharacterResult<(Self, Value, CharacterCommandResult)> {
        let mut candidate = self.clone();
        candidate.record_command_changes(&command)?;
        let result = candidate.state.apply(candidate.capabilities, command)?;
        candidate.advance_next_entity_id()?;
        let projected = candidate.project(document)?;
        Ok((candidate, projected, result))
    }

    fn record_command_changes(&mut self, command: &CharacterCommand) -> JsonCharacterResult<()> {
        let mut state = self.state.clone();
        self.record_command_changes_against(&mut state, command)
    }

    fn record_command_changes_against(
        &mut self,
        state: &mut CharacterState,
        command: &CharacterCommand,
    ) -> JsonCharacterResult<()> {
        match command {
            CharacterCommand::Batch(commands) => {
                for command in commands {
                    self.record_command_changes_against(state, command)?;
                }
                return Ok(());
            }
            CharacterCommand::UpdateMetadata {
                character_id,
                update,
            } => {
                let changes = match update {
                    CharacterMetadataUpdate::SetAppearanceAndClass { .. } => {
                        MetadataFieldChanges::APPEARANCE_AND_CLASS
                    }
                    CharacterMetadataUpdate::SetAbilities(_) => MetadataFieldChanges::ABILITIES,
                    CharacterMetadataUpdate::SetSuperAndMelee { .. } => {
                        MetadataFieldChanges::SUPER_AND_MELEE
                    }
                };
                self.metadata_changes
                    .entry(*character_id)
                    .or_default()
                    .merge(changes);
            }
            CharacterCommand::CopyEquipmentItems {
                source_character_id,
                destination_character_id,
                slots,
            } => {
                if source_character_id == destination_character_id {
                    return Ok(());
                }
                for slot in slots {
                    let source_item =
                        character_equipment_item_id(state, *source_character_id, slot);
                    let destination_item =
                        character_equipment_item_id(state, *destination_character_id, slot);
                    if let (Some(source_item), Some(destination_item)) =
                        (source_item, destination_item)
                    {
                        let source = state
                            .characters()
                            .iter()
                            .flat_map(|character| character.equipment.values().flatten())
                            .find(|item| item.id == source_item)
                            .expect("source equipment was resolved above");
                        let snapshot = self.project_item(source);
                        self.item_copies.insert(destination_item, snapshot);
                        // Copy supersedes earlier edits to the destination. Subsequent
                        // commands record fresh changes against the copied row.
                        self.item_changes.remove(&destination_item);
                        self.plug_projections.remove(&destination_item);
                    }
                }
            }
            _ => {}
        }
        let changed = match command {
            CharacterCommand::AddInventoryItem { item, .. }
            | CharacterCommand::SetEquipmentItem {
                item: Some(item), ..
            } => Some((item.id, ItemFieldChanges::ALL)),
            CharacterCommand::UpdateInventoryItem { item_id, update } => {
                Some((*item_id, ItemFieldChanges::for_update(update)))
            }
            CharacterCommand::UpdateEquipmentItem { update, .. } => {
                let item_id = equipment_item_id(state, command);
                item_id.map(|item_id| (item_id, ItemFieldChanges::for_update(update)))
            }
            CharacterCommand::RemoveInventoryItem { .. }
            | CharacterCommand::MoveInventoryItem { .. }
            | CharacterCommand::SwapInventoryItemWithEquipment { .. }
            | CharacterCommand::MoveEquipmentItemToInventory { .. }
            | CharacterCommand::SetEquipmentItem { item: None, .. } => None,
            CharacterCommand::Batch(_) | CharacterCommand::UpdateMetadata { .. } => None,
            CharacterCommand::CopyEquipmentItems { .. } => None,
        };
        if let Some((item_id, changes)) = changed {
            self.item_changes.entry(item_id).or_default().merge(changes);
            if changes.plugs {
                self.plug_projections
                    .entry(item_id)
                    .or_insert_with(|| PlugProjection::Patch(BTreeSet::new()))
                    .record(plug_index(command));
            }
        }
        state.apply(self.capabilities, command.clone())?;
        Ok(())
    }

    fn advance_next_entity_id(&mut self) -> JsonCharacterResult<()> {
        let maximum = self
            .state
            .characters()
            .iter()
            .flat_map(|character| {
                std::iter::once(character.id)
                    .chain(character.inventory.iter().map(|item| item.id))
                    .chain(character.equipment.values().flatten().map(|item| item.id))
            })
            .map(EntityId::get)
            .max()
            .unwrap_or_default();
        if maximum >= self.next_entity_id.get() {
            self.next_entity_id = maximum
                .checked_add(1)
                .and_then(NonZeroU64::new)
                .ok_or(JsonAccountError::EntityIdentityExhausted)?;
        }
        Ok(())
    }
}

fn plug_index(command: &CharacterCommand) -> Option<usize> {
    match command {
        CharacterCommand::UpdateInventoryItem {
            update: ItemUpdate::SetPlug { index, .. },
            ..
        }
        | CharacterCommand::UpdateEquipmentItem {
            update: ItemUpdate::SetPlug { index, .. },
            ..
        } => Some(*index),
        _ => None,
    }
}

fn equipment_item_id(state: &CharacterState, command: &CharacterCommand) -> Option<EntityId> {
    let CharacterCommand::UpdateEquipmentItem {
        character_id, slot, ..
    } = command
    else {
        return None;
    };
    character_equipment_item_id(state, *character_id, slot)
}

fn character_equipment_item_id(
    state: &CharacterState,
    character_id: EntityId,
    slot: &EquipmentSlot,
) -> Option<EntityId> {
    state
        .characters()
        .iter()
        .find(|character| character.id == character_id)
        .and_then(|character| character.equipment.get(slot))
        .and_then(Option::as_ref)
        .map(|item| item.id)
}
