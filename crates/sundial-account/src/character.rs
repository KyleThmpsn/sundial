//! Character equipment and inventory as one storage-neutral aggregate.

use std::collections::{BTreeMap, BTreeSet};

use crate::validation::{
    is_no_definition_hash, validate_authored_definition_hash, validate_positive_quantity,
};
use crate::{AccountError, AccountResult, DefinitionHash, EntityId, EntityKind, InstanceSoid};

/// Adapter-derived rules for character and item mutations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterCapabilities {
    pub metadata_writable: bool,
    pub inventory_writable: bool,
    pub equipment_writable: bool,
    pub equipment_flags_writable: bool,
    pub inventory_capacity: Option<usize>,
    pub enforce_loaded_inventory_capacity: bool,
    pub max_item_plugs: usize,
    pub item_flag_mask: u32,
    pub enforce_unique_instance_soids: bool,
}

/// The five coordinated ability selections stored on a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterAbilities {
    pub movement: u8,
    pub grenade: u8,
    pub super_ability: u8,
    pub melee: u8,
    pub class_ability: u8,
}

/// Storage-neutral character fields edited alongside subclass equipment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterMetadata {
    pub race: u8,
    pub gender: u8,
    pub class_type: u8,
    pub abilities: CharacterAbilities,
}

/// A focused character-field mutation. Adapters may load only this facet of a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharacterMetadataUpdate {
    SetAppearanceAndClass {
        race: u8,
        gender: u8,
        class_type: u8,
    },
    SetAbilities(CharacterAbilities),
    SetSuperAndMelee {
        super_ability: u8,
        melee: u8,
    },
}

/// A persistence-neutral equipment slot identifier.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EquipmentSlot(Box<str>);

impl EquipmentSlot {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().into_boxed_str())
    }

    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

/// An item's authored plug state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemPlugs {
    NativeDefaults,
    Authored(Vec<Option<DefinitionHash>>),
}

/// One exact item instance, independent of whether it is stored or equipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemInstance {
    pub id: EntityId,
    pub instance_soid: InstanceSoid,
    pub definition_hash: DefinitionHash,
    pub level: i32,
    pub quantity: i32,
    pub plugs: ItemPlugs,
    pub flags: Option<u32>,
}

/// Character-owned inventory and equipment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Character {
    pub id: EntityId,
    pub soid: Option<InstanceSoid>,
    /// `None` means the adapter deliberately did not load character metadata for this operation.
    pub metadata: Option<CharacterMetadata>,
    pub inventory: Vec<ItemInstance>,
    pub equipment: BTreeMap<EquipmentSlot, Option<ItemInstance>>,
}

/// A field-level item mutation shared by stored and equipped instances.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemUpdate {
    SetDefinitionHash(DefinitionHash),
    SetDefinitionAndPlugs {
        definition_hash: DefinitionHash,
        plugs: ItemPlugs,
    },
    SetLevel(i32),
    SetQuantity(i32),
    SetPlugs(ItemPlugs),
    SetPlug {
        index: usize,
        plug: Option<DefinitionHash>,
        default_plugs: Vec<Option<DefinitionHash>>,
    },
    SetFlags(Option<u32>),
}

/// An atomic character-aggregate mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CharacterCommand {
    /// Applies several character commands as one validated mutation.
    Batch(Vec<CharacterCommand>),
    UpdateMetadata {
        character_id: EntityId,
        update: CharacterMetadataUpdate,
    },
    /// Copies mutable equipment state while retaining each destination instance identity.
    CopyEquipmentItems {
        source_character_id: EntityId,
        destination_character_id: EntityId,
        slots: Vec<EquipmentSlot>,
    },
    AddInventoryItem {
        character_id: EntityId,
        item: ItemInstance,
    },
    UpdateInventoryItem {
        item_id: EntityId,
        update: ItemUpdate,
    },
    RemoveInventoryItem {
        item_id: EntityId,
    },
    MoveInventoryItem {
        item_id: EntityId,
        destination_character_id: EntityId,
    },
    SwapInventoryItemWithEquipment {
        item_id: EntityId,
        slot: EquipmentSlot,
    },
    MoveEquipmentItemToInventory {
        character_id: EntityId,
        slot: EquipmentSlot,
    },
    UpdateEquipmentItem {
        character_id: EntityId,
        slot: EquipmentSlot,
        update: ItemUpdate,
    },
    SetEquipmentItem {
        character_id: EntityId,
        slot: EquipmentSlot,
        item: Option<ItemInstance>,
    },
}

