//! Schema-aware access, validation, and mutation helpers for authored items.
//!
//! This module deliberately has no UI or catalog dependencies. Read operations never
//! materialize missing JSON fields, while explicit add operations may create the leaf array
//! they target after validating the containing document shape.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use serde_json::{Map, Value};

use crate::{
    game_settings::{MAX_SUPPORTED_SCHEMA, MIN_SUPPORTED_SCHEMA},
    hash::parse_unsigned_value,
};

pub(crate) const LEGACY_PROFILE_ITEM_CAPACITY: usize = 32;
pub(crate) const PROFILE_ITEM_CAPACITY: usize = 701;
pub(crate) const CHARACTER_INVENTORY_CAPACITY: usize = 135;
pub(crate) const MAX_ITEM_PLUGS: usize = 12;
pub(crate) const INVENTORY_SCHEMA_VERSION: u64 = 6;
pub(crate) const EQUIPMENT_FLAGS_SCHEMA_VERSION: u64 = 4;
pub(crate) const DISMANTLE_REWARDS_SCHEMA_VERSION: u64 = 5;
pub(crate) const GENERATED_INSTANCE_SOID_START: u64 = 0x4000_0000_0000_0001;
pub(crate) const INVENTORY_FLAG_LOCKED: u8 = 1;
pub(crate) const INVENTORY_FLAG_TRACKED: u8 = 2;
pub(crate) const INVENTORY_FLAG_MASTERWORK: u8 = 4;
pub(crate) const INVENTORY_FLAG_MASK: u8 =
    INVENTORY_FLAG_LOCKED | INVENTORY_FLAG_TRACKED | INVENTORY_FLAG_MASTERWORK;

pub(crate) fn set_inventory_locked_flag(flags: Option<u8>, locked: bool) -> Option<u8> {
    set_inventory_flag(flags, INVENTORY_FLAG_LOCKED, locked)
}

pub(crate) fn set_inventory_masterwork_flag(flags: Option<u8>, masterworked: bool) -> Option<u8> {
    set_inventory_flag(flags, INVENTORY_FLAG_MASTERWORK, masterworked)
}

pub(crate) fn inventory_masterwork_feature_present(flags: Option<u8>) -> bool {
    flags.is_some_and(|flags| flags & INVENTORY_FLAG_MASTERWORK != 0)
}

fn set_inventory_flag(flags: Option<u8>, flag: u8, enabled: bool) -> Option<u8> {
    let mut flags = flags.unwrap_or_default();
    if enabled {
        flags |= flag;
    } else {
        flags &= !flag;
    }
    (flags != 0).then_some(flags)
}

const NO_DEFINITION_HASH: u32 = 0x811C_9DC5;
const DISMANTLE_REWARD_CAPACITY: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SchemaMode {
    MissingOrInvalid,
    Unsupported(u64),
    PreInventory(u64),
    InventoryV6,
    Future(u64),
}

impl SchemaMode {
    pub(crate) const fn version(self) -> Option<u64> {
        match self {
            Self::MissingOrInvalid => None,
            Self::Unsupported(version) | Self::PreInventory(version) | Self::Future(version) => {
                Some(version)
            }
            Self::InventoryV6 => Some(INVENTORY_SCHEMA_VERSION),
        }
    }

    pub(crate) const fn is_read_only(self) -> bool {
        matches!(
            self,
            Self::MissingOrInvalid | Self::Unsupported(_) | Self::Future(_)
        )
    }

    pub(crate) const fn can_mutate_profile_items(self) -> bool {
        matches!(self, Self::PreInventory(_) | Self::InventoryV6)
    }

    pub(crate) const fn can_mutate_character_inventory(self) -> bool {
        matches!(self, Self::InventoryV6)
    }

    pub(crate) const fn can_mutate_equipment(self) -> bool {
        matches!(self, Self::PreInventory(_) | Self::InventoryV6)
    }

    pub(crate) const fn supports_equipment_flags(self) -> bool {
        match self.version() {
            Some(version) => version >= EQUIPMENT_FLAGS_SCHEMA_VERSION,
            None => false,
        }
    }

    pub(crate) const fn can_mutate_equipment_flags(self) -> bool {
        self.can_mutate_equipment() && self.supports_equipment_flags()
    }

    const fn supports_dismantle_rewards(self) -> bool {
        match self.version() {
            Some(version) => version >= DISMANTLE_REWARDS_SCHEMA_VERSION,
            None => false,
        }
    }

    pub(crate) const fn profile_item_capacity(self) -> Option<usize> {
        match self {
            Self::PreInventory(version) => Some(profile_item_capacity(version)),
            Self::InventoryV6 => Some(PROFILE_ITEM_CAPACITY),
            Self::MissingOrInvalid | Self::Unsupported(_) | Self::Future(_) => None,
        }
    }

    const fn enforces_character_inventory_capacity(self) -> bool {
        !matches!(self, Self::Future(_))
    }
}

pub(crate) fn schema_mode(document: &Value) -> SchemaMode {
    match document.get("version").and_then(Value::as_u64) {
        None => SchemaMode::MissingOrInvalid,
        Some(INVENTORY_SCHEMA_VERSION) => SchemaMode::InventoryV6,
        Some(version) if version > MAX_SUPPORTED_SCHEMA => SchemaMode::Future(version),
        Some(version) if version >= MIN_SUPPORTED_SCHEMA => SchemaMode::PreInventory(version),
        Some(version) => SchemaMode::Unsupported(version),
    }
}

pub(crate) const fn profile_item_capacity(schema_version: u64) -> usize {
    if schema_version <= 3 {
        LEGACY_PROFILE_ITEM_CAPACITY
    } else {
        PROFILE_ITEM_CAPACITY
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InventoryError {
    path: String,
    message: String,
}

impl InventoryError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            formatter.write_str(&self.message)
        } else {
            write!(formatter, "{}: {}", self.path, self.message)
        }
    }
}

impl Error for InventoryError {}

