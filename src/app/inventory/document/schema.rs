//! Inventory schema capabilities, limits, flags, and edit gates.

use serde_json::Value;

pub(crate) use crate::account_contract::{
    CHARACTER_INVENTORY_CAPACITY, DISMANTLE_REWARDS_SCHEMA_VERSION, EQUIPMENT_FLAGS_SCHEMA_VERSION,
    FILTERED_DISMANTLE_REWARD_CAPACITY, FILTERED_DISMANTLE_REWARDS_SCHEMA_VERSION,
    INVENTORY_FLAG_LOCKED, INVENTORY_SCHEMA_VERSION, LEGACY_DISMANTLE_REWARD_CAPACITY,
    MAX_ITEM_PLUGS, PROFILE_ITEM_CAPACITY, profile_item_capacity,
};
use crate::game_settings::{MAX_SUPPORTED_SCHEMA, MIN_SUPPORTED_SCHEMA};

use super::model::{InventoryError, InventoryResult};

pub(crate) const GENERATED_INSTANCE_SOID_START: u64 = 0x4000_0000_0000_0001;
pub(in crate::app) const KNOWN_ITEM_MEMBERS: &[&str] = &[
    "instance_soid",
    "definition_hash",
    "level",
    "quantity",
    "plugs",
    "flags",
];

pub(crate) fn set_inventory_locked_flag(flags: Option<u8>, locked: bool) -> Option<u8> {
    set_inventory_flag(flags, INVENTORY_FLAG_LOCKED, locked)
}

pub(in crate::app::inventory) fn set_inventory_flag(
    flags: Option<u8>,
    flag: u8,
    enabled: bool,
) -> Option<u8> {
    let mut flags = flags.unwrap_or_default();
    if enabled {
        flags |= flag;
    } else {
        flags &= !flag;
    }
    (flags != 0).then_some(flags)
}

pub(in crate::app::inventory) const NO_DEFINITION_HASH: u32 =
    sundial_account::NO_DEFINITION_HASH.get();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SchemaMode {
    MissingOrInvalid,
    Unsupported(u64),
    PreInventory(u64),
    Inventory(u64),
    Future(u64),
}

impl SchemaMode {
    pub(crate) const fn version(self) -> Option<u64> {
        match self {
            Self::MissingOrInvalid => None,
            Self::Unsupported(version)
            | Self::PreInventory(version)
            | Self::Inventory(version)
            | Self::Future(version) => Some(version),
        }
    }

    pub(crate) const fn supports_v13(self) -> bool {
        match self.version() {
            Some(version) => crate::account_contract::supports_v13(version),
            None => false,
        }
    }

    pub(crate) const fn item_flag_mask(self) -> u8 {
        crate::account_contract::item_flag_mask(match self.version() {
            Some(version) => version,
            None => 0,
        })
    }

    pub(crate) const fn equipment_slots(
        self,
    ) -> &'static [crate::account_contract::EquipmentSlotContract] {
        crate::account_contract::equipment_slots_for_schema(match self.version() {
            Some(version) => version,
            None => 0,
        })
    }

    pub(crate) const fn is_read_only(self) -> bool {
        matches!(self, Self::MissingOrInvalid | Self::Unsupported(_))
    }

    pub(crate) const fn is_future(self) -> bool {
        matches!(self, Self::Future(_))
    }

    pub(crate) const fn can_mutate_profile_items(self) -> bool {
        matches!(
            self,
            Self::PreInventory(_) | Self::Inventory(_) | Self::Future(_)
        )
    }

    pub(crate) const fn can_mutate_character_inventory(self) -> bool {
        matches!(self, Self::Inventory(_) | Self::Future(_))
    }

    pub(crate) const fn can_mutate_equipment(self) -> bool {
        matches!(
            self,
            Self::PreInventory(_) | Self::Inventory(_) | Self::Future(_)
        )
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

    pub(crate) const fn supports_dismantle_rewards(self) -> bool {
        match self.version() {
            Some(version) => version >= DISMANTLE_REWARDS_SCHEMA_VERSION,
            None => false,
        }
    }

    pub(crate) const fn can_mutate_dismantle_rewards(self) -> bool {
        self.supports_dismantle_rewards() && !self.is_read_only() && !self.is_future()
    }

    pub(crate) const fn supports_filtered_dismantle_rewards(self) -> bool {
        match self.version() {
            Some(version) => version >= FILTERED_DISMANTLE_REWARDS_SCHEMA_VERSION,
            None => false,
        }
    }

    pub(crate) const fn dismantle_reward_capacity(self) -> Option<usize> {
        if !self.supports_dismantle_rewards() || self.is_future() {
            None
        } else if self.supports_filtered_dismantle_rewards() {
            Some(FILTERED_DISMANTLE_REWARD_CAPACITY)
        } else {
            Some(LEGACY_DISMANTLE_REWARD_CAPACITY)
        }
    }

    pub(crate) const fn profile_item_capacity(self) -> Option<usize> {
        match self {
            Self::PreInventory(version) => Some(profile_item_capacity(version)),
            Self::Inventory(_) | Self::Future(_) => Some(PROFILE_ITEM_CAPACITY),
            Self::MissingOrInvalid | Self::Unsupported(_) => None,
        }
    }

    pub(in crate::app::inventory) const fn enforces_profile_item_capacity(self) -> bool {
        !matches!(self, Self::Future(_))
    }

    pub(in crate::app::inventory) const fn enforces_character_inventory_capacity(self) -> bool {
        !matches!(self, Self::Future(_))
    }
}

pub(crate) fn schema_mode(document: &Value) -> SchemaMode {
    match document.get("version").and_then(Value::as_u64) {
        None => SchemaMode::MissingOrInvalid,
        Some(version) if (INVENTORY_SCHEMA_VERSION..=MAX_SUPPORTED_SCHEMA).contains(&version) => {
            SchemaMode::Inventory(version)
        }
        Some(version) if version > MAX_SUPPORTED_SCHEMA => SchemaMode::Future(version),
        Some(version) if version >= MIN_SUPPORTED_SCHEMA => SchemaMode::PreInventory(version),
        Some(version) => SchemaMode::Unsupported(version),
    }
}

pub(in crate::app::inventory) fn require_readable_schema(
    document: &Value,
) -> InventoryResult<SchemaMode> {
    match schema_mode(document) {
        SchemaMode::MissingOrInvalid => Err(InventoryError::new(
            "/version",
            "settings schema version is missing or invalid",
        )),
        mode => Ok(mode),
    }
}

#[cfg(test)]
pub(in crate::app::inventory) fn require_profile_mutation(
    document: &Value,
) -> InventoryResult<SchemaMode> {
    let mode = require_readable_schema(document)?;
    if mode.can_mutate_profile_items() {
        Ok(mode)
    } else {
        Err(read_only_schema_error(mode, "profile items"))
    }
}

pub(in crate::app::inventory) fn require_inventory_mutation(
    document: &Value,
) -> InventoryResult<SchemaMode> {
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

pub(in crate::app::inventory) fn read_only_schema_error(
    mode: SchemaMode,
    section: &str,
) -> InventoryError {
    match mode {
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
        SchemaMode::PreInventory(_) | SchemaMode::Inventory(_) | SchemaMode::Future(_) => {
            InventoryError::new("/version", format!("{section} is read-only"))
        }
    }
}

#[cfg(test)]
pub(crate) use crate::account_contract::{INVENTORY_FLAG_TRACKED, LEGACY_PROFILE_ITEM_CAPACITY};
