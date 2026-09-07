//! Workspace-level account source selection and operation routing.
//!
//! JSON and SQLite remain independent persistence adapters. A workspace selects one account
//! source when it loads and never falls back after selection.

mod change_summary;
mod equipment_dispatch;
mod inventory_dispatch;
pub(super) use equipment_dispatch::*;
pub(super) use inventory_dispatch::*;
#[cfg(feature = "sqlite-account")]
mod sqlite;

use std::{
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
};

#[cfg(feature = "sqlite-account")]
use serde_json::Number;
use serde_json::{Map, Value};
#[cfg(feature = "sqlite-account")]
use sundial_account::{
    AccountSettingGroup, AccountSettingKey, AccountSettingValue, KeyBindingSlot,
};
use sundial_account::{AccountSettingsCommand, CharacterMetadata, CharacterMetadataUpdate};

use change_summary::account_members_except_settings;
#[cfg(feature = "sqlite-account")]
use change_summary::sqlite_change_summaries;

use super::equipment::EquippedItemSnapshot;
#[cfg(feature = "sqlite-account")]
use super::equipment::{EquippedItemPlugs, EquippedPlugValue};
#[cfg(feature = "sqlite-account")]
use super::inventory::{DismantleGearClass, DismantleRarity, ItemPlugs};
use super::inventory::{
    DismantleRewardAction, DismantleRewardLocation, DismantleRewardSnapshot, InventoryError,
    InventoryItemAction, InventoryItemLocation, InventoryItemSnapshot, NewInventoryItem,
    ProfileItemAction, ProfileItemLocation, ProfileItemSnapshot,
};
use super::{account_settings, character_metadata};
use crate::persistence::json_account::{JsonCharacterAdapter, ensure_schema_v8_preferences};
#[cfg(feature = "sqlite-account")]
use crate::persistence::sqlite_account::{
    self as sqlite_persistence, SqliteAccountDocument, SqliteAccountDocumentLoad, SqliteSaveReceipt,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AccountSourceKind {
    Json,
    #[cfg_attr(not(feature = "sqlite-account"), allow(dead_code))]
    Sqlite,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JsonSelectionReason {
    DatabaseMissing,
    #[cfg_attr(not(feature = "sqlite-account"), allow(dead_code))]
    DatabaseEmpty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AccountDocument {
    Json(JsonSelectionReason),
    #[cfg(feature = "sqlite-account")]
    Sqlite(Box<SqliteAccountDocument>),
    #[cfg_attr(not(feature = "sqlite-account"), allow(dead_code))]
    Blocked(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkspaceDocument {
    json: Value,
    database_path: PathBuf,
    account: AccountDocument,
}

#[derive(Clone, Debug)]
pub(super) struct AccountSourceInfo {
    pub kind: AccountSourceKind,
    pub label: &'static str,
    pub detail: String,
    #[cfg_attr(not(feature = "sqlite-account"), allow(dead_code))]
    pub database_path: PathBuf,
    pub contract: &'static str,
}

impl WorkspaceDocument {
    pub(super) fn load(mut json: Value, settings_path: &Path) -> Self {
        let database_path = settings_path.with_file_name("state.sqlite3");
        #[cfg(feature = "sqlite-account")]
        let account = match sqlite_persistence::load_document(&database_path) {
            Ok(SqliteAccountDocumentLoad::Missing) => {
                AccountDocument::Json(JsonSelectionReason::DatabaseMissing)
            }
            Ok(SqliteAccountDocumentLoad::Empty) => {
                AccountDocument::Json(JsonSelectionReason::DatabaseEmpty)
            }
            Ok(SqliteAccountDocumentLoad::Loaded(document)) => AccountDocument::Sqlite(document),
            Ok(SqliteAccountDocumentLoad::Incompatible(reason)) => AccountDocument::Blocked(
                format!("{reason}. Reload after Sunrise or Sundial is updated."),
            ),
            Err(error) => AccountDocument::Blocked(format!(
                "Sundial could not safely read state.sqlite3: {error}"
            )),
        };
        #[cfg(not(feature = "sqlite-account"))]
        // PR 88 was closed. A neighboring database is not an upstream account contract.
        let account = AccountDocument::Json(JsonSelectionReason::DatabaseMissing);

        if matches!(account, AccountDocument::Json(_)) {
            ensure_schema_v8_preferences(&mut json);
        }

        Self {
            json,
            database_path,
            account,
        }
    }

    #[cfg(test)]
    pub(super) fn json_only(json: Value) -> Self {
        Self {
            json,
            database_path: PathBuf::from("state.sqlite3"),
            account: AccountDocument::Json(JsonSelectionReason::DatabaseMissing),
        }
    }

    pub(super) const fn json(&self) -> &Value {
        &self.json
    }

    pub(super) fn json_mut(&mut self) -> &mut Value {
        &mut self.json
    }

    pub(super) fn replace_json(&mut self, json: Value) {
        self.json = json;
    }

    pub(super) fn json_changed_from(&self, before: &Self) -> bool {
        self.json != before.json
    }

    pub(super) fn account_changed_from(&self, before: &Self) -> bool {
        match (&self.account, &before.account) {
            #[cfg(feature = "sqlite-account")]
            (AccountDocument::Sqlite(current), AccountDocument::Sqlite(previous)) => {
                current != previous
            }
            _ => false,
        }
    }

    pub(super) fn json_account_changed_from(&self, before: &Self) -> bool {
        if !self.uses_json_account() || !before.uses_json_account() {
            return false;
        }
        const STATE_PATHS: [&str; 4] = [
            "/state/investment",
            "/state/unlocks",
            "/state/activity",
            "/state/characters",
        ];
        if STATE_PATHS
            .iter()
            .any(|path| self.json.pointer(path) != before.json.pointer(path))
        {
            return true;
        }

        let current = self
            .json
            .pointer("/state/account")
            .and_then(Value::as_object);
        let previous = before
            .json
            .pointer("/state/account")
            .and_then(Value::as_object);
        account_members_except_settings(current) != account_members_except_settings(previous)
    }

    pub(super) fn account_change_summaries(&self, before: &Self, limit: usize) -> Vec<String> {
        #[cfg(feature = "sqlite-account")]
        if let (AccountDocument::Sqlite(current), AccountDocument::Sqlite(previous)) =
            (&self.account, &before.account)
        {
            return sqlite_change_summaries(previous, current, limit);
        }
        let _ = (before, limit);
        Vec::new()
    }

    pub(super) fn source_info(&self) -> AccountSourceInfo {
        match &self.account {
            AccountDocument::Json(_) => AccountSourceInfo {
                kind: AccountSourceKind::Json,
                label: "settings.json",
                detail: "Account and settings edits are saved to settings.json.".to_owned(),
                database_path: self.database_path.clone(),
                contract: "JSON schema selected by settings.json version",
            },
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => AccountSourceInfo {
                kind: AccountSourceKind::Sqlite,
                label: "state.sqlite3",
                detail: "Account, character, inventory, equipment, and account-setting edits use state.sqlite3. Player identity and client/server settings remain in settings.json; Sundial does not sync account data between them.".to_owned(),
                database_path: self.database_path.clone(),
                contract: match (
                    document.schema_version(),
                    document.account_format_version(),
                    document.settings_payload_version(),
                ) {
                    (1, 1, 1) => "SQLite schema 1 · account format 1 · settings payload 1",
                    _ => "Unsupported SQLite contract",
                },
            },
            AccountDocument::Blocked(reason) => AccountSourceInfo {
                kind: AccountSourceKind::Blocked,
                label: "Account editing blocked",
                detail: format!(
                    "{reason} Sundial will not fall back to possibly stale JSON account data."
                ),
                database_path: self.database_path.clone(),
                contract: "No compatible SQLite contract selected",
            },
        }
    }

    pub(super) fn uses_json_account(&self) -> bool {
        matches!(self.account, AccountDocument::Json(_))
    }

    pub(super) fn account_editing_blocked(&self) -> Option<&str> {
        match &self.account {
            AccountDocument::Blocked(reason) => Some(reason),
            _ => None,
        }
    }

    pub(super) fn verify_account_source_unchanged(&self) -> Result<(), String> {
        #[cfg(feature = "sqlite-account")]
        if matches!(self.account, AccountDocument::Json(_)) {
            return match sqlite_persistence::load_document(&self.database_path) {
                Ok(SqliteAccountDocumentLoad::Missing | SqliteAccountDocumentLoad::Empty) => Ok(()),
                Ok(SqliteAccountDocumentLoad::Loaded(_)) => Err(
                    "state.sqlite3 became authoritative after this workspace loaded. Reload before saving so account edits are not written to inactive JSON data"
                        .to_owned(),
                ),
                Ok(SqliteAccountDocumentLoad::Incompatible(reason)) => Err(format!(
                    "state.sqlite3 appeared or changed after this workspace loaded, but its contract is incompatible: {reason}. Reload before saving"
                )),
                Err(error) => Err(format!(
                    "state.sqlite3 appeared or changed after this workspace loaded and could not be read safely: {error}. Reload before saving"
                )),
            };
        }
        Ok(())
    }

    #[cfg(feature = "sqlite-account")]
    pub(super) fn save_sqlite(&mut self) -> Result<SqliteSaveReceipt, String> {
        match &mut self.account {
            AccountDocument::Sqlite(document) => {
                sqlite_persistence::save_document(document).map_err(|error| error.to_string())
            }
            AccountDocument::Json(_) => {
                Err("internal error: the selected account source is settings.json".to_owned())
            }
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    #[cfg(feature = "sqlite-account")]
    pub(super) fn restore_sqlite_backup(&self, backup: &Path) -> Result<(), String> {
        sqlite_persistence::restore_backup(&self.database_path, backup)
            .map_err(|error| error.to_string())
    }

    #[cfg(feature = "sqlite-account")]
    pub(super) fn validate_sqlite_backup(&self, backup: &Path) -> Result<(), String> {
        sqlite_persistence::validate_backup(backup).map_err(|error| error.to_string())
    }

    #[cfg(feature = "sqlite-account")]
    pub(super) fn restore_sqlite_backup_safely(&self, backup: &Path) -> Result<PathBuf, String> {
        sqlite_persistence::restore_backup_safely(&self.database_path, backup)
            .map(|receipt| receipt.safety_backup)
            .map_err(|error| error.to_string())
    }

    pub(super) fn rebase_account_revision_from(&mut self, _source: &Self) {
        #[cfg(feature = "sqlite-account")]
        if let (AccountDocument::Sqlite(current), AccountDocument::Sqlite(source)) =
            (&mut self.account, &_source.account)
        {
            current.adopt_revision_from(source);
        }
    }
}

impl Deref for WorkspaceDocument {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.json
    }
}

impl Default for WorkspaceDocument {
    fn default() -> Self {
        Self {
            json: Value::Null,
            database_path: PathBuf::from("state.sqlite3"),
            account: AccountDocument::Json(JsonSelectionReason::DatabaseMissing),
        }
    }
}

fn blocked_string(document: &WorkspaceDocument) -> String {
    document
        .account_editing_blocked()
        .unwrap_or("Account source is unavailable")
        .to_owned()
}

fn blocked_inventory(document: &WorkspaceDocument) -> InventoryError {
    InventoryError::new("state.sqlite3", blocked_string(document))
}

pub(super) fn character_count(document: &WorkspaceDocument) -> usize {
    match &document.account {
        AccountDocument::Json(_) => document
            .json
            .pointer("/state/characters")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(sqlite) => sqlite.characters().characters().len(),
        AccountDocument::Blocked(_) => 0,
    }
}

pub(super) fn character_metadata(
    document: &WorkspaceDocument,
    character_index: usize,
) -> Result<CharacterMetadata, String> {
    match &document.account {
        AccountDocument::Json(_) => {
            let adapter =
                JsonCharacterAdapter::load_character_metadata(&document.json, character_index)
                    .map_err(|error| error.to_string())?;
            let character_id = adapter
                .character_id_at_index(character_index)
                .ok_or_else(|| format!("Character {} does not exist", character_index + 1))?;
            adapter
                .state()
                .characters()
                .iter()
                .find(|character| character.id == character_id)
                .and_then(|character| character.metadata)
                .ok_or_else(|| format!("Character {} metadata was not loaded", character_index + 1))
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => sqlite::character_metadata(document, character_index),
        AccountDocument::Blocked(_) => Err(blocked_string(document)),
    }
}

pub(super) fn class_armor_default_characters(document: &WorkspaceDocument) -> HashMap<u64, usize> {
    match &document.account {
        AccountDocument::Json(_) => {
            super::equipment::collect_class_armor_default_characters(&document.json)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => sqlite::class_armor_default_characters(document),
        AccountDocument::Blocked(_) => HashMap::new(),
    }
}

pub(super) fn apply_character_updates(
    document: &mut WorkspaceDocument,
    character_index: usize,
    updates: Vec<CharacterMetadataUpdate>,
) -> Result<bool, String> {
    match &mut document.account {
        AccountDocument::Json(_) => {
            character_metadata::apply_updates(&mut document.json, character_index, updates)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::apply_character_updates(document, character_index, updates)
        }
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(super) fn apply_account_settings(
    document: &mut WorkspaceDocument,
    commands: Vec<AccountSettingsCommand>,
) -> Result<bool, String> {
    match &mut document.account {
        AccountDocument::Json(_) => account_settings::apply_commands(&mut document.json, commands),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => sqlite::apply_account_settings(document, commands),
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(super) fn account_settings_map(
    document: &WorkspaceDocument,
) -> Result<Map<String, Value>, String> {
    match &document.account {
        AccountDocument::Json(_) => document
            .json
            .pointer("/state/account/settings")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| "This settings.json has no state.account.settings object.".to_owned()),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => Ok(settings_map(document.settings().values())),
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(super) fn named_key_bindings_editable(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json(_) => crate::game_settings::key_bindings_editable(&document.json),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(_) => false,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn supports_combined_dismantle_gear_class(_document: &WorkspaceDocument) -> bool {
    #[cfg(feature = "sqlite-account")]
    if matches!(_document.account, AccountDocument::Sqlite(_)) {
        return true;
    }
    false
}

pub(super) fn can_mutate_equipment(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json(_) => {
            super::inventory::schema_mode(&document.json).can_mutate_equipment()
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn can_mutate_character_inventory(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json(_) => {
            super::inventory::schema_mode(&document.json).can_mutate_character_inventory()
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn can_mutate_equipment_flags(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json(_) => {
            super::inventory::schema_mode(&document.json).can_mutate_equipment_flags()
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn character_soid(document: &WorkspaceDocument, character_index: usize) -> Option<u64> {
    match &document.account {
        AccountDocument::Json(_) => document
            .json
            .pointer("/state/characters")
            .and_then(Value::as_array)
            .and_then(|characters| characters.get(character_index))
            .and_then(|character| character.get("soid"))
            .and_then(crate::hash::parse_unsigned_value),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => document
            .characters()
            .characters()
            .get(character_index)
            .and_then(|character| character.soid)
            .map(sundial_account::InstanceSoid::get),
        AccountDocument::Blocked(_) => None,
    }
}

pub(super) fn character_inventory_capacity(_document: &WorkspaceDocument) -> usize {
    #[cfg(feature = "sqlite-account")]
    if matches!(_document.account, AccountDocument::Sqlite(_)) {
        return SqliteAccountDocument::character_capabilities()
            .inventory_capacity
            .unwrap_or(super::inventory::CHARACTER_INVENTORY_CAPACITY);
    }
    super::inventory::CHARACTER_INVENTORY_CAPACITY
}

pub(super) fn profile_items_editable(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json(_) => {
            super::inventory::schema_mode(&document.json).can_mutate_profile_items()
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn profile_item_capacity(document: &WorkspaceDocument) -> Option<usize> {
    #[cfg(feature = "sqlite-account")]
    if matches!(document.account, AccountDocument::Sqlite(_)) {
        return SqliteAccountDocument::profile_capabilities().profile_item_capacity;
    }
    super::inventory::schema_mode(&document.json).profile_item_capacity()
}

pub(super) fn dismantle_rewards_available(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json(_) => {
            let mode = super::inventory::schema_mode(&document.json);
            mode.supports_dismantle_rewards() && !mode.is_future()
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn dismantle_rewards_editable(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json(_) => {
            super::inventory::schema_mode(&document.json).can_mutate_dismantle_rewards()
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn dismantle_reward_capacity(document: &WorkspaceDocument) -> Option<usize> {
    #[cfg(feature = "sqlite-account")]
    if matches!(document.account, AccountDocument::Sqlite(_)) {
        return SqliteAccountDocument::profile_capabilities().dismantle_reward_capacity;
    }
    super::inventory::schema_mode(&document.json).dismantle_reward_capacity()
}

pub(super) fn filtered_dismantle_rewards(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json(_) => {
            super::inventory::schema_mode(&document.json).supports_filtered_dismantle_rewards()
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn account_collection_ready(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json(_) => {
            super::inventory::profile_item_target_exists(&document.json).unwrap_or(false)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

#[cfg(feature = "sqlite-account")]
fn settings_map(
    values: &std::collections::BTreeMap<AccountSettingKey, AccountSettingValue>,
) -> Map<String, Value> {
    let mut settings = Map::new();
    for (key, value) in values {
        match key {
            AccountSettingKey::Preference { group, name } => {
                let value = setting_value(value);
                match setting_group_name(*group) {
                    None => {
                        settings.insert(name.to_string(), value);
                    }
                    Some(group) => {
                        settings
                            .entry(group)
                            .or_insert_with(|| Value::Object(Map::new()))
                            .as_object_mut()
                            .expect("settings groups are objects")
                            .insert(name.to_string(), value);
                    }
                }
            }
            AccountSettingKey::KeyBinding { action, slot } => {
                let bindings = settings
                    .entry("key_bindings".to_owned())
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .expect("key bindings is an object");
                bindings
                    .entry(action.to_string())
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .expect("a key binding is an object")
                    .insert(
                        match slot {
                            KeyBindingSlot::Primary => "primary",
                            KeyBindingSlot::Secondary => "secondary",
                        }
                        .to_owned(),
                        setting_value(value),
                    );
            }
        }
    }
    settings
}

#[cfg(feature = "sqlite-account")]
const fn setting_group_name(group: AccountSettingGroup) -> Option<&'static str> {
    match group {
        AccountSettingGroup::Root => None,
        AccountSettingGroup::Controls => Some("controls"),
        AccountSettingGroup::Audio => Some("audio"),
        AccountSettingGroup::Display => Some("display"),
        AccountSettingGroup::Interface => Some("interface"),
        AccountSettingGroup::Social => Some("social"),
    }
}

#[cfg(feature = "sqlite-account")]
fn setting_value(value: &AccountSettingValue) -> Value {
    match value {
        AccountSettingValue::Boolean(value) => Value::Bool(*value),
        AccountSettingValue::Unsigned(value) => Value::Number(Number::from(*value)),
        AccountSettingValue::Decimal(value) => Number::from_f64(value.get())
            .map(Value::Number)
            .unwrap_or(Value::Null),
        AccountSettingValue::Text(value) => Value::String(value.to_string()),
        AccountSettingValue::InputCode(value) => Value::Number(Number::from(*value)),
        AccountSettingValue::Unassigned => Value::Null,
    }
}

#[cfg(test)]
mod tests;
