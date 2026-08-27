//! Lossless JSON projection for the character equipment/inventory aggregate.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

use serde_json::{Map, Value};
use sundial_account::{
    Character, CharacterAbilities, CharacterCapabilities, CharacterCommand, CharacterCommandResult,
    CharacterMetadata, CharacterMetadataUpdate, CharacterState, DefinitionHash, EntityId,
    EquipmentSlot, InstanceSoid, ItemInstance, ItemPlugs, ItemUpdate,
};

use crate::hash::parse_unsigned_value;

use super::JsonAccountError;

const MIN_SUPPORTED_JSON_SCHEMA: u64 = 2;
const MAX_SUPPORTED_JSON_SCHEMA: u64 = 8;
const INVENTORY_SCHEMA_VERSION: u64 = 6;
const EQUIPMENT_FLAGS_SCHEMA_VERSION: u64 = 4;
const CHARACTER_INVENTORY_CAPACITY: usize = 135;
const MAX_ITEM_PLUGS: usize = 12;
const ITEM_FLAG_MASK: u32 = 3;
const NO_DEFINITION_HASH: u32 = 0x811C_9DC5;
const KNOWN_EQUIPMENT_SLOTS: &[&str] = &[
    "kinetic",
    "energy",
    "heavy",
    "helmet",
    "gauntlets",
    "chest",
    "legs",
    "class_item",
    "ghost",
    "vehicle",
    "ship",
    "subclass",
    "clan_banner",
    "emblem",
    "emote",
    "finisher",
];

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

#[derive(Clone, Debug)]
enum EquipmentLoad {
    None,
    All,
    Slots {
        slots: BTreeSet<String>,
        item_load: EquipmentItemLoad,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EquipmentItemLoad {
    /// Parse a complete item because the command may move or otherwise depend on every field.
    Full,
    /// Preserve a row as an opaque sidecar and synthesize valid neutral values for untouched fields.
    Patch,
    /// Patch-load an item while requiring its plugs because a single plug will be changed.
    PlugPatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InventoryRequirement {
    Optional,
    Required,
}

impl EquipmentLoad {
    fn includes(&self, slot: &str) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Slots { slots, .. } => slots.contains(slot),
        }
    }

    fn requires_equipment_object(&self) -> bool {
        matches!(self, Self::Slots { .. })
    }

    fn is_full(&self) -> bool {
        matches!(self, Self::All)
    }

    fn item_load(&self) -> EquipmentItemLoad {
        match self {
            Self::None | Self::All => EquipmentItemLoad::Full,
            Self::Slots { item_load, .. } => *item_load,
        }
    }
}

#[derive(Clone, Debug)]
struct CharacterLoad {
    metadata: bool,
    inventory: Option<InventoryRequirement>,
    equipment: EquipmentLoad,
}

#[derive(Clone, Debug)]
struct LoadScope {
    characters: Option<Vec<(usize, CharacterLoad)>>,
    collect_all_soids: bool,
    enforce_unique_soids: bool,
    /// Legacy equipment field helpers also accept focused, versionless fixture documents.
    /// This is never enabled for full-account or inventory operations.
    allow_missing_schema_version: bool,
}

impl LoadScope {
    #[cfg(test)]
    fn full() -> Self {
        Self {
            characters: None,
            collect_all_soids: false,
            enforce_unique_soids: true,
            allow_missing_schema_version: false,
        }
    }

    fn selected(
        characters: Vec<(usize, CharacterLoad)>,
        collect_all_soids: bool,
        allow_missing_schema_version: bool,
    ) -> Self {
        Self {
            characters: Some(characters),
            collect_all_soids,
            enforce_unique_soids: false,
            allow_missing_schema_version,
        }
    }
}

struct EquipmentLoadContext<'a> {
    next_id: &'a mut u64,
    origins: &'a mut BTreeMap<(EntityId, EquipmentSlot), EquipmentOrigin>,
    item_rows: &'a mut BTreeMap<EntityId, Map<String, Value>>,
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
    /// Destination item ID to source item ID for exact opaque-field equipment copies.
    item_copies: BTreeMap<EntityId, EntityId>,
    item_changes: BTreeMap<EntityId, ItemFieldChanges>,
    /// Plug indices to patch in-place instead of normalizing the complete authored array.
    item_plug_changes: BTreeMap<EntityId, BTreeSet<usize>>,
}

impl JsonCharacterAdapter {
    #[cfg(test)]
    pub(crate) fn load(document: &Value) -> JsonCharacterResult<Self> {
        Self::load_scoped(document, LoadScope::full())
    }

    pub(crate) fn load_inventory_item(
        document: &Value,
        character_index: usize,
    ) -> JsonCharacterResult<Self> {
        Self::load_scoped(
            document,
            LoadScope::selected(
                vec![(
                    character_index,
                    CharacterLoad {
                        metadata: false,
                        inventory: Some(InventoryRequirement::Required),
                        equipment: EquipmentLoad::None,
                    },
                )],
                false,
                false,
            ),
        )
    }

