//! Rows this build does not model, carried across a save so it cannot destroy them.
//!
//! The writer replaces the whole account graph, and `item_rolls` cascades from
//! `character_items`, so a save would drop every roll and reset every column added after this
//! build was written. Reading them here and putting them back keeps a database Sundial saved as
//! complete as the one it opened.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

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
}

/// One `item_rolls` row, kept exactly as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ItemRoll {
    pub instance_soid: String,
    pub entropy: Vec<u8>,
    pub lane_mask: i64,
    pub owned_rows: Vec<u8>,
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
    /// `profile_items.mutation_serial`, by instance SOID.
    pub profile_mutation_serials: BTreeMap<String, i64>,
    /// `item_rolls`, which cascades away when its item row is deleted.
    pub item_rolls: Vec<ItemRoll>,
}

pub(super) fn read(connection: &Connection) -> Result<Carried, DawnAccountError> {
    let mut characters = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT soid,last_selected,level,accepted,preview_available,appearance,last_destination,         content_bypass,next_inventory_serial,vendor_campaigns FROM characters",
    )?;
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

    let mut profile_mutation_serials = BTreeMap::new();
    let mut statement =
        connection.prepare("SELECT instance_soid,mutation_serial FROM profile_items")?;
    for row in statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })? {
        let (soid, serial) = row?;
        profile_mutation_serials.insert(soid, serial);
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
    Ok(Carried {
        characters,
        postmaster,
        item_mutation_serials,
        profile_mutation_serials,
        item_rolls,
    })
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
    }
    for (soid, serial) in &carried.item_mutation_serials {
        transaction.execute(
            "UPDATE character_items SET mutation_serial=?2 WHERE instance_soid=?1",
            params![soid, serial],
        )?;
    }
    for (soid, serial) in &carried.profile_mutation_serials {
        transaction.execute(
            "UPDATE profile_items SET mutation_serial=?2 WHERE instance_soid=?1",
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
