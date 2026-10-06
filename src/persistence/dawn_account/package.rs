//! Account cleanup for a package install or uninstall on a Dawn runtime.
//!
//! When Parhelion replaces or removes packages, the items those packages defined stop resolving.
//! Dawn abandons the whole character loadout over a single unresolvable item, so anything the
//! account still holds from a removed package has to go with it.
//!
//! Dawn keeps that account in `player-state.db`. It imports the JSON account seed only when it
//! creates the database, so cleaning account members in settings.json later removes nothing. The
//! operation reports success while the account keeps the orphan, and the next launch fails at
//! `family4 stage=prepare step=loadout`. Dawn's unrelated runtime configuration remains in JSON.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};

use rusqlite::{Connection, TransactionBehavior, params};
use sundial_account::{AuthoredUnlock, UnlockScope};

use crate::account::{
    AuthoredAccountCleanup, AuthoredCollectionUnlock, AuthoredItemMove, AuthoredMoveOutcome,
    AuthoredSlotReplacement, AuthoredSocketChange, placement,
};
use crate::persistence::native_account::snapshot::{self, Snapshot};

/// The tables a cleanup may change. `item_sockets` and `item_rolls` follow their item through
/// its cascade, and `metadata` carries the advanced revision. A change anywhere else came from
/// something the review did not plan, such as a trigger, and is refused.
const CLEANED: [&str; 7] = [
    "metadata",
    "character_items",
    "profile_items",
    "item_sockets",
    "item_rolls",
    "dismantle_rewards",
    "durable_flags",
];

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The same layout check an ordinary account save makes. A package operation must not reach
/// further into an account than the editor would.
fn guard(db: &Connection) -> Result<(), String> {
    super::schema_guard::validate(db).map_err(err)
}

/// A complete logical snapshot, with any uncheckpointed write-ahead log folded in.
///
/// Dawn runs in WAL mode, so the file's bytes alone would miss whatever it has not checkpointed.
/// Reading through SQLite inside one read transaction sees exactly the committed account.
pub(crate) fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut db = snapshot::open(path, false)?;
    let tx = db.transaction().map_err(err)?;
    guard(&tx)?;
    snapshot::capture(&tx)
}

/// Applies a reviewed proposal, refusing when the account moved since the review.
///
/// The comparison and the update share one SQLite write transaction, and SQLite keeps its own
/// journal. Replacing the file instead would let a writer commit between the check and the
/// rename, and a write-ahead log left beside the new file could bring removed rows back.
pub(crate) fn replace(path: &Path, expected: &[u8], updated: &[u8]) -> Result<(), String> {
    let expected: Snapshot = serde_json::from_slice(expected).map_err(err)?;
    let after: Snapshot = serde_json::from_slice(updated).map_err(err)?;
    let mut db = snapshot::open(path, true)?;
    // Changed tables are rewritten whole, so a cascade must not take rows from a table restored
    // earlier. The foreign key check below still covers the result.
    db.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA synchronous=FULL;")
        .map_err(err)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    guard(&tx)?;
    let before = snapshot::snapshot(&tx)?;
    if before != expected {
        return Err("The Dawn account changed after review. Reload the package operation".into());
    }
    if before.schema != after.schema
        || before.version != after.version
        || before.application != after.application
        || before.tables.keys().ne(after.tables.keys())
        || before
            .tables
            .iter()
            .any(|(name, table)| table.columns != after.tables[name].columns)
    {
        return Err("The account proposal changes the Dawn database schema".into());
    }
    let changed = after
        .tables
        .iter()
        .filter(|(name, table)| before.tables[*name] != **table)
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    for name in &changed {
        tx.execute(&format!("DELETE FROM {}", snapshot::quote(name)), [])
            .map_err(err)?;
    }
    // Explicit inserts may advance sequences, so the sequence table goes back last.
    for name in changed
        .iter()
        .filter(|name| **name != "sqlite_sequence")
        .chain(changed.iter().filter(|name| **name == "sqlite_sequence"))
    {
        snapshot::insert_table(&tx, name, &after.tables[*name])?;
    }
    let invalid = tx
        .prepare("PRAGMA foreign_key_check")
        .map_err(err)?
        .exists([])
        .map_err(err)?;
    if invalid || snapshot::snapshot(&tx)? != after {
        return Err(
            "The Dawn account update did not match the reviewed proposal. The account was not changed"
                .into(),
        );
    }
    tx.commit().map_err(err)
}

