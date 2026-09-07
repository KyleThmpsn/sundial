#[cfg(test)]
use std::path::Path;
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

#[cfg(test)]
use rusqlite::OpenFlags;
use rusqlite::{Connection, OptionalExtension};
use sundial_account::{
    Character, CharacterAbilities, CharacterCapabilities, CharacterMetadata, CharacterState,
    DefinitionHash, DismantleGearClass, DismantleRarity, DismantleReward, EntityId, EquipmentSlot,
    InstanceSoid, ItemInstance, ItemPlugs, NO_DEFINITION_HASH, ProfileCapabilities, ProfileItem,
    ProfileState,
};

use super::{
    SqliteAccountError, SqliteAccountIncompatibility, SqliteAccountLoad, SqliteAccountSnapshot,
    contract::{
        ACCOUNT_FORMAT_VERSION, CHARACTER_CAPACITY, CHARACTER_ITEM_CAPACITY,
        DISMANTLE_REWARD_CAPACITY, EQUIPMENT_LOCATION, EQUIPMENT_SLOTS, INVENTORY_LOCATION,
        PLUG_CAPACITY, PROFILE_ITEM_CAPACITY, SCHEMA_VERSION, SETTINGS_PAYLOAD_VERSION, TABLES,
    },
    settings,
};

const DEFAULT_ABILITIES: CharacterAbilities = CharacterAbilities {
    movement: 4,
    grenade: 7,
    super_ability: 10,
    melee: 11,
    class_ability: 2,
};

#[cfg(test)]
pub(super) fn load(path: &Path) -> Result<SqliteAccountLoad, SqliteAccountError> {
    if !path.try_exists().map_err(SqliteAccountError::FileSystem)? {
        return Ok(SqliteAccountLoad::Missing);
    }
    if std::fs::metadata(path)
        .map_err(SqliteAccountError::FileSystem)?
        .len()
        == 0
    {
        return Ok(SqliteAccountLoad::Empty);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| SqliteAccountError::sqlite("open", error))?;
    load_connection(&connection)
}

pub(super) fn load_connection(
    connection: &Connection,
) -> Result<SqliteAccountLoad, SqliteAccountError> {
    let schema_version: i64 = connection
        .query_row("PRAGMA user_version;", [], |row| row.get(0))
        .map_err(|error| SqliteAccountError::sqlite("read schema version from", error))?;
    if schema_version == 0 && database_is_uninitialized(connection)? {
        return Ok(SqliteAccountLoad::Empty);
    }
    if schema_version > SCHEMA_VERSION {
        return Ok(SqliteAccountLoad::Incompatible(
            SqliteAccountIncompatibility::Schema {
                found: schema_version,
                supported: SCHEMA_VERSION,
            },
        ));
    }
    if schema_version != SCHEMA_VERSION {
        return Err(SqliteAccountError::InvalidSchema(format!(
            "expected user_version {SCHEMA_VERSION}, found {schema_version}"
        )));
    }
    validate_schema(connection)?;

    let Some(root) = load_root(connection)? else {
        ensure_child_tables_empty(connection)?;
        return Ok(SqliteAccountLoad::Empty);
    };
    if root.format_version > ACCOUNT_FORMAT_VERSION {
        return Ok(SqliteAccountLoad::Incompatible(
            SqliteAccountIncompatibility::AccountFormat {
                found: root.format_version,
                supported: ACCOUNT_FORMAT_VERSION,
            },
        ));
    }
    if root.format_version != ACCOUNT_FORMAT_VERSION {
        return Err(SqliteAccountError::invalid_data(
            "account_state.format_version",
            format!(
                "expected account format {ACCOUNT_FORMAT_VERSION}, found {}",
                root.format_version
            ),
        ));
    }
    let payload_version = settings::payload_version(&root.settings_payload)?;
    if payload_version > SETTINGS_PAYLOAD_VERSION {
        return Ok(SqliteAccountLoad::Incompatible(
            SqliteAccountIncompatibility::SettingsPayload {
                found: payload_version,
                supported: SETTINGS_PAYLOAD_VERSION,
            },
        ));
    }
    if payload_version != SETTINGS_PAYLOAD_VERSION {
        return Err(SqliteAccountError::invalid_data(
            "account_state.settings_payload.version",
            format!(
                "expected settings payload {SETTINGS_PAYLOAD_VERSION}, found {payload_version}"
            ),
        ));
    }

    let primary_soid = u64_from_sql(root.primary_soid);
    if primary_soid == 0 {
        if root.dismantle_reward_count != 0
            || root.profile_item_count != 0
            || root.character_count != 0
        {
            return Err(SqliteAccountError::invalid_data(
                "account_state",
                "an empty account cannot declare child rows",
            ));
        }
        settings::decode(&root.settings_payload, false)?;
        ensure_child_tables_empty(connection)?;
        return Ok(SqliteAccountLoad::Empty);
    }
    if primary_soid & (1_u64 << 63) == 0 {
        return Err(SqliteAccountError::invalid_data(
            "account_state.primary_soid",
            "nonempty accounts require a signed-negative SOID with bit 63 set",
        ));
    }
    let primary_soid = InstanceSoid::try_from_u64(primary_soid).ok_or_else(|| {
        SqliteAccountError::invalid_data("account_state.primary_soid", "SOID must be nonzero")
    })?;
    let settings = settings::decode(&root.settings_payload, true)?;
    let mut entity_ids = EntityIdAllocator::default();
    let (profile_items, mut reserved_soids) =
        load_profile_items(connection, root.profile_item_count, &mut entity_ids)?;
    reserved_soids.insert(0, primary_soid);
    let rewards = load_dismantle_rewards(connection, root.dismantle_reward_count, &mut entity_ids)?;
    let characters = load_characters(
        connection,
        root.character_count,
        &mut entity_ids,
        &reserved_soids,
    )?;
    validate_foreign_keys(connection)?;

    let profile = ProfileState::try_new(
        ProfileCapabilities {
            profile_items_writable: false,
            profile_item_capacity: Some(PROFILE_ITEM_CAPACITY),
            enforce_loaded_profile_item_capacity: true,
            dismantle_rewards_writable: false,
            dismantle_reward_capacity: Some(DISMANTLE_REWARD_CAPACITY),
            filtered_dismantle_rewards: true,
            combined_dismantle_gear_class: true,
        },
        profile_items,
        rewards,
    )?;
    Ok(SqliteAccountLoad::Loaded(SqliteAccountSnapshot {
        primary_soid,
        profile,
        characters,
        settings,
    }))
}

