use std::{
    collections::BTreeMap,
    num::NonZeroU64,
    path::{Path, PathBuf},
};

mod inventory;
mod metadata;
mod rewards;

use rusqlite::{Connection, OpenFlags, types::ValueRef};
use sundial_account::{
    AccountSettingsCapabilities, AccountSettingsState, CharacterAbilities, CharacterCapabilities,
    CharacterState, EntityId, EquipmentSlot, InstanceSoid, ProfileCapabilities, ProfileState,
};

use super::{
    SqliteAccountError, SqliteAccountIncompatibility, SqliteAccountLoad, SqliteAccountSnapshot,
    contract::{
        CHARACTER_ITEM_CAPACITY, DISMANTLE_REWARD_CAPACITY, EQUIPMENT_SLOTS, PLUG_CAPACITY,
        PROFILE_ITEM_CAPACITY,
    },
    reader,
};

const REVISION_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const REVISION_PRIME: u64 = 0x0000_0100_0000_01B3;
const DEFAULT_ABILITIES: CharacterAbilities = CharacterAbilities {
    movement: 4,
    grenade: 7,
    super_ability: 10,
    melee: 11,
    class_ability: 2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SourceRevision(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProfilePersistence {
    instance_soid: u64,
    mutation_serial: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CharacterPersistence {
    next_inventory_serial: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ItemPersistence {
    mutation_serial: i32,
    abilities: CharacterAbilities,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SqliteAccountDocument {
    inventory_state: super::inventory_state::InventoryState,
    progression: super::progression::Progression,
    entitlements: serde_json::Value,
    runtime: serde_json::Value,
    preserved_rows: BTreeMap<String, Vec<super::writer::NativeRow>>,
    path: PathBuf,
    revision: SourceRevision,
    primary_soid: InstanceSoid,
    profile: ProfileState,
    characters: CharacterState,
    settings: AccountSettingsState,
    profile_persistence: BTreeMap<EntityId, ProfilePersistence>,
    character_persistence: BTreeMap<EntityId, CharacterPersistence>,
    item_persistence: BTreeMap<EntityId, ItemPersistence>,
    pending_item_abilities: BTreeMap<EntityId, CharacterAbilities>,
    profile_positions: BTreeMap<EntityId, usize>,
    reward_positions: BTreeMap<EntityId, usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SqliteAccountDocumentLoad {
    Missing,
    Empty,
    Incompatible(SqliteAccountIncompatibility),
    Loaded(Box<SqliteAccountDocument>),
}

impl SqliteAccountDocument {
    pub(super) fn preserved_rows(&self, table: &str) -> &[super::writer::NativeRow] {
        self.preserved_rows.get(table).map_or(&[], Vec::as_slice)
    }
    pub(super) fn capture_preserved_rows(
        &mut self,
        db: &Connection,
    ) -> Result<(), SqliteAccountError> {
        self.preserved_rows = load_preserved_rows(db)?;
        Ok(())
    }

    pub(crate) fn runtime(&self) -> &serde_json::Value {
        &self.runtime
    }
    pub(crate) fn set_runtime(&mut self, value: serde_json::Value) {
        self.runtime = value;
    }
    pub(super) fn save_runtime(&self, db: &Connection) -> Result<(), SqliteAccountError> {
        super::runtime::save(db, &self.runtime)
    }

    pub(super) fn validate_native_edits(&self, db: &Connection) -> Result<(), SqliteAccountError> {
        super::runtime::validate(&self.runtime)?;
        let mut view = serde_json::json!({"state": self.runtime});
        if self.entitlements != super::entitlements::load(db)? {
            view["server"] = serde_json::json!({"entitlements": self.entitlements});
        }
        crate::game_settings::runtime::validate_native_details(&view)
            .map_err(|error| SqliteAccountError::invalid_data("runtime", error))
    }

    pub(crate) fn entitlements(&self) -> &serde_json::Value {
        &self.entitlements
    }
    pub(crate) fn set_entitlements(&mut self, value: serde_json::Value) {
        self.entitlements = value;
    }
    pub(super) fn save_entitlements(&self, db: &Connection) -> Result<(), SqliteAccountError> {
        super::entitlements::save(db, &self.entitlements, self.preserved_rows("entitlements"))
    }

    pub(crate) fn progression_view(&self, index: usize) -> serde_json::Value {
        let mut view = self.progression.view(index);
        view["_reward_context"] = serde_json::json!({"character":index,"character_count":self.characters.characters().len(),"class":self.characters.characters().get(index).and_then(|character|character.metadata).map(|metadata|metadata.class_type),"pending":self.pending_rewards()});
        view["_reward_context"]["consumables"] = serde_json::json!(self.character_stacks(index).iter().map(|item| {
            serde_json::json!({"definition_hash":item.definition_hash,"quantity":item.quantity,"mutation_serial":item.mutation_serial})
        }).collect::<Vec<_>>());
        if let Some(character) = self.characters.characters().get(index) {
            view["_reward_context"]["inventory"] = serde_json::json!(
                character
                    .inventory
                    .iter()
                    .map(|item| (item.id.get(), item.definition_hash.get(), item.quantity))
                    .collect::<Vec<_>>()
            );
            view["_reward_context"]["equipment"] = serde_json::json!(
                character
                    .equipment
                    .values()
                    .flatten()
                    .map(|item| (item.id.get(), item.definition_hash.get(), item.quantity))
                    .collect::<Vec<_>>()
            );
            view["_reward_context"]["next_serial"] =
                serde_json::json!(self.next_inventory_serial(character.id).ok());
        }
        view
    }
    pub(crate) fn account_flag_is_set(&self, definition_index: u16, slot: u16) -> bool {
        self.progression.account_flag_is_set(definition_index, slot)
    }
    pub(crate) fn set_account_flag(
        &mut self,
        definition_index: u16,
        slot: u16,
    ) -> Result<bool, SqliteAccountError> {
        self.progression.set_account_flag(definition_index, slot)
    }
    pub(crate) fn account_flags_changed_from(&self, before: &Self) -> bool {
        self.progression
            .account_flags_changed_from(&before.progression)
    }
    pub(crate) fn apply_progression_view(
        &mut self,
        index: usize,
        value: &serde_json::Value,
    ) -> Result<(), SqliteAccountError> {
        if value.get("_progression_rewards").is_none()
            && value.get("_progression_consumables").is_none()
        {
            return self.progression.apply(index, value);
        }
        let invalid = |message: &str| SqliteAccountError::invalid_data("pending_rewards", message);
        if value.get("_reward_context") != self.progression_view(index).get("_reward_context") {
            return Err(invalid(
                "The selected character, inventory or pending rewards changed. Prepare the progression edit again.",
            ));
        }
        let mut candidate = self.clone();
        candidate.progression.apply(index, value)?;
        let empty = Vec::new();
        let rewards = match value.get("_progression_rewards") {
            Some(value) => value
                .as_array()
                .ok_or_else(|| invalid("The progression reward plan is invalid"))?,
            None => &empty,
        };
        for reward in rewards {
            let kind = reward
                .get("kind")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u8::try_from(value).ok())
                .ok_or_else(|| invalid("The reward kind is invalid"))?;
            let hash = reward
                .get("hash")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| invalid("The reward definition is invalid"))?;
            let quantity = reward
                .get("quantity")
                .and_then(serde_json::Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| invalid("The reward quantity is invalid"))?;
            candidate.add_pending_reward(index, kind, hash, quantity)?;
        }
        candidate.apply_consumable_rewards(index, value)?;
        *self = candidate;
        Ok(())
    }
    pub(super) fn save_progression(&self, db: &Connection) -> Result<(), SqliteAccountError> {
        self.progression.save(db)
    }

    pub(super) fn original_profile_position(&self, id: EntityId) -> Option<usize> {
        self.profile_positions.get(&id).copied()
    }
    pub(super) fn original_reward_position(&self, id: EntityId) -> Option<usize> {
        self.reward_positions.get(&id).copied()
    }
    pub(super) fn refresh_positions(&mut self) {
        self.inventory_state.refresh_positions();
        self.profile_positions = self
            .profile
            .profile_items()
            .iter()
            .enumerate()
            .map(|(i, item)| (item.id, i))
            .collect();
        self.reward_positions = self
            .profile
            .dismantle_rewards()
            .iter()
            .enumerate()
            .map(|(i, item)| (item.id, i))
            .collect();
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn primary_soid(&self) -> u64 {
        self.primary_soid.get()
    }

    pub(crate) fn write_conversion_copy(&self, path: &Path) -> Result<(), SqliteAccountError> {
        let source = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| SqliteAccountError::sqlite("read the conversion source", error))?;
        if database_revision(&source)? != self.revision {
            return Err(SqliteAccountError::SourceChanged);
        }
        super::writer::create_integrity_checked_snapshot(&self.path, path)?;
        let mut candidate = self.clone();
        candidate.path = path.to_owned();
        super::writer::save_with_backup(
            &mut candidate,
            Some(path.with_extension("before.sqlite3")),
        )?;
        Ok(())
    }

    pub(crate) const fn profile(&self) -> &ProfileState {
        &self.profile
    }

    pub(crate) fn profile_mut(&mut self) -> &mut ProfileState {
        &mut self.profile
    }

    pub(crate) const fn characters(&self) -> &CharacterState {
        &self.characters
    }

    pub(crate) fn characters_mut(&mut self) -> &mut CharacterState {
        &mut self.characters
    }

    pub(crate) const fn settings(&self) -> &AccountSettingsState {
        &self.settings
    }

    pub(crate) fn settings_mut(&mut self) -> &mut AccountSettingsState {
        &mut self.settings
    }

    /// Returns the Sunrise ability selection persisted with one exact item instance.
    pub(crate) fn persisted_item_abilities(&self, id: EntityId) -> Option<CharacterAbilities> {
        self.pending_item_abilities.get(&id).copied().or_else(|| {
            self.item_persistence
                .get(&id)
                .map(|persistence| persistence.abilities)
        })
    }

    /// Keeps the Sunrise item sidecar aligned with edits made through character metadata.
    ///
    /// New items retain their selections before their persistence serial is assigned at Save.
    pub(crate) fn set_persisted_item_abilities(
        &mut self,
        id: EntityId,
        abilities: CharacterAbilities,
    ) {
        if let Some(persistence) = self.item_persistence.get_mut(&id) {
            persistence.abilities = abilities;
        } else {
            self.pending_item_abilities.insert(id, abilities);
        }
    }

    pub(crate) const fn profile_capabilities() -> ProfileCapabilities {
        ProfileCapabilities {
            profile_items_writable: true,
            profile_item_capacity: Some(PROFILE_ITEM_CAPACITY),
            enforce_loaded_profile_item_capacity: true,
            dismantle_rewards_writable: true,
            dismantle_reward_capacity: Some(DISMANTLE_REWARD_CAPACITY),
            filtered_dismantle_rewards: true,
            combined_dismantle_gear_class: true,
        }
    }

    pub(crate) const fn character_capabilities() -> CharacterCapabilities {
        CharacterCapabilities {
            metadata_writable: true,
            inventory_writable: true,
            equipment_writable: true,
            equipment_flags_writable: true,
            inventory_capacity: Some(CHARACTER_ITEM_CAPACITY),
            enforce_loaded_inventory_capacity: true,
            max_item_plugs: PLUG_CAPACITY,
            item_flag_mask: 7,
            enforce_unique_instance_soids: true,
        }
    }

    pub(crate) const fn settings_capabilities() -> AccountSettingsCapabilities {
        AccountSettingsCapabilities {
            writable: true,
            named_key_bindings_writable: false,
            numeric_key_bindings_writable: true,
            extended_field_of_view: true,
        }
    }

    pub(crate) fn next_entity_id(&self) -> Result<EntityId, SqliteAccountError> {
        let max = self
            .profile
            .profile_items()
            .iter()
            .map(|item| item.id.get())
            .chain(
                self.profile
                    .dismantle_rewards()
                    .iter()
                    .map(|reward| reward.id.get()),
            )
            .chain(self.characters.characters().iter().flat_map(|character| {
                std::iter::once(character.id.get())
                    .chain(character.inventory.iter().map(|item| item.id.get()))
                    .chain(
                        character
                            .equipment
                            .values()
                            .flatten()
                            .map(|item| item.id.get()),
                    )
            }))
            // Removed rows retain their opaque persistence state until the workspace reloads.
            // A new entity must never inherit that state by reusing its ID.
            .chain(self.profile_persistence.keys().map(|id| id.get()))
            .chain(self.character_persistence.keys().map(|id| id.get()))
            .chain(self.item_persistence.keys().map(|id| id.get()))
            .chain(self.pending_item_abilities.keys().map(|id| id.get()))
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(SqliteAccountError::EntityIdentityExhausted)?;
        Ok(EntityId::new(max))
    }

    pub(super) const fn revision(&self) -> SourceRevision {
        self.revision
    }

    pub(super) fn set_revision(&mut self, revision: SourceRevision) {
        self.revision = revision;
    }

    pub(crate) fn adopt_revision_from(&mut self, source: &Self) {
        if self.path == source.path {
            self.revision = source.revision;
        }
    }

    pub(super) fn prepare_persistence(&mut self) -> Result<(), SqliteAccountError> {
        for item in self.profile.profile_items() {
            self.profile_persistence
                .entry(item.id)
                .or_insert(ProfilePersistence {
                    instance_soid: 0,
                    mutation_serial: 0,
                });
        }

        let mut missing = Vec::new();
        for character in self.characters.characters() {
            let abilities = character
                .metadata
                .map_or(DEFAULT_ABILITIES, |metadata| metadata.abilities);
            for item in &character.inventory {
                if !self.item_persistence.contains_key(&item.id) {
                    missing.push((character.id, item.id, DEFAULT_ABILITIES));
                }
            }
            for (slot, item) in &character.equipment {
                if let Some(item) = item
                    && !self.item_persistence.contains_key(&item.id)
                {
                    missing.push((
                        character.id,
                        item.id,
                        if slot.as_str() == "subclass" {
                            abilities
                        } else {
                            DEFAULT_ABILITIES
                        },
                    ));
                }
            }
        }
        for (character_id, item_id, abilities) in missing {
            let abilities = self
                .pending_item_abilities
                .get(&item_id)
                .copied()
                .unwrap_or(abilities);
            let persistence = self
                .character_persistence
                .get_mut(&character_id)
                .ok_or_else(|| {
                    SqliteAccountError::invalid_data(
                        "characters",
                        "persistence metadata is missing for a loaded character",
                    )
                })?;
            let mutation_serial =
                i32::try_from(persistence.next_inventory_serial).map_err(|_| {
                    SqliteAccountError::invalid_data(
                        "characters.next_inventory_serial",
                        "next inventory serial no longer fits the persisted signed serial",
                    )
                })?;
            persistence.next_inventory_serial = persistence
                .next_inventory_serial
                .checked_add(1)
                .ok_or_else(|| {
                    SqliteAccountError::invalid_data(
                        "characters.next_inventory_serial",
                        "next inventory serial is exhausted",
                    )
                })?;
            self.item_persistence.insert(
                item_id,
                ItemPersistence {
                    mutation_serial,
                    abilities,
                },
            );
            self.pending_item_abilities.remove(&item_id);
        }
        Ok(())
    }

    pub(super) fn profile_persistence(
        &self,
        id: EntityId,
    ) -> Result<(u64, i32), SqliteAccountError> {
        let value = self.profile_persistence.get(&id).ok_or_else(|| {
            SqliteAccountError::invalid_data("profile_items", "persistence metadata is missing")
        })?;
        Ok((value.instance_soid, value.mutation_serial))
    }

    pub(super) fn next_inventory_serial(&self, id: EntityId) -> Result<u32, SqliteAccountError> {
        self.character_persistence
            .get(&id)
            .map(|p| p.next_inventory_serial)
            .ok_or_else(|| {
                SqliteAccountError::invalid_data("characters", "missing persistence metadata")
            })
    }

    pub(super) fn item_persistence(
        &self,
        id: EntityId,
    ) -> Result<(i32, CharacterAbilities), SqliteAccountError> {
        let value = self.item_persistence.get(&id).ok_or_else(|| {
            SqliteAccountError::invalid_data("character_items", "persistence metadata is missing")
        })?;
        Ok((value.mutation_serial, value.abilities))
    }
}

pub(crate) fn load(path: &Path) -> Result<SqliteAccountDocumentLoad, SqliteAccountError> {
    if !path.try_exists().map_err(SqliteAccountError::FileSystem)? {
        return Ok(SqliteAccountDocumentLoad::Missing);
    }
    if std::fs::metadata(path)
        .map_err(SqliteAccountError::FileSystem)?
        .len()
        == 0
    {
        return Ok(SqliteAccountDocumentLoad::Empty);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| SqliteAccountError::sqlite("open", error))?;
    connection
        .execute_batch("BEGIN DEFERRED;")
        .map_err(|error| SqliteAccountError::sqlite("start a read transaction for", error))?;
    let result = load_in_transaction(path, &connection);
    let finish = if result.is_ok() {
        "COMMIT;"
    } else {
        "ROLLBACK;"
    };
    let finish_result = connection
        .execute_batch(finish)
        .map_err(|error| SqliteAccountError::sqlite("finish reading", error));
    match (result, finish_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

fn load_in_transaction(
    path: &Path,
    connection: &Connection,
) -> Result<SqliteAccountDocumentLoad, SqliteAccountError> {
    let snapshot = match reader::load_connection(connection)? {
        SqliteAccountLoad::Empty => return Ok(SqliteAccountDocumentLoad::Empty),
        SqliteAccountLoad::Incompatible(reason) => {
            return Ok(SqliteAccountDocumentLoad::Incompatible(reason));
        }
        SqliteAccountLoad::Loaded(snapshot) => snapshot,
    };
    let revision = database_revision(connection)?;
    let profile_persistence = metadata::profile(connection, &snapshot)?;
    let character_persistence = metadata::characters(connection, &snapshot)?;
    let item_persistence = metadata::items(connection, &snapshot)?;
    Ok(SqliteAccountDocumentLoad::Loaded(Box::new(
        SqliteAccountDocument {
            inventory_state: super::inventory_state::InventoryState::load(connection)?,
            progression: super::progression::Progression::load(connection)?,
            entitlements: super::entitlements::load(connection)?,
            runtime: super::runtime::load(connection)?,
            preserved_rows: load_preserved_rows(connection)?,
            path: path.to_path_buf(),
            revision,
            primary_soid: snapshot.primary_soid,
            profile_positions: snapshot
                .profile
                .profile_items()
                .iter()
                .enumerate()
                .map(|(i, item)| (item.id, i))
                .collect(),
            reward_positions: snapshot
                .profile
                .dismantle_rewards()
                .iter()
                .enumerate()
                .map(|(i, item)| (item.id, i))
                .collect(),
            profile: snapshot.profile,
            characters: snapshot.characters,
            settings: snapshot.settings,
            profile_persistence,
            character_persistence,
            item_persistence,
            pending_item_abilities: BTreeMap::new(),
        },
    )))
}

pub(super) fn database_revision(
    connection: &Connection,
) -> Result<SourceRevision, SqliteAccountError> {
    let schema_version: i64 = connection
        .query_row("PRAGMA user_version;", [], |row| row.get(0))
        .map_err(|error| SqliteAccountError::sqlite("read revision from", error))?;
    let mut hash = REVISION_OFFSET;
    feed(&mut hash, &schema_version.to_le_bytes());
    let mut names = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .map_err(|error| SqliteAccountError::sqlite("read revision from", error))?;
    let mut tables = names
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| SqliteAccountError::sqlite("read revision from", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SqliteAccountError::sqlite("read revision from", error))?;
    tables.push("sqlite_schema".into());
    for table in tables {
        feed(&mut hash, table.as_bytes());
        let escaped = table.replace('"', "\"\"");
        let probe = connection
            .prepare(&format!("SELECT * FROM \"{escaped}\""))
            .map_err(|error| SqliteAccountError::sqlite("read revision from", error))?;
        let order = (1..=probe.column_count())
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT * FROM \"{escaped}\" ORDER BY {order}");
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| SqliteAccountError::sqlite("read revision from", error))?;
        let column_count = statement.column_count();
        let mut rows = statement
            .query([])
            .map_err(|error| SqliteAccountError::sqlite("read revision from", error))?;
        while let Some(row) = rows
            .next()
            .map_err(|error| SqliteAccountError::sqlite("read revision from", error))?
        {
            feed(&mut hash, &[0xFF]);
            for column in 0..column_count {
                match row
                    .get_ref(column)
                    .map_err(|error| SqliteAccountError::sqlite("read revision from", error))?
                {
                    ValueRef::Null => feed(&mut hash, &[0]),
                    ValueRef::Integer(value) => {
                        feed(&mut hash, &[1]);
                        feed(&mut hash, &value.to_le_bytes());
                    }
                    ValueRef::Real(value) => {
                        feed(&mut hash, &[2]);
                        feed(&mut hash, &value.to_bits().to_le_bytes());
                    }
                    ValueRef::Text(value) => {
                        feed(&mut hash, &[3]);
                        feed_length(&mut hash, value.len());
                        feed(&mut hash, value);
                    }
                    ValueRef::Blob(value) => {
                        feed(&mut hash, &[4]);
                        feed_length(&mut hash, value.len());
                        feed(&mut hash, value);
                    }
                }
            }
        }
    }
    Ok(SourceRevision(hash))
}

fn feed_length(hash: &mut u64, length: usize) {
    feed(hash, &length.to_le_bytes());
}

fn feed(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(REVISION_PRIME);
    }
}

fn load_preserved_rows(
    db: &Connection,
) -> Result<BTreeMap<String, Vec<super::writer::NativeRow>>, SqliteAccountError> {
    [
        "items",
        "sockets",
        "profile_items",
        "dismantle_rewards",
        "entitlements",
        "character_stacks",
        "pending_rewards",
    ]
    .into_iter()
    .map(|table| Ok((table.into(), super::writer::rows(db, table)?)))
    .collect()
}

impl crate::persistence::native_account::NativeAccountDocument for SqliteAccountDocument {
    const LABEL: &'static str = "investment.sqlite3";
    fn profile(&self) -> &ProfileState {
        Self::profile(self)
    }
    fn profile_mut(&mut self) -> &mut ProfileState {
        Self::profile_mut(self)
    }
    fn characters(&self) -> &CharacterState {
        Self::characters(self)
    }
    fn characters_mut(&mut self) -> &mut CharacterState {
        Self::characters_mut(self)
    }
    fn settings(&self) -> &AccountSettingsState {
        Self::settings(self)
    }
    fn settings_mut(&mut self) -> &mut AccountSettingsState {
        Self::settings_mut(self)
    }
    fn next_entity_id(&self) -> Result<EntityId, String> {
        Self::next_entity_id(self).map_err(|error| error.to_string())
    }
    fn persisted_item_abilities(&self, id: EntityId) -> Option<CharacterAbilities> {
        Self::persisted_item_abilities(self, id)
    }
    fn set_persisted_item_abilities(&mut self, id: EntityId, abilities: CharacterAbilities) {
        Self::set_persisted_item_abilities(self, id, abilities);
    }
    fn profile_capabilities() -> ProfileCapabilities {
        Self::profile_capabilities()
    }
    fn character_capabilities() -> CharacterCapabilities {
        Self::character_capabilities()
    }
    fn settings_capabilities() -> AccountSettingsCapabilities {
        Self::settings_capabilities()
    }
}