/// Information needed by the application after a successful command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CharacterCommandResult {
    None,
    Batch(Vec<CharacterCommandResult>),
    InventoryItemAdded {
        item_id: EntityId,
    },
    InventoryItemMoved {
        item_id: EntityId,
        destination_character_id: EntityId,
    },
    EquipmentSwapped {
        replaced: bool,
    },
    EquipmentItemMovedToInventory {
        item_id: EntityId,
    },
}

/// All character state required to enforce cross-character item and SOID invariants.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CharacterState {
    reserved_soids: Vec<InstanceSoid>,
    characters: Vec<Character>,
}

impl CharacterState {
    pub fn try_new(
        capabilities: CharacterCapabilities,
        reserved_soids: Vec<InstanceSoid>,
        characters: Vec<Character>,
    ) -> AccountResult<Self> {
        let state = Self {
            reserved_soids,
            characters,
        };
        state.validate(capabilities)?;
        if capabilities.enforce_loaded_inventory_capacity
            && let Some(capacity) = capabilities.inventory_capacity
            && state
                .characters
                .iter()
                .any(|character| character.inventory.len() > capacity)
        {
            return Err(AccountError::CapacityExceeded {
                entity: EntityKind::ItemInstance,
                capacity,
            });
        }
        Ok(state)
    }

    #[must_use]
    pub fn reserved_soids(&self) -> &[InstanceSoid] {
        &self.reserved_soids
    }

    #[must_use]
    pub fn characters(&self) -> &[Character] {
        &self.characters
    }

    pub fn apply(
        &mut self,
        capabilities: CharacterCapabilities,
        command: CharacterCommand,
    ) -> AccountResult<CharacterCommandResult> {
        let mut candidate = self.clone();
        let result = candidate.apply_inner(capabilities, command)?;
        candidate.validate(capabilities)?;
        *self = candidate;
        Ok(result)
    }

    pub fn next_available_instance_soid(
        &self,
        first_candidate: InstanceSoid,
    ) -> AccountResult<InstanceSoid> {
        let used = self.used_instance_soids();
        let mut candidate = first_candidate.get();
        loop {
            if !used.contains(&candidate) {
                return InstanceSoid::try_from_u64(candidate)
                    .ok_or(AccountError::NoAvailableInstanceSoid);
            }
            candidate = candidate
                .checked_add(1)
                .ok_or(AccountError::NoAvailableInstanceSoid)?;
        }
    }

