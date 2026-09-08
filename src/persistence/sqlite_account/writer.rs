use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, MAIN_DB, OpenFlags, Transaction, TransactionBehavior, params};
use sundial_account::{
    CharacterAbilities, DismantleGearClass, DismantleRarity, ItemInstance, ItemPlugs,
};

use crate::storage;

use super::{
    SqliteAccountDocument, SqliteAccountDocumentLoad, SqliteAccountError,
    contract::{ACCOUNT_FORMAT_VERSION, EQUIPMENT_LOCATION, EQUIPMENT_SLOTS, INVENTORY_LOCATION},
    document::{self, SourceRevision},
    reader, settings,
};

pub(crate) struct SqliteSaveReceipt {
    pub(crate) backup: PathBuf,
    pub(crate) checkpoint_warning: Option<String>,
}

pub(crate) struct SqliteRestoreReceipt {
    pub(crate) safety_backup: PathBuf,
}

pub(crate) fn save(
    document: &mut SqliteAccountDocument,
) -> Result<SqliteSaveReceipt, SqliteAccountError> {
    save_with_backup(document, None)
}

fn save_with_backup(
    document: &mut SqliteAccountDocument,
    backup: Option<PathBuf>,
) -> Result<SqliteSaveReceipt, SqliteAccountError> {
    let mut candidate = document.clone();
    candidate.prepare_persistence()?;
    let settings_payload = settings::encode(candidate.settings())?;

    let mut connection = Connection::open_with_flags(
        document.path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| SqliteAccountError::sqlite("open for writing", error))?;
    connection
        .busy_timeout(Duration::from_secs(2))
        .map_err(|error| SqliteAccountError::sqlite("configure", error))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| SqliteAccountError::sqlite("start a write transaction for", error))?;
    reader::validate_schema(&transaction)?;
    if document::database_revision(&transaction)? != document.revision() {
        return Err(SqliteAccountError::SourceChanged);
    }
    let backup = if let Some(backup) = backup {
        create_verified_backup(document.path(), &backup, document.revision())?;
        backup
    } else {
        indexed_backup(document.path(), "state-v1", true, |backup| {
            create_verified_backup(document.path(), backup, document.revision())
        })?
    };
    write_document(&transaction, &candidate, &settings_payload)?;
    transaction
        .commit()
        .map_err(|error| SqliteAccountError::sqlite("commit", error))?;
    let checkpoint_warning = truncate_wal(&connection)
        .err()
        .map(|error| error.to_string());
    let revision = document::database_revision(&connection)?;
    candidate.set_revision(revision);
    *document = candidate;
    Ok(SqliteSaveReceipt {
        backup,
        checkpoint_warning,
    })
}

#[cfg(test)]
pub(super) fn save_for_test(
    document: &mut SqliteAccountDocument,
    backup: PathBuf,
) -> Result<SqliteSaveReceipt, SqliteAccountError> {
    save_with_backup(document, Some(backup))
}

pub(crate) fn restore_backup(destination: &Path, backup: &Path) -> Result<(), SqliteAccountError> {
    let expected = compatible_backup_revision(backup)?;
    let mut connection = Connection::open(destination)
        .map_err(|error| SqliteAccountError::sqlite("open for restoration", error))?;
    connection
        .restore(MAIN_DB, backup, None::<fn(rusqlite::backup::Progress)>)
        .map_err(|error| SqliteAccountError::sqlite("restore", error))?;
    truncate_wal(&connection)?;
    if document::database_revision(&connection)? != expected {
        return Err(SqliteAccountError::Backup(format!(
            "SQLite backup {} was restored but did not verify",
            backup.display()
        )));
    }
    Ok(())
}

pub(crate) fn validate_backup(backup: &Path) -> Result<(), SqliteAccountError> {
    compatible_backup_revision(backup).map(|_| ())
}

pub(crate) fn restore_backup_safely(
    destination: &Path,
    backup: &Path,
) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
    restore_backup_safely_with_path(destination, backup, None)
}

