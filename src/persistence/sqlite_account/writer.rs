use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, MAIN_DB, OpenFlags, Transaction, TransactionBehavior, params};
use sundial_account::{
    CharacterAbilities, DismantleGearClass, DismantleRarity, ItemInstance, ItemPlugs,
};

use crate::storage;

use super::{
    SqliteAccountDocument, SqliteAccountDocumentLoad, SqliteAccountError,
    contract::{EQUIPMENT_LOCATION, EQUIPMENT_SLOTS, INVENTORY_LOCATION},
    document::{self, SourceRevision},
    reader, settings,
};

pub(crate) struct SqliteSaveReceipt {
    pub(crate) backup: PathBuf,
    pub(crate) checkpoint_warning: Option<String>,
    before: Vec<u8>,
    committed: Vec<u8>,
}

pub(crate) fn rollback_save(
    path: &Path,
    receipt: &SqliteSaveReceipt,
) -> Result<(), SqliteAccountError> {
    super::package::rollback(path, &receipt.committed, &receipt.before)
        .map_err(SqliteAccountError::Backup)
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

    let mut connection = Connection::open_with_flags(
        document.path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| SqliteAccountError::sqlite("open for writing", error))?;
    connection
        .busy_timeout(Duration::from_secs(2))
        .map_err(|error| SqliteAccountError::sqlite("configure", error))?;
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")
        .map_err(|error| SqliteAccountError::sqlite("configure", error))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| SqliteAccountError::sqlite("start a write transaction for", error))?;
    reader::validate_schema(&transaction)?;
    if document::database_revision(&transaction)? != document.revision() {
        return Err(SqliteAccountError::SourceChanged);
    }
    candidate.validate_native_edits(&transaction)?;
    let backup = if let Some(backup) = backup {
        create_verified_backup(document.path(), &backup, document.revision())?;
        backup
    } else {
        indexed_backup(document.path(), "investment-v2", true, |backup| {
            create_verified_backup(document.path(), backup, document.revision())
        })?
    };
    let before = super::package::capture(&transaction).map_err(SqliteAccountError::Backup)?;
    write_document(&transaction, &candidate)?;
    reader::load_connection(&transaction)?;
    let revision = document::database_revision(&transaction)?;
    candidate.capture_preserved_rows(&transaction)?;
    let committed = super::package::capture(&transaction).map_err(SqliteAccountError::Backup)?;
    super::package::verify_unedited_tables(
        &before,
        &committed,
        &[
            "account",
            "characters",
            "items",
            "sockets",
            "profile_items",
            "dismantle_rewards",
            "unlocks",
            "family5",
            "entitlements",
            "account_preferences",
            "account_controls",
            "account_audio",
            "account_display",
            "account_interface",
            "account_social",
            "account_key_bindings",
        ],
    )
    .map_err(SqliteAccountError::Backup)?;
    transaction
        .commit()
        .map_err(|error| SqliteAccountError::sqlite("commit", error))?;
    let checkpoint_warning = truncate_wal(&connection)
        .err()
        .map(|error| error.to_string());
    candidate.set_revision(revision);
    candidate.refresh_positions();
    *document = candidate;
    Ok(SqliteSaveReceipt {
        backup,
        checkpoint_warning,
        before,
        committed,
    })
}

#[cfg(test)]
pub(super) fn save_for_test(
    document: &mut SqliteAccountDocument,
    backup: PathBuf,
) -> Result<SqliteSaveReceipt, SqliteAccountError> {
    save_with_backup(document, Some(backup))
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
        indexed_backup(destination, "investment-recovery", false, |backup| {
            create_integrity_checked_snapshot(destination, backup)
        })?
    };
    let expected =
        super::package::capture_path(&safety_backup).map_err(SqliteAccountError::Backup)?;
    super::package::restore(destination, &expected, backup).map_err(|error| {
        SqliteAccountError::Backup(format!(
            "Could not restore the database: {error}. The recovery snapshot is at {}",
            safety_backup.display()
        ))
    })?;
    Ok(SqliteRestoreReceipt { safety_backup })
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
        validate_integrity(&source, "the current investment.sqlite3")?;
        source
            .backup(MAIN_DB, backup, None)
            .map_err(|error| SqliteAccountError::sqlite("create a recovery snapshot of", error))?;
        let snapshot = Connection::open_with_flags(backup, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| SqliteAccountError::sqlite("open a recovery snapshot for", error))?;
        validate_integrity(&snapshot, "the recovery snapshot")
    })();
    finish_backup_attempt(backup, result)
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
                "{operation_error}. The incomplete SQLite backup at {} could not be removed: {cleanup_error}",
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
            "could not locate Sundial's local backup folder for investment.sqlite3".to_owned(),
        )
    })?;
    crate::backups::create(&root, source, prefix, "sqlite3", automatic, |path, _| {
        write(path).map_err(|error| error.to_string())
    })
    .map_err(SqliteAccountError::Backup)
}

