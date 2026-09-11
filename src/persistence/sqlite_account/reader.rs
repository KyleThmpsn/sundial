use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

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
        APPLICATION_ID, CHARACTER_CAPACITY, CHARACTER_ITEM_CAPACITY, DISMANTLE_REWARD_CAPACITY,
        EQUIPMENT_LOCATION, EQUIPMENT_SLOTS, INVENTORY_LOCATION, PLUG_CAPACITY,
        PROFILE_ITEM_CAPACITY, SCHEMA, SCHEMA_VERSION, SETTINGS_SCHEMA,
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
    super::validation::validate(connection)?;

    let primary_soid: i64 = connection
        .query_row("SELECT soid FROM account WHERE id=1", [], |row| row.get(0))
        .map_err(|error| SqliteAccountError::sqlite("read account from", error))?;
    let primary_soid = u64_from_sql(primary_soid);
    let settings = settings::load(connection)?;
    let primary_soid = InstanceSoid::try_from_u64(primary_soid).ok_or_else(|| {
        SqliteAccountError::invalid_data("account_state.primary_soid", "SOID must be nonzero")
    })?;

    let mut entity_ids = EntityIdAllocator::default();
    let (profile_items, mut reserved_soids) = load_profile_items(
        connection,
        table_count(connection, "profile_items")?,
        &mut entity_ids,
    )?;
    reserved_soids.insert(0, primary_soid);
    let rewards = load_dismantle_rewards(
        connection,
        table_count(connection, "dismantle_rewards")?,
        &mut entity_ids,
    )?;
    let characters = load_characters(
        connection,
        table_count(connection, "characters")?,
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
    let application: i64 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|error| SqliteAccountError::sqlite("read application identifier from", error))?;
    if application != APPLICATION_ID {
        return Err(SqliteAccountError::InvalidSchema(format!(
            "expected Sunrise application ID {APPLICATION_ID}, found {application}"
        )));
    }
    // Compare required columns against the shipped schema. Additional columns and tables are
    // retained. Triggers and substituted views are rejected before any account edits are enabled.
    let reference = Connection::open_in_memory()
        .map_err(|error| SqliteAccountError::sqlite("validate contract for", error))?;
    reference
        .execute_batch(SCHEMA)
        .and_then(|()| reference.execute_batch(SETTINGS_SCHEMA))
        .map_err(|error| SqliteAccountError::sqlite("validate contract for", error))?;
    let mut names = reference
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .map_err(|error| SqliteAccountError::sqlite("inspect contract for", error))?;
    let tables = names
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| SqliteAccountError::sqlite("inspect contract for", error))?;
    for table in tables {
        let table =
            table.map_err(|error| SqliteAccountError::sqlite("inspect contract for", error))?;
        let kind: Option<String> = connection
            .query_row(
                "SELECT type FROM sqlite_schema WHERE name=?",
                [&table],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| SqliteAccountError::sqlite("inspect", error))?;
        if kind.as_deref() != Some("table") {
            return Err(SqliteAccountError::InvalidSchema(format!(
                "missing Sunrise table {table}"
            )));
        }
        let expected = columns(&reference, &table)?;
        let actual = columns(connection, &table)?;
        for (name, contract) in expected {
            if actual.get(&name) != Some(&contract) {
                return Err(SqliteAccountError::InvalidSchema(format!(
                    "incompatible column {table}.{name}"
                )));
            }
        }
    }
    let triggers: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='trigger'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| SqliteAccountError::sqlite("inspect", error))?;
    if triggers != 0 {
        return Err(SqliteAccountError::InvalidSchema(
            "unexpected database triggers".into(),
        ));
    }
    validate_foreign_keys(connection)
}

fn columns(
    connection: &Connection,
    table: &str,
) -> Result<BTreeMap<String, (String, bool, i64)>, SqliteAccountError> {
    let mut statement = connection
        .prepare("SELECT name, type, [notnull], pk FROM pragma_table_info(?)")
        .map_err(|error| SqliteAccountError::sqlite("inspect columns of", error))?;
    let rows = statement
        .query_map([table], |row| {
            Ok((
                row.get(0)?,
                (row.get(1)?, row.get::<_, i64>(2)? != 0, row.get(3)?),
            ))
        })
        .map_err(|error| SqliteAccountError::sqlite("inspect columns of", error))?;
    rows.collect::<Result<_, _>>()
        .map_err(|error| SqliteAccountError::sqlite("inspect columns of", error))
}

fn table_count(connection: &Connection, table: &str) -> Result<usize, SqliteAccountError> {
    connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .map_err(|error| SqliteAccountError::sqlite("count rows in", error))
}

fn load_profile_items(
    connection: &Connection,
    expected_count: usize,
    entity_ids: &mut EntityIdAllocator,
) -> Result<(Vec<ProfileItem>, Vec<InstanceSoid>), SqliteAccountError> {
    let mut statement = connection
        .prepare(
            "SELECT position, instance_soid, definition_hash, quantity, mutation_serial \
             FROM profile_items ORDER BY position;",
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
             FROM dismantle_rewards ORDER BY position;",
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
            "SELECT slot, soid, 0, race, gender, class, level, 1, \
             preview_available, appearance_value, last_orbited_destination, content_bypass, \
             acquired_subclass_mask, (SELECT count(*) FROM items WHERE character_slot=characters.slot AND location=1), next_inventory_serial \
             FROM characters ORDER BY slot;",
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
            item_flag_mask: 7,
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
            "SELECT location, position, instance_soid, definition_hash, level, quantity, \
             mutation_serial, flags, socket_policy, plug_count, movement_ability, \
             grenade_ability, super_ability, melee_ability, \
             class_ability FROM items WHERE character_slot = (SELECT slot FROM characters ORDER BY slot LIMIT 1 OFFSET ?) ORDER BY location, position;",
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
                    "inventory positions must be contiguous. Expected {}, found {position}",
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
            movement: row_u8(row, 10, &format!("{item_path}.movement_ability"))?,
            grenade: row_u8(row, 11, &format!("{item_path}.grenade_ability"))?,
            super_ability: row_u8(row, 12, &format!("{item_path}.super_ability"))?,
            melee: row_u8(row, 13, &format!("{item_path}.melee_ability"))?,
            class_ability: row_u8(row, 14, &format!("{item_path}.class_ability"))?,
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
    let mut statement = connection.prepare("SELECT lane, plug_hash FROM sockets WHERE instance_soid = (SELECT instance_soid FROM items WHERE character_slot=(SELECT slot FROM characters ORDER BY slot LIMIT 1 OFFSET ?) AND location=? AND position=?) ORDER BY lane")
        .map_err(|error| SqliteAccountError::sqlite("read sockets from", error))?;
    let mut rows = statement
        .query(rusqlite::params![
            character_position,
            location,
            item_position
        ])
        .map_err(|error| SqliteAccountError::sqlite("read sockets from", error))?;
    let mut plugs = vec![None; expected_count];
    while let Some(row) = rows
        .next()
        .map_err(|error| SqliteAccountError::sqlite("read sockets from", error))?
    {
        let lane = row_count(row, 0, PLUG_CAPACITY, "sockets.lane")?;
        let hash = row_u32(row, 1, "sockets.plug_hash")?;
        require_definition_hash(hash, "sockets.plug_hash")?;
        let value = plugs.get_mut(lane).ok_or_else(|| {
            SqliteAccountError::invalid_data(
                "sockets.lane",
                "socket lane exceeds authored plug count",
            )
        })?;
        *value = Some(DefinitionHash::new(hash));
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