fn restore_backup_safely_with_path(
    destination: &Path,
    backup: &Path,
    safety_backup: Option<PathBuf>,
) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
    compatible_backup_revision(backup)?;
    let safety_backup = if let Some(safety_backup) = safety_backup {
        create_integrity_checked_snapshot(destination, &safety_backup)?;
        safety_backup
    } else {
        indexed_backup(destination, "state-recovery", false, |backup| {
            create_integrity_checked_snapshot(destination, backup)
        })?
    };
    if let Err(restore_error) = restore_backup(destination, backup) {
        return match restore_integrity_checked_snapshot(destination, &safety_backup) {
            Ok(()) => Err(SqliteAccountError::Backup(format!(
                "Could not restore {} ({restore_error}); the original state.sqlite3 was restored from {}",
                backup.display(),
                safety_backup.display()
            ))),
            Err(rollback_error) => Err(SqliteAccountError::Backup(format!(
                "CRITICAL: restoring {} failed ({restore_error}), and restoring the original database also failed ({rollback_error}). The recovery snapshot is at {}",
                backup.display(),
                safety_backup.display()
            ))),
        };
    }
    Ok(SqliteRestoreReceipt { safety_backup })
}

#[cfg(test)]
pub(super) fn restore_backup_safely_for_test(
    destination: &Path,
    backup: &Path,
    safety_backup: PathBuf,
) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
    restore_backup_safely_with_path(destination, backup, Some(safety_backup))
}

fn compatible_backup_revision(backup: &Path) -> Result<SourceRevision, SqliteAccountError> {
    match document::load(backup)? {
        SqliteAccountDocumentLoad::Loaded(document) => Ok(document.revision()),
        _ => Err(SqliteAccountError::Backup(format!(
            "SQLite backup {} does not contain a compatible account snapshot",
            backup.display()
        ))),
    }
}

fn create_integrity_checked_snapshot(
    source_path: &Path,
    backup: &Path,
) -> Result<(), SqliteAccountError> {
    let result = (|| {
        let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| SqliteAccountError::sqlite("open a recovery source for", error))?;
        source
            .busy_timeout(Duration::from_secs(2))
            .map_err(|error| {
                SqliteAccountError::sqlite("configure a recovery source for", error)
            })?;
        validate_integrity(&source, "the current state.sqlite3")?;
        source
            .backup(MAIN_DB, backup, None)
            .map_err(|error| SqliteAccountError::sqlite("create a recovery snapshot of", error))?;
        let snapshot = Connection::open_with_flags(backup, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| SqliteAccountError::sqlite("open a recovery snapshot for", error))?;
        validate_integrity(&snapshot, "the recovery snapshot")
    })();
    finish_backup_attempt(backup, result)
}

fn restore_integrity_checked_snapshot(
    destination: &Path,
    snapshot: &Path,
) -> Result<(), SqliteAccountError> {
    let snapshot_connection =
        Connection::open_with_flags(snapshot, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| SqliteAccountError::sqlite("open a recovery snapshot for", error))?;
    validate_integrity(&snapshot_connection, "the recovery snapshot")?;
    let mut destination_connection = Connection::open(destination)
        .map_err(|error| SqliteAccountError::sqlite("open for recovery", error))?;
    destination_connection
        .restore(MAIN_DB, snapshot, None::<fn(rusqlite::backup::Progress)>)
        .map_err(|error| SqliteAccountError::sqlite("restore the recovery snapshot to", error))?;
    truncate_wal(&destination_connection)?;
    validate_integrity(&destination_connection, "the restored state.sqlite3")
}

fn truncate_wal(connection: &Connection) -> Result<(), SqliteAccountError> {
    let (busy, log_frames, checkpointed_frames) = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| SqliteAccountError::sqlite("checkpoint", error))?;
    validate_checkpoint_status(busy, log_frames, checkpointed_frames)
}