// Snapshot rows before replacing positional collections, so opaque columns follow their exact
// item identity rather than being reset to defaults during an equip, move or removal.
pub(super) type NativeRow = std::collections::BTreeMap<String, super::package::Cell>;

pub(super) fn rows(
    connection: &Connection,
    table: &str,
) -> Result<Vec<NativeRow>, SqliteAccountError> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table}"))
        .map_err(|error| SqliteAccountError::sqlite("preserve native rows from", error))?;
    let names = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mapped = statement
        .query_map([], |row| {
            names
                .iter()
                .enumerate()
                .map(|(i, name)| Ok((name.clone(), super::package::Cell::from_value(row.get(i)?))))
                .collect()
        })
        .map_err(|error| SqliteAccountError::sqlite("preserve native rows from", error))?;
    mapped
        .collect::<Result<_, _>>()
        .map_err(|error| SqliteAccountError::sqlite("preserve native rows from", error))
}

fn matching(rows: &[NativeRow], keys: &[(&str, i64)]) -> NativeRow {
    rows.iter()
        .find(|row| {
            keys.iter()
                .all(|(key, value)| row.get(*key) == Some(&super::package::Cell::Integer(*value)))
        })
        .cloned()
        .unwrap_or_default()
}

fn put(row: &mut NativeRow, name: &str, value: impl Into<rusqlite::types::Value>) {
    row.insert(name.into(), super::package::Cell::from_value(value.into()));
}