    pub(crate) fn load_character_metadata(
        document: &Value,
        character_index: usize,
    ) -> JsonCharacterResult<Self> {
        Self::load_scoped(
            document,
            LoadScope::selected(
                vec![(
                    character_index,
                    CharacterLoad {
                        metadata: true,
                        inventory: None,
                        equipment: EquipmentLoad::None,
                    },
                )],
                false,
                false,
            ),
        )
    }

    pub(crate) fn load_equipment_copy(
        document: &Value,
        source_character_index: usize,
        destination_character_index: usize,
        slots: &[&str],
    ) -> JsonCharacterResult<Self> {
        let equipment = EquipmentLoad::Slots {
            slots: slots.iter().map(|slot| (*slot).to_owned()).collect(),
            item_load: EquipmentItemLoad::Patch,
        };
        let character_load = |equipment| CharacterLoad {
            metadata: false,
            inventory: None,
            equipment,
        };
        let characters = if source_character_index == destination_character_index {
            vec![(source_character_index, character_load(equipment))]
        } else {
            vec![
                (source_character_index, character_load(equipment.clone())),
                (destination_character_index, character_load(equipment)),
            ]
        };
        Self::load_scoped(document, LoadScope::selected(characters, false, false))
    }

    pub(crate) fn load_inventory_move(
        document: &Value,
        source_character_index: usize,
        destination_character_index: usize,
    ) -> JsonCharacterResult<Self> {
        Self::load_scoped(
            document,
            LoadScope::selected(
                vec![
                    (
                        source_character_index,
                        CharacterLoad {
                            metadata: false,
                            inventory: Some(InventoryRequirement::Required),
                            equipment: EquipmentLoad::None,
                        },
                    ),
                    (
                        destination_character_index,
                        CharacterLoad {
                            metadata: false,
                            inventory: Some(InventoryRequirement::Optional),
                            equipment: EquipmentLoad::None,
                        },
                    ),
                ],
                false,
                false,
            ),
        )
    }

    pub(crate) fn load_inventory_equipment_slot(
        document: &Value,
        character_index: usize,
        slot: &str,
    ) -> JsonCharacterResult<Self> {
        Self::load_selected_slot(
            document,
            character_index,
            slot,
            Some(InventoryRequirement::Optional),
            false,
            EquipmentItemLoad::Full,
            false,
        )
    }

    pub(crate) fn load_inventory_swap(
        document: &Value,
        character_index: usize,
        slot: &str,
    ) -> JsonCharacterResult<Self> {
        Self::load_selected_slot(
            document,
            character_index,
            slot,
            Some(InventoryRequirement::Required),
            false,
            EquipmentItemLoad::Full,
            false,
        )
    }

    #[cfg(test)]
    pub(crate) fn load_equipment_slot(
        document: &Value,
        character_index: usize,
        slot: &str,
    ) -> JsonCharacterResult<Self> {
        Self::load_selected_slot(
            document,
            character_index,
            slot,
            None,
            false,
            EquipmentItemLoad::Full,
            false,
        )
    }

    #[cfg(test)]
    pub(crate) fn load_equipment_slot_with_soids(
        document: &Value,
        character_index: usize,
        slot: &str,
    ) -> JsonCharacterResult<Self> {
        Self::load_selected_slot(
            document,
            character_index,
            slot,
            None,
            true,
            EquipmentItemLoad::Full,
            false,
        )
    }

    pub(crate) fn load_equipment_patch_slot(
        document: &Value,
        character_index: usize,
        slot: &str,
    ) -> JsonCharacterResult<Self> {
        Self::load_selected_slot(
            document,
            character_index,
            slot,
            None,
            false,
            EquipmentItemLoad::Patch,
            true,
        )
    }

    pub(crate) fn load_equipment_plug_patch_slot(
        document: &Value,
        character_index: usize,
        slot: &str,
    ) -> JsonCharacterResult<Self> {
        Self::load_selected_slot(
            document,
            character_index,
            slot,
            None,
            false,
            EquipmentItemLoad::PlugPatch,
            true,
        )
    }

    pub(crate) fn load_for_equipment_definition(
        document: &Value,
        character_index: usize,
        slot: &str,
    ) -> JsonCharacterResult<Self> {
        let collect_all_soids = selected_slot_needs_instance_soid(document, character_index, slot);
        Self::load_selected_slot(
            document,
            character_index,
            slot,
            None,
            collect_all_soids,
            EquipmentItemLoad::Patch,
            true,
        )
    }

    pub(crate) fn load_for_inventory_add(document: &Value) -> JsonCharacterResult<Self> {
        let character_count = optional_characters(document)?.map_or(0, Vec::len);
        let characters = (0..character_count)
            .map(|character_index| {
                (
                    character_index,
                    CharacterLoad {
                        metadata: false,
                        inventory: Some(InventoryRequirement::Optional),
                        equipment: EquipmentLoad::None,
                    },
                )
            })
            .collect();
        Self::load_scoped(document, LoadScope::selected(characters, true, false))
    }

