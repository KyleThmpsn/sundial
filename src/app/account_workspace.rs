//! Workspace-level account source selection and operation routing.
//!
//! JSON and SQLite remain independent persistence adapters. A workspace selects one account
//! source when it loads and never falls back after selection.

mod change_summary;
mod equipment_dispatch;
mod inventory_dispatch;
pub(super) use equipment_dispatch::*;
pub(super) use inventory_dispatch::*;
mod sqlite;

use std::{
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
};

use serde_json::{Map, Number, Value};
use sundial_account::{
    AccountSettingGroup, AccountSettingKey, AccountSettingValue, AccountSettingsCommand,
    CharacterMetadata, CharacterMetadataUpdate, KeyBindingSlot,
};

use change_summary::{account_members_except_settings, sqlite_change_summaries};

use super::equipment::{EquippedItemPlugs, EquippedItemSnapshot, EquippedPlugValue};
use super::inventory::{
    DismantleGearClass, DismantleRarity, DismantleRewardAction, DismantleRewardLocation,
    DismantleRewardSnapshot, InventoryError, InventoryItemAction, InventoryItemLocation,
    InventoryItemSnapshot, ItemPlugs, NewInventoryItem, ProfileItemAction, ProfileItemLocation,
    ProfileItemSnapshot,
};
use super::{account_settings, character_metadata};
use crate::persistence::json_account::{JsonCharacterAdapter, ensure_schema_v8_preferences};
use crate::persistence::sqlite_account::{
    self as sqlite_persistence, SqliteAccountDocument, SqliteAccountDocumentLoad, SqliteSaveReceipt,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AccountSourceKind {
    Json,
    Sqlite,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AccountDocument {
    Json,
    Sqlite(Box<SqliteAccountDocument>),
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
    pub database_path: PathBuf,
    pub contract: &'static str,
}

impl WorkspaceDocument {
    pub(super) fn native_account(&self) -> Option<&SqliteAccountDocument> {
        match &self.account {
            AccountDocument::Sqlite(document) => Some(document),
            _ => None,
        }
    }
    pub(super) fn native_account_mut(&mut self) -> Option<&mut SqliteAccountDocument> {
        match &mut self.account {
            AccountDocument::Sqlite(document) => Some(document),
            _ => None,
        }
    }
    pub(super) fn runtime_view(&self) -> Value {
        if let AccountDocument::Sqlite(document) = &self.account {
            let mut view = self.json.clone();
            if view.get("server").is_none_or(Value::is_null) {
                view["server"] = serde_json::json!({});
            }
            if view["server"].is_object() {
                view["server"]["entitlements"] = document.entitlements().clone();
            }
            if view.get("state").is_none_or(Value::is_null) {
                view["state"] = serde_json::json!({});
            }
            if view["state"].is_object() {
                view["state"]["account"] = document.runtime()["account"].clone();
                view["state"]["characters"] = document.runtime()["characters"].clone();
            }
            return view;
        }
        self.json.clone()
    }
    pub(super) fn apply_runtime_view(&mut self, mut view: Value) -> Result<(), String> {
        if let AccountDocument::Sqlite(document) = &mut self.account {
            let native = serde_json::json!({
                "account": view["state"]["account"],
                "characters": view["state"]["characters"],
            });
            if let Some(state) = view.get_mut("state").and_then(Value::as_object_mut) {
                for key in ["account", "characters"] {
                    match self.json.pointer(&format!("/state/{key}")) {
                        Some(value) => {
                            state.insert(key.into(), value.clone());
                        }
                        None => {
                            state.remove(key);
                        }
                    }
                }
                if state.is_empty() && self.json.get("state").is_none() {
                    view.as_object_mut().unwrap().remove("state");
                }
            }
            document.set_runtime(native);
            let entitlements = view
                .pointer("/server/entitlements")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            if let Some(server) = view.get_mut("server").and_then(Value::as_object_mut) {
                match self.json.pointer("/server/entitlements") {
                    Some(value) => {
                        server.insert("entitlements".into(), value.clone());
                    }
                    None => {
                        server.remove("entitlements");
                    }
                }
                if server.is_empty() && self.json.get("server").is_none() {
                    view.as_object_mut().unwrap().remove("server");
                }
            }
            document.set_entitlements(entitlements);
        }
        self.json = view;
        Ok(())
    }

    pub(super) fn progression_view(&self, index: usize) -> Value {
        if let AccountDocument::Sqlite(document) = &self.account {
            return document.progression_view(index);
        }
        self.json.clone()
    }
    pub(super) fn apply_progression_view(
        &mut self,
        index: usize,
        value: Value,
    ) -> Result<(), String> {
        super::progression::validate(&value)?;
        match &mut self.account {
            AccountDocument::Sqlite(document) => document
                .apply_progression_view(index, &value)
                .map_err(|e| e.to_string()),
            AccountDocument::Json => {
                self.json = value;
                Ok(())
            }
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    pub(super) fn load(mut json: Value, settings_path: &Path) -> Self {
        let database_path = crate::persistence::investment_path(settings_path);
        let account = if !requires_sqlite(&json) {
            AccountDocument::Json
        } else {
            match sqlite_persistence::load_document(&database_path) {
            Ok(SqliteAccountDocumentLoad::Missing) => {
                AccountDocument::Blocked(
                    "Settings v18 requires data/investment.sqlite3. Start Sunrise to initialize it, then reload.".into(),
                )
            }
            Ok(SqliteAccountDocumentLoad::Empty) => AccountDocument::Blocked(
                "The Sunrise database is empty or uninitialized. Start Sunrise to initialize it, then reload.".into(),
            ),
            Ok(SqliteAccountDocumentLoad::Loaded(document)) => AccountDocument::Sqlite(document),
            Ok(SqliteAccountDocumentLoad::Incompatible(reason)) => AccountDocument::Blocked(
                format!("{reason}. Reload after Sunrise or Sundial is updated."),
            ),
            Err(error) => AccountDocument::Blocked(format!(
                "Sundial could not safely read investment.sqlite3: {error}"
            )),
        }
        };

        if matches!(account, AccountDocument::Json) {
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
            database_path: PathBuf::from("investment.sqlite3"),
            account: AccountDocument::Json,
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
        if let (AccountDocument::Sqlite(current), AccountDocument::Sqlite(previous)) =
            (&self.account, &before.account)
        {
            return sqlite_change_summaries(previous, current, limit);
        }
        Vec::new()
    }

    pub(super) fn source_info(&self) -> AccountSourceInfo {
        match &self.account {
            AccountDocument::Json => AccountSourceInfo {
                kind: AccountSourceKind::Json,
                label: "settings.json",
                detail: "Account and settings edits are saved to settings.json.".to_owned(),
                database_path: self.database_path.clone(),
                contract: "JSON schema selected by settings.json version",
            },
            AccountDocument::Sqlite(_) => AccountSourceInfo {
                kind: AccountSourceKind::Sqlite,
                label: "investment.sqlite3",
                detail: "Characters, inventory, equipment, preferences, ownership and progression use investment.sqlite3. Player identity and runtime configuration use settings.json.".to_owned(),
                database_path: self.database_path.clone(),
                contract: "SQLite · Schema 2",
            },
            AccountDocument::Blocked(reason) => AccountSourceInfo {
                kind: AccountSourceKind::Blocked,
                label: "Account Editing Blocked",
                detail: format!(
                    "{reason} Sundial will not fall back to possibly stale JSON account data."
                ),
                database_path: self.database_path.clone(),
                contract: "No compatible SQLite contract selected",
            },
        }
    }

    pub(super) fn uses_json_account(&self) -> bool {
        matches!(self.account, AccountDocument::Json)
    }

    pub(super) fn account_editing_blocked(&self) -> Option<&str> {
        match &self.account {
            AccountDocument::Blocked(reason) => Some(reason),
            _ => None,
        }
    }

    pub(super) fn verify_account_source_unchanged(&self) -> Result<(), String> {
        if self.uses_json_account() == requires_sqlite(&self.json) {
            return Err(
                "The settings schema changed its account source. Reload before saving".into(),
            );
        }

        Ok(())
    }

    pub(super) fn save_sqlite(&mut self) -> Result<SqliteSaveReceipt, String> {
        match &mut self.account {
            AccountDocument::Sqlite(document) => {
                sqlite_persistence::save_document(document).map_err(|error| error.to_string())
            }
            AccountDocument::Json => {
                Err("internal error: the selected account source is settings.json".to_owned())
            }
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    pub(super) fn rollback_sqlite_save(&self, receipt: &SqliteSaveReceipt) -> Result<(), String> {
        sqlite_persistence::rollback_save(&self.database_path, receipt)
            .map_err(|error| error.to_string())
    }

    pub(super) fn validate_sqlite_backup(&self, backup: &Path) -> Result<(), String> {
        sqlite_persistence::validate_backup(backup).map_err(|error| error.to_string())
    }

    pub(super) fn restore_sqlite_backup_safely(&self, backup: &Path) -> Result<PathBuf, String> {
        sqlite_persistence::restore_backup_safely(&self.database_path, backup)
            .map(|receipt| receipt.safety_backup)
            .map_err(|error| error.to_string())
    }

    pub(super) fn rebase_account_revision_from(&mut self, source: &Self) {
        if let (AccountDocument::Sqlite(current), AccountDocument::Sqlite(source)) =
            (&mut self.account, &source.account)
        {
            current.adopt_revision_from(source);
        }
    }
}

pub(super) fn requires_sqlite(json: &Value) -> bool {
    crate::game_settings::requires_sqlite_account(json)
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
            database_path: PathBuf::from("investment.sqlite3"),
            account: AccountDocument::Json,
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
    InventoryError::new("investment.sqlite3", blocked_string(document))
}

pub(super) fn character_count(document: &WorkspaceDocument) -> usize {
    match &document.account {
        AccountDocument::Json => document
            .json
            .pointer("/state/characters")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        AccountDocument::Sqlite(sqlite) => sqlite.characters().characters().len(),
        AccountDocument::Blocked(_) => 0,
    }
}

pub(super) fn character_metadata(
    document: &WorkspaceDocument,
    character_index: usize,
) -> Result<CharacterMetadata, String> {
    match &document.account {
        AccountDocument::Json => {
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
        AccountDocument::Sqlite(document) => sqlite::character_metadata(document, character_index),
        AccountDocument::Blocked(_) => Err(blocked_string(document)),
    }
}

pub(super) fn class_armor_default_characters(document: &WorkspaceDocument) -> HashMap<u64, usize> {
    match &document.account {
        AccountDocument::Json => {
            super::equipment::collect_class_armor_default_characters(&document.json)
        }
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
        AccountDocument::Json => {
            character_metadata::apply_updates(&mut document.json, character_index, updates)
        }
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
        AccountDocument::Json => account_settings::apply_commands(&mut document.json, commands),
        AccountDocument::Sqlite(document) => sqlite::apply_account_settings(document, commands),
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(super) fn account_settings_map(
    document: &WorkspaceDocument,
) -> Result<Map<String, Value>, String> {
    match &document.account {
        AccountDocument::Json => document
            .json
            .pointer("/state/account/settings")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| "This settings.json has no state.account.settings object.".to_owned()),
        AccountDocument::Sqlite(document) => Ok(settings_map(document.settings().values())),
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(super) fn named_key_bindings_editable(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => crate::game_settings::key_bindings_editable(&document.json),
        AccountDocument::Sqlite(_) => false,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn supports_combined_dismantle_gear_class(_document: &WorkspaceDocument) -> bool {
    if matches!(_document.account, AccountDocument::Sqlite(_)) {
        return true;
    }
    false
}

pub(super) fn can_mutate_equipment(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_equipment()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn can_mutate_character_inventory(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_character_inventory()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn can_mutate_equipment_flags(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_equipment_flags()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn character_soid(document: &WorkspaceDocument, character_index: usize) -> Option<u64> {
    match &document.account {
        AccountDocument::Json => document
            .json
            .pointer("/state/characters")
            .and_then(Value::as_array)
            .and_then(|characters| characters.get(character_index))
            .and_then(|character| character.get("soid"))
            .and_then(crate::hash::parse_unsigned_value),
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
    if matches!(_document.account, AccountDocument::Sqlite(_)) {
        return SqliteAccountDocument::character_capabilities()
            .inventory_capacity
            .unwrap_or(super::inventory::CHARACTER_INVENTORY_CAPACITY);
    }
    super::inventory::CHARACTER_INVENTORY_CAPACITY
}

pub(super) fn profile_items_editable(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_profile_items()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn profile_item_capacity(document: &WorkspaceDocument) -> Option<usize> {
    if matches!(document.account, AccountDocument::Sqlite(_)) {
        return SqliteAccountDocument::profile_capabilities().profile_item_capacity;
    }
    super::inventory::schema_mode(&document.json).profile_item_capacity()
}

pub(super) fn dismantle_rewards_available(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            let mode = super::inventory::schema_mode(&document.json);
            mode.supports_dismantle_rewards() && !mode.is_future()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn dismantle_rewards_editable(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_dismantle_rewards()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn dismantle_reward_capacity(document: &WorkspaceDocument) -> Option<usize> {
    if matches!(document.account, AccountDocument::Sqlite(_)) {
        return SqliteAccountDocument::profile_capabilities().dismantle_reward_capacity;
    }
    super::inventory::schema_mode(&document.json).dismantle_reward_capacity()
}

pub(super) fn filtered_dismantle_rewards(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).supports_filtered_dismantle_rewards()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn account_collection_ready(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::profile_item_target_exists(&document.json).unwrap_or(false)
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

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