    fn apply_inner(
        &mut self,
        capabilities: CharacterCapabilities,
        command: CharacterCommand,
    ) -> AccountResult<CharacterCommandResult> {
        match command {
            CharacterCommand::Batch(commands) => {
                let mut results = Vec::with_capacity(commands.len());
                for command in commands {
                    results.push(self.apply_inner(capabilities, command)?);
                    self.validate(capabilities)?;
                }
                Ok(CharacterCommandResult::Batch(results))
            }
            CharacterCommand::UpdateMetadata {
                character_id,
                update,
            } => {
                require_metadata_writable(capabilities)?;
                let metadata = self
                    .character_mut(character_id)?
                    .metadata
                    .as_mut()
                    .ok_or(AccountError::CharacterMetadataNotLoaded)?;
                match update {
                    CharacterMetadataUpdate::SetAppearanceAndClass {
                        race,
                        gender,
                        class_type,
                    } => {
                        metadata.race = race;
                        metadata.gender = gender;
                        metadata.class_type = class_type;
                    }
                    CharacterMetadataUpdate::SetAbilities(abilities) => {
                        metadata.abilities = abilities;
                    }
                    CharacterMetadataUpdate::SetSuperAndMelee {
                        super_ability,
                        melee,
                    } => {
                        metadata.abilities.super_ability = super_ability;
                        metadata.abilities.melee = melee;
                    }
                }
                Ok(CharacterCommandResult::None)
            }
            CharacterCommand::CopyEquipmentItems {
                source_character_id,
                destination_character_id,
                slots,
            } => {
                require_equipment_writable(capabilities)?;
                let source_index = self.character_index(source_character_id)?;
                let destination_index = self.character_index(destination_character_id)?;
                if source_index == destination_index {
                    return Ok(CharacterCommandResult::None);
                }
                for slot in slots {
                    let source = self.characters[source_index]
                        .equipment
                        .get(&slot)
                        .and_then(Option::as_ref)
                        .cloned();
                    let destination = self.characters[destination_index]
                        .equipment
                        .get_mut(&slot)
                        .and_then(Option::as_mut);
                    if let (Some(source), Some(destination)) = (source, destination) {
                        destination.definition_hash = source.definition_hash;
                        destination.level = source.level;
                        destination.quantity = source.quantity;
                        destination.plugs = source.plugs;
                        destination.flags = source.flags;
                    }
                }
                Ok(CharacterCommandResult::None)
            }
            CharacterCommand::AddInventoryItem { character_id, item } => {
                require_inventory_writable(capabilities)?;
                let character = self.character_mut(character_id)?;
                ensure_inventory_capacity(character, capabilities)?;
                let item_id = item.id;
                character.inventory.push(item);
                Ok(CharacterCommandResult::InventoryItemAdded { item_id })
            }
            CharacterCommand::UpdateInventoryItem { item_id, update } => {
                require_inventory_writable(capabilities)?;
                let (character_index, item_index) = self.inventory_item_location(item_id)?;
                apply_item_update(
                    &mut self.characters[character_index].inventory[item_index],
                    update,
                    capabilities,
                    false,
                )?;
                Ok(CharacterCommandResult::None)
            }
            CharacterCommand::RemoveInventoryItem { item_id } => {
                require_inventory_writable(capabilities)?;
                let (character_index, item_index) = self.inventory_item_location(item_id)?;
                self.characters[character_index]
                    .inventory
                    .remove(item_index);
                Ok(CharacterCommandResult::None)
            }
            CharacterCommand::MoveInventoryItem {
                item_id,
                destination_character_id,
            } => {
                require_inventory_writable(capabilities)?;
                let (source_index, item_index) = self.inventory_item_location(item_id)?;
                let destination_index = self.character_index(destination_character_id)?;
                if source_index == destination_index {
                    return Err(AccountError::SameCharacterMove);
                }
                ensure_inventory_capacity(&self.characters[destination_index], capabilities)?;
                let item = self.characters[source_index].inventory.remove(item_index);
                self.characters[destination_index].inventory.push(item);
                Ok(CharacterCommandResult::InventoryItemMoved {
                    item_id,
                    destination_character_id,
                })
            }
            CharacterCommand::SwapInventoryItemWithEquipment { item_id, slot } => {
                require_inventory_writable(capabilities)?;
                require_equipment_writable(capabilities)?;
                let (character_index, item_index) = self.inventory_item_location(item_id)?;
                let character = &mut self.characters[character_index];
                let stored = character.inventory.remove(item_index);
                let previous = character.equipment.insert(slot, Some(stored)).flatten();
                let replaced = previous.is_some();
                if let Some(previous) = previous {
                    character.inventory.insert(item_index, previous);
                }
                Ok(CharacterCommandResult::EquipmentSwapped { replaced })
            }
            CharacterCommand::MoveEquipmentItemToInventory { character_id, slot } => {
                require_inventory_writable(capabilities)?;
                require_equipment_writable(capabilities)?;
                let character = self.character_mut(character_id)?;
                ensure_inventory_capacity(character, capabilities)?;
                let item = character
                    .equipment
                    .get_mut(&slot)
                    .and_then(Option::take)
                    .ok_or(AccountError::EquipmentSlotEmpty)?;
                let item_id = item.id;
                character.inventory.push(item);
                Ok(CharacterCommandResult::EquipmentItemMovedToInventory { item_id })
            }
            CharacterCommand::UpdateEquipmentItem {
                character_id,
                slot,
                update,
            } => {
                require_equipment_writable(capabilities)?;
                let item = self
                    .character_mut(character_id)?
                    .equipment
                    .get_mut(&slot)
                    .and_then(Option::as_mut)
                    .ok_or(AccountError::EquipmentSlotEmpty)?;
                apply_item_update(item, update, capabilities, true)?;
                Ok(CharacterCommandResult::None)
            }
            CharacterCommand::SetEquipmentItem {
                character_id,
                slot,
                item,
            } => {
                require_equipment_writable(capabilities)?;
                self.character_mut(character_id)?
                    .equipment
                    .insert(slot, item);
                Ok(CharacterCommandResult::None)
            }
        }
    }