type InventoryResult<T> = Result<T, InventoryError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProfileItemLocation {
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProfileItemSnapshot {
    pub location: ProfileItemLocation,
    pub definition_hash: u32,
    pub quantity: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InventoryItemLocation {
    pub character_index: usize,
    pub item_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ItemPlugs {
    NativeDefaults,
    Authored(Vec<Option<u32>>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InventoryItemSnapshot {
    pub location: InventoryItemLocation,
    pub instance_soid: u64,
    pub definition_hash: u32,
    pub level: i32,
    pub quantity: i32,
    pub plugs: ItemPlugs,
    pub flags: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProfileItemAction {
    SetDefinitionHash(u32),
    SetQuantity(i32),
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InventoryItemAction {
    SetDefinitionHash(u32),
    SetLevel(i32),
    SetQuantity(i32),
    SetPlugs(ItemPlugs),
    SetFlags(Option<u8>),
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NewInventoryItem {
    pub definition_hash: u32,
    pub level: i32,
    pub quantity: i32,
}

impl NewInventoryItem {
    pub(crate) const fn single(definition_hash: u32, level: i32) -> Self {
        Self {
            definition_hash,
            level,
            quantity: 1,
        }
    }
}

pub(crate) fn profile_items(document: &Value) -> InventoryResult<Option<Vec<ProfileItemSnapshot>>> {
    let mode = require_readable_schema(document)?;
    let Some(state) = optional_root_object_member(document, "state", "/state")? else {
        return Ok(None);
    };
    let Some(account) = optional_object_member(state, "account", "/state/account")? else {
        return Ok(None);
    };
    let Some(value) = account.get("profile_items") else {
        return Ok(None);
    };
    let array = value.as_array().ok_or_else(|| {
        InventoryError::new(
            "/state/account/profile_items",
            "profile_items must be an array",
        )
    })?;
    if let Some(capacity) = mode.profile_item_capacity()
        && array.len() > capacity
    {
        return Err(InventoryError::new(
            "/state/account/profile_items",
            format!(
                "profile_items contains {} rows, but schema {} permits at most {capacity}",
                array.len(),
                mode.version().unwrap_or_default()
            ),
        ));
    }
    array
        .iter()
        .enumerate()
        .map(|(index, value)| parse_profile_item(value, index))
        .collect::<InventoryResult<Vec<_>>>()
        .map(Some)
}

pub(crate) fn profile_item_target_exists(document: &Value) -> InventoryResult<bool> {
    require_readable_schema(document)?;
    let Some(state) = optional_root_object_member(document, "state", "/state")? else {
        return Ok(false);
    };
    Ok(optional_object_member(state, "account", "/state/account")?.is_some())
}

pub(crate) fn character_inventory(
    document: &Value,
    character_index: usize,
) -> InventoryResult<Option<Vec<InventoryItemSnapshot>>> {
    let mode = require_readable_schema(document)?;
    let character = character_object(document, character_index)?;
    let Some(value) = character.get("inventory") else {
        return Ok(None);
    };
    parse_character_inventory(value, character_index, mode).map(Some)
}

pub(crate) fn validate_document_items(document: &Value) -> InventoryResult<()> {
    let mode = match schema_mode(document) {
        SchemaMode::MissingOrInvalid => {
            return Err(InventoryError::new(
                "/version",
                "settings schema version is missing or invalid",
            ));
        }
        SchemaMode::Future(version) => {
            return Err(InventoryError::new(
                "/version",
                format!(
                    "settings schema {version} is newer than supported schema {MAX_SUPPORTED_SCHEMA}; inventory is read-only"
                ),
            ));
        }
        SchemaMode::Unsupported(version) => {
            return Err(InventoryError::new(
                "/version",
                format!(
                    "settings schema {version} predates supported schema {MIN_SUPPORTED_SCHEMA}; inventory is read-only"
                ),
            ));
        }
        mode => mode,
    };

    validate_required_account_primary_soid(document)?;
    let _ = profile_items(document)?;
    validate_existing_character_inventories(document, mode)?;
    if mode.supports_dismantle_rewards() {
        validate_dismantle_rewards(document)?;
    }
    if matches!(mode, SchemaMode::InventoryV6) {
        validate_v6_item_members(document)?;
    }
    validate_unique_soids(document)
}

pub(crate) fn add_profile_item(
    document: &mut Value,
    definition_hash: u32,
    quantity: i32,
) -> InventoryResult<ProfileItemLocation> {
    let mode = require_profile_mutation(document)?;
    validate_inventory_definition_hash(
        definition_hash,
        "/state/account/profile_items/<new>/definition_hash",
    )?;
    validate_positive_i32(quantity, "/state/account/profile_items/<new>/quantity")?;

    let existing = profile_items(document)?;
    let length = existing.as_ref().map_or(0, Vec::len);
    let capacity = mode
        .profile_item_capacity()
        .expect("writable schemas always have a known profile capacity");
    if length >= capacity {
        return Err(InventoryError::new(
            "/state/account/profile_items",
            format!("profile_items is full for this schema (maximum {capacity})"),
        ));
    }
    ensure_account_object(document)?;

    let mut item = Map::new();
    item.insert(
        "definition_hash".into(),
        Value::String(format_definition_hash(definition_hash)),
    );
    item.insert("quantity".into(), Value::from(quantity));

    let account = account_object_mut(document)?;
    match account.get_mut("profile_items") {
        Some(Value::Array(items)) => items.push(Value::Object(item)),
        Some(_) => unreachable!("profile_items shape was validated before mutation"),
        None => {
            account.insert(
                "profile_items".into(),
                Value::Array(vec![Value::Object(item)]),
            );
        }
    }
    Ok(ProfileItemLocation { index: length })
}

pub(crate) fn apply_profile_item_action(
    document: &mut Value,
    location: ProfileItemLocation,
    action: ProfileItemAction,
) -> InventoryResult<()> {
    require_profile_mutation(document)?;
    let snapshots = profile_items(document)?.ok_or_else(|| {
        InventoryError::new(
            "/state/account/profile_items",
            "profile_items is missing; add an item before editing a row",
        )
    })?;
    if location.index >= snapshots.len() {
        return Err(InventoryError::new(
            format!("/state/account/profile_items/{}", location.index),
            "profile item index is out of range",
        ));
    }
    match &action {
        ProfileItemAction::SetDefinitionHash(hash) => validate_inventory_definition_hash(
            *hash,
            &format!(
                "/state/account/profile_items/{}/definition_hash",
                location.index
            ),
        )?,
        ProfileItemAction::SetQuantity(quantity) => validate_positive_i32(
            *quantity,
            &format!("/state/account/profile_items/{}/quantity", location.index),
        )?,
        ProfileItemAction::Remove => {}
    }

    let items = profile_array_mut(document)?;
    match action {
        ProfileItemAction::Remove => {
            items.remove(location.index);
        }
        ProfileItemAction::SetDefinitionHash(hash) => {
            let item = items[location.index]
                .as_object_mut()
                .expect("profile row shape was validated before mutation");
            item.insert(
                "definition_hash".into(),
                Value::String(format_definition_hash(hash)),
            );
        }
        ProfileItemAction::SetQuantity(quantity) => {
            let item = items[location.index]
                .as_object_mut()
                .expect("profile row shape was validated before mutation");
            item.insert("quantity".into(), Value::from(quantity));
        }
    }
    Ok(())
}

pub(crate) fn add_inventory_item(
    document: &mut Value,
    character_index: usize,
    item: NewInventoryItem,
) -> InventoryResult<InventoryItemLocation> {
    let mode = require_inventory_mutation(document)?;
    validate_inventory_definition_hash(
        item.definition_hash,
        &format!("/state/characters/{character_index}/inventory/<new>/definition_hash"),
    )?;
    validate_nonnegative_i32(
        item.level,
        &format!("/state/characters/{character_index}/inventory/<new>/level"),
    )?;
    validate_positive_i32(
        item.quantity,
        &format!("/state/characters/{character_index}/inventory/<new>/quantity"),
    )?;

    validate_existing_character_inventories(document, mode)?;
    let existing = character_inventory(document, character_index)?;
    let length = existing.as_ref().map_or(0, Vec::len);
    if length >= CHARACTER_INVENTORY_CAPACITY {
        return Err(InventoryError::new(
            format!("/state/characters/{character_index}/inventory"),
            format!("character inventory is full (maximum {CHARACTER_INVENTORY_CAPACITY} items)"),
        ));
    }

    let instance_soid = allocate_instance_soid(document)?;
    let mut object = Map::new();
    object.insert(
        "instance_soid".into(),
        Value::String(format_instance_soid(instance_soid)),
    );
    object.insert(
        "definition_hash".into(),
        Value::String(format_definition_hash(item.definition_hash)),
    );
    object.insert("level".into(), Value::from(item.level));
    object.insert("quantity".into(), Value::from(item.quantity));
    object.insert("plugs".into(), Value::Null);

    let character = character_object_mut(document, character_index)?;
    match character.get_mut("inventory") {
        Some(Value::Array(items)) => items.push(Value::Object(object)),
        Some(_) => unreachable!("inventory shape was validated before mutation"),
        None => {
            character.insert(
                "inventory".into(),
                Value::Array(vec![Value::Object(object)]),
            );
        }
    }
    Ok(InventoryItemLocation {
        character_index,
        item_index: length,
    })
}

pub(crate) fn apply_inventory_item_action(
    document: &mut Value,
    location: InventoryItemLocation,
    action: InventoryItemAction,
) -> InventoryResult<()> {
    require_inventory_mutation(document)?;
    let snapshots = character_inventory(document, location.character_index)?.ok_or_else(|| {
        InventoryError::new(
            format!("/state/characters/{}/inventory", location.character_index),
            "character inventory is missing; add an item before editing a row",
        )
    })?;
    if location.item_index >= snapshots.len() {
        return Err(InventoryError::new(
            inventory_item_path(location),
            "inventory item index is out of range",
        ));
    }
    validate_inventory_action(location, &action)?;

    let items = inventory_array_mut(document, location.character_index)?;
    match action {
        InventoryItemAction::Remove => {
            items.remove(location.item_index);
        }
        InventoryItemAction::SetDefinitionHash(hash) => {
            inventory_object_mut(items, location.item_index).insert(
                "definition_hash".into(),
                Value::String(format_definition_hash(hash)),
            );
        }
        InventoryItemAction::SetLevel(level) => {
            inventory_object_mut(items, location.item_index)
                .insert("level".into(), Value::from(level));
        }
        InventoryItemAction::SetQuantity(quantity) => {
            inventory_object_mut(items, location.item_index)
                .insert("quantity".into(), Value::from(quantity));
        }
        InventoryItemAction::SetPlugs(plugs) => {
            inventory_object_mut(items, location.item_index)
                .insert("plugs".into(), encode_plugs(plugs));
        }
        InventoryItemAction::SetFlags(Some(flags)) => {
            inventory_object_mut(items, location.item_index)
                .insert("flags".into(), Value::from(flags));
        }
        InventoryItemAction::SetFlags(None) => {
            inventory_object_mut(items, location.item_index).remove("flags");
        }
    }
    Ok(())
}

pub(crate) fn collect_used_soids(document: &Value) -> InventoryResult<BTreeSet<u64>> {
    let mut used = BTreeSet::new();
    visit_soids(document, |soid, _path| {
        used.insert(soid);
        Ok(())
    })?;
    Ok(used)
}

pub(crate) fn allocate_instance_soid(document: &Value) -> InventoryResult<u64> {
    next_available_instance_soid(document, GENERATED_INSTANCE_SOID_START)
}

pub(crate) fn next_available_instance_soid(
    document: &Value,
    first_candidate: u64,
) -> InventoryResult<u64> {
    if first_candidate == 0 {
        return Err(InventoryError::new(
            "instance_soid",
            "the first generated instance SOID must be nonzero",
        ));
    }
    let used = collect_used_soids(document)?;
    let mut candidate = first_candidate;
    loop {
        if !used.contains(&candidate) {
            return Ok(candidate);
        }
        candidate = candidate.checked_add(1).ok_or_else(|| {
            InventoryError::new(
                "instance_soid",
                "no unused instance SOID remains at or above the requested start",
            )
        })?;
    }
}

fn require_readable_schema(document: &Value) -> InventoryResult<SchemaMode> {
    match schema_mode(document) {
        SchemaMode::MissingOrInvalid => Err(InventoryError::new(
            "/version",
            "settings schema version is missing or invalid",
        )),
        mode => Ok(mode),
    }
}

fn require_profile_mutation(document: &Value) -> InventoryResult<SchemaMode> {
    let mode = require_readable_schema(document)?;
    if mode.can_mutate_profile_items() {
        Ok(mode)
    } else {
        Err(read_only_schema_error(mode, "profile items"))
    }
}

fn require_inventory_mutation(document: &Value) -> InventoryResult<SchemaMode> {
    let mode = require_readable_schema(document)?;
    if mode.can_mutate_character_inventory() {
        Ok(mode)
    } else if let SchemaMode::PreInventory(version) = mode {
        Err(InventoryError::new(
            "/version",
            format!(
                "character inventory mutation requires schema {INVENTORY_SCHEMA_VERSION}; schema {version} remains read-only"
            ),
        ))
    } else {
        Err(read_only_schema_error(mode, "character inventory"))
    }
}

fn read_only_schema_error(mode: SchemaMode, section: &str) -> InventoryError {
    match mode {
        SchemaMode::Future(version) => InventoryError::new(
            "/version",
            format!(
                "settings schema {version} is newer than supported schema {MAX_SUPPORTED_SCHEMA}; {section} is read-only"
            ),
        ),
        SchemaMode::Unsupported(version) => InventoryError::new(
            "/version",
            format!(
                "settings schema {version} predates supported schema {MIN_SUPPORTED_SCHEMA}; {section} is read-only"
            ),
        ),
        SchemaMode::MissingOrInvalid => InventoryError::new(
            "/version",
            format!("settings schema version is missing or invalid; {section} is read-only"),
        ),
        SchemaMode::PreInventory(_) | SchemaMode::InventoryV6 => {
            InventoryError::new("/version", format!("{section} is read-only"))
        }
    }
}

fn optional_object_member<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> InventoryResult<Option<&'a Map<String, Value>>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| InventoryError::new(path, format!("{key} must be an object")))
        })
        .transpose()
}

fn optional_root_object_member<'a>(
    document: &'a Value,
    key: &str,
    path: &str,
) -> InventoryResult<Option<&'a Map<String, Value>>> {
    let root = document
        .as_object()
        .ok_or_else(|| InventoryError::new("", "settings document must be an object"))?;
    optional_object_member(root, key, path)
}

fn parse_profile_item(value: &Value, index: usize) -> InventoryResult<ProfileItemSnapshot> {
    let path = format!("/state/account/profile_items/{index}");
    let object = value
        .as_object()
        .ok_or_else(|| InventoryError::new(&path, "profile item must be an object"))?;
    let definition_hash = parse_hash_field(object, "definition_hash", &path)?;
    let quantity = parse_positive_i32_field(object, "quantity", &path)?;
    Ok(ProfileItemSnapshot {
        location: ProfileItemLocation { index },
        definition_hash,
        quantity,
    })
}

fn parse_character_inventory(
    value: &Value,
    character_index: usize,
    mode: SchemaMode,
) -> InventoryResult<Vec<InventoryItemSnapshot>> {
    let path = format!("/state/characters/{character_index}/inventory");
    let array = value
        .as_array()
        .ok_or_else(|| InventoryError::new(&path, "inventory must be an array"))?;
    if mode.enforces_character_inventory_capacity() && array.len() > CHARACTER_INVENTORY_CAPACITY {
        return Err(InventoryError::new(
            &path,
            format!(
                "inventory contains {} items, but at most {CHARACTER_INVENTORY_CAPACITY} are permitted",
                array.len()
            ),
        ));
    }
    array
        .iter()
        .enumerate()
        .map(|(item_index, value)| parse_inventory_item(value, character_index, item_index))
        .collect()
}

fn parse_inventory_item(
    value: &Value,
    character_index: usize,
    item_index: usize,
) -> InventoryResult<InventoryItemSnapshot> {
    let location = InventoryItemLocation {
        character_index,
        item_index,
    };
    let path = inventory_item_path(location);
    let object = value
        .as_object()
        .ok_or_else(|| InventoryError::new(&path, "inventory item must be an object"))?;
    let instance_soid = object
        .get("instance_soid")
        .ok_or_else(|| InventoryError::new(&path, "inventory item is missing instance_soid"))
        .and_then(|value| parse_nonzero_soid(value, &format!("{path}/instance_soid")))?;
    let definition_hash = parse_hash_field(object, "definition_hash", &path)?;
    let level = parse_nonnegative_i32_field(object, "level", &path)?;
    let quantity = parse_positive_i32_field(object, "quantity", &path)?;
    let plugs = object
        .get("plugs")
        .ok_or_else(|| InventoryError::new(&path, "inventory item is missing plugs"))
        .and_then(|value| parse_plugs(value, &format!("{path}/plugs")))?;
    let flags = object
        .get("flags")
        .map(|value| parse_flags(value, &format!("{path}/flags")))
        .transpose()?;
    Ok(InventoryItemSnapshot {
        location,
        instance_soid,
        definition_hash,
        level,
        quantity,
        plugs,
        flags,
    })
}

fn validate_existing_character_inventories(
    document: &Value,
    mode: SchemaMode,
) -> InventoryResult<()> {
    let Some(state) = optional_root_object_member(document, "state", "/state")? else {
        return Ok(());
    };
    let Some(value) = state.get("characters") else {
        return Ok(());
    };
    let characters = value
        .as_array()
        .ok_or_else(|| InventoryError::new("/state/characters", "characters must be an array"))?;
    for (character_index, value) in characters.iter().enumerate() {
        let path = format!("/state/characters/{character_index}");
        let character = value
            .as_object()
            .ok_or_else(|| InventoryError::new(&path, "character must be an object"))?;
        if let Some(inventory) = character.get("inventory") {
            parse_character_inventory(inventory, character_index, mode)?;
        }
    }
    Ok(())
}

fn validate_dismantle_rewards(document: &Value) -> InventoryResult<()> {
    let path = "/state/account/dismantle_rewards";
    let Some(value) = document.pointer(path) else {
        return Ok(());
    };
    let rewards = value
        .as_array()
        .ok_or_else(|| InventoryError::new(path, "dismantle_rewards must be an array"))?;
    if rewards.len() > DISMANTLE_REWARD_CAPACITY {
        return Err(InventoryError::new(
            path,
            format!(
                "dismantle_rewards cannot contain more than {DISMANTLE_REWARD_CAPACITY} entries"
            ),
        ));
    }

    let mut definitions = BTreeSet::new();
    for (index, value) in rewards.iter().enumerate() {
        let reward_path = format!("{path}/{index}");
        let reward = value.as_object().ok_or_else(|| {
            InventoryError::new(&reward_path, "dismantle reward must be an object")
        })?;

        let hash_path = format!("{reward_path}/definition_hash");
        let hash = reward
            .get("definition_hash")
            .ok_or_else(|| {
                InventoryError::new(&hash_path, "dismantle reward is missing definition_hash")
            })
            .and_then(|value| {
                parse_unsigned_value(value).ok_or_else(|| {
                    InventoryError::new(
                        &hash_path,
                        "definition_hash must be an unsigned integer or a 0x hex string",
                    )
                })
            })
            .and_then(|value| {
                u32::try_from(value).map_err(|_| {
                    InventoryError::new(
                        &hash_path,
                        "definition_hash must fit in an unsigned 32-bit value",
                    )
                })
            })?;
        if hash == 0 {
            return Err(InventoryError::new(
                &hash_path,
                "definition_hash must be nonzero",
            ));
        }
        validate_inventory_definition_hash(hash, &hash_path)?;
        if !definitions.insert(hash) {
            return Err(InventoryError::new(
                &hash_path,
                "dismantle reward definition_hash values must be unique",
            ));
        }

        let quantity_path = format!("{reward_path}/quantity");
        reward
            .get("quantity")
            .ok_or_else(|| {
                InventoryError::new(&quantity_path, "dismantle reward is missing quantity")
            })
            .and_then(|value| {
                value.as_u64().ok_or_else(|| {
                    InventoryError::new(
                        &quantity_path,
                        "quantity must be a positive 32-bit integer",
                    )
                })
            })
            .and_then(|quantity| {
                if (1..=i32::MAX as u64).contains(&quantity) {
                    Ok(quantity)
                } else {
                    Err(InventoryError::new(
                        &quantity_path,
                        "quantity must be a positive 32-bit integer",
                    ))
                }
            })?;
    }
    Ok(())
}

fn validate_v6_item_members(document: &Value) -> InventoryResult<()> {
    const KNOWN_MEMBERS: &[&str] = &[
        "instance_soid",
        "definition_hash",
        "level",
        "quantity",
        "plugs",
        "flags",
    ];

    let Some(characters) = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    for (character_index, character) in characters.iter().enumerate() {
        let Some(character) = character.as_object() else {
            continue;
        };
        if let Some(equipment) = character.get("equipment").and_then(Value::as_object) {
            for (slot, item) in equipment {
                if let Some(item) = item.as_object() {
                    validate_known_item_members(
                        item,
                        &format!("/state/characters/{character_index}/equipment/{slot}"),
                        KNOWN_MEMBERS,
                    )?;
                }
            }
        }
        if let Some(inventory) = character.get("inventory").and_then(Value::as_array) {
            for (item_index, item) in inventory.iter().enumerate() {
                if let Some(item) = item.as_object() {
                    validate_known_item_members(
                        item,
                        &format!("/state/characters/{character_index}/inventory/{item_index}"),
                        KNOWN_MEMBERS,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn validate_known_item_members(
    item: &Map<String, Value>,
    path: &str,
    known_members: &[&str],
) -> InventoryResult<()> {
    if let Some(key) = item
        .keys()
        .find(|key| !known_members.contains(&key.as_str()))
    {
        Err(InventoryError::new(
            format!("{path}/{key}"),
            format!(
                "schema 6 item member {key:?} is preserved by Sundial but is not accepted by Sunrise"
            ),
        ))
    } else {
        Ok(())
    }
}

fn character_object(
    document: &Value,
    character_index: usize,
) -> InventoryResult<&Map<String, Value>> {
    let state = document
        .get("state")
        .and_then(Value::as_object)
        .ok_or_else(|| InventoryError::new("/state", "state must be an object"))?;
    let characters = state
        .get("characters")
        .and_then(Value::as_array)
        .ok_or_else(|| InventoryError::new("/state/characters", "characters must be an array"))?;
    characters
        .get(character_index)
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}"),
                "character index is out of range",
            )
        })?
        .as_object()
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}"),
                "character must be an object",
            )
        })
}

fn character_object_mut(
    document: &mut Value,
    character_index: usize,
) -> InventoryResult<&mut Map<String, Value>> {
    document
        .get_mut("state")
        .and_then(Value::as_object_mut)
        .and_then(|state| state.get_mut("characters"))
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}"),
                "character object disappeared before mutation",
            )
        })
}