fn database_is_uninitialized(connection: &Connection) -> Result<bool, SqliteAccountError> {
    let table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%';",
            [],
            |row| row.get(0),
        )
        .map_err(|error| SqliteAccountError::sqlite("inspect uninitialized", error))?;
    Ok(table_count == 0)
}

pub(super) fn validate_schema(connection: &Connection) -> Result<(), SqliteAccountError> {
    let mut statement = connection
        .prepare(
            "SELECT name, type FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' \
             AND type IN ('table', 'view', 'trigger') ORDER BY name;",
        )
        .map_err(|error| SqliteAccountError::sqlite("inspect", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| SqliteAccountError::sqlite("inspect", error))?;
    let mut objects = BTreeMap::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("inspect", error))?
    {
        let name: String = row
            .get(0)
            .map_err(|error| SqliteAccountError::sqlite("inspect", error))?;
        let object_type: String = row
            .get(1)
            .map_err(|error| SqliteAccountError::sqlite("inspect", error))?;
        objects.insert(name, object_type);
    }
    let expected_names = TABLES
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    let actual_names = objects.keys().cloned().collect::<BTreeSet<_>>();
    if actual_names != expected_names {
        return Err(SqliteAccountError::InvalidSchema(format!(
            "expected tables {expected_names:?}, found {actual_names:?}"
        )));
    }
    if let Some((name, object_type)) = objects
        .iter()
        .find(|(_, object_type)| object_type.as_str() != "table")
    {
        return Err(SqliteAccountError::InvalidSchema(format!(
            "{name} must be a table, found {object_type}"
        )));
    }

    for (table, expected_columns) in TABLES {
        let sql = format!("PRAGMA table_info({table});");
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| SqliteAccountError::sqlite("inspect", error))?;
        let mut rows = statement
            .query([])
            .map_err(|error| SqliteAccountError::sqlite("inspect", error))?;
        let mut found = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| SqliteAccountError::sqlite("inspect", error))?
        {
            found.push((
                row.get::<_, String>(1)
                    .map_err(|error| SqliteAccountError::sqlite("inspect", error))?,
                row.get::<_, String>(2)
                    .map_err(|error| SqliteAccountError::sqlite("inspect", error))?,
                row.get::<_, i64>(3)
                    .map_err(|error| SqliteAccountError::sqlite("inspect", error))?
                    != 0,
                row.get::<_, i64>(5)
                    .map_err(|error| SqliteAccountError::sqlite("inspect", error))?,
            ));
        }
        if found.len() != expected_columns.len() {
            return Err(SqliteAccountError::InvalidSchema(format!(
                "table {table} has {} columns; expected {}",
                found.len(),
                expected_columns.len()
            )));
        }
        for (index, (actual, expected)) in found.iter().zip(expected_columns).enumerate() {
            let expected_tuple = (
                expected.name,
                expected.declared_type,
                expected.not_null,
                expected.primary_key_position,
            );
            if (actual.0.as_str(), actual.1.as_str(), actual.2, actual.3) != expected_tuple {
                return Err(SqliteAccountError::InvalidSchema(format!(
                    "table {table} column {index} is {actual:?}; expected {expected_tuple:?}"
                )));
            }
        }
    }
    Ok(())
}