    fn validate(&self, capabilities: CharacterCapabilities) -> AccountResult<()> {
        let mut entity_ids = BTreeSet::new();
        let mut instance_soids = BTreeSet::new();
        for soid in &self.reserved_soids {
            ensure_unique_soid(
                &mut instance_soids,
                *soid,
                capabilities.enforce_unique_instance_soids,
            )?;
        }
        for character in &self.characters {
            if !entity_ids.insert(character.id) {
                return Err(AccountError::DuplicateEntityId(EntityKind::Character));
            }
            if let Some(soid) = character.soid {
                ensure_unique_soid(
                    &mut instance_soids,
                    soid,
                    capabilities.enforce_unique_instance_soids,
                )?;
            }
            if let Some(metadata) = character.metadata {
                validate_metadata(metadata)?;
            }
            for item in &character.inventory {
                validate_item(item, capabilities, &mut entity_ids, &mut instance_soids)?;
            }
            for item in character.equipment.values().flatten() {
                validate_item(item, capabilities, &mut entity_ids, &mut instance_soids)?;
            }
        }
        Ok(())
    }

    fn character_index(&self, id: EntityId) -> AccountResult<usize> {
        self.characters
            .iter()
            .position(|character| character.id == id)
            .ok_or(AccountError::EntityNotFound(EntityKind::Character))
    }

    fn character_mut(&mut self, id: EntityId) -> AccountResult<&mut Character> {
        let index = self.character_index(id)?;
        Ok(&mut self.characters[index])
    }

    fn inventory_item_location(&self, id: EntityId) -> AccountResult<(usize, usize)> {
        self.characters
            .iter()
            .enumerate()
            .find_map(|(character_index, character)| {
                character
                    .inventory
                    .iter()
                    .position(|item| item.id == id)
                    .map(|item_index| (character_index, item_index))
            })
            .ok_or(AccountError::EntityNotFound(EntityKind::ItemInstance))
    }

    fn used_instance_soids(&self) -> BTreeSet<u64> {
        self.reserved_soids
            .iter()
            .map(|soid| soid.get())
            .chain(self.characters.iter().flat_map(|character| {
                character
                    .soid
                    .iter()
                    .map(|soid| soid.get())
                    .chain(
                        character
                            .inventory
                            .iter()
                            .map(|item| item.instance_soid.get()),
                    )
                    .chain(
                        character
                            .equipment
                            .values()
                            .flatten()
                            .map(|item| item.instance_soid.get()),
                    )
            }))
            .collect()
    }
}

fn require_metadata_writable(capabilities: CharacterCapabilities) -> AccountResult<()> {
    if capabilities.metadata_writable {
        Ok(())
    } else {
        Err(AccountError::CharacterMetadataReadOnly)
    }
}