/// Removes everything the account holds from packages that are going away.
pub(crate) fn preview_replacement(
    path: &Path,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    changes: &[AuthoredSocketChange],
    slots: Option<&AuthoredSlotReplacement>,
) -> Result<AuthoredAccountCleanup, String> {
    let mut source = snapshot::open(path, false)?;
    let tx = source.transaction().map_err(err)?;
    guard(&tx)?;
    let original = snapshot::snapshot(&tx)?;
    let original_bytes = serde_json::to_vec(&original).map_err(err)?;
    let mut staged = Connection::open_in_memory().map_err(err)?;
    rusqlite::backup::Backup::new(&tx, &mut staged)
        .map_err(err)?
        .run_to_completion(128, Duration::from_millis(1), None)
        .map_err(err)?;
    drop(tx);
    drop(source);
    // The item tables cascade into item_sockets and item_rolls, which is how a removed item takes
    // its own sockets and roll with it rather than orphaning them.
    staged
        .execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(err)?;

    let mut report = AuthoredAccountCleanup {
        settings_path: path.to_path_buf(),
        original_bytes,
        cleaned_bytes: Vec::new(),
        removed_items: BTreeMap::new(),
        cleared_plugs: 0,
        removed_reward_rules: 0,
        cleared_unlocks: 0,
        resized_items: BTreeMap::new(),
        slot_moves: Vec::new(),
    };

    for hash in hashes {
        let plug = i64::from(*hash);
        report.cleared_plugs += staged
            .execute(
                "UPDATE item_sockets SET plug_hash=NULL WHERE plug_hash=?1",
                [plug],
            )
            .map_err(err)?;
        let mut removed = 0;
        for table in ["character_items", "profile_items"] {
            removed += staged
                .execute(
                    &format!("DELETE FROM {table} WHERE definition_hash=?1"),
                    [plug],
                )
                .map_err(err)?;
        }
        if removed != 0 {
            report.removed_items.insert(*hash, removed);
        }
        report.removed_reward_rules += staged
            .execute(
                "DELETE FROM dismantle_rewards WHERE definition_hash=?1",
                [plug],
            )
            .map_err(err)?;
    }

    for unlock in unlocks {
        report.cleared_unlocks += clear_unlock(&staged, *unlock)?;
    }

    compact(&staged, "profile_items", "1=1")?;
    compact(&staged, "dismantle_rewards", "1=1")?;
    // Dawn requires inventory positions to run contiguously from zero for each character.
    // Equipment is the exception: location 0 is sparse by design and must keep its slot numbers.
    let characters: Vec<String> = staged
        .prepare("SELECT soid FROM characters ORDER BY position")
        .map_err(err)?
        .query_map([], |row| row.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    for soid in &characters {
        compact(
            &staged,
            "character_items",
            &format!("location=1 AND character_soid='{soid}'"),
        )?;
    }
    // Moves append to inventories that already run contiguously, so they come after compaction.
    report.slot_moves = relocate(&staged, &characters, hashes, slots)?;
    resize(&staged, hashes, changes, &mut report.resized_items)?;
    // Dawn advances account_revision on every commit and checks it before the next one, and so
    // does Sundial's own account save. A cleanup that left the revision alone would let a save
    // from a document loaded before the cleanup pass that check and write the removed items
    // straight back. Advancing it turns that into the conflict it is.
    if report.changed_anything() {
        staged
            .execute(
                "UPDATE metadata SET value=value+1 WHERE key='account_revision'",
                [],
            )
            .map_err(err)?;
    }

    let cleaned = snapshot::snapshot(&staged)?;
    if let Some(name) = original
        .tables
        .iter()
        .find(|(name, table)| {
            !CLEANED.contains(&name.as_str()) && cleaned.tables.get(*name) != Some(*table)
        })
        .map(|(name, _)| name)
    {
        return Err(format!(
            "The Dawn account cleanup would also change {name}. The account was not changed"
        ));
    }
    if original.schema != cleaned.schema {
        return Err("The Dawn account cleanup would change the database schema".into());
    }
    report.cleaned_bytes = serde_json::to_vec(&cleaned).map_err(err)?;
    Ok(report)
}

/// Moves equipped weapons whose native slot changed in the incoming generation.
///
/// Dawn keys equipment by slot position, so a weapon that stays equipped in a slot its new
/// definition no longer fits fails the loadout the same way an orphan does. The plan itself is
/// shared with the other backends; only the row shapes differ here.
fn relocate(
    staged: &Connection,
    characters: &[String],
    removed: &BTreeSet<u32>,
    replacement: Option<&AuthoredSlotReplacement>,
) -> Result<Vec<AuthoredItemMove>, String> {
    let Some(replacement) = replacement else {
        return Ok(vec![]);
    };
    let mut held = (0..characters.len())
        .map(|character_index| placement::CharacterItems {
            character_index,
            inventory: vec![],
            equipment: vec![],
        })
        .collect::<Vec<_>>();
    let rows: Vec<(String, i64, i64, i64)> = staged
        .prepare(
            "SELECT character_soid,location,position,definition_hash FROM character_items \
             ORDER BY character_soid,location,position",
        )
        .map_err(err)?
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    for (soid, location, position, hash) in rows {
        let hash = u32::try_from(hash).map_err(|_| "An item hash is not a u32")?;
        let character = characters
            .iter()
            .position(|known| *known == soid)
            .ok_or("An item names a character the account does not have")?;
        match location {
            super::contract::EQUIPMENT_LOCATION => {
                let slot = usize::try_from(position)
                    .ok()
                    .and_then(|slot| super::contract::EQUIPMENT_SLOTS.get(slot))
                    .ok_or("An equipped item has an unsupported slot")?;
                held[character].equipment.push(((*slot).to_owned(), hash));
            }
            super::contract::INVENTORY_LOCATION => held[character].inventory.push(hash),
            _ => return Err("An item has an unsupported account location".into()),
        }
    }
    let moves = placement::plan(&held, removed, replacement)?;
    for movement in &moves {
        let position = super::contract::EQUIPMENT_SLOTS
            .iter()
            .position(|slot| *slot == movement.equipment_slot)
            .ok_or("An equipment move has an unsupported slot")?;
        let soid = &characters[movement.character_index];
        let affected = match movement.outcome {
            AuthoredMoveOutcome::MovedToInventory => staged.execute(
                "UPDATE character_items SET location=?2,position=(SELECT count(*) FROM \
                 character_items WHERE character_soid=?1 AND location=?2) \
                 WHERE character_soid=?1 AND location=?3 AND position=?4 AND definition_hash=?5",
                params![
                    soid,
                    super::contract::INVENTORY_LOCATION,
                    super::contract::EQUIPMENT_LOCATION,
                    i64::try_from(position).map_err(err)?,
                    i64::from(movement.definition_hash)
                ],
            ),
            AuthoredMoveOutcome::DeletedInventoryFull => staged.execute(
                "DELETE FROM character_items WHERE character_soid=?1 AND location=?2 \
                 AND position=?3 AND definition_hash=?4",
                params![
                    soid,
                    super::contract::EQUIPMENT_LOCATION,
                    i64::try_from(position).map_err(err)?,
                    i64::from(movement.definition_hash)
                ],
            ),
        }
        .map_err(err)?;
        if affected != 1 {
            return Err("The reviewed equipment move no longer matches its account item".into());
        }
    }
    Ok(moves)
}

/// Grows or shrinks the stored socket lanes of retained items whose definition changed shape,
/// and moves a lane that still holds a replaced default to the new default.
///
/// Only items that carry their own lanes are touched: an item on native defaults has no lanes
/// to resize, and Dawn refuses one that stores lanes anyway. Every lane up to the new count is
/// written, empty ones included, because Dawn requires lanes to run contiguously from zero.
fn resize(
    staged: &Connection,
    removed: &BTreeSet<u32>,
    changes: &[AuthoredSocketChange],
    report: &mut BTreeMap<u32, usize>,
) -> Result<(), String> {
    crate::account::validate_socket_changes(removed, changes)?;
    for change in changes {
        let items: Vec<(String, usize)> = staged
            .prepare(
                "SELECT i.instance_soid,(SELECT count(*) FROM item_sockets s \
                 WHERE s.instance_soid=i.instance_soid) FROM character_items i \
                 WHERE i.definition_hash=?1 AND i.socket_policy=1",
            )
            .map_err(err)?
            .query_map([i64::from(change.definition_hash)], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        for (soid, count) in items {
            let new = change.default_plugs.len();
            let mut changed = false;
            if count != new {
                if count != change.previous_socket_count {
                    return Err(
                        "An authored item's socket count differs from the reviewed package".into(),
                    );
                }
                staged
                    .execute(
                        "DELETE FROM item_sockets WHERE instance_soid=?1 AND lane>=?2",
                        params![soid, i64::try_from(new).map_err(err)?],
                    )
                    .map_err(err)?;
                for (lane, hash) in change.default_plugs.iter().enumerate().skip(count) {
                    staged
                        .execute(
                            "INSERT INTO item_sockets(instance_soid,lane,plug_hash) VALUES(?1,?2,?3)",
                            params![soid, i64::try_from(lane).map_err(err)?, hash.map(i64::from)],
                        )
                        .map_err(err)?;
                }
                trim_roll(staged, &soid, new)?;
                changed = true;
            }
            // A saved selection of a lane's replaced default follows the definition to its new
            // one. Dawn keeps every lane's row, so an empty new default stores an empty lane.
            for &(lane, old) in &change.replaced_defaults {
                let updated = staged
                    .execute(
                        "UPDATE item_sockets SET plug_hash=?1 WHERE instance_soid=?2 AND lane=?3 AND plug_hash=?4",
                        params![
                            change.default_plugs[lane].map(i64::from),
                            soid,
                            i64::try_from(lane).map_err(err)?,
                            i64::from(old)
                        ],
                    )
                    .map_err(err)?;
                changed |= updated > 0;
            }
            if changed {
                *report.entry(change.definition_hash).or_default() += 1;
            }
        }
    }
    Ok(())
}

/// Drops the saved roll of lanes a shrunk item no longer has. Dawn requires every rolled lane to
/// have a socket at that position, and the socket is gone with the definition that defined it.
fn trim_roll(staged: &Connection, soid: &str, lanes: usize) -> Result<(), String> {
    let roll: Option<(i64, Vec<u8>)> = staged
        .prepare("SELECT lane_mask,owned_rows FROM item_rolls WHERE instance_soid=?1")
        .map_err(err)?
        .query_map([soid], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(err)?
        .next()
        .transpose()
        .map_err(err)?;
    let Some((mask, mut owned)) = roll else {
        return Ok(());
    };
    if owned.len() != 96 {
        return Err("An item's saved roll does not have twelve lanes".into());
    }
    let kept = if lanes >= 12 {
        mask
    } else {
        mask & ((1_i64 << lanes) - 1)
    };
    for row in owned.chunks_exact_mut(8).skip(lanes) {
        row.fill(0);
    }
    staged
        .execute(
            "UPDATE item_rolls SET lane_mask=?2,owned_rows=?3 WHERE instance_soid=?1",
            params![soid, kept, owned],
        )
        .map_err(err)?;
    Ok(())
}

/// Clears one authored collection unlock from the durable flag bank that holds it.
fn clear_unlock(staged: &Connection, unlock: AuthoredCollectionUnlock) -> Result<usize, String> {
    let authored = AuthoredUnlock {
        definition_index: unlock.definition_index,
        bank: unlock.bank,
        slot: unlock.slot,
    };
    let (scope, slot) = authored.target()?;
    let ordinal = match scope {
        UnlockScope::Account => 0,
        UnlockScope::Profile => 1,
        UnlockScope::Character => 2,
        UnlockScope::CharacterObject => 3,
    };
    let removed = staged
        .execute(
            "DELETE FROM durable_flags WHERE scope=?1 AND slot=?2",
            params![ordinal, i64::try_from(slot).map_err(err)?],
        )
        .map_err(err)?;
    Ok(usize::from(removed != 0))
}

/// Renumbers a table's `position` column so it runs contiguously from zero again, which is what
/// Dawn checks at load. A gap left by a removed row makes it refuse the account outright.
fn compact(staged: &Connection, table: &str, predicate: &str) -> Result<(), String> {
    let rows: Vec<i64> = staged
        .prepare(&format!(
            "SELECT position FROM {table} WHERE {predicate} ORDER BY position"
        ))
        .map_err(err)?
        .query_map([], |row| row.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    for (target, current) in rows.into_iter().enumerate() {
        let target = i64::try_from(target).map_err(err)?;
        if target != current {
            staged
                .execute(
                    &format!("UPDATE {table} SET position=?1 WHERE position=?2 AND {predicate}"),
                    params![target, current],
                )
                .map_err(err)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
