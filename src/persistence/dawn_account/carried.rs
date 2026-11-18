//! Rows this build does not model, carried across a save so it cannot destroy them.
//!
//! The writer replaces the whole account graph, and `item_rolls` cascades from
//! `character_items`, so a save would drop every roll and reset every column added after this
//! build was written. Known bookkeeping is restored here. The schema guard refuses unknown
//! extensions to the rewritten graph before saving.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};
use sundial_account::{CharacterAbilities, EntityId, ProfileState};

use super::contract;
use super::error::DawnAccountError;

/// The `characters` columns Sundial does not model, kept exactly as stored.
///
/// The writer rebuilds every character row from the storage-neutral account, which holds only
/// race, gender, class and abilities. Everything else Dawn keeps there is runtime bookkeeping with
/// no control in Sundial, so it is read with the account and put back verbatim rather than
/// invented. Before this, a save wrote literals over all of it — every character came back level
/// 50, accepted, preview-available, in orbit at destination 0.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CharacterRow {
    pub last_selected: i64,
    pub level: i64,
    pub accepted: i64,
    pub preview_available: i64,
    pub appearance: f64,
    pub last_destination: i64,
    pub content_bypass: i64,
    pub next_inventory_serial: i64,
    pub vendor_campaigns: i64,
    /// `selected_title_record` and the 36-byte native `customization` block. Schema 7 only.
    pub title_and_customization: Option<(i64, Vec<u8>)>,
}

/// One `item_values` row, kept exactly as stored. Schema 7 only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ItemValue {
    pub instance_soid: String,
    pub lane: i64,
    pub value: i64,
}

/// One `item_rolls` row, kept exactly as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ItemRoll {
    pub instance_soid: String,
    pub entropy: Vec<u8>,
    pub lane_mask: i64,
    pub owned_rows: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProfileRow {
    pub soid: String,
    pub serial: i64,
}

/// Everything a save has to put back that this build does not otherwise represent.
/// `appearance` is a float, so this compares by value rather than deriving `Eq`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Carried {
    /// The `characters` columns Sundial does not model, by character SOID.
    pub characters: BTreeMap<String, CharacterRow>,
    /// The instance SOIDs whose `character_items.postmaster` is set.
    pub postmaster: BTreeSet<String>,
    /// `character_items.mutation_serial`, by instance SOID. That column is UNIQUE, so it is a
    /// stable key across a save; position is not.
    pub item_mutation_serials: BTreeMap<String, i64>,
    /// Session-local row identity, not SOID. Non-instanced stacks all have SOID zero.
    pub profile_rows: BTreeMap<EntityId, ProfileRow>,
    /// `item_rolls`, which cascades away when its item row is deleted.
    pub item_rolls: Vec<ItemRoll>,
    /// The five ability entries each item row stores, by instance SOID. Schema 7 only. Dawn reads
    /// the equipped subclass's entries and ignores the character row's.
    pub item_abilities: BTreeMap<String, CharacterAbilities>,
    /// `item_values`, which cascades away like `item_rolls`. Schema 7 only.
    pub item_values: Vec<ItemValue>,
}