fn require_inventory_writable(capabilities: CharacterCapabilities) -> AccountResult<()> {
    if capabilities.inventory_writable {
        Ok(())
    } else {
        Err(AccountError::InventoryReadOnly)
    }
}

fn require_equipment_writable(capabilities: CharacterCapabilities) -> AccountResult<()> {
    if capabilities.equipment_writable {
        Ok(())
    } else {
        Err(AccountError::EquipmentReadOnly)
    }
}

fn ensure_inventory_capacity(
    character: &Character,
    capabilities: CharacterCapabilities,
) -> AccountResult<()> {
    if let Some(capacity) = capabilities.inventory_capacity
        && character.inventory.len() >= capacity
    {
        return Err(AccountError::CapacityExceeded {
            entity: EntityKind::ItemInstance,
            capacity,
        });
    }
    Ok(())
}

fn apply_item_update(
    item: &mut ItemInstance,
    update: ItemUpdate,
    capabilities: CharacterCapabilities,
    equipped: bool,
) -> AccountResult<()> {
    match update {
        ItemUpdate::SetDefinitionHash(definition_hash) => {
            validate_authored_definition_hash(definition_hash)?;
            item.definition_hash = definition_hash;
        }
        ItemUpdate::SetDefinitionAndPlugs {
            definition_hash,
            plugs,
        } => {
            validate_authored_definition_hash(definition_hash)?;
            validate_plugs(&plugs, capabilities.max_item_plugs)?;
            item.definition_hash = definition_hash;
            item.plugs = plugs;
        }
        ItemUpdate::SetLevel(level) => {
            validate_level(level)?;
            item.level = level;
        }
        ItemUpdate::SetQuantity(quantity) => {
            validate_positive_quantity(quantity)?;
            item.quantity = quantity;
        }
        ItemUpdate::SetPlugs(plugs) => {
            validate_plugs(&plugs, capabilities.max_item_plugs)?;
            item.plugs = plugs;
        }
        ItemUpdate::SetPlug {
            index,
            plug,
            default_plugs,
        } => {
            if index >= capabilities.max_item_plugs {
                return Err(AccountError::TooManyItemPlugs {
                    maximum: capabilities.max_item_plugs,
                });
            }
            if let Some(plug) = plug {
                validate_authored_definition_hash(plug)?;
            }
            let mut plugs = match &item.plugs {
                ItemPlugs::NativeDefaults => {
                    validate_plugs(
                        &ItemPlugs::Authored(default_plugs.clone()),
                        capabilities.max_item_plugs,
                    )?;
                    default_plugs
                }
                ItemPlugs::Authored(plugs) => plugs.clone(),
            };
            while plugs.len() <= index {
                plugs.push(None);
            }
            plugs[index] = plug;
            item.plugs = ItemPlugs::Authored(plugs);
        }
        ItemUpdate::SetFlags(flags) => {
            if equipped && !capabilities.equipment_flags_writable {
                return Err(AccountError::EquipmentFlagsReadOnly);
            }
            validate_flags(flags, capabilities.item_flag_mask)?;
            item.flags = flags;
        }
    }
    Ok(())
}

fn validate_item(
    item: &ItemInstance,
    capabilities: CharacterCapabilities,
    entity_ids: &mut BTreeSet<EntityId>,
    instance_soids: &mut BTreeSet<u64>,
) -> AccountResult<()> {
    if !entity_ids.insert(item.id) {
        return Err(AccountError::DuplicateEntityId(EntityKind::ItemInstance));
    }
    ensure_unique_soid(
        instance_soids,
        item.instance_soid,
        capabilities.enforce_unique_instance_soids,
    )?;
    validate_authored_definition_hash(item.definition_hash)?;
    validate_level(item.level)?;
    validate_positive_quantity(item.quantity)?;
    validate_plugs(&item.plugs, capabilities.max_item_plugs)?;
    validate_flags(item.flags, capabilities.item_flag_mask)
}