fn ensure_account_object(document: &Value) -> InventoryResult<()> {
    let state = document
        .get("state")
        .and_then(Value::as_object)
        .ok_or_else(|| InventoryError::new("/state", "state must be an object"))?;
    state
        .get("account")
        .and_then(Value::as_object)
        .map(|_| ())
        .ok_or_else(|| InventoryError::new("/state/account", "account must be an object"))
}

fn account_object_mut(document: &mut Value) -> InventoryResult<&mut Map<String, Value>> {
    document
        .get_mut("state")
        .and_then(Value::as_object_mut)
        .and_then(|state| state.get_mut("account"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            InventoryError::new(
                "/state/account",
                "account object disappeared before mutation",
            )
        })
}

fn profile_array_mut(document: &mut Value) -> InventoryResult<&mut Vec<Value>> {
    account_object_mut(document)?
        .get_mut("profile_items")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            InventoryError::new(
                "/state/account/profile_items",
                "profile_items array disappeared before mutation",
            )
        })
}

fn inventory_array_mut(
    document: &mut Value,
    character_index: usize,
) -> InventoryResult<&mut Vec<Value>> {
    character_object_mut(document, character_index)?
        .get_mut("inventory")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            InventoryError::new(
                format!("/state/characters/{character_index}/inventory"),
                "inventory array disappeared before mutation",
            )
        })
}