struct RootRow {
    format_version: i64,
    primary_soid: i64,
    dismantle_reward_count: usize,
    profile_item_count: usize,
    character_count: usize,
    settings_payload: Vec<u8>,
}

fn load_root(connection: &Connection) -> Result<Option<RootRow>, SqliteAccountError> {
    let mut statement = connection
        .prepare(
            "SELECT singleton, format_version, primary_soid, dismantle_reward_count, \
             profile_item_count, character_count, settings_payload FROM account_state \
             ORDER BY singleton;",
        )
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let Some(row) = rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("read", error))?
    else {
        return Ok(None);
    };
    let singleton = row_i64(row, 0, "account_state.singleton")?;
    if singleton != 1 {
        return Err(SqliteAccountError::invalid_data(
            "account_state.singleton",
            format!("expected 1, found {singleton}"),
        ));
    }
    let root = RootRow {
        format_version: row_i64(row, 1, "account_state.format_version")?,
        primary_soid: row_i64(row, 2, "account_state.primary_soid")?,
        dismantle_reward_count: row_count(
            row,
            3,
            DISMANTLE_REWARD_CAPACITY,
            "account_state.dismantle_reward_count",
        )?,
        profile_item_count: row_count(
            row,
            4,
            PROFILE_ITEM_CAPACITY,
            "account_state.profile_item_count",
        )?,
        character_count: row_count(row, 5, CHARACTER_CAPACITY, "account_state.character_count")?,
        settings_payload: row
            .get(6)
            .map_err(|error| SqliteAccountError::sqlite("read", error))?,
    };
    if rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("read", error))?
        .is_some()
    {
        return Err(SqliteAccountError::invalid_data(
            "account_state",
            "expected exactly one singleton row",
        ));
    }
    Ok(Some(root))
}

fn load_profile_items(
    connection: &Connection,
    expected_count: usize,
    entity_ids: &mut EntityIdAllocator,
) -> Result<(Vec<ProfileItem>, Vec<InstanceSoid>), SqliteAccountError> {
    let mut statement = connection
        .prepare(
            "SELECT position, instance_soid, definition_hash, quantity, mutation_serial \
             FROM profile_items WHERE account_id = 1 ORDER BY position;",
        )
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let mut items = Vec::with_capacity(expected_count);
    let mut soids = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("read", error))?
    {
        let index = items.len();
        require_position(row, 0, index, PROFILE_ITEM_CAPACITY, "profile_items")?;
        if index >= expected_count {
            return Err(count_mismatch("profile_items", expected_count, index + 1));
        }
        let instance_soid = u64_from_sql(row_i64(
            row,
            1,
            &format!("profile_items[{index}].instance_soid"),
        )?);
        if let Some(instance_soid) = InstanceSoid::try_from_u64(instance_soid) {
            soids.push(instance_soid);
        }
        let definition_hash = row_u32(row, 2, &format!("profile_items[{index}].definition_hash"))?;
        require_definition_hash(definition_hash, &format!("profile_items[{index}]"))?;
        let quantity = row_i32(row, 3, &format!("profile_items[{index}].quantity"))?;
        if quantity <= 0 {
            return Err(SqliteAccountError::invalid_data(
                format!("profile_items[{index}].quantity"),
                "quantity must be positive",
            ));
        }
        let mutation_serial = row_i32(row, 4, &format!("profile_items[{index}].mutation_serial"))?;
        if mutation_serial < 0 {
            return Err(SqliteAccountError::invalid_data(
                format!("profile_items[{index}].mutation_serial"),
                "mutation serial must be nonnegative",
            ));
        }
        items.push(ProfileItem {
            id: entity_ids.next()?,
            definition_hash: DefinitionHash::new(definition_hash),
            quantity,
        });
    }
    if items.len() != expected_count {
        return Err(count_mismatch("profile_items", expected_count, items.len()));
    }
    Ok((items, soids))
}