    fn load_selected_slot(
        document: &Value,
        character_index: usize,
        slot: &str,
        inventory: Option<InventoryRequirement>,
        collect_all_soids: bool,
        item_load: EquipmentItemLoad,
        allow_missing_schema_version: bool,
    ) -> JsonCharacterResult<Self> {
        Self::load_scoped(
            document,
            LoadScope::selected(
                vec![(
                    character_index,
                    CharacterLoad {
                        metadata: false,
                        inventory,
                        equipment: EquipmentLoad::Slots {
                            slots: BTreeSet::from([slot.to_owned()]),
                            item_load,
                        },
                    },
                )],
                collect_all_soids,
                allow_missing_schema_version,
            ),
        )
    }

    fn load_scoped(document: &Value, scope: LoadScope) -> JsonCharacterResult<Self> {
        let source_schema_version = document.get("version").and_then(Value::as_u64);
        let schema_version = if scope.allow_missing_schema_version {
            source_schema_version.unwrap_or(MAX_SUPPORTED_JSON_SCHEMA)
        } else {
            schema_version(document)?
        };
        let mut capabilities = capabilities_for_schema(schema_version)?;
        capabilities.enforce_unique_instance_soids = scope.enforce_unique_soids;
        if scope.characters.is_some() {
            capabilities.enforce_loaded_inventory_capacity = false;
        }
        let mut next_id = 1_u64;
        let reserved_soids = if scope.collect_all_soids {
            collect_all_soids(document, schema_version)?
        } else if scope.characters.is_none() {
            load_reserved_soids(document)?
        } else {
            Vec::new()
        };
        let characters_value = optional_characters(document)?;
        let mut characters = Vec::new();
        let mut character_indices = BTreeMap::new();
        let mut inventory_origins = BTreeMap::new();
        let mut equipment_origins = BTreeMap::new();
        let mut item_rows = BTreeMap::new();

        if let Some(character_values) = characters_value {
            let loads = match scope.characters {
                None => character_values
                    .iter()
                    .enumerate()
                    .map(|(character_index, _)| {
                        (
                            character_index,
                            CharacterLoad {
                                metadata: false,
                                inventory: Some(InventoryRequirement::Optional),
                                equipment: EquipmentLoad::All,
                            },
                        )
                    })
                    .collect::<Vec<_>>(),
                Some(loads) => loads,
            };
            characters.reserve(loads.len());
            for (character_index, load) in loads {
                let value = character_values.get(character_index).ok_or_else(|| {
                    JsonAccountError::format(
                        format!("/state/characters/{character_index}"),
                        "character index is out of range",
                    )
                })?;
                let path = format!("/state/characters/{character_index}");
                let row = value.as_object().ok_or_else(|| {
                    JsonAccountError::format(&path, "character must be an object")
                })?;
                let character_id = take_entity_id(&mut next_id)?;
                let soid = parse_optional_soid(row, "soid", &path)?;
                let metadata = load.metadata.then(|| load_metadata(row));
                let inventory = match load.inventory {
                    None => Vec::new(),
                    Some(InventoryRequirement::Optional) => {
                        load_inventory(row, character_index, false, &mut next_id, &mut item_rows)?
                    }
                    Some(InventoryRequirement::Required) => {
                        load_inventory(row, character_index, true, &mut next_id, &mut item_rows)?
                    }
                };
                let inventory_order = inventory.iter().map(|item| item.id).collect();
                let mut equipment_context = EquipmentLoadContext {
                    next_id: &mut next_id,
                    origins: &mut equipment_origins,
                    item_rows: &mut item_rows,
                };
                let equipment = load_equipment(
                    row,
                    character_index,
                    character_id,
                    schema_version,
                    &load.equipment,
                    &mut equipment_context,
                )?;

                character_indices.insert(character_id, character_index);
                inventory_origins.insert(character_id, inventory_order);
                characters.push(Character {
                    id: character_id,
                    soid,
                    metadata,
                    inventory,
                    equipment,
                });
            }
        }

        let state = CharacterState::try_new(capabilities, reserved_soids, characters)?;
        let next_entity_id =
            NonZeroU64::new(next_id).ok_or(JsonAccountError::EntityIdentityExhausted)?;
        Ok(Self {
            source_schema_version,
            capabilities,
            state,
            next_entity_id,
            character_indices,
            inventory_origins,
            equipment_origins,
            item_rows,
            metadata_changes: BTreeMap::new(),
            item_copies: BTreeMap::new(),
            item_changes: BTreeMap::new(),
            item_plug_changes: BTreeMap::new(),
        })
    }

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
                for slot in slots {
                    let source_item =
                        character_equipment_item_id(state, *source_character_id, slot);
                    let destination_item =
                        character_equipment_item_id(state, *destination_character_id, slot);
                    if let (Some(source_item), Some(destination_item)) =
                        (source_item, destination_item)
                    {
                        self.item_copies.insert(destination_item, source_item);
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
            if let Some(index) = plug_index(command) {
                self.item_plug_changes
                    .entry(item_id)
                    .or_default()
                    .insert(index);
            } else if changes.plugs {
                self.item_plug_changes.remove(&item_id);
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

    fn project(&self, document: &Value) -> JsonCharacterResult<Value> {
        if document.get("version").and_then(Value::as_u64) != self.source_schema_version {
            return Err(JsonAccountError::format(
                "/version",
                "the JSON schema changed after the character projection was loaded",
            ));
        }
        let mut candidate = document.clone();
        let characters = candidate_characters_mut(&mut candidate)?;
        for character in self.state.characters() {
            let character_index = *self.character_indices.get(&character.id).ok_or_else(|| {
                JsonAccountError::format(
                    "/state/characters",
                    "a loaded character lost its JSON index",
                )
            })?;
            let row = characters
                .get_mut(character_index)
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    JsonAccountError::format(
                        format!("/state/characters/{character_index}"),
                        "character must be an object",
                    )
                })?;

            if let Some(changes) = self.metadata_changes.get(&character.id) {
                let metadata = character.metadata.ok_or_else(|| {
                    JsonAccountError::format(
                        format!("/state/characters/{character_index}"),
                        "character metadata was not loaded",
                    )
                })?;
                project_metadata(row, metadata, *changes);
            }

            let inventory_ids = character
                .inventory
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>();
            let inventory_changed = self
                .inventory_origins
                .get(&character.id)
                .is_none_or(|origin| *origin != inventory_ids)
                || inventory_ids
                    .iter()
                    .any(|item_id| self.item_changes.contains_key(item_id));
            if inventory_changed {
                row.insert(
                    "inventory".into(),
                    Value::Array(
                        character
                            .inventory
                            .iter()
                            .map(|item| Value::Object(self.project_item(item)))
                            .collect(),
                    ),
                );
            }

            let slots = self.character_slots(character);
            if slots.iter().any(|slot| self.slot_changed(character, slot)) {
                let equipment = row
                    .get_mut("equipment")
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| {
                        JsonAccountError::format(
                            format!("/state/characters/{character_index}/equipment"),
                            "equipment must be an object",
                        )
                    })?;
                for slot in slots {
                    if !self.slot_changed(character, &slot) {
                        continue;
                    }
                    match character.equipment.get(&slot) {
                        None => {
                            equipment.remove(slot.as_str());
                        }
                        Some(None) => {
                            equipment.insert(slot.as_str().into(), Value::Null);
                        }
                        Some(Some(item)) => {
                            equipment.insert(
                                slot.as_str().into(),
                                Value::Object(self.project_item(item)),
                            );
                        }
                    }
                }
            }
        }
        Ok(candidate)
    }