fn inventory_object_mut(items: &mut [Value], item_index: usize) -> &mut Map<String, Value> {
    items[item_index]
        .as_object_mut()
        .expect("inventory row shape was validated before mutation")
}

fn validate_inventory_action(
    location: InventoryItemLocation,
    action: &InventoryItemAction,
) -> InventoryResult<()> {
    let path = inventory_item_path(location);
    match action {
        InventoryItemAction::SetDefinitionHash(hash) => {
            validate_inventory_definition_hash(*hash, &format!("{path}/definition_hash"))
        }
        InventoryItemAction::SetLevel(level) => {
            validate_nonnegative_i32(*level, &format!("{path}/level"))
        }
        InventoryItemAction::SetQuantity(quantity) => {
            validate_positive_i32(*quantity, &format!("{path}/quantity"))
        }
        InventoryItemAction::SetPlugs(plugs) => {
            validate_plug_snapshot(plugs, &format!("{path}/plugs"))
        }
        InventoryItemAction::SetFlags(Some(flags)) if *flags > INVENTORY_FLAG_MASK => {
            Err(InventoryError::new(
                format!("{path}/flags"),
                format!("flags must be between 0 and {INVENTORY_FLAG_MASK}"),
            ))
        }
        InventoryItemAction::SetFlags(_) | InventoryItemAction::Remove => Ok(()),
    }
}

fn parse_hash_field(
    object: &Map<String, Value>,
    key: &str,
    base_path: &str,
) -> InventoryResult<u32> {
    let path = format!("{base_path}/{key}");
    let value = object
        .get(key)
        .ok_or_else(|| InventoryError::new(&path, format!("item is missing {key}")))?;
    let raw = parse_unsigned_value(value).ok_or_else(|| {
        InventoryError::new(&path, "hash must be an unsigned integer or 0x hex string")
    })?;
    let hash = u32::try_from(raw)
        .map_err(|_| InventoryError::new(&path, "hash must fit in an unsigned 32-bit value"))?;
    validate_inventory_definition_hash(hash, &path)?;
    Ok(hash)
}

