//! Workspace-level account source selection and operation routing.
//!
//! JSON and SQLite remain independent persistence adapters. A workspace selects one account
//! source when it loads and never falls back after selection.

mod change_summary;
mod equipment_dispatch;
mod inventory_dispatch;
mod runtime;
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
    AccountSettingKey, AccountSettingValue, AccountSettingsCommand, CharacterMetadata,
    CharacterMetadataUpdate, KeyBindingSlot,
};

use change_summary::{account_members_except_settings, sqlite_change_summaries};

use super::equipment::{EquippedItemPlugs, EquippedItemSnapshot, EquippedPlugValue};
use super::inventory::{
    DismantleRewardAction, DismantleRewardLocation, DismantleRewardSnapshot, InventoryError,
    InventoryItemAction, InventoryItemLocation, InventoryItemSnapshot, ItemPlugs, NewInventoryItem,
    ProfileItemAction, ProfileItemLocation, ProfileItemSnapshot,
};
use super::{account_settings, character_metadata};
use crate::persistence::dawn_account::{
    self as dawn_persistence, DawnAccountDocument, DawnAccountDocumentLoad,
};
use crate::persistence::json_account::{
    JsonCharacterAdapter, ensure_schema_v8_preferences, setting_group_name,
};
use crate::persistence::sqlite_account::{
    self as sqlite_persistence, SqliteAccountDocument, SqliteAccountDocumentLoad, SqliteSaveReceipt,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AccountSourceKind {
    Json,
    Sqlite,
    /// A Dawn runtime keeps its account in player-state.db beside the settings file.
    Dawn,
    Blocked,
}

/// Not `Eq`: a Dawn document carries `characters.appearance`, which is a float.
#[derive(Clone, Debug, PartialEq)]
enum AccountDocument {
    Json,
    Sqlite(Box<SqliteAccountDocument>),
    Dawn(Box<DawnAccountDocument>),
    Blocked(String),
}

/// Not `Eq`: see [`AccountDocument`].
#[derive(Clone, Debug, PartialEq)]
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

impl super::SundialApp {
    /// True when the installed runtime is Dawn, which keeps its account in player-state.db.
    pub(super) fn dawn_account_runtime(&self) -> bool {
        self.runtime_choice
            .inspection
            .launch_copy()
            .is_some_and(|copy| copy.dawn)
    }
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
    pub(super) fn progression_view(&self, index: usize) -> Value {
        match &self.account {
            AccountDocument::Sqlite(document) => document.progression_view(index),
            // Dawn keeps progression in player-state.db durable_flags, and its settings.json is the
            // seed it consumed on first boot. Serving that seed here presented values Dawn stopped
            // reading long ago as if they were live, and an edit to them was only refused after the
            // fact.
            AccountDocument::Dawn(_) => Value::Null,
            _ => self.json.clone(),
        }
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
            AccountDocument::Dawn(_) => Err(
                "Dawn keeps progression in player-state.db durable flags. Sundial writes those only through authored collection unlocks."
                    .to_owned(),
            ),
            AccountDocument::Json => {
                self.json = value;
                Ok(())
            }
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    fn load_dawn(json: Value, settings_path: &Path) -> Self {
        let database_path = crate::persistence::dawn_path(settings_path);
        let account = match dawn_persistence::load(&database_path) {
            Ok(DawnAccountDocumentLoad::Loaded(document)) => AccountDocument::Dawn(document),
            Ok(DawnAccountDocumentLoad::Missing) => AccountDocument::Blocked(
                "Dawn has not created player-state.db yet. Start Dawn once to import settings.json, then reload.".into(),
            ),
            Ok(DawnAccountDocumentLoad::Empty) => AccountDocument::Blocked(
                "player-state.db is empty or uninitialized. Start Dawn to initialize it, then reload.".into(),
            ),
            Ok(DawnAccountDocumentLoad::Incompatible(reason)) => AccountDocument::Blocked(format!(
                "{reason}. Reload after Dawn or Sundial is updated."
            )),
            Err(error) => AccountDocument::Blocked(format!(
                "Sundial could not safely read player-state.db: {error}"
            )),
        };
        Self {
            json,
            database_path,
            account,
        }
    }

    /// Selects the account source once, from positive runtime detection rather than from JSON.
    ///
    /// A Dawn install keeps its account in player-state.db beside settings.json, so the settings
    /// schema alone cannot choose the source. Dawn creates that database on its first boot by
    /// importing settings.json, and Sundial never creates or seeds it.
    pub(super) fn load(mut json: Value, settings_path: &Path, dawn: bool) -> Self {
        if dawn {
            return Self::load_dawn(json, settings_path);
        }
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
            // Dawn keeps its account in player-state.db, so an edit there is a change even though
            // settings.json never moves. Without this arm no Dawn edit was ever offered for saving.
            (AccountDocument::Dawn(current), AccountDocument::Dawn(previous)) => {
                current.differs_from(previous)
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

    pub(super) fn source_kind(&self) -> AccountSourceKind {
        match self.account {
            AccountDocument::Json => AccountSourceKind::Json,
            AccountDocument::Sqlite(_) => AccountSourceKind::Sqlite,
            AccountDocument::Dawn(_) => AccountSourceKind::Dawn,
            AccountDocument::Blocked(_) => AccountSourceKind::Blocked,
        }
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
            AccountDocument::Dawn(_) => AccountSourceInfo {
                kind: AccountSourceKind::Dawn,
                label: "player-state.db",
                detail: "This install runs Dawn, which keeps characters, inventory, equipment and preferences in player-state.db beside settings.json. Dawn reads settings.json once when it first creates that database and never again, so account edits go to player-state.db.".to_owned(),
                database_path: self.database_path.clone(),
                contract: DAWN_CONTRACT,
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

    /// Whether this workspace's account belongs to Dawn, including one that failed to load.
    ///
    /// A Dawn database that is missing, empty or of an unsupported schema becomes `Blocked`, which
    /// on its own is indistinguishable from a blocked Sunrise account. The recovery controls act on
    /// `database_path`, so they have to tell the two apart or they aim Sunrise's tools at
    /// player-state.db.
    pub(super) fn account_is_dawn(&self) -> bool {
        matches!(self.account, AccountDocument::Dawn(_))
            || self
                .database_path
                .file_name()
                .is_some_and(|name| name == crate::persistence::DAWN_DATABASE_NAME)
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
        // Dawn's account source is decided by detecting the installed runtime, and its account
        // lives in player-state.db whatever settings.json says. The test below is Sunrise's own
        // move from JSON accounts to data/investment.sqlite3 at settings v18, which never applied
        // to Dawn: running it against a Dawn workspace refused every save.
        if matches!(self.account, AccountDocument::Dawn(_)) {
            return Ok(());
        }
        if self.uses_json_account() == requires_sqlite(&self.json) {
            return Err(
                "The settings schema changed its account source. Reload before applying or saving changes".into(),
            );
        }

        Ok(())
    }

    pub(super) fn save_sqlite(&mut self) -> Result<SqliteSaveReceipt, String> {
        match &mut self.account {
            AccountDocument::Sqlite(document) => {
                sqlite_persistence::save_document(document).map_err(|error| error.to_string())
            }
            AccountDocument::Dawn(_) => {
                Err("internal error: a Dawn account is saved through player-state.db".to_owned())
            }
            AccountDocument::Json => {
                Err("internal error: the selected account source is settings.json".to_owned())
            }
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    /// Writes the edited account through the adapter that owns its file.
    pub(super) fn save_account(&mut self) -> Result<AccountSaveReceipt, String> {
        match &self.account {
            AccountDocument::Dawn(_) => self.save_dawn().map(AccountSaveReceipt::Dawn),
            _ => self.save_sqlite().map(AccountSaveReceipt::Sqlite),
        }
    }

    /// Puts back whichever account file the matching save wrote.
    pub(super) fn rollback_account_save(&self, receipt: &AccountSaveReceipt) -> Result<(), String> {
        match receipt {
            AccountSaveReceipt::Sqlite(receipt) => self.rollback_sqlite_save(receipt),
            AccountSaveReceipt::Dawn(receipt) => {
                dawn_persistence::restore_backup(&self.database_path, &receipt.backup)
                    .map_err(|error| error.to_string())
            }
        }
    }

    /// Writes an edited Dawn account back to player-state.db.
    pub(super) fn save_dawn(&mut self) -> Result<dawn_persistence::DawnSaveReceipt, String> {
        match &mut self.account {
            AccountDocument::Dawn(document) => {
                dawn_persistence::save(document).map_err(|error| error.to_string())
            }
            _ => {
                Err("internal error: the selected account source is not player-state.db".to_owned())
            }
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
        match (&mut self.account, &source.account) {
            (AccountDocument::Sqlite(current), AccountDocument::Sqlite(source)) => {
                current.adopt_revision_from(source);
            }
            // Dawn guards every commit with a compare and swap on account_revision. After a
            // rollback restores the verified copy, the revision on disk is the restored one, so
            // the held document has to adopt it or no further save can ever match.
            (AccountDocument::Dawn(current), AccountDocument::Dawn(source)) => {
                current.adopt_revision(source);
            }
            _ => {}
        }
    }
}

/// The schema this build accepts. `dawn_contract_names_the_supported_schema` keeps it honest.
const DAWN_CONTRACT: &str = "SQLite · Dawn schema 5";

pub(super) use crate::game_settings::requires_sqlite_account as requires_sqlite;

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
        AccountDocument::Dawn(dawn) => dawn.characters().characters().len(),
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
        AccountDocument::Dawn(document) => sqlite::character_metadata(document, character_index),
        AccountDocument::Blocked(_) => Err(blocked_string(document)),
    }
}

pub(super) fn class_armor_default_characters(document: &WorkspaceDocument) -> HashMap<u64, usize> {
    match &document.account {
        AccountDocument::Json => {
            super::equipment::collect_class_armor_default_characters(&document.json)
        }
        AccountDocument::Sqlite(document) => sqlite::class_armor_default_characters(document),
        AccountDocument::Dawn(document) => sqlite::class_armor_default_characters(document),
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
        AccountDocument::Dawn(document) => {
            sqlite::apply_character_updates(document, character_index, updates)
        }
    }
}

pub(super) fn apply_account_settings(
    document: &mut WorkspaceDocument,
    commands: Vec<AccountSettingsCommand>,
) -> Result<bool, String> {
    match &mut document.account {
        AccountDocument::Json => account_settings::apply_commands(&mut document.json, commands),
        AccountDocument::Sqlite(document) => sqlite::apply_account_settings(document, commands),
        AccountDocument::Dawn(document) => sqlite::apply_account_settings(document, commands),
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
        AccountDocument::Dawn(document) => Ok(settings_map(document.settings().values())),
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(super) fn named_key_bindings_editable(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => crate::game_settings::key_bindings_editable(&document.json),
        // Dawn stores key bindings by numeric action, the same as a native Sunrise account.
        AccountDocument::Sqlite(_) | AccountDocument::Dawn(_) | AccountDocument::Blocked(_) => {
            false
        }
    }
}

pub(super) fn supports_combined_dismantle_gear_class(document: &WorkspaceDocument) -> bool {
    matches!(document.account, AccountDocument::Sqlite(_))
}

pub(super) fn can_mutate_equipment(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_equipment()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Dawn(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn can_mutate_character_inventory(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_character_inventory()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Dawn(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn can_mutate_equipment_flags(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_equipment_flags()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Dawn(_) => true,
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
        AccountDocument::Dawn(document) => document
            .characters()
            .characters()
            .get(character_index)
            .and_then(|character| character.soid)
            .map(sundial_account::InstanceSoid::get),
        AccountDocument::Blocked(_) => None,
    }
}

pub(super) fn character_inventory_capacity(document: &WorkspaceDocument) -> usize {
    match &document.account {
        AccountDocument::Sqlite(_) => SqliteAccountDocument::character_capabilities()
            .inventory_capacity
            .unwrap_or(super::inventory::CHARACTER_INVENTORY_CAPACITY),
        // Dawn's limit is compiled into Dawn, not derived from a settings schema.
        AccountDocument::Dawn(_) => DawnAccountDocument::character_capabilities()
            .inventory_capacity
            .unwrap_or(super::inventory::CHARACTER_INVENTORY_CAPACITY),
        _ => super::inventory::CHARACTER_INVENTORY_CAPACITY,
    }
}

pub(super) fn profile_items_editable(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_profile_items()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Dawn(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn profile_item_capacity(document: &WorkspaceDocument) -> Option<usize> {
    match &document.account {
        AccountDocument::Sqlite(_) => {
            SqliteAccountDocument::profile_capabilities().profile_item_capacity
        }
        AccountDocument::Dawn(_) => {
            DawnAccountDocument::profile_capabilities().profile_item_capacity
        }
        _ => super::inventory::schema_mode(&document.json).profile_item_capacity(),
    }
}

pub(super) fn dismantle_rewards_available(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            let mode = super::inventory::schema_mode(&document.json);
            mode.supports_dismantle_rewards() && !mode.is_future()
        }
        AccountDocument::Sqlite(_) => true,
        // Dawn keeps dismantle rewards, but this build does not read them, so presenting the
        // section would state "none" as fact about an account it has not looked at.
        AccountDocument::Dawn(_) => false,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn dismantle_rewards_editable(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).can_mutate_dismantle_rewards()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Dawn(_) => {
            DawnAccountDocument::profile_capabilities().dismantle_rewards_writable
        }
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn dismantle_reward_capacity(document: &WorkspaceDocument) -> Option<usize> {
    match &document.account {
        AccountDocument::Sqlite(_) => {
            SqliteAccountDocument::profile_capabilities().dismantle_reward_capacity
        }
        AccountDocument::Dawn(_) => {
            DawnAccountDocument::profile_capabilities().dismantle_reward_capacity
        }
        _ => super::inventory::schema_mode(&document.json).dismantle_reward_capacity(),
    }
}

pub(super) fn filtered_dismantle_rewards(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::schema_mode(&document.json).supports_filtered_dismantle_rewards()
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Dawn(_) => true,
        AccountDocument::Blocked(_) => false,
    }
}

pub(super) fn account_collection_ready(document: &WorkspaceDocument) -> bool {
    match &document.account {
        AccountDocument::Json => {
            super::inventory::profile_item_target_exists(&document.json).unwrap_or(false)
        }
        AccountDocument::Sqlite(_) => true,
        AccountDocument::Dawn(_) => true,
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

fn setting_value(value: &AccountSettingValue) -> Value {
    match value {
        AccountSettingValue::Boolean(value) => Value::Bool(*value),
        AccountSettingValue::Unsigned(value) => Value::Number(Number::from(*value)),
        AccountSettingValue::Decimal(value) => {
            Number::from_f64(value.get()).map_or(Value::Null, Value::Number)
        }
        AccountSettingValue::Text(value) => Value::String(value.to_string()),
        AccountSettingValue::InputCode(value) => Value::Number(Number::from(*value)),
        AccountSettingValue::Unassigned => Value::Null,
    }
}

#[cfg(test)]
mod tests;

/// One account write, from whichever adapter owns the file.
pub(super) enum AccountSaveReceipt {
    Sqlite(SqliteSaveReceipt),
    Dawn(dawn_persistence::DawnSaveReceipt),
}

impl AccountSaveReceipt {
    /// The verified copy this write took first.
    pub(super) fn backup(&self) -> &Path {
        match self {
            Self::Sqlite(receipt) => &receipt.backup,
            Self::Dawn(receipt) => &receipt.backup,
        }
    }

    /// The account file this write landed in, for a message the user reads.
    pub(super) const fn label(&self) -> &'static str {
        match self {
            Self::Sqlite(_) => "investment.sqlite3",
            Self::Dawn(_) => "player-state.db",
        }
    }

    /// Dawn commits inside one transaction and never leaves a checkpoint to warn about.
    pub(super) fn checkpoint_warning(&self) -> Option<&str> {
        match self {
            Self::Sqlite(receipt) => receipt.checkpoint_warning.as_deref(),
            Self::Dawn(_) => None,
        }
    }
}