fn load_dismantle_rewards(
    connection: &Connection,
    expected_count: usize,
    entity_ids: &mut EntityIdAllocator,
) -> Result<Vec<DismantleReward>, SqliteAccountError> {
    let mut statement = connection
        .prepare(
            "SELECT position, definition_hash, quantity, tier_mask, class_mask, masterwork \
             FROM dismantle_rewards WHERE account_id = 1 ORDER BY position;",
        )
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let mut rewards = Vec::with_capacity(expected_count);
    while let Some(row) = rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("read", error))?
    {
        let index = rewards.len();
        require_position(
            row,
            0,
            index,
            DISMANTLE_REWARD_CAPACITY,
            "dismantle_rewards",
        )?;
        if index >= expected_count {
            return Err(count_mismatch(
                "dismantle_rewards",
                expected_count,
                index + 1,
            ));
        }
        let definition_hash = row_u32(
            row,
            1,
            &format!("dismantle_rewards[{index}].definition_hash"),
        )?;
        require_definition_hash(definition_hash, &format!("dismantle_rewards[{index}]"))?;
        let quantity = row_i32(row, 2, &format!("dismantle_rewards[{index}].quantity"))?;
        if quantity <= 0 {
            return Err(SqliteAccountError::invalid_data(
                format!("dismantle_rewards[{index}].quantity"),
                "quantity must be positive",
            ));
        }
        let tier_mask = row_u8(row, 3, &format!("dismantle_rewards[{index}].tier_mask"))?;
        if tier_mask & !0b0011_1110 != 0 {
            return Err(SqliteAccountError::invalid_data(
                format!("dismantle_rewards[{index}].tier_mask"),
                "only native rarity bits 1 through 5 may be set",
            ));
        }
        let rarities = DismantleRarity::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(rarity_index, rarity)| {
                (tier_mask & (1 << (rarity_index + 1)) != 0).then_some(rarity)
            })
            .collect();
        let class_mask = row_u8(row, 4, &format!("dismantle_rewards[{index}].class_mask"))?;
        let gear_class = match class_mask {
            0 => None,
            1 => Some(DismantleGearClass::Weapon),
            2 => Some(DismantleGearClass::Armor),
            3 => Some(DismantleGearClass::Both),
            _ => {
                return Err(SqliteAccountError::invalid_data(
                    format!("dismantle_rewards[{index}].class_mask"),
                    "only weapon and armor bits may be set",
                ));
            }
        };
        let masterworked = match row_u8(row, 5, &format!("dismantle_rewards[{index}].masterwork"))?
        {
            0 => None,
            1 => Some(true),
            2 => Some(false),
            value => {
                return Err(SqliteAccountError::invalid_data(
                    format!("dismantle_rewards[{index}].masterwork"),
                    format!("expected 0, 1, or 2, found {value}"),
                ));
            }
        };
        rewards.push(DismantleReward {
            id: entity_ids.next()?,
            definition_hash: DefinitionHash::new(definition_hash),
            quantity,
            rarities,
            gear_class,
            masterworked,
        });
    }
    if rewards.len() != expected_count {
        return Err(count_mismatch(
            "dismantle_rewards",
            expected_count,
            rewards.len(),
        ));
    }
    Ok(rewards)
}