fn parse_nonzero_soid(value: &Value, path: &str) -> InventoryResult<u64> {
    parse_unsigned_value(value)
        .filter(|soid| *soid != 0)
        .ok_or_else(|| {
            InventoryError::new(
                path,
                "SOID must be a nonzero unsigned integer or 0x hex string",
            )
        })
}

fn parse_nonnegative_i32_field(
    object: &Map<String, Value>,
    key: &str,
    base_path: &str,
) -> InventoryResult<i32> {
    let path = format!("{base_path}/{key}");
    let value = object
        .get(key)
        .ok_or_else(|| InventoryError::new(&path, format!("item is missing {key}")))?;
    let value = value.as_i64().ok_or_else(|| {
        InventoryError::new(
            &path,
            format!("{key} must be a non-negative 32-bit integer"),
        )
    })?;
    let value = i32::try_from(value).map_err(|_| {
        InventoryError::new(&path, format!("{key} must fit in a signed 32-bit integer"))
    })?;
    validate_nonnegative_i32(value, &path)?;
    Ok(value)
}

fn parse_positive_i32_field(
    object: &Map<String, Value>,
    key: &str,
    base_path: &str,
) -> InventoryResult<i32> {
    let path = format!("{base_path}/{key}");
    let value = object
        .get(key)
        .ok_or_else(|| InventoryError::new(&path, format!("item is missing {key}")))?;
    let value = value.as_i64().ok_or_else(|| {
        InventoryError::new(&path, format!("{key} must be a positive 32-bit integer"))
    })?;
    let value = i32::try_from(value).map_err(|_| {
        InventoryError::new(&path, format!("{key} must fit in a signed 32-bit integer"))
    })?;
    validate_positive_i32(value, &path)?;
    Ok(value)
}

fn parse_plugs(value: &Value, path: &str) -> InventoryResult<ItemPlugs> {
    if value.is_null() {
        return Ok(ItemPlugs::NativeDefaults);
    }
    let array = value
        .as_array()
        .ok_or_else(|| InventoryError::new(path, "plugs must be null or an array"))?;
    if array.len() > MAX_ITEM_PLUGS {
        return Err(InventoryError::new(
            path,
            format!("plugs cannot contain more than {MAX_ITEM_PLUGS} entries"),
        ));
    }
    let plugs = array
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if value.is_null() {
                return Ok(None);
            }
            let entry_path = format!("{path}/{index}");
            let raw = parse_unsigned_value(value).ok_or_else(|| {
                InventoryError::new(
                    &entry_path,
                    "plug hash must be null, an unsigned integer, or a 0x hex string",
                )
            })?;
            let hash = u32::try_from(raw).map_err(|_| {
                InventoryError::new(
                    &entry_path,
                    "plug hash must fit in an unsigned 32-bit value",
                )
            })?;
            validate_inventory_definition_hash(hash, &entry_path)?;
            Ok(Some(hash))
        })
        .collect::<InventoryResult<Vec<_>>>()?;
    Ok(ItemPlugs::Authored(plugs))
}

fn parse_flags(value: &Value, path: &str) -> InventoryResult<u8> {
    parse_unsigned_value(value)
        .filter(|flags| *flags <= u64::from(INVENTORY_FLAG_MASK))
        .map(|flags| flags as u8)
        .ok_or_else(|| {
            InventoryError::new(
                path,
                format!("flags must be a whole number between 0 and {INVENTORY_FLAG_MASK}"),
            )
        })
}

fn validate_required_account_primary_soid(document: &Value) -> InventoryResult<()> {
    let state = optional_root_object_member(document, "state", "/state")?
        .ok_or_else(|| InventoryError::new("/state", "settings state is missing"))?;
    let account = optional_object_member(state, "account", "/state/account")?
        .ok_or_else(|| InventoryError::new("/state/account", "account is missing"))?;
    let path = "/state/account/primary_soid";
    account
        .get("primary_soid")
        .ok_or_else(|| InventoryError::new(path, "account is missing primary_soid"))
        .and_then(|value| parse_nonzero_soid(value, path))?;
    Ok(())
}

fn validate_positive_i32(value: i32, path: &str) -> InventoryResult<()> {
    if value > 0 {
        Ok(())
    } else {
        Err(InventoryError::new(
            path,
            "quantity must be a positive signed 32-bit integer",
        ))
    }
}

fn validate_nonnegative_i32(value: i32, path: &str) -> InventoryResult<()> {
    if value >= 0 {
        Ok(())
    } else {
        Err(InventoryError::new(
            path,
            "level must be a non-negative signed 32-bit integer",
        ))
    }
}

fn validate_inventory_definition_hash(hash: u32, path: &str) -> InventoryResult<()> {
    if hash == NO_DEFINITION_HASH {
        Err(InventoryError::new(
            path,
            "the engine no-definition sentinel is not a valid authored hash",
        ))
    } else {
        Ok(())
    }
}

fn validate_plug_snapshot(plugs: &ItemPlugs, path: &str) -> InventoryResult<()> {
    let ItemPlugs::Authored(plugs) = plugs else {
        return Ok(());
    };
    if plugs.len() > MAX_ITEM_PLUGS {
        return Err(InventoryError::new(
            path,
            format!("plugs cannot contain more than {MAX_ITEM_PLUGS} entries"),
        ));
    }
    for (index, hash) in plugs.iter().enumerate() {
        if let Some(hash) = hash {
            validate_inventory_definition_hash(*hash, &format!("{path}/{index}"))?;
        }
    }
    Ok(())
}

fn encode_plugs(plugs: ItemPlugs) -> Value {
    match plugs {
        ItemPlugs::NativeDefaults => Value::Null,
        ItemPlugs::Authored(plugs) => Value::Array(
            plugs
                .into_iter()
                .map(|hash| {
                    hash.map(format_definition_hash)
                        .map_or(Value::Null, Value::String)
                })
                .collect(),
        ),
    }
}

fn format_definition_hash(hash: u32) -> String {
    format!("0x{hash:08X}")
}

fn format_instance_soid(soid: u64) -> String {
    format!("0x{soid:016X}")
}

fn inventory_item_path(location: InventoryItemLocation) -> String {
    format!(
        "/state/characters/{}/inventory/{}",
        location.character_index, location.item_index
    )
}

fn validate_unique_soids(document: &Value) -> InventoryResult<()> {
    let mut first_seen = BTreeMap::<u64, String>::new();
    visit_soids(document, |soid, path| {
        if let Some(first_path) = first_seen.get(&soid) {
            return Err(InventoryError::new(
                path,
                format!(
                    "duplicate nonzero SOID {}; first used at {first_path}",
                    format_instance_soid(soid)
                ),
            ));
        }
        first_seen.insert(soid, path.to_owned());
        Ok(())
    })
}

fn visit_soids(
    document: &Value,
    mut visit: impl FnMut(u64, &str) -> InventoryResult<()>,
) -> InventoryResult<()> {
    let Some(state) = optional_root_object_member(document, "state", "/state")? else {
        return Ok(());
    };

    if let Some(account) = optional_object_member(state, "account", "/state/account")? {
        for key in ["primary_soid", "soid"] {
            if let Some(value) = account.get(key) {
                let path = format!("/state/account/{key}");
                visit(parse_nonzero_soid(value, &path)?, &path)?;
            }
        }
    }

    let Some(characters_value) = state.get("characters") else {
        return Ok(());
    };
    let characters = characters_value
        .as_array()
        .ok_or_else(|| InventoryError::new("/state/characters", "characters must be an array"))?;
    for (character_index, character_value) in characters.iter().enumerate() {
        let character_path = format!("/state/characters/{character_index}");
        let character = character_value
            .as_object()
            .ok_or_else(|| InventoryError::new(&character_path, "character must be an object"))?;
        if let Some(value) = character.get("soid") {
            let path = format!("{character_path}/soid");
            visit(parse_nonzero_soid(value, &path)?, &path)?;
        }
        visit_equipment_soids(character, character_index, &mut visit)?;
        visit_inventory_soids(character, character_index, &mut visit)?;
    }
    Ok(())
}

