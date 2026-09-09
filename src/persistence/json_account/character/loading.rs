//! Command-specific loading scopes and decoding. Unselected JSON remains opaque.
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

use serde_json::{Map, Value};
use sundial_account::{
    Character, CharacterAbilities, CharacterCapabilities, CharacterMetadata, CharacterState,
    DefinitionHash, EntityId, EquipmentSlot, InstanceSoid, ItemInstance, ItemPlugs,
    NO_DEFINITION_HASH,
};

use super::super::{schema_version, take_entity_id};
use super::{EquipmentOrigin, JsonAccountError, JsonCharacterAdapter, JsonCharacterResult};
use crate::{
    account_contract::{
        CHARACTER_INVENTORY_CAPACITY, EQUIPMENT_FLAGS_SCHEMA_VERSION, INVENTORY_SCHEMA_VERSION,
        MAX_ITEM_PLUGS, RUNTIME_FEATURES_SCHEMA_VERSION, is_known_equipment_slot, item_flag_mask,
    },
    game_settings::{MAX_SUPPORTED_SCHEMA, MIN_SUPPORTED_SCHEMA},
    hash::parse_unsigned_value,
};

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

/// Named options prevent a call site from confusing SOID collection with schema fallback.
struct SlotLoad {
    inventory: Option<InventoryRequirement>,
    collect_all_soids: bool,
    item_load: EquipmentItemLoad,
    allow_missing_schema_version: bool,
}

struct EquipmentLoadContext<'a> {
    next_id: &'a mut u64,
    origins: &'a mut BTreeMap<(EntityId, EquipmentSlot), EquipmentOrigin>,
    item_rows: &'a mut BTreeMap<EntityId, Map<String, Value>>,
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
            SlotLoad {
                inventory: Some(InventoryRequirement::Optional),
                collect_all_soids: false,
                item_load: EquipmentItemLoad::Full,
                allow_missing_schema_version: false,
            },
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
            SlotLoad {
                inventory: Some(InventoryRequirement::Required),
                collect_all_soids: false,
                item_load: EquipmentItemLoad::Full,
                allow_missing_schema_version: false,
            },
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
            SlotLoad {
                inventory: None,
                collect_all_soids: false,
                item_load: EquipmentItemLoad::Full,
                allow_missing_schema_version: false,
            },
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
            SlotLoad {
                inventory: None,
                collect_all_soids: false,
                item_load: EquipmentItemLoad::Patch,
                allow_missing_schema_version: true,
            },
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
            SlotLoad {
                inventory: None,
                collect_all_soids: false,
                item_load: EquipmentItemLoad::PlugPatch,
                allow_missing_schema_version: true,
            },
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
            SlotLoad {
                inventory: None,
                collect_all_soids,
                item_load: EquipmentItemLoad::Patch,
                allow_missing_schema_version: true,
            },
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
        options: SlotLoad,
    ) -> JsonCharacterResult<Self> {
        let SlotLoad {
            inventory,
            collect_all_soids,
            item_load,
            allow_missing_schema_version,
        } = options;
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
            source_schema_version.unwrap_or(RUNTIME_FEATURES_SCHEMA_VERSION - 1)
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
                    Some(requirement) => load_inventory(
                        row,
                        character_index,
                        schema_version,
                        requirement,
                        &mut next_id,
                        &mut item_rows,
                    )?,
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
            plug_projections: BTreeMap::new(),
        })
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

fn capabilities_for_schema(version: u64) -> JsonCharacterResult<CharacterCapabilities> {
    if version < MIN_SUPPORTED_SCHEMA {
        return Err(JsonAccountError::format(
            "/version",
            format!("settings schema {version} predates supported schema {MIN_SUPPORTED_SCHEMA}"),
        ));
    }
    let future = version > MAX_SUPPORTED_SCHEMA;
    Ok(CharacterCapabilities {
        metadata_writable: true,
        inventory_writable: version >= INVENTORY_SCHEMA_VERSION,
        equipment_writable: true,
        equipment_flags_writable: version >= EQUIPMENT_FLAGS_SCHEMA_VERSION,
        inventory_capacity: Some(CHARACTER_INVENTORY_CAPACITY),
        enforce_loaded_inventory_capacity: !future,
        max_item_plugs: MAX_ITEM_PLUGS,
        item_flag_mask: u32::from(item_flag_mask(version)),
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
                let known_slot = is_known_equipment_slot(slot, schema_version);
                if schema_version > MAX_SUPPORTED_SCHEMA && !known_slot {
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
    schema_version: u64,
    requirement: InventoryRequirement,
    next_id: &mut u64,
    item_rows: &mut BTreeMap<EntityId, Map<String, Value>>,
) -> JsonCharacterResult<Vec<ItemInstance>> {
    let path = format!("/state/characters/{character_index}/inventory");
    let Some(value) = character.get("inventory") else {
        if requirement == InventoryRequirement::Required {
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
        let (item, raw) = load_item(value, &item_path, next_id, schema_version)?;
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
            && schema_version > MAX_SUPPORTED_SCHEMA
            && !is_known_equipment_slot(slot_name, schema_version)
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
            EquipmentItemLoad::Full => {
                load_item(value, &item_path, context.next_id, schema_version)?
            }
            EquipmentItemLoad::Patch => {
                load_patch_item(value, &item_path, context.next_id, schema_version, false)?
            }
            EquipmentItemLoad::PlugPatch => {
                load_patch_item(value, &item_path, context.next_id, schema_version, true)?
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
    schema_version: u64,
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
            .map(|value| parse_flags(value, &format!("{path}/flags"), schema_version))
            .transpose()?,
    };
    Ok((item, row.clone()))
}

fn load_patch_item(
    value: &Value,
    path: &str,
    next_id: &mut u64,
    schema_version: u64,
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
        .filter(|hash| *hash != NO_DEFINITION_HASH.get())
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
        .and_then(|flags| parse_flags(flags, &format!("{path}/flags"), schema_version).ok());
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
        .filter(|hash| *hash != NO_DEFINITION_HASH.get())
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
                .filter(|hash| *hash != NO_DEFINITION_HASH.get())
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

fn parse_flags(value: &Value, path: &str, schema_version: u64) -> JsonCharacterResult<u32> {
    let flag_mask = item_flag_mask(schema_version);
    parse_unsigned_value(value)
        .filter(|flags| *flags <= u64::from(flag_mask))
        .map(|flags| flags as u32)
        .ok_or_else(|| {
            JsonAccountError::format(
                path,
                format!("flags must be a whole number between 0 and {flag_mask}"),
            )
        })
}