    fn character_slots(&self, character: &Character) -> BTreeSet<EquipmentSlot> {
        character
            .equipment
            .keys()
            .cloned()
            .chain(
                self.equipment_origins
                    .keys()
                    .filter(|(character_id, _)| *character_id == character.id)
                    .map(|(_, slot)| slot.clone()),
            )
            .collect()
    }

    fn slot_changed(&self, character: &Character, slot: &EquipmentSlot) -> bool {
        let origin = self
            .equipment_origins
            .get(&(character.id, slot.clone()))
            .copied();
        let current = character.equipment.get(slot).map(|item| match item {
            Some(item) => EquipmentOrigin::Item(item.id),
            None => EquipmentOrigin::Null,
        });
        origin != current
            || current.is_some_and(|origin| match origin {
                EquipmentOrigin::Item(item_id) => {
                    self.item_changes.contains_key(&item_id)
                        || self.item_copies.contains_key(&item_id)
                }
                EquipmentOrigin::Null => false,
            })
    }

    fn project_item(&self, item: &ItemInstance) -> Map<String, Value> {
        let mut row = self.item_rows.get(&item.id).cloned().unwrap_or_default();
        if let Some(source_id) = self.item_copies.get(&item.id)
            && let Some(source) = self.item_rows.get(source_id)
        {
            for (key, value) in source {
                if key != "instance_soid" {
                    row.insert(key.clone(), value.clone());
                }
            }
        }
        let changes = self.item_changes.get(&item.id).copied().unwrap_or_else(|| {
            if self.item_rows.contains_key(&item.id) {
                ItemFieldChanges::default()
            } else {
                ItemFieldChanges::ALL
            }
        });
        if !self.item_rows.contains_key(&item.id) {
            row.insert(
                "instance_soid".into(),
                Value::String(format!("0x{:016X}", item.instance_soid.get())),
            );
        }
        if changes.definition_hash {
            row.insert(
                "definition_hash".into(),
                Value::String(format!("0x{:08X}", item.definition_hash.get())),
            );
        }
        if changes.level {
            row.insert("level".into(), Value::from(item.level));
        }
        if changes.quantity {
            row.insert("quantity".into(), Value::from(item.quantity));
        }
        if changes.plugs {
            let encoded = encode_plugs(&item.plugs);
            let patched = self
                .item_plug_changes
                .get(&item.id)
                .zip(row.get_mut("plugs").and_then(Value::as_array_mut))
                .zip(encoded.as_array())
                .map(|((indices, raw), encoded)| {
                    for &index in indices {
                        while raw.len() <= index {
                            raw.push(Value::Null);
                        }
                        raw[index] = encoded.get(index).cloned().unwrap_or(Value::Null);
                    }
                })
                .is_some();
            if !patched {
                row.insert("plugs".into(), encoded);
            }
        }
        if changes.flags {
            if let Some(flags) = item.flags {
                row.insert("flags".into(), Value::from(flags));
            } else {
                row.remove("flags");
            }
        }
        row
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

fn load_metadata(row: &Map<String, Value>) -> CharacterMetadata {
    CharacterMetadata {
        race: normalized_metadata_field(row, "race", 2, 0),
        gender: normalized_metadata_field(row, "gender", 1, 0),
        class_type: normalized_metadata_field(row, "class", 2, 0),
        abilities: CharacterAbilities {
            movement: normalized_metadata_field(row, "movement_ability", 63, 4),
            grenade: normalized_metadata_field(row, "grenade_ability", 63, 7),
            super_ability: normalized_metadata_field(row, "super_ability", 63, 10),
            melee: normalized_metadata_field(row, "melee_ability", 63, 11),
            class_ability: normalized_metadata_field(row, "class_ability", 63, 2),
        },
    }
}

fn normalized_metadata_field(
    row: &Map<String, Value>,
    field: &str,
    maximum: u8,
    fallback: u8,
) -> u8 {
    row.get(field)
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .filter(|value| *value <= maximum)
        .unwrap_or(fallback)
}

fn project_metadata(
    row: &mut Map<String, Value>,
    metadata: CharacterMetadata,
    changes: MetadataFieldChanges,
) {
    for (changed, field, value) in [
        (changes.race, "race", metadata.race),
        (changes.gender, "gender", metadata.gender),
        (changes.class_type, "class", metadata.class_type),
        (
            changes.movement,
            "movement_ability",
            metadata.abilities.movement,
        ),
        (
            changes.grenade,
            "grenade_ability",
            metadata.abilities.grenade,
        ),
        (
            changes.super_ability,
            "super_ability",
            metadata.abilities.super_ability,
        ),
        (changes.melee, "melee_ability", metadata.abilities.melee),
        (
            changes.class_ability,
            "class_ability",
            metadata.abilities.class_ability,
        ),
    ] {
        if changed {
            row.insert(field.into(), Value::from(value));
        }
    }
}

fn equipment_item_id(state: &CharacterState, command: &CharacterCommand) -> Option<EntityId> {
    let CharacterCommand::UpdateEquipmentItem {
        character_id, slot, ..
    } = command
    else {
        return None;
    };
    state
        .characters()
        .iter()
        .find(|character| character.id == *character_id)
        .and_then(|character| character.equipment.get(slot))
        .and_then(Option::as_ref)
        .map(|item| item.id)
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

fn schema_version(document: &Value) -> JsonCharacterResult<u64> {
    document
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            JsonAccountError::format("/version", "settings schema version is missing or invalid")
        })
}

fn capabilities_for_schema(version: u64) -> JsonCharacterResult<CharacterCapabilities> {
    if version < MIN_SUPPORTED_JSON_SCHEMA {
        return Err(JsonAccountError::format(
            "/version",
            format!(
                "settings schema {version} predates supported schema {MIN_SUPPORTED_JSON_SCHEMA}"
            ),
        ));
    }
    let future = version > MAX_SUPPORTED_JSON_SCHEMA;
    Ok(CharacterCapabilities {
        metadata_writable: true,
        inventory_writable: version >= INVENTORY_SCHEMA_VERSION,
        equipment_writable: true,
        equipment_flags_writable: version >= EQUIPMENT_FLAGS_SCHEMA_VERSION,
        inventory_capacity: Some(CHARACTER_INVENTORY_CAPACITY),
        enforce_loaded_inventory_capacity: !future,
        max_item_plugs: MAX_ITEM_PLUGS,
        item_flag_mask: ITEM_FLAG_MASK,
        enforce_unique_instance_soids: true,
    })
}

fn optional_characters(document: &Value) -> JsonCharacterResult<Option<&Vec<Value>>> {
    let root = document
        .as_object()
        .ok_or_else(|| JsonAccountError::format("", "settings document must be an object"))?;
    let Some(state) = root.get("state") else {
        return Ok(None);
    };
    let state = state
        .as_object()
        .ok_or_else(|| JsonAccountError::format("/state", "state must be an object"))?;
    let Some(characters) = state.get("characters") else {
        return Ok(None);
    };
    characters
        .as_array()
        .map(Some)
        .ok_or_else(|| JsonAccountError::format("/state/characters", "characters must be an array"))
}

fn candidate_characters_mut(document: &mut Value) -> JsonCharacterResult<&mut Vec<Value>> {
    document
        .get_mut("state")
        .and_then(Value::as_object_mut)
        .and_then(|state| state.get_mut("characters"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| JsonAccountError::format("/state/characters", "characters must be an array"))
}

fn load_reserved_soids(document: &Value) -> JsonCharacterResult<Vec<InstanceSoid>> {
    let Some(state) = document.get("state") else {
        return Ok(Vec::new());
    };
    let state = state
        .as_object()
        .ok_or_else(|| JsonAccountError::format("/state", "state must be an object"))?;
    let Some(account) = state.get("account") else {
        return Ok(Vec::new());
    };
    let account = account
        .as_object()
        .ok_or_else(|| JsonAccountError::format("/state/account", "account must be an object"))?;
    ["primary_soid", "soid"]
        .into_iter()
        .filter_map(|key| account.get(key).map(|value| (key, value)))
        .map(|(key, value)| parse_soid(value, &format!("/state/account/{key}")))
        .collect()
}

fn collect_all_soids(
    document: &Value,
    schema_version: u64,
) -> JsonCharacterResult<Vec<InstanceSoid>> {
    let mut soids = load_reserved_soids(document)?;
    let Some(characters) = optional_characters(document)? else {
        return Ok(soids);
    };
    for (character_index, value) in characters.iter().enumerate() {
        let character_path = format!("/state/characters/{character_index}");
        let character = value.as_object().ok_or_else(|| {
            JsonAccountError::format(&character_path, "character must be an object")
        })?;
        if let Some(soid) = parse_optional_soid(character, "soid", &character_path)? {
            soids.push(soid);
        }
        if let Some(value) = character.get("equipment") {
            let equipment_path = format!("{character_path}/equipment");
            let equipment = value.as_object().ok_or_else(|| {
                JsonAccountError::format(&equipment_path, "equipment must be an object")
            })?;
            for (slot, value) in equipment {
                if value.is_null() {
                    continue;
                }
                let item_path = format!("{equipment_path}/{slot}");
                let known_slot = KNOWN_EQUIPMENT_SLOTS.contains(&slot.as_str());
                if schema_version > MAX_SUPPORTED_JSON_SCHEMA && !known_slot {
                    if let Some(soid) = value
                        .as_object()
                        .and_then(|item| item.get("instance_soid"))
                        .and_then(parse_unsigned_value)
                        .and_then(InstanceSoid::try_from_u64)
                    {
                        soids.push(soid);
                    }
                    continue;
                }
                let item = value.as_object().ok_or_else(|| {
                    JsonAccountError::format(&item_path, "equipped item must be an object or null")
                })?;
                soids.push(parse_required_soid(item, "instance_soid", &item_path)?);
            }
        }
        if let Some(value) = character.get("inventory") {
            let inventory_path = format!("{character_path}/inventory");
            let inventory = value.as_array().ok_or_else(|| {
                JsonAccountError::format(&inventory_path, "inventory must be an array")
            })?;
            for (item_index, value) in inventory.iter().enumerate() {
                let item_path = format!("{inventory_path}/{item_index}");
                let item = value.as_object().ok_or_else(|| {
                    JsonAccountError::format(&item_path, "inventory item must be an object")
                })?;
                soids.push(parse_required_soid(item, "instance_soid", &item_path)?);
            }
        }
    }
    Ok(soids)
}

fn load_inventory(
    character: &Map<String, Value>,
    character_index: usize,
    required: bool,
    next_id: &mut u64,
    item_rows: &mut BTreeMap<EntityId, Map<String, Value>>,
) -> JsonCharacterResult<Vec<ItemInstance>> {
    let path = format!("/state/characters/{character_index}/inventory");
    let Some(value) = character.get("inventory") else {
        if required {
            return Err(JsonAccountError::missing_character_inventory(path));
        }
        return Ok(Vec::new());
    };
    let rows = value
        .as_array()
        .ok_or_else(|| JsonAccountError::format(&path, "inventory must be an array"))?;
    let mut items = Vec::with_capacity(rows.len());
    for (item_index, value) in rows.iter().enumerate() {
        let item_path = format!("{path}/{item_index}");
        if !value.is_object() {
            return Err(JsonAccountError::format(
                &item_path,
                "inventory item must be an object",
            ));
        }
        let (item, raw) = load_item(value, &item_path, next_id)?;
        item_rows.insert(item.id, raw);
        items.push(item);
    }
    Ok(items)
}

fn load_equipment(
    character: &Map<String, Value>,
    character_index: usize,
    character_id: EntityId,
    schema_version: u64,
    selection: &EquipmentLoad,
    context: &mut EquipmentLoadContext<'_>,
) -> JsonCharacterResult<BTreeMap<EquipmentSlot, Option<ItemInstance>>> {
    let Some(value) = character.get("equipment") else {
        if selection.requires_equipment_object() {
            return Err(JsonAccountError::format(
                format!("/state/characters/{character_index}/equipment"),
                "equipment is missing",
            ));
        }
        return Ok(BTreeMap::new());
    };
    let path = format!("/state/characters/{character_index}/equipment");
    let rows = value
        .as_object()
        .ok_or_else(|| JsonAccountError::format(&path, "equipment must be an object"))?;
    let mut equipment = BTreeMap::new();
    for (slot_name, value) in rows {
        if !selection.includes(slot_name) {
            continue;
        }
        if selection.is_full()
            && schema_version > MAX_SUPPORTED_JSON_SCHEMA
            && !KNOWN_EQUIPMENT_SLOTS.contains(&slot_name.as_str())
        {
            continue;
        }
        let slot = EquipmentSlot::new(slot_name);
        if value.is_null() {
            context
                .origins
                .insert((character_id, slot.clone()), EquipmentOrigin::Null);
            equipment.insert(slot, None);
            continue;
        }
        let item_path = format!("{path}/{slot_name}");
        if !value.is_object() {
            return Err(JsonAccountError::format(
                &item_path,
                "equipped item must be an object or null",
            ));
        }
        let (item, raw) = match selection.item_load() {
            EquipmentItemLoad::Full => load_item(value, &item_path, context.next_id)?,
            EquipmentItemLoad::Patch => load_patch_item(value, &item_path, context.next_id, false)?,
            EquipmentItemLoad::PlugPatch => {
                load_patch_item(value, &item_path, context.next_id, true)?
            }
        };
        context
            .origins
            .insert((character_id, slot.clone()), EquipmentOrigin::Item(item.id));
        context.item_rows.insert(item.id, raw);
        equipment.insert(slot, Some(item));
    }
    Ok(equipment)
}

fn selected_slot_needs_instance_soid(document: &Value, character_index: usize, slot: &str) -> bool {
    document
        .get("state")
        .and_then(Value::as_object)
        .and_then(|state| state.get("characters"))
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(Value::as_object)
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
        .and_then(|equipment| equipment.get(slot))
        .is_none_or(Value::is_null)
}

fn load_item(
    value: &Value,
    path: &str,
    next_id: &mut u64,
) -> JsonCharacterResult<(ItemInstance, Map<String, Value>)> {
    let row = value
        .as_object()
        .ok_or_else(|| JsonAccountError::format(path, "item must be an object"))?;
    let id = take_entity_id(next_id)?;
    let item = ItemInstance {
        id,
        instance_soid: parse_required_soid(row, "instance_soid", path)?,
        definition_hash: parse_definition_hash(row, path)?,
        level: parse_i32(row, "level", path, false)?,
        quantity: parse_i32(row, "quantity", path, true)?,
        plugs: parse_plugs(row.get("plugs"), &format!("{path}/plugs"))?,
        flags: row
            .get("flags")
            .map(|value| parse_flags(value, &format!("{path}/flags")))
            .transpose()?,
    };
    Ok((item, row.clone()))
}

fn load_patch_item(
    value: &Value,
    path: &str,
    next_id: &mut u64,
    require_valid_plugs: bool,
) -> JsonCharacterResult<(ItemInstance, Map<String, Value>)> {
    // Equipment field edits historically work on minimal rows. Surrogate values let the neutral
    // domain validate and execute only the requested command; projection starts from `row` and
    // writes only fields recorded in `item_changes`, so no surrogate value reaches JSON.
    let row = value
        .as_object()
        .ok_or_else(|| JsonAccountError::format(path, "item must be an object"))?;
    let id = take_entity_id(next_id)?;
    let fallback_soid = InstanceSoid::try_from_u64(id.get())
        .expect("storage-neutral entity IDs are always nonzero");
    let instance_soid = row
        .get("instance_soid")
        .and_then(parse_unsigned_value)
        .and_then(InstanceSoid::try_from_u64)
        .unwrap_or(fallback_soid);
    let definition_hash = row
        .get("definition_hash")
        .and_then(parse_unsigned_value)
        .and_then(|hash| u32::try_from(hash).ok())
        .filter(|hash| *hash != NO_DEFINITION_HASH)
        .map_or_else(|| DefinitionHash::new(0), DefinitionHash::new);
    let level = row
        .get("level")
        .and_then(Value::as_i64)
        .and_then(|level| i32::try_from(level).ok())
        .filter(|level| *level >= 0)
        .unwrap_or_default();
    let quantity = row
        .get("quantity")
        .and_then(Value::as_i64)
        .and_then(|quantity| i32::try_from(quantity).ok())
        .filter(|quantity| *quantity > 0)
        .unwrap_or(1);
    let plugs = if require_valid_plugs {
        parse_plugs(row.get("plugs"), &format!("{path}/plugs"))?
    } else {
        parse_plugs(row.get("plugs"), &format!("{path}/plugs")).unwrap_or(ItemPlugs::NativeDefaults)
    };
    let flags = row
        .get("flags")
        .and_then(|flags| parse_flags(flags, &format!("{path}/flags")).ok());
    Ok((
        ItemInstance {
            id,
            instance_soid,
            definition_hash,
            level,
            quantity,
            plugs,
            flags,
        },
        row.clone(),
    ))
}

fn parse_required_soid(
    row: &Map<String, Value>,
    key: &str,
    row_path: &str,
) -> JsonCharacterResult<InstanceSoid> {
    let path = format!("{row_path}/{key}");
    row.get(key)
        .ok_or_else(|| JsonAccountError::format(&path, format!("{key} is missing")))
        .and_then(|value| parse_soid(value, &path))
}

fn parse_optional_soid(
    row: &Map<String, Value>,
    key: &str,
    row_path: &str,
) -> JsonCharacterResult<Option<InstanceSoid>> {
    let Some(value) = row.get(key) else {
        return Ok(None);
    };
    parse_soid(value, &format!("{row_path}/{key}")).map(Some)
}

fn parse_soid(value: &Value, path: &str) -> JsonCharacterResult<InstanceSoid> {
    parse_unsigned_value(value)
        .and_then(InstanceSoid::try_from_u64)
        .ok_or_else(|| {
            JsonAccountError::format(path, "SOID must be a nonzero integer or a 0x hex string")
        })
}

fn parse_definition_hash(
    row: &Map<String, Value>,
    row_path: &str,
) -> JsonCharacterResult<DefinitionHash> {
    let path = format!("{row_path}/definition_hash");
    let value = row
        .get("definition_hash")
        .ok_or_else(|| JsonAccountError::format(&path, "item is missing definition_hash"))?;
    parse_unsigned_value(value)
        .and_then(|hash| u32::try_from(hash).ok())
        .filter(|hash| *hash != NO_DEFINITION_HASH)
        .map(DefinitionHash::new)
        .ok_or_else(|| {
            JsonAccountError::format(&path, "definition_hash is not valid for authored item data")
        })
}

fn parse_i32(
    row: &Map<String, Value>,
    key: &str,
    row_path: &str,
    positive: bool,
) -> JsonCharacterResult<i32> {
    let path = format!("{row_path}/{key}");
    let value = row
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| if positive { *value > 0 } else { *value >= 0 });
    value.ok_or_else(|| {
        JsonAccountError::format(
            path,
            if positive {
                format!("{key} must be a positive signed 32-bit integer")
            } else {
                format!("{key} must be a non-negative signed 32-bit integer")
            },
        )
    })
}

fn parse_plugs(value: Option<&Value>, path: &str) -> JsonCharacterResult<ItemPlugs> {
    let value = value.ok_or_else(|| JsonAccountError::format(path, "plugs is missing"))?;
    if value.is_null() {
        return Ok(ItemPlugs::NativeDefaults);
    }
    let plugs = value
        .as_array()
        .ok_or_else(|| JsonAccountError::format(path, "plugs must be null or an array"))?;
    if plugs.len() > MAX_ITEM_PLUGS {
        return Err(JsonAccountError::format(
            path,
            format!("plugs cannot contain more than {MAX_ITEM_PLUGS} entries"),
        ));
    }
    plugs
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if value.is_null() {
                return Ok(None);
            }
            parse_unsigned_value(value)
                .and_then(|hash| u32::try_from(hash).ok())
                .filter(|hash| *hash != NO_DEFINITION_HASH)
                .map(DefinitionHash::new)
                .map(Some)
                .ok_or_else(|| {
                    JsonAccountError::format(
                        format!("{path}/{index}"),
                        "plug hash must fit in an unsigned 32-bit value",
                    )
                })
        })
        .collect::<JsonCharacterResult<Vec<_>>>()
        .map(ItemPlugs::Authored)
}