fn validate_metadata(metadata: CharacterMetadata) -> AccountResult<()> {
    const MAX_ABILITY_ENTRY: u8 = 63;
    if metadata.race > 2
        || metadata.gender > 1
        || metadata.class_type > 2
        || metadata.abilities.movement > MAX_ABILITY_ENTRY
        || metadata.abilities.grenade > MAX_ABILITY_ENTRY
        || metadata.abilities.super_ability > MAX_ABILITY_ENTRY
        || metadata.abilities.melee > MAX_ABILITY_ENTRY
        || metadata.abilities.class_ability > MAX_ABILITY_ENTRY
    {
        Err(AccountError::InvalidCharacterMetadata)
    } else {
        Ok(())
    }
}

fn ensure_unique_soid(
    used: &mut BTreeSet<u64>,
    soid: InstanceSoid,
    enforce_unique: bool,
) -> AccountResult<()> {
    if used.insert(soid.get()) || !enforce_unique {
        Ok(())
    } else {
        Err(AccountError::DuplicateInstanceSoid(soid.get()))
    }
}

fn validate_level(level: i32) -> AccountResult<()> {
    if level >= 0 {
        Ok(())
    } else {
        Err(AccountError::InvalidLevel)
    }
}

fn validate_plugs(plugs: &ItemPlugs, maximum: usize) -> AccountResult<()> {
    let ItemPlugs::Authored(plugs) = plugs else {
        return Ok(());
    };
    if plugs.len() > maximum {
        return Err(AccountError::TooManyItemPlugs { maximum });
    }
    if plugs
        .iter()
        .flatten()
        .any(|hash| is_no_definition_hash(*hash))
    {
        return Err(AccountError::InvalidDefinitionHash);
    }
    Ok(())
}

fn validate_flags(flags: Option<u32>, maximum: u32) -> AccountResult<()> {
    if flags.is_some_and(|flags| flags > maximum) {
        Err(AccountError::InvalidItemFlags { maximum })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
    fn cross_character_moves_preserve_identity_and_order() {
        let mut state = state();
        state
            .apply(
                CAPABILITIES,
                CharacterCommand::MoveInventoryItem {
                    item_id: id(20),
                    destination_character_id: id(11),
                },
            )
            .unwrap();

        assert!(state.characters()[0].inventory.is_empty());
        assert_eq!(state.characters()[1].inventory[0], item(20, 3, 100));
    }

    #[test]
    fn equipment_swaps_preserve_complete_instances() {
        let mut state = state();
        let slot = EquipmentSlot::new("kinetic");
        state.characters[0]
            .equipment
            .insert(slot.clone(), Some(item(21, 5, 200)));
        let result = state
            .apply(
                CAPABILITIES,
                CharacterCommand::SwapInventoryItemWithEquipment {
                    item_id: id(20),
                    slot: slot.clone(),
                },
            )
            .unwrap();

        assert_eq!(
            result,
            CharacterCommandResult::EquipmentSwapped { replaced: true }
        );
        assert_eq!(state.characters[0].inventory[0], item(21, 5, 200));
        assert_eq!(
            state.characters[0].equipment.get(&slot),
            Some(&Some(item(20, 3, 100)))
        );
    }

    #[test]
    fn swapping_into_an_empty_slot_removes_the_inventory_row() {
        let mut state = state();
        let slot = EquipmentSlot::new("energy");
        let result = state
            .apply(
                CAPABILITIES,
                CharacterCommand::SwapInventoryItemWithEquipment {
                    item_id: id(20),
                    slot: slot.clone(),
                },
            )
            .unwrap();

        assert_eq!(
            result,
            CharacterCommandResult::EquipmentSwapped { replaced: false }
        );
        assert!(state.characters[0].inventory.is_empty());
        assert_eq!(
            state.characters[0].equipment.get(&slot),
            Some(&Some(item(20, 3, 100)))
        );
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
}