fn validate_checkpoint_status(
    busy: i64,
    log_frames: i64,
    checkpointed_frames: i64,
) -> Result<(), SqliteAccountError> {
    if busy == 0 {
        Ok(())
    } else {
        Err(SqliteAccountError::Backup(format!(
            "could not truncate the SQLite write-ahead log because another database connection kept it busy ({checkpointed_frames} of {log_frames} frames checkpointed)"
        )))
    }
}

fn validate_integrity(
    connection: &Connection,
    description: &str,
) -> Result<(), SqliteAccountError> {
    let result = connection
        .query_row("PRAGMA quick_check(1);", [], |row| row.get::<_, String>(0))
        .map_err(|error| SqliteAccountError::sqlite("check the integrity of", error))?;
    if result.eq_ignore_ascii_case("ok") {
        Ok(())
    } else {
        Err(SqliteAccountError::Backup(format!(
            "Could not preserve {description} because SQLite integrity checking reported: {result}"
        )))
    }
}

fn create_verified_backup(
    source_path: &Path,
    backup: &Path,
    expected_revision: SourceRevision,
) -> Result<(), SqliteAccountError> {
    let result = (|| {
        let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| SqliteAccountError::sqlite("open a backup source for", error))?;
        source
            .busy_timeout(Duration::from_secs(2))
            .map_err(|error| SqliteAccountError::sqlite("configure a backup source for", error))?;
        source
            .backup(MAIN_DB, backup, None)
            .map_err(|error| SqliteAccountError::sqlite("create a verified backup of", error))?;
        let loaded = document::load(backup)?;
        let SqliteAccountDocumentLoad::Loaded(loaded) = loaded else {
            return Err(SqliteAccountError::Backup(format!(
                "SQLite backup {} could not be validated",
                backup.display()
            )));
        };
        if loaded.revision() != expected_revision {
            return Err(SqliteAccountError::Backup(format!(
                "SQLite backup {} does not match the source revision",
                backup.display()
            )));
        }
        Ok(())
    })();
    finish_backup_attempt(backup, result)
}

fn finish_backup_attempt(
    backup: &Path,
    result: Result<(), SqliteAccountError>,
) -> Result<(), SqliteAccountError> {
    match result {
        Ok(()) => Ok(()),
        Err(operation_error) => match storage::remove_file_if_present(backup) {
            Ok(()) => Err(operation_error),
            Err(cleanup_error) => Err(SqliteAccountError::Backup(format!(
                "{operation_error}; the incomplete SQLite backup at {} could not be removed: {cleanup_error}",
                backup.display()
            ))),
        },
    }
}

fn indexed_backup(
    source: &Path,
    prefix: &str,
    automatic: bool,
    write: impl FnOnce(&Path) -> Result<(), SqliteAccountError>,
) -> Result<PathBuf, SqliteAccountError> {
    let root = crate::backups::root().ok_or_else(|| {
        SqliteAccountError::Backup(
            "could not locate Sundial's local backup folder for state.sqlite3".to_owned(),
        )
    })?;
    crate::backups::create(&root, source, prefix, "sqlite3", automatic, |path, _| {
        write(path).map_err(|error| error.to_string())
    })
    .map_err(SqliteAccountError::Backup)
}