fn parse_flags(value: &Value, path: &str) -> JsonCharacterResult<u32> {
    parse_unsigned_value(value)
        .filter(|flags| *flags <= u64::from(ITEM_FLAG_MASK))
        .map(|flags| flags as u32)
        .ok_or_else(|| {
            JsonAccountError::format(
                path,
                format!("flags must be a whole number between 0 and {ITEM_FLAG_MASK}"),
            )
        })
}

fn take_entity_id(next_id: &mut u64) -> JsonCharacterResult<EntityId> {
    let id = NonZeroU64::new(*next_id).ok_or(JsonAccountError::EntityIdentityExhausted)?;
    *next_id = next_id
        .checked_add(1)
        .ok_or(JsonAccountError::EntityIdentityExhausted)?;
    Ok(EntityId::new(id))
}

fn encode_plugs(plugs: &ItemPlugs) -> Value {
    match plugs {
        ItemPlugs::NativeDefaults => Value::Null,
        ItemPlugs::Authored(plugs) => Value::Array(
            plugs
                .iter()
                .map(|plug| {
                    plug.map_or(Value::Null, |hash| {
                        Value::String(format!("0x{:08X}", hash.get()))
                    })
                })
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

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
        let mut document = document(MAX_SUPPORTED_JSON_SCHEMA + 1);
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
}