pub(super) fn insert(
    connection: &Connection,
    table: &str,
    row: NativeRow,
) -> Result<(), SqliteAccountError> {
    let names = row
        .keys()
        .map(|name| format!("\"{}\"", name.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
    let placeholders = vec!["?"; row.len()].join(",");
    connection
        .execute(
            &format!("INSERT INTO {table} ({names}) VALUES ({placeholders})"),
            rusqlite::params_from_iter(row.values().map(super::package::Cell::value)),
        )
        .map_err(|error| SqliteAccountError::sqlite("write native rows to", error))?;
    Ok(())
}

fn write_document(
    transaction: &Transaction<'_>,
    document: &SqliteAccountDocument,
) -> Result<(), SqliteAccountError> {
    let old_rewards = document.preserved_rows("dismantle_rewards");
    let old_profile = document.preserved_rows("profile_items");
    let old_items = document.preserved_rows("items");
    let old_sockets = document.preserved_rows("sockets");
    transaction.execute_batch("DELETE FROM sockets; DELETE FROM items; DELETE FROM profile_items; DELETE FROM dismantle_rewards;")
        .map_err(|error| SqliteAccountError::sqlite("replace account collections in", error))?;
    for (position, reward) in document.profile().dismantle_rewards().iter().enumerate() {
        let mut row = document
            .original_reward_position(reward.id)
            .map(|p| matching(old_rewards, &[("position", p as i64)]))
            .unwrap_or_default();
        put(&mut row, "position", sql_count(position)?);
        put(
            &mut row,
            "definition_hash",
            i64::from(reward.definition_hash.get()),
        );
        put(&mut row, "quantity", reward.quantity);
        put(
            &mut row,
            "tier_mask",
            i64::from(
                reward
                    .rarities
                    .iter()
                    .fold(0_u8, |mask, rarity| mask | rarity_bit(*rarity)),
            ),
        );
        put(
            &mut row,
            "class_mask",
            match reward.gear_class {
                None => 0,
                Some(DismantleGearClass::Weapon) => 1,
                Some(DismantleGearClass::Armor) => 2,
                Some(DismantleGearClass::Both) => 3,
            },
        );
        put(
            &mut row,
            "masterwork",
            match reward.masterworked {
                None => 0,
                Some(true) => 1,
                Some(false) => 2,
            },
        );
        insert(transaction, "dismantle_rewards", row)?;
    }
    for (position, item) in document.profile().profile_items().iter().enumerate() {
        let (soid, serial) = document.profile_persistence(item.id)?;
        let mut row = document
            .original_profile_position(item.id)
            .map(|p| matching(old_profile, &[("position", p as i64)]))
            .unwrap_or_default();
        row.entry("seen".into())
            .or_insert(super::package::Cell::Integer(1));
        put(&mut row, "position", sql_count(position)?);
        put(&mut row, "instance_soid", sql_u64(soid));
        put(
            &mut row,
            "definition_hash",
            i64::from(item.definition_hash.get()),
        );
        put(&mut row, "quantity", item.quantity);
        put(&mut row, "mutation_serial", serial);
        insert(transaction, "profile_items", row)?;
    }
    for character in document.characters().characters() {
        let metadata = character
            .metadata
            .ok_or_else(|| SqliteAccountError::invalid_data("characters", "missing metadata"))?;
        let soid = character
            .soid
            .ok_or_else(|| SqliteAccountError::invalid_data("characters", "missing SOID"))?;
        let slot: usize = transaction
            .query_row(
                "SELECT slot FROM characters WHERE soid=?",
                [sql_u64(soid.get())],
                |row| row.get(0),
            )
            .map_err(|error| SqliteAccountError::sqlite("identify character in", error))?;
        let serial = document.next_inventory_serial(character.id)?;
        transaction.execute("UPDATE characters SET race=?, gender=?, class=?, next_inventory_serial=? WHERE slot=?", params![metadata.race, metadata.gender, metadata.class_type, serial, slot])
            .map_err(|error| SqliteAccountError::sqlite("write characters to", error))?;
        let context = ItemWriter {
            transaction,
            document,
            old_items,
            old_sockets,
            character_slot: slot,
        };
        for (position, name) in EQUIPMENT_SLOTS.into_iter().enumerate() {
            if let Some(item) = character
                .equipment
                .get(&sundial_account::EquipmentSlot::new(name))
                .and_then(Option::as_ref)
            {
                let abilities = if name == "subclass" {
                    metadata.abilities
                } else {
                    document.item_persistence(item.id)?.1
                };
                context.write(EQUIPMENT_LOCATION, position, item, abilities)?;
            }
        }
        for (position, item) in character.inventory.iter().enumerate() {
            context.write(
                INVENTORY_LOCATION,
                position,
                item,
                document.item_persistence(item.id)?.1,
            )?;
        }
    }
    document.save_progression(transaction)?;
    document.save_entitlements(transaction)?;
    document.save_runtime(transaction)?;
    settings::save(transaction, document.settings())
}

struct ItemWriter<'a, 'connection> {
    transaction: &'a Transaction<'connection>,
    document: &'a SqliteAccountDocument,
    old_items: &'a [NativeRow],
    old_sockets: &'a [NativeRow],
    character_slot: usize,
}
impl ItemWriter<'_, '_> {
    fn write(
        &self,
        location: i64,
        position: usize,
        item: &ItemInstance,
        abilities: CharacterAbilities,
    ) -> Result<(), SqliteAccountError> {
        let soid = sql_u64(item.instance_soid.get());
        let mut row = matching(self.old_items, &[("instance_soid", soid)]);
        row.entry("seen".into())
            .or_insert(super::package::Cell::Integer(0));
        let (policy, plugs): (i64, &[Option<sundial_account::DefinitionHash>]) = match &item.plugs {
            ItemPlugs::NativeDefaults => (0, &[]),
            ItemPlugs::Authored(plugs) => (1, plugs),
        };
        for (name, value) in [
            ("character_slot", sql_count(self.character_slot)?),
            ("location", location),
            ("position", sql_count(position)?),
            ("instance_soid", soid),
            ("definition_hash", i64::from(item.definition_hash.get())),
            ("level", i64::from(item.level)),
            ("quantity", i64::from(item.quantity)),
            (
                "mutation_serial",
                i64::from(self.document.item_persistence(item.id)?.0),
            ),
            ("flags", i64::from(item.flags.unwrap_or(0))),
            ("socket_policy", policy),
            ("plug_count", sql_count(plugs.len())?),
            ("movement_ability", i64::from(abilities.movement)),
            ("grenade_ability", i64::from(abilities.grenade)),
            ("super_ability", i64::from(abilities.super_ability)),
            ("melee_ability", i64::from(abilities.melee)),
            ("class_ability", i64::from(abilities.class_ability)),
        ] {
            put(&mut row, name, value);
        }
        insert(self.transaction, "items", row)?;
        for (lane, plug) in plugs.iter().enumerate() {
            if let Some(hash) = plug {
                let mut row = matching(
                    self.old_sockets,
                    &[("instance_soid", soid), ("lane", sql_count(lane)?)],
                );
                put(&mut row, "instance_soid", soid);
                put(&mut row, "lane", sql_count(lane)?);
                put(&mut row, "plug_hash", i64::from(hash.get()));
                insert(self.transaction, "sockets", row)?;
            }
        }
        Ok(())
    }
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