fn write_document(
    transaction: &Transaction<'_>,
    document: &SqliteAccountDocument,
    settings_payload: &[u8],
) -> Result<(), SqliteAccountError> {
    transaction
        .execute_batch(
            "DELETE FROM item_plugs; DELETE FROM character_items; DELETE FROM characters; \
             DELETE FROM profile_items; DELETE FROM dismantle_rewards; DELETE FROM account_state;",
        )
        .map_err(|error| SqliteAccountError::sqlite("replace", error))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now = i64::try_from(now).map_err(|_| {
        SqliteAccountError::invalid_data(
            "account_state.updated_unix_seconds",
            "system time does not fit SQLite's signed timestamp",
        )
    })?;
    transaction
        .execute(
            "INSERT INTO account_state (singleton, format_version, primary_soid, \
             dismantle_reward_count, profile_item_count, character_count, settings_payload, \
             updated_unix_seconds) VALUES (1, ?, ?, ?, ?, ?, ?, ?);",
            params![
                ACCOUNT_FORMAT_VERSION,
                sql_u64(document.primary_soid().get()),
                sql_count(document.profile().dismantle_rewards().len())?,
                sql_count(document.profile().profile_items().len())?,
                sql_count(document.characters().characters().len())?,
                settings_payload,
                now,
            ],
        )
        .map_err(|error| SqliteAccountError::sqlite("write root to", error))?;

    for (position, reward) in document.profile().dismantle_rewards().iter().enumerate() {
        let tier_mask = reward
            .rarities
            .iter()
            .fold(0_u8, |mask, rarity| mask | rarity_bit(*rarity));
        let class_mask = match reward.gear_class {
            None => 0,
            Some(DismantleGearClass::Weapon) => 1,
            Some(DismantleGearClass::Armor) => 2,
            Some(DismantleGearClass::Both) => 3,
        };
        let masterwork = match reward.masterworked {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        };
        transaction
            .execute(
                "INSERT INTO dismantle_rewards (account_id, position, definition_hash, quantity, \
                 tier_mask, class_mask, masterwork) VALUES (1, ?, ?, ?, ?, ?, ?);",
                params![
                    sql_count(position)?,
                    i64::from(reward.definition_hash.get()),
                    reward.quantity,
                    tier_mask,
                    class_mask,
                    masterwork,
                ],
            )
            .map_err(|error| SqliteAccountError::sqlite("write dismantle rewards to", error))?;
    }

    for (position, item) in document.profile().profile_items().iter().enumerate() {
        let (instance_soid, mutation_serial) = document.profile_persistence(item.id)?;
        transaction
            .execute(
                "INSERT INTO profile_items (account_id, position, instance_soid, definition_hash, \
                 quantity, mutation_serial) VALUES (1, ?, ?, ?, ?, ?);",
                params![
                    sql_count(position)?,
                    sql_u64(instance_soid),
                    i64::from(item.definition_hash.get()),
                    item.quantity,
                    mutation_serial,
                ],
            )
            .map_err(|error| SqliteAccountError::sqlite("write profile items to", error))?;
    }

    for (character_position, character) in document.characters().characters().iter().enumerate() {
        let (
            selected,
            level,
            accepted,
            preview_available,
            appearance,
            last_orbited_destination,
            content_bypass,
            acquired_mask,
            next_inventory_serial,
        ) = document.character_persistence(character.id)?;
        let metadata = character.metadata.ok_or_else(|| {
            SqliteAccountError::invalid_data(
                "characters",
                "SQLite characters require loaded metadata",
            )
        })?;
        let soid = character.soid.ok_or_else(|| {
            SqliteAccountError::invalid_data("characters.soid", "SQLite character SOID is missing")
        })?;
        transaction
            .execute(
                "INSERT INTO characters (account_id, position, soid, selected, race, gender, \
                 character_class, level, accepted, preview_available, appearance_value, \
                 last_orbited_destination, content_bypass, acquired_subclass_ability_mask, \
                 inventory_count, next_inventory_serial) \
                 VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
                params![
                    sql_count(character_position)?,
                    sql_u64(soid.get()),
                    selected,
                    metadata.race,
                    metadata.gender,
                    metadata.class_type,
                    level,
                    accepted,
                    preview_available,
                    appearance,
                    i64::from(last_orbited_destination),
                    content_bypass,
                    sql_u64(acquired_mask),
                    sql_count(character.inventory.len())?,
                    i64::from(next_inventory_serial),
                ],
            )
            .map_err(|error| SqliteAccountError::sqlite("write characters to", error))?;

        for (slot_position, slot) in EQUIPMENT_SLOTS.into_iter().enumerate() {
            if let Some(item) = character
                .equipment
                .get(&sundial_account::EquipmentSlot::new(slot))
                .and_then(Option::as_ref)
            {
                let abilities = if slot == "subclass" {
                    metadata.abilities
                } else {
                    document.item_persistence(item.id)?.1
                };
                write_item(
                    transaction,
                    document,
                    character_position,
                    EQUIPMENT_LOCATION,
                    slot_position,
                    item,
                    abilities,
                )?;
            }
        }
        for (position, item) in character.inventory.iter().enumerate() {
            let abilities = document.item_persistence(item.id)?.1;
            write_item(
                transaction,
                document,
                character_position,
                INVENTORY_LOCATION,
                position,
                item,
                abilities,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_item(
    transaction: &Transaction<'_>,
    document: &SqliteAccountDocument,
    character_position: usize,
    location: i64,
    position: usize,
    item: &ItemInstance,
    abilities: CharacterAbilities,
) -> Result<(), SqliteAccountError> {
    let (mutation_serial, _) = document.item_persistence(item.id)?;
    let (socket_policy, plugs): (u8, &[Option<sundial_account::DefinitionHash>]) = match &item.plugs
    {
        ItemPlugs::NativeDefaults => (0, &[]),
        ItemPlugs::Authored(plugs) => (1, plugs),
    };
    transaction
        .execute(
            "INSERT INTO character_items (account_id, character_position, location, position, \
             instance_soid, definition_hash, item_level, quantity, mutation_serial, flags, \
             socket_policy, plug_count, movement_ability_entry, grenade_ability_entry, \
             super_ability_entry, melee_ability_entry, class_ability_entry) \
             VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
            params![
                sql_count(character_position)?,
                location,
                sql_count(position)?,
                sql_u64(item.instance_soid.get()),
                i64::from(item.definition_hash.get()),
                item.level,
                item.quantity,
                mutation_serial,
                i64::from(item.flags.unwrap_or(0)),
                socket_policy,
                sql_count(plugs.len())?,
                abilities.movement,
                abilities.grenade,
                abilities.super_ability,
                abilities.melee,
                abilities.class_ability,
            ],
        )
        .map_err(|error| SqliteAccountError::sqlite("write character items to", error))?;
    for (plug_position, plug) in plugs.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO item_plugs (account_id, character_position, location, item_position, \
                 plug_position, definition_hash) VALUES (1, ?, ?, ?, ?, ?);",
                params![
                    sql_count(character_position)?,
                    location,
                    sql_count(position)?,
                    sql_count(plug_position)?,
                    plug.map(|hash| i64::from(hash.get())),
                ],
            )
            .map_err(|error| SqliteAccountError::sqlite("write item plugs to", error))?;
    }
    Ok(())
}

const fn rarity_bit(rarity: DismantleRarity) -> u8 {
    match rarity {
        DismantleRarity::Common => 1 << 1,
        DismantleRarity::Uncommon => 1 << 2,
        DismantleRarity::Rare => 1 << 3,
        DismantleRarity::Legendary => 1 << 4,
        DismantleRarity::Exotic => 1 << 5,
    }
}

fn sql_count(value: usize) -> Result<i64, SqliteAccountError> {
    i64::try_from(value).map_err(|_| {
        SqliteAccountError::invalid_data("SQLite position", "value does not fit in signed 64 bits")
    })
}

const fn sql_u64(value: u64) -> i64 {
    value as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDirectory;
    use std::fs;

    #[test]
    fn busy_checkpoint_is_reported_as_incomplete() {
        let error = validate_checkpoint_status(1, 8, 3).unwrap_err();

        assert_eq!(
            error.to_string(),
            "could not truncate the SQLite write-ahead log because another database connection kept it busy (3 of 8 frames checkpointed)"
        );
    }

    #[test]
    fn failed_backup_cleanup_preserves_the_primary_error_and_names_the_residue() {
        let directory = TestDirectory::new("sqlite-backup-cleanup-failure");
        let residue = directory.0.join("incomplete.sqlite3");
        fs::create_dir(&residue).unwrap();

        let error = finish_backup_attempt(
            &residue,
            Err(SqliteAccountError::Backup(
                "injected backup failure".to_owned(),
            )),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("injected backup failure"));
        assert!(error.contains("incomplete SQLite backup"));
        assert!(error.contains(&residue.display().to_string()));
    }
}
