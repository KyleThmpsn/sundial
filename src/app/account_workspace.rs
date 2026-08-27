//! Workspace-level account source selection and operation routing.
//!
//! JSON and SQLite remain independent persistence adapters. A workspace selects one account
//! source when it loads and never falls back after selection.

#[cfg(feature = "sqlite-account")]
mod sqlite;

use std::{
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
};

#[cfg(feature = "sqlite-account")]
use std::collections::{BTreeMap, BTreeSet};

#[cfg(feature = "sqlite-account")]
use serde_json::Number;
use serde_json::{Map, Value};
#[cfg(feature = "sqlite-account")]
use sundial_account::{
    AccountSettingGroup, AccountSettingKey, AccountSettingValue, DismantleReward, ItemInstance,
    KeyBindingSlot, ProfileItem,
};
use sundial_account::{AccountSettingsCommand, CharacterMetadata, CharacterMetadataUpdate};

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
        let account = match std::fs::metadata(&database_path) {
            Ok(metadata) if metadata.len() == 0 => {
                AccountDocument::Json(JsonSelectionReason::DatabaseEmpty)
            }
            Ok(_) => AccountDocument::Blocked(
                "This Sundial build does not include SQLite account support. Install a standard Sundial build before editing this Sunrise account"
                    .to_owned(),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                AccountDocument::Json(JsonSelectionReason::DatabaseMissing)
            }
            Err(error) => AccountDocument::Blocked(format!(
                "Sundial could not inspect state.sqlite3 without SQLite support: {error}"
            )),
        };

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
            AccountDocument::Json(JsonSelectionReason::DatabaseMissing) => AccountSourceInfo {
                kind: AccountSourceKind::Json,
                label: "settings.json",
                detail: "No state.sqlite3 was found. Account editing uses the existing JSON behavior for this Sunrise build.".to_owned(),
                database_path: self.database_path.clone(),
                contract: "JSON schema selected by settings.json version",
            },
            AccountDocument::Json(JsonSelectionReason::DatabaseEmpty) => AccountSourceInfo {
                kind: AccountSourceKind::Json,
                label: "settings.json",
                detail: "state.sqlite3 is empty or uninitialized. Account editing stays on settings.json until Sunrise creates a compatible account database.".to_owned(),
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
        #[cfg(not(feature = "sqlite-account"))]
        if matches!(self.account, AccountDocument::Json(_)) {
            return match std::fs::metadata(&self.database_path) {
                Ok(metadata) if metadata.len() == 0 => Ok(()),
                Ok(_) => Err(
                    "state.sqlite3 became authoritative after this workspace loaded, but this Sundial build cannot read it. Install a standard Sundial build and reload before saving"
                        .to_owned(),
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!(
                    "state.sqlite3 appeared or changed after this workspace loaded and could not be inspected safely: {error}. Reload before saving"
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

#[cfg(feature = "sqlite-account")]
fn sqlite_change_summaries(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
) -> Vec<String> {
    let mut changes = Vec::new();
    summarize_profile_items(before, after, limit, &mut changes);
    summarize_dismantle_rewards(before, after, limit, &mut changes);
    summarize_characters(before, after, limit, &mut changes);
    summarize_account_settings(before, after, limit, &mut changes);
    changes
}

#[cfg(feature = "sqlite-account")]
fn summarize_profile_items(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
    changes: &mut Vec<String>,
) {
    let before = before
        .profile()
        .profile_items()
        .iter()
        .map(|item| (item.id.get(), item))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .profile()
        .profile_items()
        .iter()
        .map(|item| (item.id.get(), item))
        .collect::<BTreeMap<_, _>>();
    for id in before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        summarize_profile_item(
            &format!("state.sqlite3/profile_items/{id}"),
            before.get(&id).copied(),
            after.get(&id).copied(),
            limit,
            changes,
        );
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_profile_item(
    path: &str,
    before: Option<&ProfileItem>,
    after: Option<&ProfileItem>,
    limit: usize,
    changes: &mut Vec<String>,
) {
    match (before, after) {
        (None, Some(item)) => push_summary(
            changes,
            limit,
            format!(
                "{path}: added hash {} ×{}",
                item.definition_hash.get(),
                item.quantity
            ),
        ),
        (Some(item), None) => push_summary(
            changes,
            limit,
            format!(
                "{path}: removed hash {} ×{}",
                item.definition_hash.get(),
                item.quantity
            ),
        ),
        (Some(before), Some(after)) => {
            push_field_change(
                changes,
                limit,
                &format!("{path}/definition_hash"),
                before.definition_hash.get(),
                after.definition_hash.get(),
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/quantity"),
                before.quantity,
                after.quantity,
            );
        }
        (None, None) => {}
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_dismantle_rewards(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
    changes: &mut Vec<String>,
) {
    let before = before
        .profile()
        .dismantle_rewards()
        .iter()
        .map(|reward| (reward.id.get(), reward))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .profile()
        .dismantle_rewards()
        .iter()
        .map(|reward| (reward.id.get(), reward))
        .collect::<BTreeMap<_, _>>();
    for id in before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let path = format!("state.sqlite3/dismantle_rewards/{id}");
        match (before.get(&id).copied(), after.get(&id).copied()) {
            (None, Some(reward)) => push_summary(
                changes,
                limit,
                format!("{path}: added {}", dismantle_reward_label(reward)),
            ),
            (Some(reward), None) => push_summary(
                changes,
                limit,
                format!("{path}: removed {}", dismantle_reward_label(reward)),
            ),
            (Some(before), Some(after)) if before != after => push_summary(
                changes,
                limit,
                format!(
                    "{path}: {} -> {}",
                    dismantle_reward_label(before),
                    dismantle_reward_label(after)
                ),
            ),
            _ => {}
        }
    }
}

#[cfg(feature = "sqlite-account")]
fn dismantle_reward_label(reward: &DismantleReward) -> String {
    format!(
        "hash {} ×{} · rarities {:?} · class {:?} · masterworked {:?}",
        reward.definition_hash.get(),
        reward.quantity,
        reward.rarities,
        reward.gear_class,
        reward.masterworked
    )
}

#[cfg(feature = "sqlite-account")]
fn summarize_characters(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
    changes: &mut Vec<String>,
) {
    let before = before
        .characters()
        .characters()
        .iter()
        .map(|character| (character.id.get(), character))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .characters()
        .characters()
        .iter()
        .map(|character| (character.id.get(), character))
        .collect::<BTreeMap<_, _>>();
    for id in before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let path = format!("state.sqlite3/characters/{id}");
        match (before.get(&id).copied(), after.get(&id).copied()) {
            (None, Some(character)) => push_summary(
                changes,
                limit,
                format!("{path}: added character SOID {:?}", character.soid),
            ),
            (Some(character), None) => push_summary(
                changes,
                limit,
                format!("{path}: removed character SOID {:?}", character.soid),
            ),
            (Some(before), Some(after)) => {
                push_field_change(
                    changes,
                    limit,
                    &format!("{path}/metadata"),
                    format!("{:?}", before.metadata),
                    format!("{:?}", after.metadata),
                );
                summarize_items(
                    &format!("{path}/inventory"),
                    &before.inventory,
                    &after.inventory,
                    limit,
                    changes,
                );
                for slot in before
                    .equipment
                    .keys()
                    .chain(after.equipment.keys())
                    .collect::<BTreeSet<_>>()
                {
                    summarize_item(
                        &format!("{path}/equipment/{}", slot.as_str()),
                        before.equipment.get(slot).and_then(Option::as_ref),
                        after.equipment.get(slot).and_then(Option::as_ref),
                        limit,
                        changes,
                    );
                }
            }
            (None, None) => {}
        }
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_items(
    path: &str,
    before: &[ItemInstance],
    after: &[ItemInstance],
    limit: usize,
    changes: &mut Vec<String>,
) {
    let before = before
        .iter()
        .map(|item| (item.id.get(), item))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .iter()
        .map(|item| (item.id.get(), item))
        .collect::<BTreeMap<_, _>>();
    for id in before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        summarize_item(
            &format!("{path}/{id}"),
            before.get(&id).copied(),
            after.get(&id).copied(),
            limit,
            changes,
        );
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_item(
    path: &str,
    before: Option<&ItemInstance>,
    after: Option<&ItemInstance>,
    limit: usize,
    changes: &mut Vec<String>,
) {
    match (before, after) {
        (None, Some(item)) => push_summary(
            changes,
            limit,
            format!(
                "{path}: added hash {} · Power {} · quantity {}",
                item.definition_hash.get(),
                item.level,
                item.quantity
            ),
        ),
        (Some(item), None) => push_summary(
            changes,
            limit,
            format!(
                "{path}: removed hash {} · Power {} · quantity {}",
                item.definition_hash.get(),
                item.level,
                item.quantity
            ),
        ),
        (Some(before), Some(after)) => {
            push_field_change(
                changes,
                limit,
                &format!("{path}/definition_hash"),
                before.definition_hash.get(),
                after.definition_hash.get(),
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/power"),
                before.level,
                after.level,
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/quantity"),
                before.quantity,
                after.quantity,
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/plugs"),
                format!("{:?}", before.plugs),
                format!("{:?}", after.plugs),
            );
            push_field_change(
                changes,
                limit,
                &format!("{path}/flags"),
                format!("{:?}", before.flags),
                format!("{:?}", after.flags),
            );
        }
        (None, None) => {}
    }
}

#[cfg(feature = "sqlite-account")]
fn summarize_account_settings(
    before: &SqliteAccountDocument,
    after: &SqliteAccountDocument,
    limit: usize,
    changes: &mut Vec<String>,
) {
    for key in before
        .settings()
        .values()
        .keys()
        .chain(after.settings().values().keys())
        .collect::<BTreeSet<_>>()
    {
        let before = before.settings().values().get(key);
        let after = after.settings().values().get(key);
        if before != after {
            push_summary(
                changes,
                limit,
                format!(
                    "state.sqlite3/account_settings/{}: {} -> {}",
                    account_setting_key_label(key),
                    account_setting_value_label(before),
                    account_setting_value_label(after)
                ),
            );
        }
    }
}

#[cfg(feature = "sqlite-account")]
fn account_setting_key_label(key: &AccountSettingKey) -> String {
    match key {
        AccountSettingKey::Preference { group, name } => {
            format!("{}/{name}", format!("{group:?}").to_ascii_lowercase())
        }
        AccountSettingKey::KeyBinding { action, slot } => {
            format!(
                "key_bindings/{action}/{}",
                format!("{slot:?}").to_ascii_lowercase()
            )
        }
    }
}

#[cfg(feature = "sqlite-account")]
fn account_setting_value_label(value: Option<&AccountSettingValue>) -> String {
    match value {
        Some(AccountSettingValue::Boolean(value)) => value.to_string(),
        Some(AccountSettingValue::Unsigned(value)) => value.to_string(),
        Some(AccountSettingValue::Decimal(value)) => value.get().to_string(),
        Some(AccountSettingValue::Text(value)) => format!("{value:?}"),
        Some(AccountSettingValue::InputCode(value)) => format!("input code {value}"),
        Some(AccountSettingValue::Unassigned) => "unassigned".to_owned(),
        None => "missing".to_owned(),
    }
}

#[cfg(feature = "sqlite-account")]
fn push_field_change<T: PartialEq + std::fmt::Display>(
    changes: &mut Vec<String>,
    limit: usize,
    path: &str,
    before: T,
    after: T,
) {
    if before != after {
        push_summary(changes, limit, format!("{path}: {before} -> {after}"));
    }
}

#[cfg(feature = "sqlite-account")]
fn push_summary(changes: &mut Vec<String>, limit: usize, summary: String) {
    if changes.len() < limit {
        changes.push(summary);
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AccountWorkspace;

impl AccountWorkspace {
    pub(super) const fn json() -> Self {
        Self
    }

    fn blocked_string(document: &WorkspaceDocument) -> String {
        document
            .account_editing_blocked()
            .unwrap_or("Account source is unavailable")
            .to_owned()
    }

    fn blocked_inventory(document: &WorkspaceDocument) -> InventoryError {
        InventoryError::new("state.sqlite3", Self::blocked_string(document))
    }

    pub(super) fn character_count(self, document: &WorkspaceDocument) -> usize {
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
        self,
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
                    .ok_or_else(|| {
                        format!("Character {} metadata was not loaded", character_index + 1)
                    })
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::character_metadata(document, character_index)
            }
            AccountDocument::Blocked(_) => Err(Self::blocked_string(document)),
        }
    }

    pub(super) fn class_armor_default_characters(
        self,
        document: &WorkspaceDocument,
    ) -> HashMap<u64, usize> {
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
        self,
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
        self,
        document: &mut WorkspaceDocument,
        commands: Vec<AccountSettingsCommand>,
    ) -> Result<bool, String> {
        match &mut document.account {
            AccountDocument::Json(_) => {
                account_settings::apply_commands(&mut document.json, commands)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => sqlite::apply_account_settings(document, commands),
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    pub(super) fn account_settings_map(
        self,
        document: &WorkspaceDocument,
    ) -> Result<Map<String, Value>, String> {
        match &document.account {
            AccountDocument::Json(_) => document
                .json
                .pointer("/state/account/settings")
                .and_then(Value::as_object)
                .cloned()
                .ok_or_else(|| {
                    "This settings.json has no state.account.settings object.".to_owned()
                }),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => Ok(settings_map(document.settings().values())),
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    pub(super) fn named_key_bindings_editable(self, document: &WorkspaceDocument) -> bool {
        match &document.account {
            AccountDocument::Json(_) => crate::game_settings::key_bindings_editable(&document.json),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(_) => false,
            AccountDocument::Blocked(_) => false,
        }
    }

    pub(super) fn supports_combined_dismantle_gear_class(
        self,
        _document: &WorkspaceDocument,
    ) -> bool {
        #[cfg(feature = "sqlite-account")]
        if matches!(_document.account, AccountDocument::Sqlite(_)) {
            return true;
        }
        false
    }

    pub(super) fn can_mutate_equipment(self, document: &WorkspaceDocument) -> bool {
        match &document.account {
            AccountDocument::Json(_) => {
                super::inventory::schema_mode(&document.json).can_mutate_equipment()
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(_) => true,
            AccountDocument::Blocked(_) => false,
        }
    }

    pub(super) fn can_mutate_character_inventory(self, document: &WorkspaceDocument) -> bool {
        match &document.account {
            AccountDocument::Json(_) => {
                super::inventory::schema_mode(&document.json).can_mutate_character_inventory()
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(_) => true,
            AccountDocument::Blocked(_) => false,
        }
    }

    pub(super) fn can_mutate_equipment_flags(self, document: &WorkspaceDocument) -> bool {
        match &document.account {
            AccountDocument::Json(_) => {
                super::inventory::schema_mode(&document.json).can_mutate_equipment_flags()
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(_) => true,
            AccountDocument::Blocked(_) => false,
        }
    }

    pub(super) fn character_soid(
        self,
        document: &WorkspaceDocument,
        character_index: usize,
    ) -> Option<u64> {
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

    pub(super) fn character_inventory_capacity(self, _document: &WorkspaceDocument) -> usize {
        #[cfg(feature = "sqlite-account")]
        if matches!(_document.account, AccountDocument::Sqlite(_)) {
            return 135;
        }
        super::inventory::CHARACTER_INVENTORY_CAPACITY
    }

    pub(super) fn profile_items_editable(self, document: &WorkspaceDocument) -> bool {
        match &document.account {
            AccountDocument::Json(_) => {
                super::inventory::schema_mode(&document.json).can_mutate_profile_items()
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(_) => true,
            AccountDocument::Blocked(_) => false,
        }
    }

    pub(super) fn profile_item_capacity(self, document: &WorkspaceDocument) -> Option<usize> {
        #[cfg(feature = "sqlite-account")]
        if matches!(document.account, AccountDocument::Sqlite(_)) {
            return Some(701);
        }
        super::inventory::schema_mode(&document.json).profile_item_capacity()
    }

    pub(super) fn dismantle_rewards_available(self, document: &WorkspaceDocument) -> bool {
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

    pub(super) fn dismantle_rewards_editable(self, document: &WorkspaceDocument) -> bool {
        match &document.account {
            AccountDocument::Json(_) => {
                super::inventory::schema_mode(&document.json).can_mutate_dismantle_rewards()
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(_) => true,
            AccountDocument::Blocked(_) => false,
        }
    }

    pub(super) fn dismantle_reward_capacity(self, document: &WorkspaceDocument) -> Option<usize> {
        #[cfg(feature = "sqlite-account")]
        if matches!(document.account, AccountDocument::Sqlite(_)) {
            return Some(32);
        }
        super::inventory::schema_mode(&document.json).dismantle_reward_capacity()
    }

    pub(super) fn filtered_dismantle_rewards(self, document: &WorkspaceDocument) -> bool {
        match &document.account {
            AccountDocument::Json(_) => {
                super::inventory::schema_mode(&document.json).supports_filtered_dismantle_rewards()
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(_) => true,
            AccountDocument::Blocked(_) => false,
        }
    }

    pub(super) fn account_collection_ready(self, document: &WorkspaceDocument) -> bool {
        match &document.account {
            AccountDocument::Json(_) => {
                super::inventory::profile_item_target_exists(&document.json).unwrap_or(false)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(_) => true,
            AccountDocument::Blocked(_) => false,
        }
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

impl AccountWorkspace {
    pub(super) fn profile_items(
        self,
        document: &WorkspaceDocument,
    ) -> Result<Option<Vec<ProfileItemSnapshot>>, InventoryError> {
        match &document.account {
            AccountDocument::Json(_) => super::inventory::profile_items(&document.json),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => Ok(sqlite::profile_items(document)),
            AccountDocument::Blocked(_) => Err(Self::blocked_inventory(document)),
        }
    }

    pub(super) fn dismantle_rewards(
        self,
        document: &WorkspaceDocument,
    ) -> Result<Option<Vec<DismantleRewardSnapshot>>, InventoryError> {
        match &document.account {
            AccountDocument::Json(_) => super::inventory::dismantle_rewards(&document.json),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => Ok(sqlite::dismantle_rewards(document)),
            AccountDocument::Blocked(_) => Err(Self::blocked_inventory(document)),
        }
    }

    pub(super) fn character_inventory(
        self,
        document: &WorkspaceDocument,
        character_index: usize,
    ) -> Result<Option<Vec<InventoryItemSnapshot>>, InventoryError> {
        match &document.account {
            AccountDocument::Json(_) => {
                super::inventory::character_inventory(&document.json, character_index)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::character_inventory(document, character_index)
            }
            AccountDocument::Blocked(_) => Err(Self::blocked_inventory(document)),
        }
    }

    pub(super) fn add_profile_item(
        self,
        document: &mut WorkspaceDocument,
        definition_hash: u32,
        quantity: i32,
    ) -> Result<ProfileItemLocation, InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => {
                super::inventory::add_profile_item(&mut document.json, definition_hash, quantity)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::add_profile_item(document, definition_hash, quantity)
            }
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn apply_profile_item_action(
        self,
        document: &mut WorkspaceDocument,
        location: ProfileItemLocation,
        action: ProfileItemAction,
    ) -> Result<(), InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => {
                super::inventory::apply_profile_item_action(&mut document.json, location, action)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::apply_profile_item_action(document, location, action)
            }
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn add_dismantle_reward(
        self,
        document: &mut WorkspaceDocument,
        definition_hash: u32,
    ) -> Result<DismantleRewardLocation, InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => {
                super::inventory::add_dismantle_reward(&mut document.json, definition_hash)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::add_dismantle_reward(document, definition_hash)
            }
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn apply_dismantle_reward_action(
        self,
        document: &mut WorkspaceDocument,
        location: DismantleRewardLocation,
        action: DismantleRewardAction,
    ) -> Result<(), InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => super::inventory::apply_dismantle_reward_action(
                &mut document.json,
                location,
                action,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::apply_dismantle_reward_action(document, location, action)
            }
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn add_inventory_item(
        self,
        document: &mut WorkspaceDocument,
        character_index: usize,
        item: NewInventoryItem,
    ) -> Result<InventoryItemLocation, InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => {
                super::inventory::add_inventory_item(&mut document.json, character_index, item)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::add_inventory_item(document, character_index, item)
            }
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn apply_inventory_item_action(
        self,
        document: &mut WorkspaceDocument,
        location: InventoryItemLocation,
        action: InventoryItemAction,
    ) -> Result<(), InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => {
                super::inventory::apply_inventory_item_action(&mut document.json, location, action)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::apply_inventory_item_action(document, location, action)
            }
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn remove_character_inventory_items(
        self,
        document: &mut WorkspaceDocument,
        character_index: usize,
        item_indices: impl IntoIterator<Item = usize>,
    ) -> Result<usize, InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => super::inventory::remove_character_inventory_items(
                &mut document.json,
                character_index,
                item_indices,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::remove_character_inventory_items(document, character_index, item_indices)
            }
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn swap_inventory_item_with_equipment(
        self,
        document: &mut WorkspaceDocument,
        location: InventoryItemLocation,
        slot: &str,
    ) -> Result<bool, InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => super::inventory::swap_inventory_item_with_equipment(
                &mut document.json,
                location,
                slot,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::swap_inventory_item_with_equipment(document, location, slot)
            }
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn move_inventory_item_to_character(
        self,
        document: &mut WorkspaceDocument,
        location: InventoryItemLocation,
        destination_character_index: usize,
    ) -> Result<InventoryItemLocation, InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => super::inventory::move_inventory_item_to_character(
                &mut document.json,
                location,
                destination_character_index,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => sqlite::move_inventory_item_to_character(
                document,
                location,
                destination_character_index,
            ),
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn move_equipment_item_to_inventory(
        self,
        document: &mut WorkspaceDocument,
        character_index: usize,
        slot: &str,
    ) -> Result<(), InventoryError> {
        match &mut document.account {
            AccountDocument::Json(_) => super::inventory::move_equipment_item_to_inventory(
                &mut document.json,
                character_index,
                slot,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::move_equipment_item_to_inventory(document, character_index, slot)
            }
            AccountDocument::Blocked(reason) => {
                Err(InventoryError::new("state.sqlite3", reason.clone()))
            }
        }
    }

    pub(super) fn equipped_item_snapshots(
        self,
        document: &WorkspaceDocument,
        character_index: usize,
    ) -> Result<Vec<EquippedItemSnapshot>, String> {
        match &document.account {
            AccountDocument::Json(_) => {
                super::equipment::equipped_item_snapshots(&document.json, character_index)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::equipped_item_snapshots(document, character_index)
            }
            AccountDocument::Blocked(_) => Err(Self::blocked_string(document)),
        }
    }

    pub(super) fn equip_definition(
        self,
        document: &mut WorkspaceDocument,
        character_index: usize,
        slot: &str,
        definition_hash: u64,
        default_plugs: &[Option<String>],
    ) -> Result<(), String> {
        match &mut document.account {
            AccountDocument::Json(_) => super::equipment::equip_definition(
                &mut document.json,
                character_index,
                slot,
                definition_hash,
                default_plugs,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => sqlite::equip_definition(
                document,
                character_index,
                slot,
                definition_hash,
                default_plugs,
            ),
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    pub(super) fn set_equipment_item_level(
        self,
        document: &mut WorkspaceDocument,
        character_index: usize,
        slot: &str,
        level: i64,
    ) -> Result<(), String> {
        match &mut document.account {
            AccountDocument::Json(_) => super::equipment::set_equipment_item_level(
                &mut document.json,
                character_index,
                slot,
                level,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::set_equipment_item_level(document, character_index, slot, level)
            }
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    pub(super) fn set_equipment_item_flags(
        self,
        document: &mut WorkspaceDocument,
        character_index: usize,
        slot: &str,
        flags: Option<u8>,
    ) -> Result<(), String> {
        match &mut document.account {
            AccountDocument::Json(_) => super::equipment::set_equipment_item_flags(
                &mut document.json,
                character_index,
                slot,
                flags,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::set_equipment_item_flags(document, character_index, slot, flags)
            }
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn set_equipment_item_plug(
        self,
        document: &mut WorkspaceDocument,
        character_index: usize,
        slot: &str,
        socket_index: usize,
        default_plugs: &[Option<String>],
        hash: Option<u64>,
    ) -> Result<(), String> {
        match &mut document.account {
            AccountDocument::Json(_) => super::equipment::set_equipment_item_plug(
                &mut document.json,
                character_index,
                slot,
                socket_index,
                default_plugs,
                hash,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => sqlite::set_equipment_item_plug(
                document,
                character_index,
                slot,
                socket_index,
                default_plugs,
                hash,
            ),
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    pub(super) fn set_weapon_slot_empty(
        self,
        document: &mut WorkspaceDocument,
        character_index: usize,
        slot: &str,
    ) -> Result<(), String> {
        match &mut document.account {
            AccountDocument::Json(_) => {
                super::equipment::set_weapon_slot_empty(&mut document.json, character_index, slot)
            }
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => {
                sqlite::set_weapon_slot_empty(document, character_index, slot)
            }
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }

    pub(super) fn restore_class_armor(
        self,
        document: &mut WorkspaceDocument,
        source_character_index: usize,
        destination_character_index: usize,
    ) -> Result<bool, String> {
        match &mut document.account {
            AccountDocument::Json(_) => super::equipment::restore_class_armor_from_character(
                &mut document.json,
                source_character_index,
                destination_character_index,
            ),
            #[cfg(feature = "sqlite-account")]
            AccountDocument::Sqlite(document) => sqlite::restore_class_armor(
                document,
                source_character_index,
                destination_character_index,
            ),
            AccountDocument::Blocked(reason) => Err(reason.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "sqlite-account")]
    use std::fs;

    #[cfg(feature = "sqlite-account")]
    use rusqlite::Connection;
    use serde_json::json;
    #[cfg(feature = "sqlite-account")]
    use sundial_account::{
        AccountSettingKey, AccountSettingValue, AccountSettingsCommand, CharacterAbilities,
        CharacterMetadataUpdate,
    };

    #[cfg(feature = "sqlite-account")]
    use super::super::inventory::{
        InventoryItemAction, InventoryItemLocation, ProfileItemAction, ProfileItemLocation,
    };
    use super::{AccountSourceKind, AccountWorkspace, WorkspaceDocument};
    use crate::test_support::TestDirectory;

    #[test]
    fn json_workspace_exposes_neutral_character_metadata() {
        let document = WorkspaceDocument::json_only(json!({
            "version": 8,
            "state": {"characters": [{
                "race": 2,
                "gender": 1,
                "class": 2,
                "movement_ability": 6,
                "grenade_ability": 9,
                "super_ability": 20,
                "melee_ability": 21,
                "class_ability": 3
            }]}
        }));

        let metadata = AccountWorkspace::json()
            .character_metadata(&document, 0)
            .unwrap();
        assert_eq!(metadata.class_type, 2);
        assert_eq!(metadata.abilities.super_ability, 20);
    }

    #[test]
    fn missing_database_keeps_existing_json_account_behavior() {
        let directory = TestDirectory::new("workspace-json-source");
        let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));

        assert_eq!(document.source_info().kind, AccountSourceKind::Json);
        assert_eq!(AccountWorkspace::json().character_count(&document), 2);
    }

    #[cfg(not(feature = "sqlite-account"))]
    #[test]
    fn legacy_build_blocks_nonempty_sqlite_sources_and_source_transitions() {
        let directory = TestDirectory::new("workspace-no-sqlite-feature");
        let settings_path = settings_path(&directory);
        let json_document = WorkspaceDocument::load(json_characters(2), &settings_path);
        assert_eq!(json_document.source_info().kind, AccountSourceKind::Json);

        std::fs::write(directory.0.join("state.sqlite3"), b"SQLite source").unwrap();
        assert!(
            json_document
                .verify_account_source_unchanged()
                .unwrap_err()
                .contains("cannot read it")
        );

        let blocked = WorkspaceDocument::load(json_characters(2), &settings_path);
        assert_eq!(blocked.source_info().kind, AccountSourceKind::Blocked);
        assert!(
            blocked
                .account_editing_blocked()
                .is_some_and(|reason| reason.contains("standard Sundial build"))
        );
    }

    #[test]
    fn json_source_keeps_legacy_schema_eight_preference_materialization() {
        let directory = TestDirectory::new("workspace-json-normalization");
        let document = WorkspaceDocument::load(
            json!({
                "version": 8,
                "state": {"account": {"settings": {"display": {}}}}
            }),
            &settings_path(&directory),
        );

        assert_eq!(
            document
                .json()
                .pointer("/state/account/settings/key_binding_source"),
            Some(&json!("computer"))
        );
        assert_eq!(
            document
                .json()
                .pointer("/state/account/settings/display/vertical_sync_interval"),
            Some(&json!(0))
        );
        assert_eq!(
            document
                .json()
                .pointer("/state/account/settings/display/field_of_view"),
            Some(&json!(85))
        );
    }

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn empty_and_uninitialized_databases_keep_json_account_behavior() {
        for name in ["zero-byte", "uninitialized"] {
            let directory = TestDirectory::new(name);
            let database_path = directory.0.join("state.sqlite3");
            if name == "zero-byte" {
                fs::File::create(&database_path).unwrap();
            } else {
                drop(Connection::open(&database_path).unwrap());
            }
            let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));

            assert_eq!(document.source_info().kind, AccountSourceKind::Json);
            assert_eq!(AccountWorkspace::json().character_count(&document), 2);
        }
    }

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn exact_pr88_database_is_authoritative_over_stale_json_account_data() {
        let directory = TestDirectory::new("workspace-sqlite-source");
        crate::persistence::sqlite_account::tests::create_fixture(
            &directory.0.join("state.sqlite3"),
            3,
        );
        let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));

        assert_eq!(document.source_info().kind, AccountSourceKind::Sqlite);
        assert_eq!(AccountWorkspace::json().character_count(&document), 1);
    }

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn json_workspace_requires_reload_when_sqlite_becomes_authoritative() {
        let directory = TestDirectory::new("workspace-source-transition");
        let settings_path = settings_path(&directory);
        let document = WorkspaceDocument::load(json_characters(2), &settings_path);
        assert_eq!(document.verify_account_source_unchanged(), Ok(()));

        crate::persistence::sqlite_account::tests::create_fixture(
            &directory.0.join("state.sqlite3"),
            3,
        );

        let error = document.verify_account_source_unchanged().unwrap_err();
        assert!(error.contains("became authoritative"));
        assert_eq!(document.source_info().kind, AccountSourceKind::Json);
    }

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn missing_to_empty_database_transition_keeps_legacy_json_source() {
        let directory = TestDirectory::new("workspace-empty-transition");
        let document = WorkspaceDocument::load(json_characters(2), &settings_path(&directory));
        fs::File::create(directory.0.join("state.sqlite3")).unwrap();

        assert_eq!(document.verify_account_source_unchanged(), Ok(()));
    }

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn sqlite_source_does_not_materialize_stale_json_account_preferences() {
        let directory = TestDirectory::new("workspace-sqlite-no-json-normalization");
        crate::persistence::sqlite_account::tests::create_fixture(
            &directory.0.join("state.sqlite3"),
            3,
        );
        let json = json!({
            "version": 8,
            "state": {"account": {"settings": {"display": {}}}}
        });
        let document = WorkspaceDocument::load(json.clone(), &settings_path(&directory));

        assert_eq!(document.source_info().kind, AccountSourceKind::Sqlite);
        assert_eq!(document.json(), &json);
    }

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn sqlite_workspace_validation_ignores_stale_json_account_domains() {
        let directory = TestDirectory::new("workspace-sqlite-validation");
        crate::persistence::sqlite_account::tests::create_fixture(
            &directory.0.join("state.sqlite3"),
            3,
        );
        let stale_json = json!({
            "version": 8,
            "state": {
                "account": {"settings": "stale and invalid"},
                "characters": "stale and invalid"
            }
        });
        let sqlite_document =
            WorkspaceDocument::load(stale_json.clone(), &settings_path(&directory));
        assert_eq!(
            super::super::settings::validate_workspace_document(&sqlite_document),
            Ok(())
        );

        let json_directory = TestDirectory::new("workspace-json-validation");
        let json_document = WorkspaceDocument::load(stale_json, &settings_path(&json_directory));
        assert!(super::super::settings::validate_workspace_document(&json_document).is_err());
    }

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn sqlite_facade_mutates_every_account_domain_without_touching_json() {
        let directory = TestDirectory::new("workspace-sqlite-mutations");
        crate::persistence::sqlite_account::tests::create_fixture(
            &directory.0.join("state.sqlite3"),
            3,
        );
        let mut document = WorkspaceDocument::load(
            json!({
                "version": 8,
                "state": {
                    "account": {"settings": {"sentinel": true}},
                    "characters": [{"sentinel": true}]
                }
            }),
            &settings_path(&directory),
        );
        let persisted = document.clone();
        let json_before = document.json().clone();
        let workspace = AccountWorkspace::json();

        workspace
            .apply_profile_item_action(
                &mut document,
                ProfileItemLocation { index: 0 },
                ProfileItemAction::SetQuantity(26),
            )
            .unwrap();
        workspace
            .apply_inventory_item_action(
                &mut document,
                InventoryItemLocation {
                    character_index: 0,
                    item_index: 0,
                },
                InventoryItemAction::SetQuantity(3),
            )
            .unwrap();
        workspace
            .set_equipment_item_level(&mut document, 0, "kinetic", 104)
            .unwrap();
        workspace
            .apply_character_updates(
                &mut document,
                0,
                vec![CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
                    movement: 5,
                    grenade: 8,
                    super_ability: 20,
                    melee: 21,
                    class_ability: 3,
                })],
            )
            .unwrap();
        workspace
            .apply_account_settings(
                &mut document,
                vec![AccountSettingsCommand::Set {
                    key: AccountSettingKey::known_preference("show_fps").unwrap(),
                    value: AccountSettingValue::Boolean(false),
                }],
            )
            .unwrap();

        assert_eq!(
            workspace.profile_items(&document).unwrap().unwrap()[0].quantity,
            26
        );
        assert_eq!(
            workspace
                .character_inventory(&document, 0)
                .unwrap()
                .unwrap()[0]
                .quantity,
            3
        );
        assert_eq!(
            workspace
                .equipped_item_snapshots(&document, 0)
                .unwrap()
                .into_iter()
                .find(|item| item.slot == "kinetic")
                .unwrap()
                .level,
            Some(104)
        );
        assert_eq!(
            workspace
                .character_metadata(&document, 0)
                .unwrap()
                .abilities
                .movement,
            5
        );
        let settings =
            serde_json::Value::Object(workspace.account_settings_map(&document).unwrap());
        assert_eq!(
            settings.pointer("/display/show_fps"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(document.json(), &json_before);
        assert!(document.account_changed_from(&persisted));
        assert!(!document.json_changed_from(&persisted));
        let summaries = document.account_change_summaries(&persisted, 20);
        assert!(
            summaries
                .iter()
                .any(|summary| summary.contains("/profile_items/") && summary.contains("/quantity"))
        );
        assert!(
            summaries
                .iter()
                .any(|summary| summary.contains("/inventory/") && summary.contains("/quantity"))
        );
        assert!(
            summaries
                .iter()
                .any(|summary| summary.contains("/equipment/kinetic/power"))
        );
        assert!(
            summaries
                .iter()
                .any(|summary| summary.contains("/metadata"))
        );
        assert!(
            summaries
                .iter()
                .any(|summary| summary.contains("/account_settings/display/show_fps"))
        );
    }

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn corrupt_or_incompatible_database_blocks_stale_json_fallback() {
        let corrupt = TestDirectory::new("workspace-corrupt-source");
        fs::write(corrupt.0.join("state.sqlite3"), b"not a SQLite database").unwrap();
        let corrupt_document =
            WorkspaceDocument::load(json_characters(2), &settings_path(&corrupt));
        assert_eq!(
            corrupt_document.source_info().kind,
            AccountSourceKind::Blocked
        );
        assert_eq!(
            AccountWorkspace::json().character_count(&corrupt_document),
            0
        );

        let incompatible = TestDirectory::new("workspace-incompatible-source");
        let connection = Connection::open(incompatible.0.join("state.sqlite3")).unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
        drop(connection);
        let incompatible_document =
            WorkspaceDocument::load(json_characters(2), &settings_path(&incompatible));
        assert_eq!(
            incompatible_document.source_info().kind,
            AccountSourceKind::Blocked
        );
        assert_eq!(
            AccountWorkspace::json().character_count(&incompatible_document),
            0
        );
    }

    fn settings_path(directory: &TestDirectory) -> std::path::PathBuf {
        directory.0.join("settings.json")
    }

    fn json_characters(count: usize) -> serde_json::Value {
        json!({"state": {"characters": vec![json!({}); count]}})
    }
}
