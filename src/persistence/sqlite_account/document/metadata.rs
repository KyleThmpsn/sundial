//! Match native persistence metadata to the validated account entities.
use super::*;

pub(super) fn profile(
    connection: &Connection,
    snapshot: &SqliteAccountSnapshot,
) -> Result<BTreeMap<EntityId, ProfilePersistence>, SqliteAccountError> {
    let error = |error| SqliteAccountError::sqlite("read profile metadata from", error);
    let mut statement = connection
        .prepare(
            "SELECT position, instance_soid, mutation_serial FROM profile_items \
             ORDER BY position;",
        )
        .map_err(error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                ProfilePersistence {
                    instance_soid: row.get::<_, i64>(1)? as u64,
                    mutation_serial: row.get(2)?,
                },
            ))
        })
        .map_err(error)?;
    rows.map(|row| {
        let (position, persistence) = row.map_err(error)?;
        let item = snapshot
            .profile
            .profile_items()
            .get(position)
            .ok_or_else(|| {
                SqliteAccountError::invalid_data(
                    "profile_items.position",
                    "metadata position does not identify a loaded profile item",
                )
            })?;
        Ok((item.id, persistence))
    })
    .collect()
}

pub(super) fn characters(
    connection: &Connection,
    snapshot: &SqliteAccountSnapshot,
) -> Result<BTreeMap<EntityId, CharacterPersistence>, SqliteAccountError> {
    let error = |error| SqliteAccountError::sqlite("read character metadata from", error);
    let mut statement = connection
        .prepare(
            "SELECT row_number() OVER (ORDER BY slot)-1, next_inventory_serial FROM characters ORDER BY slot;",
        )
        .map_err(error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                CharacterPersistence {
                    next_inventory_serial: row.get(1)?,
                },
            ))
        })
        .map_err(error)?;
    rows.map(|row| {
        let (position, persistence) = row.map_err(error)?;
        let character = snapshot
            .characters
            .characters()
            .get(position)
            .ok_or_else(|| {
                SqliteAccountError::invalid_data(
                    "characters.position",
                    "metadata position does not identify a loaded character",
                )
            })?;
        Ok((character.id, persistence))
    })
    .collect()
}

pub(super) fn items(
    connection: &Connection,
    snapshot: &SqliteAccountSnapshot,
) -> Result<BTreeMap<EntityId, ItemPersistence>, SqliteAccountError> {
    let error = |error| SqliteAccountError::sqlite("read item metadata from", error);
    let mut statement = connection
        .prepare(
            "SELECT (SELECT count(*) FROM characters WHERE slot < items.character_slot), location, position, mutation_serial, \
             movement_ability, grenade_ability, super_ability, \
             melee_ability, class_ability FROM items ORDER BY character_slot, location, position;",
        )
        .map_err(error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, usize>(2)?,
                ItemPersistence {
                    mutation_serial: row.get(3)?,
                    abilities: CharacterAbilities {
                        movement: row.get(4)?,
                        grenade: row.get(5)?,
                        super_ability: row.get(6)?,
                        melee: row.get(7)?,
                        class_ability: row.get(8)?,
                    },
                },
            ))
        })
        .map_err(error)?;
    rows.map(|row| {
        let (character_position, location, position, persistence) = row.map_err(error)?;
        let character = snapshot
            .characters
            .characters()
            .get(character_position)
            .ok_or_else(|| {
                SqliteAccountError::invalid_data(
                    "character_items.character_position",
                    "metadata does not identify a loaded character",
                )
            })?;
        let item = match location {
            0 => EQUIPMENT_SLOTS.get(position).and_then(|slot| {
                character
                    .equipment
                    .get(&EquipmentSlot::new(*slot))
                    .and_then(Option::as_ref)
            }),
            1 => character.inventory.get(position),
            _ => None,
        }
        .ok_or_else(|| {
            SqliteAccountError::invalid_data(
                "character_items.position",
                "metadata does not identify a loaded item",
            )
        })?;
        Ok((item.id, persistence))
    })
    .collect()
}