fn visit_equipment_soids(
    character: &Map<String, Value>,
    character_index: usize,
    visit: &mut impl FnMut(u64, &str) -> InventoryResult<()>,
) -> InventoryResult<()> {
    let Some(value) = character.get("equipment") else {
        return Ok(());
    };
    let path = format!("/state/characters/{character_index}/equipment");
    let equipment = value
        .as_object()
        .ok_or_else(|| InventoryError::new(&path, "equipment must be an object"))?;
    for (slot, value) in equipment {
        if value.is_null() {
            continue;
        }
        let item_path = format!("{path}/{slot}");
        let item = value.as_object().ok_or_else(|| {
            InventoryError::new(&item_path, "equipped item must be an object or null")
        })?;
        let soid_path = format!("{item_path}/instance_soid");
        let soid = item
            .get("instance_soid")
            .ok_or_else(|| {
                InventoryError::new(&item_path, "equipped item is missing instance_soid")
            })
            .and_then(|value| parse_nonzero_soid(value, &soid_path))?;
        visit(soid, &soid_path)?;
    }
    Ok(())
}

fn visit_inventory_soids(
    character: &Map<String, Value>,
    character_index: usize,
    visit: &mut impl FnMut(u64, &str) -> InventoryResult<()>,
) -> InventoryResult<()> {
    let Some(value) = character.get("inventory") else {
        return Ok(());
    };
    let path = format!("/state/characters/{character_index}/inventory");
    let items = value
        .as_array()
        .ok_or_else(|| InventoryError::new(&path, "inventory must be an array"))?;
    for (item_index, value) in items.iter().enumerate() {
        let item_path = format!("{path}/{item_index}");
        let item = value
            .as_object()
            .ok_or_else(|| InventoryError::new(&item_path, "inventory item must be an object"))?;
        let soid_path = format!("{item_path}/instance_soid");
        let soid = item
            .get("instance_soid")
            .ok_or_else(|| {
                InventoryError::new(&item_path, "inventory item is missing instance_soid")
            })
            .and_then(|value| parse_nonzero_soid(value, &soid_path))?;
        visit(soid, &soid_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(soid: u64, hash: u32) -> Value {
        json!({
            "instance_soid": format_instance_soid(soid),
            "definition_hash": format_definition_hash(hash),
            "level": 106,
            "quantity": 1,
            "plugs": null
        })
    }

    fn document(version: u64) -> Value {
        json!({
            "version": version,
            "state": {
                "account": {
                    "primary_soid": "0x9EAA300100100100",
                    "profile_items": []
                },
                "characters": [{
                    "soid": "0x9EAA300200100100",
                    "class": 0,
                    "equipment": {},
                    "inventory": []
                }]
            }
        })
    }

    #[test]
    fn schema_modes_are_explicit_about_mutability() {
        assert_eq!(schema_mode(&json!({})), SchemaMode::MissingOrInvalid);
        assert_eq!(
            schema_mode(&json!({"version": 1})),
            SchemaMode::Unsupported(1)
        );
        assert_eq!(
            schema_mode(&json!({"version": 3})),
            SchemaMode::PreInventory(3)
        );
        assert_eq!(schema_mode(&json!({"version": 6})), SchemaMode::InventoryV6);
        assert_eq!(schema_mode(&json!({"version": 7})), SchemaMode::Future(7));
        assert!(SchemaMode::Future(7).is_read_only());
        assert!(SchemaMode::Unsupported(1).is_read_only());
        assert!(!SchemaMode::Unsupported(1).can_mutate_profile_items());
        assert!(!SchemaMode::MissingOrInvalid.can_mutate_equipment());
        assert!(!SchemaMode::Unsupported(1).can_mutate_equipment());
        assert!(!SchemaMode::Future(7).can_mutate_equipment());
        assert!(!SchemaMode::MissingOrInvalid.supports_equipment_flags());
        assert!(!SchemaMode::Unsupported(1).supports_equipment_flags());
        assert!(SchemaMode::Future(7).supports_equipment_flags());
        assert!(!SchemaMode::Future(7).can_mutate_equipment_flags());
        for version in 2..=5 {
            let mode = schema_mode(&json!({"version": version}));
            assert!(mode.can_mutate_profile_items());
            assert!(!mode.can_mutate_character_inventory());
            assert!(mode.can_mutate_equipment());
            assert_eq!(mode.supports_equipment_flags(), version >= 4);
            assert_eq!(mode.can_mutate_equipment_flags(), version >= 4);
            assert_eq!(mode.supports_dismantle_rewards(), version >= 5);
        }
        assert!(SchemaMode::InventoryV6.can_mutate_profile_items());
        assert!(SchemaMode::InventoryV6.can_mutate_character_inventory());
        assert!(SchemaMode::InventoryV6.can_mutate_equipment());
        assert!(SchemaMode::InventoryV6.supports_equipment_flags());
        assert!(SchemaMode::InventoryV6.can_mutate_equipment_flags());
        assert!(SchemaMode::InventoryV6.supports_dismantle_rewards());
        assert_eq!(profile_item_capacity(3), 32);
        assert_eq!(profile_item_capacity(4), 701);
    }

    #[test]
    fn reading_missing_sections_never_materializes_them() {
        let document = json!({
            "version": 6,
            "state": {"account": {}, "characters": [{"soid": 1}]}
        });
        let before = document.clone();
        assert_eq!(profile_items(&document).unwrap(), None);
        assert_eq!(character_inventory(&document, 0).unwrap(), None);
        assert_eq!(document, before);
    }

    #[test]
    fn profile_actions_preserve_order_and_unknown_fields() {
        let mut document = document(3);
        *document
            .pointer_mut("/state/account/profile_items")
            .unwrap() = json!([
            {"definition_hash": "0x00000001", "quantity": 2, "future": [1, 2]},
            {"definition_hash": 2, "quantity": 3}
        ]);

        apply_profile_item_action(
            &mut document,
            ProfileItemLocation { index: 0 },
            ProfileItemAction::SetQuantity(9),
        )
        .unwrap();
        assert_eq!(
            document.pointer("/state/account/profile_items/0/future"),
            Some(&json!([1, 2]))
        );
        assert_eq!(
            document.pointer("/state/account/profile_items/1/definition_hash"),
            Some(&Value::from(2))
        );

        let added = add_profile_item(&mut document, 3, 4).unwrap();
        assert_eq!(added.index, 2);
        assert_eq!(
            document.pointer("/state/account/profile_items/2"),
            Some(&json!({"definition_hash": "0x00000003", "quantity": 4}))
        );

        apply_profile_item_action(
            &mut document,
            ProfileItemLocation { index: 1 },
            ProfileItemAction::Remove,
        )
        .unwrap();
        assert_eq!(
            document.pointer("/state/account/profile_items/1/definition_hash"),
            Some(&Value::String("0x00000003".into()))
        );
    }

    #[test]
    fn malformed_profile_sections_are_rejected_without_mutation() {
        let mut document = document(6);
        *document
            .pointer_mut("/state/account/profile_items")
            .unwrap() = Value::String("bad".into());
        let before = document.clone();
        let error = add_profile_item(&mut document, 1, 1).unwrap_err();
        assert_eq!(error.path(), "/state/account/profile_items");
        assert!(error.message().contains("array"));
        assert_eq!(document, before);
    }

    #[test]
    fn profile_hashes_reject_the_engine_sentinel_without_mutation() {
        let mut parsed = document(6);
        *parsed.pointer_mut("/state/account/profile_items").unwrap() = json!([{
            "definition_hash": format_definition_hash(NO_DEFINITION_HASH),
            "quantity": 1
        }]);
        let error = validate_document_items(&parsed).unwrap_err();
        assert_eq!(
            error.path(),
            "/state/account/profile_items/0/definition_hash"
        );

        let mut added = document(6);
        let before = added.clone();
        assert!(add_profile_item(&mut added, NO_DEFINITION_HASH, 1).is_err());
        assert_eq!(added, before);

        add_profile_item(&mut added, 1, 1).unwrap();
        let before = added.clone();
        assert!(
            apply_profile_item_action(
                &mut added,
                ProfileItemLocation { index: 0 },
                ProfileItemAction::SetDefinitionHash(NO_DEFINITION_HASH),
            )
            .is_err()
        );
        assert_eq!(added, before);
    }

    #[test]
    fn profile_capacity_follows_schema_history() {
        let mut legacy = document(3);
        *legacy.pointer_mut("/state/account/profile_items").unwrap() = Value::Array(
            (0..LEGACY_PROFILE_ITEM_CAPACITY)
                .map(|index| json!({"definition_hash": index, "quantity": 1}))
                .collect(),
        );
        let before = legacy.clone();
        assert!(add_profile_item(&mut legacy, 99, 1).is_err());
        assert_eq!(legacy, before);

        let mut newer = document(4);
        *newer.pointer_mut("/state/account/profile_items").unwrap() = Value::Array(
            (0..LEGACY_PROFILE_ITEM_CAPACITY)
                .map(|index| json!({"definition_hash": index, "quantity": 1}))
                .collect(),
        );
        assert!(add_profile_item(&mut newer, 99, 1).is_ok());
    }

    #[test]
    fn old_and_future_schemas_are_read_only_where_required() {
        let mut unsupported = document(1);
        let before = unsupported.clone();
        assert!(add_profile_item(&mut unsupported, 1, 1).is_err());
        assert!(validate_document_items(&unsupported).is_err());
        assert_eq!(unsupported, before);

        let mut old = document(5);
        *old.pointer_mut("/state/characters/0/inventory").unwrap() =
            Value::Array(vec![item(GENERATED_INSTANCE_SOID_START, 1)]);
        assert_eq!(character_inventory(&old, 0).unwrap().unwrap().len(), 1);
        let before = old.clone();
        assert!(
            apply_inventory_item_action(
                &mut old,
                InventoryItemLocation {
                    character_index: 0,
                    item_index: 0
                },
                InventoryItemAction::SetQuantity(2)
            )
            .is_err()
        );
        assert_eq!(old, before);

        let mut future = document(7);
        let before = future.clone();
        assert!(add_profile_item(&mut future, 1, 1).is_err());
        assert_eq!(future, before);
    }

    #[test]
    fn explicit_v6_add_materializes_only_the_inventory_leaf() {
        let mut document = document(6);
        let character = document
            .pointer_mut("/state/characters/0")
            .unwrap()
            .as_object_mut()
            .unwrap();
        character.remove("inventory");
        character.insert("future_character_data".into(), json!({"keep": true}));

        let location =
            add_inventory_item(&mut document, 0, NewInventoryItem::single(42, 106)).unwrap();
        assert_eq!(
            location,
            InventoryItemLocation {
                character_index: 0,
                item_index: 0
            }
        );
        let created = document.pointer("/state/characters/0/inventory/0").unwrap();
        assert_eq!(created.get("plugs"), Some(&Value::Null));
        assert!(created.get("flags").is_none());
        assert_eq!(created.get("quantity"), Some(&Value::from(1)));
        assert_eq!(
            document.pointer("/state/characters/0/future_character_data/keep"),
            Some(&Value::Bool(true))
        );
    }

    #[test]
    fn item_state_edits_preserve_unrelated_flags() {
        assert!(!inventory_masterwork_feature_present(None));
        assert!(!inventory_masterwork_feature_present(Some(
            INVENTORY_FLAG_LOCKED
        )));
        assert!(inventory_masterwork_feature_present(Some(
            INVENTORY_FLAG_MASTERWORK
        )));
        assert_eq!(
            set_inventory_locked_flag(None, true),
            Some(INVENTORY_FLAG_LOCKED)
        );
        assert_eq!(
            set_inventory_locked_flag(Some(INVENTORY_FLAG_LOCKED), false),
            None
        );
        assert_eq!(
            set_inventory_locked_flag(Some(INVENTORY_FLAG_TRACKED), true),
            Some(INVENTORY_FLAG_LOCKED | INVENTORY_FLAG_TRACKED)
        );
        assert_eq!(
            set_inventory_locked_flag(Some(INVENTORY_FLAG_MASK), false),
            Some(INVENTORY_FLAG_TRACKED | INVENTORY_FLAG_MASTERWORK)
        );
        assert_eq!(
            set_inventory_masterwork_flag(Some(INVENTORY_FLAG_LOCKED), true),
            Some(INVENTORY_FLAG_LOCKED | INVENTORY_FLAG_MASTERWORK)
        );
        assert_eq!(
            set_inventory_masterwork_flag(Some(INVENTORY_FLAG_MASK), false),
            Some(INVENTORY_FLAG_LOCKED | INVENTORY_FLAG_TRACKED)
        );
        assert_eq!(
            set_inventory_masterwork_flag(None, true),
            Some(INVENTORY_FLAG_MASTERWORK)
        );
    }

    #[test]
    fn inventory_actions_preserve_identity_order_and_unknown_fields() {
        let mut document = document(6);
        *document
            .pointer_mut("/state/characters/0/inventory")
            .unwrap() = json!([
            {
                "instance_soid": "0x4000000000000001",
                "definition_hash": "0x00000001",
                "level": 106,
                "quantity": 1,
                "plugs": null,
                "flags": 1,
                "future": {"keep": true}
            },
            {
                "instance_soid": "0x4000000000000002",
                "definition_hash": "0x00000002",
                "level": 106,
                "quantity": 1,
                "plugs": []
            }
        ]);
        let first = InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        };
        apply_inventory_item_action(
            &mut document,
            first,
            InventoryItemAction::SetDefinitionHash(3),
        )
        .unwrap();
        apply_inventory_item_action(
            &mut document,
            first,
            InventoryItemAction::SetPlugs(ItemPlugs::Authored(vec![Some(4), None])),
        )
        .unwrap();
        apply_inventory_item_action(&mut document, first, InventoryItemAction::SetFlags(None))
            .unwrap();
        let edited = document.pointer("/state/characters/0/inventory/0").unwrap();
        assert_eq!(
            edited.get("instance_soid"),
            Some(&Value::String("0x4000000000000001".into()))
        );
        assert_eq!(edited.pointer("/future/keep"), Some(&Value::Bool(true)));
        assert_eq!(edited.get("plugs"), Some(&json!(["0x00000004", null])));
        assert!(edited.get("flags").is_none());
        assert_eq!(
            document.pointer("/state/characters/0/inventory/1/instance_soid"),
            Some(&Value::String("0x4000000000000002".into()))
        );
    }

    #[test]
    fn strict_v6_validation_reports_unknown_item_members_without_deleting_them() {
        let mut inventory_document = document(6);
        let mut row = item(1, 1);
        row["future"] = json!({"keep": true});
        *inventory_document
            .pointer_mut("/state/characters/0/inventory")
            .unwrap() = Value::Array(vec![row]);

        let error = validate_document_items(&inventory_document).unwrap_err();
        assert_eq!(error.path(), "/state/characters/0/inventory/0/future");
        apply_inventory_item_action(
            &mut inventory_document,
            InventoryItemLocation {
                character_index: 0,
                item_index: 0,
            },
            InventoryItemAction::SetQuantity(2),
        )
        .unwrap();
        assert_eq!(
            inventory_document.pointer("/state/characters/0/inventory/0/future/keep"),
            Some(&Value::Bool(true))
        );

        let mut profile = document(6);
        *profile.pointer_mut("/state/account/profile_items").unwrap() =
            json!([{"definition_hash": 1, "quantity": 1, "future": true}]);
        assert_eq!(validate_document_items(&profile), Ok(()));
    }

    #[test]
    fn inventory_validation_covers_required_bounds() {
        let invalid_values = [
            ("instance_soid", Value::from(0)),
            ("definition_hash", Value::from(u64::from(u32::MAX) + 1)),
            ("level", Value::from(-1)),
            ("quantity", Value::from(0)),
            ("flags", Value::from(8)),
        ];
        for (key, value) in invalid_values {
            let mut document = document(6);
            let mut row = item(1, 1);
            row.as_object_mut().unwrap().insert(key.into(), value);
            *document
                .pointer_mut("/state/characters/0/inventory")
                .unwrap() = Value::Array(vec![row]);
            let error = validate_document_items(&document).unwrap_err();
            assert!(error.path().ends_with(key), "unexpected error: {error}");
        }

        let mut too_many_plugs = document(6);
        let mut row = item(1, 1);
        row["plugs"] = Value::Array(vec![Value::Null; MAX_ITEM_PLUGS + 1]);
        *too_many_plugs
            .pointer_mut("/state/characters/0/inventory")
            .unwrap() = Value::Array(vec![row]);
        assert!(validate_document_items(&too_many_plugs).is_err());

        let mut too_many_items = document(6);
        *too_many_items
            .pointer_mut("/state/characters/0/inventory")
            .unwrap() = Value::Array(
            (0..=CHARACTER_INVENTORY_CAPACITY)
                .map(|index| item(index as u64 + 1, 1))
                .collect(),
        );
        assert!(validate_document_items(&too_many_items).is_err());

        let mut quoted_flags = document(6);
        let mut row = item(1, 1);
        row["flags"] = Value::String("0x3".into());
        *quoted_flags
            .pointer_mut("/state/characters/0/inventory")
            .unwrap() = Value::Array(vec![row]);
        assert_eq!(
            character_inventory(&quoted_flags, 0).unwrap().unwrap()[0].flags,
            Some(3)
        );
    }

    #[test]
    fn document_validation_requires_the_account_primary_soid() {
        let mut document = document(6);
        document
            .pointer_mut("/state/account")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("primary_soid");
        let error = validate_document_items(&document).unwrap_err();
        assert_eq!(error.path(), "/state/account/primary_soid");
    }

    #[test]
    fn schema_five_and_newer_dismantle_rewards_follow_sunrise_constraints() {
        for version in 5..=6 {
            let mut valid = document(version);
            *valid
                .pointer_mut("/state/account")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .entry("dismantle_rewards")
                .or_insert(Value::Null) = json!([
                {
                    "definition_hash": "0x00000001",
                    "quantity": 1,
                    "future": {"preserved": true}
                },
                {"definition_hash": 2, "quantity": i32::MAX}
            ]);
            assert_eq!(validate_document_items(&valid), Ok(()));
            assert_eq!(
                valid.pointer("/state/account/dismantle_rewards/0/future/preserved"),
                Some(&Value::Bool(true))
            );
        }

        let invalid = [
            (json!("not an array"), "/state/account/dismantle_rewards"),
            (json!(["not an object"]), "/dismantle_rewards/0"),
            (
                json!([{"quantity": 1}]),
                "/dismantle_rewards/0/definition_hash",
            ),
            (
                json!([{"definition_hash": 1}]),
                "/dismantle_rewards/0/quantity",
            ),
            (
                json!([{"definition_hash": 0, "quantity": 1}]),
                "/dismantle_rewards/0/definition_hash",
            ),
            (
                json!([{
                    "definition_hash": format_definition_hash(NO_DEFINITION_HASH),
                    "quantity": 1
                }]),
                "/dismantle_rewards/0/definition_hash",
            ),
            (
                json!([{
                    "definition_hash": u64::from(u32::MAX) + 1,
                    "quantity": 1
                }]),
                "/dismantle_rewards/0/definition_hash",
            ),
            (
                json!([{"definition_hash": 1, "quantity": 0}]),
                "/dismantle_rewards/0/quantity",
            ),
            (
                json!([{"definition_hash": 1, "quantity": "0x1"}]),
                "/dismantle_rewards/0/quantity",
            ),
            (
                json!([{
                    "definition_hash": 1,
                    "quantity": i64::from(i32::MAX) + 1
                }]),
                "/dismantle_rewards/0/quantity",
            ),
            (
                json!([
                    {"definition_hash": 1, "quantity": 1},
                    {"definition_hash": 1, "quantity": 2}
                ]),
                "/dismantle_rewards/1/definition_hash",
            ),
            (
                Value::Array(
                    (1..=DISMANTLE_REWARD_CAPACITY + 1)
                        .map(|hash| json!({"definition_hash": hash, "quantity": 1}))
                        .collect(),
                ),
                "/state/account/dismantle_rewards",
            ),
        ];
        for version in 5..=6 {
            for (rewards, expected_path_suffix) in &invalid {
                let mut candidate = document(version);
                candidate
                    .pointer_mut("/state/account")
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .insert("dismantle_rewards".into(), rewards.clone());
                let before = candidate.clone();
                let error = validate_document_items(&candidate).unwrap_err();
                assert!(
                    error.path().ends_with(expected_path_suffix),
                    "unexpected error for schema {version}: {error}"
                );
                assert_eq!(candidate, before);
            }
        }

        let mut legacy = document(4);
        legacy
            .pointer_mut("/state/account")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("dismantle_rewards".into(), json!({"future": true}));
        assert_eq!(validate_document_items(&legacy), Ok(()));
    }

    #[test]
    fn document_validation_rejects_duplicate_soids_with_both_locations() {
        let duplicate = 0x4000_0000_0000_1234;
        let mut equipment_inventory = document(6);
        *equipment_inventory
            .pointer_mut("/state/characters/0/equipment")
            .unwrap() = json!({"kinetic": item(duplicate, 1)});
        *equipment_inventory
            .pointer_mut("/state/characters/0/inventory")
            .unwrap() = Value::Array(vec![item(duplicate, 2)]);

        let error = validate_document_items(&equipment_inventory).unwrap_err();
        assert_eq!(
            error.path(),
            "/state/characters/0/inventory/0/instance_soid"
        );
        assert!(
            error
                .message()
                .contains("/state/characters/0/equipment/kinetic/instance_soid")
        );

        let mut account_character = document(6);
        let account_soid = account_character
            .pointer("/state/account/primary_soid")
            .unwrap()
            .clone();
        *account_character
            .pointer_mut("/state/characters/0/soid")
            .unwrap() = account_soid;

        let error = validate_document_items(&account_character).unwrap_err();
        assert_eq!(error.path(), "/state/characters/0/soid");
        assert!(error.message().contains("/state/account/primary_soid"));
    }

    #[test]
    fn soid_allocation_scans_account_characters_equipment_and_inventory() {
        let start = GENERATED_INSTANCE_SOID_START;
        let mut document = json!({
            "version": 6,
            "state": {
                "account": {"primary_soid": format_instance_soid(start)},
                "characters": [
                    {
                        "soid": format_instance_soid(start + 1),
                        "equipment": {"kinetic": item(start + 2, 1)},
                        "inventory": [item(start + 3, 2)]
                    },
                    {
                        "soid": "0x9EAA300200100101",
                        "equipment": {},
                        "inventory": []
                    }
                ]
            }
        });
        assert_eq!(allocate_instance_soid(&document).unwrap(), start + 4);

        let location = add_inventory_item(
            &mut document,
            1,
            NewInventoryItem {
                definition_hash: 3,
                level: 106,
                quantity: 2,
            },
        )
        .unwrap();
        assert_eq!(location.item_index, 0);
        assert_eq!(
            document.pointer("/state/characters/1/inventory/0/instance_soid"),
            Some(&Value::String(format_instance_soid(start + 4)))
        );
    }

    #[test]
    fn allocation_errors_on_malformed_identity_sources_and_exhaustion() {
        let malformed = json!({
            "state": {"characters": [{"equipment": {"kinetic": {"instance_soid": 0}}}]}
        });
        let error = collect_used_soids(&malformed).unwrap_err();
        assert!(error.path().ends_with("instance_soid"));

        let exhausted = json!({
            "state": {"account": {"primary_soid": u64::MAX}}
        });
        assert!(next_available_instance_soid(&exhausted, u64::MAX).is_err());
    }

    #[test]
    fn failed_actions_leave_documents_untouched() {
        let mut document = document(6);
        *document
            .pointer_mut("/state/characters/0/inventory")
            .unwrap() = Value::Array(vec![item(1, 1)]);
        let before = document.clone();
        assert!(
            apply_inventory_item_action(
                &mut document,
                InventoryItemLocation {
                    character_index: 0,
                    item_index: 0
                },
                InventoryItemAction::SetQuantity(0)
            )
            .is_err()
        );
        assert_eq!(document, before);

        let before = document.clone();
        assert!(
            apply_inventory_item_action(
                &mut document,
                InventoryItemLocation {
                    character_index: 0,
                    item_index: 0
                },
                InventoryItemAction::SetPlugs(ItemPlugs::Authored(vec![None; MAX_ITEM_PLUGS + 1]))
            )
            .is_err()
        );
        assert_eq!(document, before);
    }
}