pub(super) fn read(
    connection: &Connection,
    profile: &ProfileState,
) -> Result<Carried, DawnAccountError> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let per_item = contract::per_item(version);
    let mut characters = BTreeMap::new();
    let mut statement = connection.prepare(if per_item {
        "SELECT soid,last_selected,level,accepted,preview_available,appearance,last_destination,\
         content_bypass,next_inventory_serial,vendor_campaigns,selected_title_record,customization \
         FROM characters"
    } else {
        "SELECT soid,last_selected,level,accepted,preview_available,appearance,last_destination,\
         content_bypass,next_inventory_serial,vendor_campaigns FROM characters"
    })?;
    for row in statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            CharacterRow {
                last_selected: row.get(1)?,
                level: row.get(2)?,
                accepted: row.get(3)?,
                preview_available: row.get(4)?,
                appearance: row.get(5)?,
                last_destination: row.get(6)?,
                content_bypass: row.get(7)?,
                next_inventory_serial: row.get(8)?,
                vendor_campaigns: row.get(9)?,
                title_and_customization: if per_item {
                    Some((row.get(10)?, row.get(11)?))
                } else {
                    None
                },
            },
        ))
    })? {
        let (soid, carried) = row?;
        characters.insert(soid, carried);
    }
    drop(statement);

    let mut item_mutation_serials = BTreeMap::new();
    let mut statement =
        connection.prepare("SELECT instance_soid,mutation_serial FROM character_items")?;
    for row in statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })? {
        let (soid, serial) = row?;
        item_mutation_serials.insert(soid, serial);
    }
    drop(statement);

    let mut profile_rows = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT position,instance_soid,mutation_serial FROM profile_items ORDER BY position",
    )?;
    for row in statement.query_map([], |row| {
        Ok((
            row.get::<_, usize>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })? {
        let (position, soid, serial) = row?;
        let item = profile.profile_items().get(position).ok_or_else(|| {
            DawnAccountError::Unwritable("Profile inventory changed. Reload before saving".into())
        })?;
        profile_rows.insert(item.id, ProfileRow { soid, serial });
    }
    drop(statement);

    let mut postmaster = BTreeSet::new();
    let mut statement =
        connection.prepare("SELECT instance_soid FROM character_items WHERE postmaster<>0")?;
    for row in statement.query_map([], |row| row.get::<_, String>(0))? {
        postmaster.insert(row?);
    }
    drop(statement);

    let mut item_rolls = Vec::new();
    let mut statement =
        connection.prepare("SELECT instance_soid,entropy,lane_mask,owned_rows FROM item_rolls")?;
    for row in statement.query_map([], |row| {
        Ok(ItemRoll {
            instance_soid: row.get(0)?,
            entropy: row.get(1)?,
            lane_mask: row.get(2)?,
            owned_rows: row.get(3)?,
        })
    })? {
        item_rolls.push(row?);
    }
    drop(statement);

    let mut item_abilities = BTreeMap::new();
    let mut item_values = Vec::new();
    if per_item {
        let mut statement = connection.prepare(
            "SELECT instance_soid,movement_ability,grenade_ability,super_ability,melee_ability,\
             class_ability FROM character_items",
        )?;
        for row in statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                CharacterAbilities {
                    movement: row.get(1)?,
                    grenade: row.get(2)?,
                    super_ability: row.get(3)?,
                    melee: row.get(4)?,
                    class_ability: row.get(5)?,
                },
            ))
        })? {
            let (soid, abilities) = row?;
            item_abilities.insert(soid, abilities);
        }
        drop(statement);
        let mut statement = connection.prepare(
            "SELECT instance_soid,lane,value FROM item_values ORDER BY instance_soid,lane",
        )?;
        for row in statement.query_map([], |row| {
            Ok(ItemValue {
                instance_soid: row.get(0)?,
                lane: row.get(1)?,
                value: row.get(2)?,
            })
        })? {
            item_values.push(row?);
        }
    }
    Ok(Carried {
        characters,
        postmaster,
        item_mutation_serials,
        profile_rows,
        item_rolls,
        item_abilities,
        item_values,
    })
}

impl super::DawnAccountDocument {
    /// The key one item's ability entries are carried under, in the spelling its row is written
    /// with. `None` on a schema that keeps abilities on the character row.
    pub(super) fn item_abilities_key(&self, id: EntityId) -> Option<String> {
        if !contract::per_item(self.schema_version) {
            return None;
        }
        let soid = self
            .snapshot
            .characters
            .characters()
            .iter()
            .flat_map(|character| {
                character
                    .inventory
                    .iter()
                    .chain(character.equipment.values().flatten())
            })
            .find(|item| item.id == id)?
            .instance_soid
            .get();
        let canonical = contract::format_soid(soid);
        Some(
            self.carried
                .item_mutation_serials
                .keys()
                .find(|key| key.eq_ignore_ascii_case(&canonical))
                .cloned()
                .unwrap_or(canonical),
        )
    }