fn load_characters(
    connection: &Connection,
    expected_count: usize,
    entity_ids: &mut EntityIdAllocator,
    reserved_soids: &[InstanceSoid],
) -> Result<CharacterState, SqliteAccountError> {
    let mut statement = connection
        .prepare(
            "SELECT position, soid, selected, race, gender, character_class, level, accepted, \
             preview_available, appearance_value, last_orbited_destination, content_bypass, \
             acquired_subclass_ability_mask, inventory_count, next_inventory_serial \
             FROM characters WHERE account_id = 1 ORDER BY position;",
        )
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let mut loaded = Vec::with_capacity(expected_count);
    let mut selected_seen = false;
    while let Some(row) = rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("read", error))?
    {
        let index = loaded.len();
        require_position(row, 0, index, CHARACTER_CAPACITY, "characters")?;
        if index >= expected_count {
            return Err(count_mismatch("characters", expected_count, index + 1));
        }
        let soid = InstanceSoid::try_from_u64(u64_from_sql(row_i64(
            row,
            1,
            &format!("characters[{index}].soid"),
        )?))
        .ok_or_else(|| {
            SqliteAccountError::invalid_data(
                format!("characters[{index}].soid"),
                "SOID must be nonzero",
            )
        })?;
        let selected = row_bool(row, 2, &format!("characters[{index}].selected"))?;
        if selected && selected_seen {
            return Err(SqliteAccountError::invalid_data(
                "characters.selected",
                "at most one character may be selected",
            ));
        }
        selected_seen |= selected;
        let race = row_u8(row, 3, &format!("characters[{index}].race"))?;
        let gender = row_u8(row, 4, &format!("characters[{index}].gender"))?;
        let class_type = row_u8(row, 5, &format!("characters[{index}].character_class"))?;
        if race > 2 || gender > 1 || class_type > 2 {
            return Err(SqliteAccountError::invalid_data(
                format!("characters[{index}]"),
                "race, gender, or class is outside the supported domain",
            ));
        }
        let _level = row_u8(row, 6, &format!("characters[{index}].level"))?;
        let _accepted = row_bool(row, 7, &format!("characters[{index}].accepted"))?;
        let _preview_available =
            row_bool(row, 8, &format!("characters[{index}].preview_available"))?;
        let appearance = row_f32(row, 9, &format!("characters[{index}].appearance_value"))?;
        if !appearance.is_finite() {
            return Err(SqliteAccountError::invalid_data(
                format!("characters[{index}].appearance_value"),
                "appearance value must be finite",
            ));
        }
        let _last_orbited_destination = row_u32(
            row,
            10,
            &format!("characters[{index}].last_orbited_destination"),
        )?;
        let _content_bypass = row_bool(row, 11, &format!("characters[{index}].content_bypass"))?;
        let _acquired_mask = row_i64(
            row,
            12,
            &format!("characters[{index}].acquired_subclass_ability_mask"),
        )?;
        let inventory_count = row_count(
            row,
            13,
            CHARACTER_ITEM_CAPACITY,
            &format!("characters[{index}].inventory_count"),
        )?;
        let _next_inventory_serial = row_u32(
            row,
            14,
            &format!("characters[{index}].next_inventory_serial"),
        )?;
        let mut character = Character {
            id: entity_ids.next()?,
            soid: Some(soid),
            metadata: Some(CharacterMetadata {
                race,
                gender,
                class_type,
                abilities: DEFAULT_ABILITIES,
            }),
            inventory: Vec::with_capacity(inventory_count),
            equipment: EQUIPMENT_SLOTS
                .into_iter()
                .map(|slot| (EquipmentSlot::new(slot), None))
                .collect(),
        };
        load_items(
            connection,
            index,
            inventory_count,
            entity_ids,
            &mut character,
        )?;
        loaded.push(character);
    }
    if loaded.len() != expected_count {
        return Err(count_mismatch("characters", expected_count, loaded.len()));
    }
    CharacterState::try_new(
        CharacterCapabilities {
            metadata_writable: false,
            inventory_writable: false,
            equipment_writable: false,
            equipment_flags_writable: false,
            inventory_capacity: Some(CHARACTER_ITEM_CAPACITY),
            enforce_loaded_inventory_capacity: true,
            max_item_plugs: PLUG_CAPACITY,
            item_flag_mask: u32::MAX,
            enforce_unique_instance_soids: true,
        },
        reserved_soids.to_vec(),
        loaded,
    )
    .map_err(Into::into)
}

