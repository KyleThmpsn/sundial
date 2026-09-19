//! Writes one edited account back to a Dawn player-state database.
//!
//! Dawn rewrites the whole account graph inside a single transaction and then advances
//! `account_revision` with a compare and swap. This writer does the same, so a database Sundial
//! saved is one Dawn can still open. It never touches `metadata` row membership, the allocator
//! names, `missions` or `reward_debts`, because Dawn refuses to boot when any of those change
//! shape.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, TransactionBehavior, params};
use sundial_account::ItemPlugs;

use super::DawnAccountDocument;
use super::contract;
use super::error::DawnAccountError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DawnSaveReceipt {
    pub backup: PathBuf,
    pub revision: i64,
}

/// Replaces the stored account with the loaded document's current state.
///
/// The revision the document loaded must still be the one on disk. Dawn advances that value on
/// every commit, so a mismatch means the runtime wrote while the editor held the account and the
/// save is refused rather than overwriting it.
pub(crate) fn save(
    document: &mut DawnAccountDocument,
) -> Result<DawnSaveReceipt, DawnAccountError> {
    let backup = create_backup(&document.path)?;
    let mut connection = Connection::open(&document.path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let revision: i64 = transaction.query_row(
        "SELECT value FROM metadata WHERE key='account_revision'",
        [],
        |row| row.get(0),
    )?;
    if revision != document.metadata.account_revision {
        return Err(DawnAccountError::Conflict {
            expected: document.metadata.account_revision,
            found: revision,
        });
    }

    write_account(&transaction, document)?;
    write_allocators(&transaction, document)?;
    // The account graph has been replaced, so anything this build does not model goes back now.
    super::carried::restore(&transaction, &document.carried)?;
    // Settings live outside that graph: their tables are never deleted, so only what the user
    // changed is updated in place.
    super::settings::save(
        &transaction,
        &document.settings_index,
        &document.loaded_settings,
        &document.snapshot.settings,
    )?;

    let advanced = transaction.execute(
        "UPDATE metadata SET value=value+1 WHERE key='account_revision' AND value=?1",
        params![revision],
    )?;
    if advanced != 1 {
        return Err(DawnAccountError::Conflict {
            expected: revision,
            found: -1,
        });
    }
    transaction.commit()?;

    document.metadata.account_revision = revision + 1;
    document.loaded_settings = document.snapshot.settings.clone();
    Ok(DawnSaveReceipt {
        backup,
        revision: document.metadata.account_revision,
    })
}

/// Takes a verified copy into Sundial's backup folder, the way the investment account does.
///
/// It used to drop a single `player-state.db.sundial-backup` beside the database, overwritten on
/// every save, so the previous account was gone the moment a second save ran and none of it was
/// reachable from Browse Backups. These are indexed and retained with the rest.
pub(super) fn create_backup(path: &Path) -> Result<PathBuf, DawnAccountError> {
    let root = crate::backups::root().ok_or_else(|| {
        DawnAccountError::Backup(
            "could not locate Sundial's local backup folder for player-state.db".to_owned(),
        )
    })?;
    crate::backups::create(&root, path, "player-state-v2", "db", true, |backup, _| {
        write_verified_copy(path, backup).map_err(|error| error.to_string())
    })
    .map_err(DawnAccountError::Backup)
}

/// Copies the database with SQLite's own backup API so any write ahead log is folded in.
fn write_verified_copy(path: &Path, backup: &Path) -> Result<(), DawnAccountError> {
    let source = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut destination = Connection::open(backup)?;
    let copy = rusqlite::backup::Backup::new(&source, &mut destination)?;
    copy.run_to_completion(64, std::time::Duration::from_millis(0), None)?;
    drop(copy);
    let ok: String = destination.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if ok != "ok" {
        return Err(DawnAccountError::Backup(format!(
            "the verified copy reported {ok}"
        )));
    }
    Ok(())
}

/// Rewrites the account graph exactly as Dawn's own `write_account` does.
fn write_account(
    transaction: &rusqlite::Transaction<'_>,
    document: &mut DawnAccountDocument,
) -> Result<(), DawnAccountError> {
    // Split the borrows: the account is read from the snapshot while the profile allocator is
    // advanced for any stack that does not have a durable identity yet.
    let DawnAccountDocument {
        snapshot,
        allocators,
        ..
    } = document;
    transaction.execute_batch(
        "DELETE FROM item_sockets;
         DELETE FROM character_items;
         DELETE FROM characters;
         DELETE FROM profile_items;
         DELETE FROM account;",
    )?;
    transaction.execute(
        "INSERT INTO account(id,primary_soid) VALUES(1,?1)",
        params![contract::format_soid(snapshot.primary_soid.get())],
    )?;

    for (position, item) in snapshot.profile.profile_items().iter().enumerate() {
        // The stack keeps the identity it was read with. One the user added has none yet, so it
        // takes the next the profile allocator offers, the way Dawn allocates its own.
        let soid = match item.instance_soid {
            Some(soid) => soid.get(),
            None => {
                let next = allocators
                    .profile_item
                    .max(contract::FIRST_PROFILE_ITEM_SOID);
                allocators.profile_item = next.saturating_add(1);
                next
            }
        };
        transaction.execute(
            "INSERT INTO profile_items(position,instance_soid,definition_hash,quantity,mutation_serial)\
             VALUES(?1,?2,?3,?4,0)",
            params![
                position as i64,
                contract::format_soid(soid),
                i64::from(item.definition_hash.get()),
                item.quantity
            ],
        )?;
    }

    for (position, character) in snapshot.characters.characters().iter().enumerate() {
        let soid = character
            .soid
            .ok_or_else(|| DawnAccountError::Unwritable("a character has no SOID".into()))?;
        let metadata = character.metadata.ok_or_else(|| {
            DawnAccountError::Unwritable("a character has no metadata to write".into())
        })?;
        transaction.execute(
            "INSERT INTO characters(position,soid,last_selected,race,gender,class,level,             accepted,preview_available,appearance,last_destination,content_bypass,             movement_ability,grenade_ability,super_ability,melee_ability,class_ability,             next_inventory_serial)              VALUES(?1,?2,0,?3,?4,?5,?6,1,1,1.0,0,1,?7,?8,?9,?10,?11,?12)",
            params![
                position as i64,
                contract::format_soid(soid.get()),
                i64::from(metadata.race),
                i64::from(metadata.gender),
                i64::from(metadata.class_type),
                i64::from(50_u8),
                i64::from(metadata.abilities.movement),
                i64::from(metadata.abilities.grenade),
                i64::from(metadata.abilities.super_ability),
                i64::from(metadata.abilities.melee),
                i64::from(metadata.abilities.class_ability),
                character.inventory.len() as i64,
            ],
        )?;

        let character_soid = contract::format_soid(soid.get());
        for (slot, item) in &character.equipment {
            let Some(item) = item else { continue };
            let Some(position) = contract::EQUIPMENT_SLOTS
                .iter()
                .position(|known| *known == slot.as_str())
            else {
                return Err(DawnAccountError::Unwritable(format!(
                    "Dawn has no equipment slot named {}",
                    slot.as_str()
                )));
            };
            write_item(
                transaction,
                &character_soid,
                contract::EQUIPMENT_LOCATION,
                position as i64,
                item,
            )?;
        }
        for (position, item) in character.inventory.iter().enumerate() {
            write_item(
                transaction,
                &character_soid,
                contract::INVENTORY_LOCATION,
                position as i64,
                item,
            )?;
        }
    }
    Ok(())
}

fn write_item(
    transaction: &rusqlite::Transaction<'_>,
    character_soid: &str,
    location: i64,
    position: i64,
    item: &sundial_account::ItemInstance,
) -> Result<(), DawnAccountError> {
    let soid = contract::format_soid(item.instance_soid.get());
    let policy = i64::from(matches!(item.plugs, ItemPlugs::Authored(_)));
    transaction.execute(
        "INSERT INTO character_items(character_soid,location,position,instance_soid,definition_hash,\
         level,quantity,mutation_serial,flags,socket_policy) VALUES(?1,?2,?3,?4,?5,?6,?7,0,?8,?9)",
        params![
            character_soid,
            location,
            position,
            soid,
            i64::from(item.definition_hash.get()),
            item.level,
            item.quantity,
            i64::from(item.flags.unwrap_or_default()),
            policy
        ],
    )?;
    if let ItemPlugs::Authored(plugs) = &item.plugs {
        for (lane, plug) in plugs.iter().enumerate() {
            transaction.execute(
                "INSERT INTO item_sockets(instance_soid,lane,plug_hash) VALUES(?1,?2,?3)",
                params![soid, lane as i64, plug.map(|hash| i64::from(hash.get()))],
            )?;
        }
    }
    Ok(())
}

/// Advances the two allocators the way Dawn's `observe_allocators` does.
fn write_allocators(
    transaction: &rusqlite::Transaction<'_>,
    document: &mut DawnAccountDocument,
) -> Result<(), DawnAccountError> {
    let mut item = document.allocators.item.max(contract::FIRST_ITEM_SOID);
    for character in document.snapshot.characters.characters() {
        for instance in character
            .inventory
            .iter()
            .chain(character.equipment.values().flatten())
        {
            let soid = instance.instance_soid.get();
            if soid >= contract::FIRST_ITEM_SOID {
                item = item.max(soid.saturating_add(1));
            }
        }
    }
    let profile_item = document
        .allocators
        .profile_item
        .max(contract::FIRST_PROFILE_ITEM_SOID);

    for (name, value) in [
        (contract::ITEM_ALLOCATOR, item),
        (contract::PROFILE_ITEM_ALLOCATOR, profile_item),
    ] {
        transaction.execute(
            "UPDATE allocators SET next_value=?2 WHERE name=?1",
            params![name, contract::format_soid(value)],
        )?;
    }
    document.allocators.item = item;
    document.allocators.profile_item = profile_item;
    Ok(())
}

/// Puts back the verified copy taken before a write.
pub(crate) fn restore_backup(path: &Path, backup: &Path) -> Result<(), DawnAccountError> {
    let source = Connection::open_with_flags(backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut destination = Connection::open(path)?;
    let copy = rusqlite::backup::Backup::new(&source, &mut destination)?;
    copy.run_to_completion(64, std::time::Duration::from_millis(0), None)?;
    drop(copy);
    let ok: String = destination.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if ok == "ok" {
        Ok(())
    } else {
        Err(DawnAccountError::Backup(format!(
            "the restored database reported {ok}"
        )))
    }
}