    /// Gives each equipped subclass its character's current ability picks before a save.
    ///
    /// The editor holds one selection per character, which on schema 7 is the equipped
    /// subclass's. Any edit to it therefore belongs on that item, whichever path made it.
    pub(super) fn settle_item_abilities(&mut self) -> Result<(), DawnAccountError> {
        if !contract::per_item(self.schema_version) {
            return Ok(());
        }
        let subclass = sundial_account::EquipmentSlot::new("subclass");
        let equipped = self
            .snapshot
            .characters
            .characters()
            .iter()
            .filter_map(|character| {
                let item = character.equipment.get(&subclass)?.as_ref()?;
                Some((item.id, character.metadata?.abilities))
            })
            .collect::<Vec<_>>();
        for (id, abilities) in equipped {
            if let Some(key) = self.item_abilities_key(id) {
                self.carried.item_abilities.insert(key, abilities);
            }
        }
        for abilities in self.carried.item_abilities.values() {
            let entries = [
                abilities.movement,
                abilities.grenade,
                abilities.super_ability,
                abilities.melee,
                abilities.class_ability,
            ];
            if entries
                .iter()
                .any(|entry| *entry > contract::ITEM_ABILITY_MAXIMUM)
            {
                return Err(DawnAccountError::Unwritable(format!(
                    "Dawn stores subclass ability entries from 0 to {}. No changes were saved",
                    contract::ITEM_ABILITY_MAXIMUM
                )));
            }
        }
        Ok(())
    }
}

/// Puts the carried rows back, after the account graph has been rewritten.
///
/// A roll whose item is gone stays gone: the author removed that item, and its roll belongs to it.
pub(super) fn restore(
    transaction: &rusqlite::Transaction<'_>,
    carried: &Carried,
) -> Result<(), DawnAccountError> {
    for (soid, row) in &carried.characters {
        transaction.execute(
            "UPDATE characters SET last_selected=?2,level=?3,accepted=?4,preview_available=?5,             appearance=?6,last_destination=?7,content_bypass=?8,next_inventory_serial=?9,             vendor_campaigns=?10 WHERE soid=?1",
            params![
                soid,
                row.last_selected,
                row.level,
                row.accepted,
                row.preview_available,
                row.appearance,
                row.last_destination,
                row.content_bypass,
                row.next_inventory_serial,
                row.vendor_campaigns
            ],
        )?;
        if let Some((title, customization)) = &row.title_and_customization {
            transaction.execute(
                "UPDATE characters SET selected_title_record=?2,customization=?3 WHERE soid=?1",
                params![soid, title, customization],
            )?;
        }
    }
    for (soid, abilities) in &carried.item_abilities {
        transaction.execute(
            "UPDATE character_items SET movement_ability=?2,grenade_ability=?3,super_ability=?4,\
             melee_ability=?5,class_ability=?6 WHERE instance_soid=?1",
            params![
                soid,
                abilities.movement,
                abilities.grenade,
                abilities.super_ability,
                abilities.melee,
                abilities.class_ability
            ],
        )?;
    }
    for value in &carried.item_values {
        transaction.execute(
            "INSERT INTO item_values(instance_soid,lane,value) \
             SELECT ?1,?2,?3 WHERE EXISTS(SELECT 1 FROM character_items WHERE instance_soid=?1)",
            params![value.instance_soid, value.lane, value.value],
        )?;
    }
    for (soid, serial) in &carried.item_mutation_serials {
        transaction.execute(
            "UPDATE character_items SET mutation_serial=?2 WHERE instance_soid=?1",
            params![soid, serial],
        )?;
    }
    for soid in &carried.postmaster {
        transaction.execute(
            "UPDATE character_items SET postmaster=1 WHERE instance_soid=?1",
            params![soid],
        )?;
    }
    for roll in &carried.item_rolls {
        transaction.execute(
            "INSERT INTO item_rolls(instance_soid,entropy,lane_mask,owned_rows) \
             SELECT ?1,?2,?3,?4 WHERE EXISTS(SELECT 1 FROM character_items WHERE instance_soid=?1)",
            params![
                roll.instance_soid,
                roll.entropy,
                roll.lane_mask,
                roll.owned_rows
            ],
        )?;
    }
    Ok(())
}