fn load_items(
    connection: &Connection,
    character_position: usize,
    expected_inventory_count: usize,
    entity_ids: &mut EntityIdAllocator,
    character: &mut Character,
) -> Result<(), SqliteAccountError> {
    let mut statement = connection
        .prepare(
            "SELECT location, position, instance_soid, definition_hash, item_level, quantity, \
             mutation_serial, flags, socket_policy, plug_count, movement_ability_entry, \
             grenade_ability_entry, super_ability_entry, melee_ability_entry, \
             class_ability_entry FROM character_items WHERE account_id = 1 \
             AND character_position = ? ORDER BY location, position;",
        )
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let character_position_sql =
        sql_index(character_position, "character_items.character_position")?;
    let mut rows = statement
        .query([character_position_sql])
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let mut equipment_seen = BTreeSet::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("read", error))?
    {
        let location = row_i64(row, 0, "character_items.location")?;
        let capacity = match location {
            EQUIPMENT_LOCATION => EQUIPMENT_SLOTS.len(),
            INVENTORY_LOCATION => CHARACTER_ITEM_CAPACITY,
            _ => {
                return Err(SqliteAccountError::invalid_data(
                    "character_items.location",
                    format!("expected 0 or 1, found {location}"),
                ));
            }
        };
        let position = row_count(row, 1, capacity, "character_items.position")?;
        if position >= capacity {
            return Err(SqliteAccountError::invalid_data(
                "character_items.position",
                format!("position {position} is outside capacity {capacity}"),
            ));
        }
        if location == EQUIPMENT_LOCATION && !equipment_seen.insert(position) {
            return Err(SqliteAccountError::invalid_data(
                "character_items.position",
                format!("equipment position {position} is duplicated"),
            ));
        }
        if location == INVENTORY_LOCATION && position != character.inventory.len() {
            return Err(SqliteAccountError::invalid_data(
                "character_items.position",
                format!(
                    "inventory positions must be contiguous; expected {}, found {position}",
                    character.inventory.len()
                ),
            ));
        }
        let item_path = format!("character_items[{character_position},{location},{position}]");
        let instance_soid = InstanceSoid::try_from_u64(u64_from_sql(row_i64(
            row,
            2,
            &format!("{item_path}.instance_soid"),
        )?))
        .ok_or_else(|| {
            SqliteAccountError::invalid_data(
                format!("{item_path}.instance_soid"),
                "SOID must be nonzero",
            )
        })?;
        let definition_hash = row_u32(row, 3, &format!("{item_path}.definition_hash"))?;
        require_definition_hash(definition_hash, &item_path)?;
        let level = row_i32(row, 4, &format!("{item_path}.item_level"))?;
        if level < 0 {
            return Err(SqliteAccountError::invalid_data(
                format!("{item_path}.item_level"),
                "item level must be nonnegative",
            ));
        }
        let quantity = row_i32(row, 5, &format!("{item_path}.quantity"))?;
        if quantity <= 0 {
            return Err(SqliteAccountError::invalid_data(
                format!("{item_path}.quantity"),
                "quantity must be positive",
            ));
        }
        let mutation_serial = row_i32(row, 6, &format!("{item_path}.mutation_serial"))?;
        if mutation_serial < 0 {
            return Err(SqliteAccountError::invalid_data(
                format!("{item_path}.mutation_serial"),
                "mutation serial must be nonnegative",
            ));
        }
        let flags = row_u32(row, 7, &format!("{item_path}.flags"))?;
        let socket_policy = row_u8(row, 8, &format!("{item_path}.socket_policy"))?;
        let plug_count = row_count(row, 9, PLUG_CAPACITY, &format!("{item_path}.plug_count"))?;
        let abilities = CharacterAbilities {
            movement: row_u8(row, 10, &format!("{item_path}.movement_ability_entry"))?,
            grenade: row_u8(row, 11, &format!("{item_path}.grenade_ability_entry"))?,
            super_ability: row_u8(row, 12, &format!("{item_path}.super_ability_entry"))?,
            melee: row_u8(row, 13, &format!("{item_path}.melee_ability_entry"))?,
            class_ability: row_u8(row, 14, &format!("{item_path}.class_ability_entry"))?,
        };
        let plugs = load_plugs(
            connection,
            character_position,
            location,
            position,
            plug_count,
        )?;
        let plugs = match socket_policy {
            0 if plug_count == 0 => ItemPlugs::NativeDefaults,
            0 => {
                return Err(SqliteAccountError::invalid_data(
                    format!("{item_path}.socket_policy"),
                    "native-default sockets cannot have persisted plugs",
                ));
            }
            1 => ItemPlugs::Authored(plugs),
            value => {
                return Err(SqliteAccountError::invalid_data(
                    format!("{item_path}.socket_policy"),
                    format!("expected 0 or 1, found {value}"),
                ));
            }
        };
        let item = ItemInstance {
            id: entity_ids.next()?,
            instance_soid,
            definition_hash: DefinitionHash::new(definition_hash),
            level,
            quantity,
            plugs,
            flags: Some(flags),
        };
        if location == EQUIPMENT_LOCATION {
            let slot = EQUIPMENT_SLOTS.get(position).copied().ok_or_else(|| {
                SqliteAccountError::invalid_data(
                    "character_items.position",
                    format!("equipment position {position} is outside the slot contract"),
                )
            })?;
            character
                .equipment
                .insert(EquipmentSlot::new(slot), Some(item));
            if slot == "subclass" {
                let metadata = character.metadata.as_mut().ok_or_else(|| {
                    SqliteAccountError::invalid_data(
                        format!("characters[{character_position}].metadata"),
                        "subclass abilities require character metadata",
                    )
                })?;
                metadata.abilities = abilities;
            }
        } else {
            character.inventory.push(item);
        }
    }
    if character.inventory.len() != expected_inventory_count {
        return Err(count_mismatch(
            &format!("characters[{character_position}].inventory"),
            expected_inventory_count,
            character.inventory.len(),
        ));
    }
    Ok(())
}

fn load_plugs(
    connection: &Connection,
    character_position: usize,
    location: i64,
    item_position: usize,
    expected_count: usize,
) -> Result<Vec<Option<DefinitionHash>>, SqliteAccountError> {
    let mut statement = connection
        .prepare(
            "SELECT plug_position, definition_hash FROM item_plugs WHERE account_id = 1 \
             AND character_position = ? AND location = ? AND item_position = ? \
             ORDER BY plug_position;",
        )
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let parameters = [
        sql_index(character_position, "item_plugs.character_position")?,
        location,
        sql_index(item_position, "item_plugs.item_position")?,
    ];
    let mut rows = statement
        .query(parameters)
        .map_err(|error| SqliteAccountError::sqlite("read", error))?;
    let mut plugs = Vec::with_capacity(expected_count);
    while let Some(row) = rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("read", error))?
    {
        let index = plugs.len();
        require_position(row, 0, index, PLUG_CAPACITY, "item_plugs")?;
        if index >= expected_count {
            return Err(count_mismatch("item_plugs", expected_count, index + 1));
        }
        let hash = row
            .get::<_, Option<i64>>(1)
            .map_err(|error| SqliteAccountError::sqlite("read", error))?
            .map(|value| {
                let hash = u32_from_i64(value, "item_plugs.definition_hash")?;
                require_definition_hash(hash, "item_plugs")?;
                Ok::<DefinitionHash, SqliteAccountError>(DefinitionHash::new(hash))
            })
            .transpose()?;
        plugs.push(hash);
    }
    if plugs.len() != expected_count {
        return Err(count_mismatch("item_plugs", expected_count, plugs.len()));
    }
    Ok(plugs)
}

fn validate_foreign_keys(connection: &Connection) -> Result<(), SqliteAccountError> {
    let violation: Option<(String, i64)> = connection
        .query_row("PRAGMA foreign_key_check;", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()
        .map_err(|error| SqliteAccountError::sqlite("validate", error))?;
    if let Some((table, row_id)) = violation {
        return Err(SqliteAccountError::invalid_data(
            table,
            format!("row {row_id} violates a foreign key"),
        ));
    }
    Ok(())
}

fn ensure_child_tables_empty(connection: &Connection) -> Result<(), SqliteAccountError> {
    for table in [
        "dismantle_rewards",
        "profile_items",
        "characters",
        "character_items",
        "item_plugs",
    ] {
        let sql = format!("SELECT COUNT(*) FROM {table};");
        let count: i64 = connection
            .query_row(&sql, [], |row| row.get(0))
            .map_err(|error| SqliteAccountError::sqlite("read", error))?;
        if count != 0 {
            return Err(SqliteAccountError::invalid_data(
                table,
                format!("expected no rows without an account root, found {count}"),
            ));
        }
    }
    Ok(())
}

fn row_i64(
    row: &rusqlite::Row<'_>,
    index: usize,
    location: &str,
) -> Result<i64, SqliteAccountError> {
    row.get(index).map_err(|error| {
        SqliteAccountError::invalid_data(location, format!("expected an integer: {error}"))
    })
}

fn row_i32(
    row: &rusqlite::Row<'_>,
    index: usize,
    location: &str,
) -> Result<i32, SqliteAccountError> {
    let value = row_i64(row, index, location)?;
    i32::try_from(value).map_err(|_| {
        SqliteAccountError::invalid_data(location, format!("{value} does not fit in i32"))
    })
}

fn row_u32(
    row: &rusqlite::Row<'_>,
    index: usize,
    location: &str,
) -> Result<u32, SqliteAccountError> {
    u32_from_i64(row_i64(row, index, location)?, location)
}

fn u32_from_i64(value: i64, location: &str) -> Result<u32, SqliteAccountError> {
    u32::try_from(value).map_err(|_| {
        SqliteAccountError::invalid_data(location, format!("{value} does not fit in u32"))
    })
}

fn row_u8(row: &rusqlite::Row<'_>, index: usize, location: &str) -> Result<u8, SqliteAccountError> {
    let value = row_i64(row, index, location)?;
    u8::try_from(value).map_err(|_| {
        SqliteAccountError::invalid_data(location, format!("{value} does not fit in u8"))
    })
}

fn row_bool(
    row: &rusqlite::Row<'_>,
    index: usize,
    location: &str,
) -> Result<bool, SqliteAccountError> {
    match row_i64(row, index, location)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(SqliteAccountError::invalid_data(
            location,
            format!("expected 0 or 1, found {value}"),
        )),
    }
}

fn row_count(
    row: &rusqlite::Row<'_>,
    index: usize,
    capacity: usize,
    location: &str,
) -> Result<usize, SqliteAccountError> {
    let value = row_i64(row, index, location)?;
    let value = usize::try_from(value)
        .map_err(|_| SqliteAccountError::invalid_data(location, "count must be nonnegative"))?;
    if value > capacity {
        return Err(SqliteAccountError::invalid_data(
            location,
            format!("count {value} exceeds capacity {capacity}"),
        ));
    }
    Ok(value)
}

fn sql_index(value: usize, location: &str) -> Result<i64, SqliteAccountError> {
    i64::try_from(value).map_err(|_| {
        SqliteAccountError::invalid_data(location, format!("index {value} does not fit in i64"))
    })
}

fn row_f32(
    row: &rusqlite::Row<'_>,
    index: usize,
    location: &str,
) -> Result<f32, SqliteAccountError> {
    let value: f64 = row.get(index).map_err(|error| {
        SqliteAccountError::invalid_data(location, format!("expected a real value: {error}"))
    })?;
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(SqliteAccountError::invalid_data(
            location,
            "real value must fit in a finite f32",
        ));
    }
    Ok(value as f32)
}

fn require_position(
    row: &rusqlite::Row<'_>,
    column: usize,
    expected: usize,
    capacity: usize,
    table: &str,
) -> Result<(), SqliteAccountError> {
    let position = row_count(row, column, capacity, &format!("{table}.position"))?;
    if position == expected {
        Ok(())
    } else {
        Err(SqliteAccountError::invalid_data(
            format!("{table}.position"),
            format!("expected contiguous position {expected}, found {position}"),
        ))
    }
}

fn require_definition_hash(hash: u32, location: &str) -> Result<(), SqliteAccountError> {
    if hash == NO_DEFINITION_HASH.get() {
        Err(SqliteAccountError::invalid_data(
            format!("{location}.definition_hash"),
            "the no-definition sentinel cannot identify persisted state",
        ))
    } else {
        Ok(())
    }
}

const fn u64_from_sql(value: i64) -> u64 {
    value as u64
}

fn count_mismatch(table: &str, expected: usize, found: usize) -> SqliteAccountError {
    SqliteAccountError::invalid_data(
        table,
        format!("root count is {expected}, but {found} rows were found"),
    )
}

#[derive(Default)]
struct EntityIdAllocator {
    next: u64,
}

impl EntityIdAllocator {
    fn next(&mut self) -> Result<EntityId, SqliteAccountError> {
        let value = self
            .next
            .checked_add(1)
            .ok_or(SqliteAccountError::EntityIdentityExhausted)?;
        let id = NonZeroU64::new(value).ok_or(SqliteAccountError::EntityIdentityExhausted)?;
        self.next = value;
        Ok(EntityId::new(id))
    }
}
