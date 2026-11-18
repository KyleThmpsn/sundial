mod collections;
use collections::write_document;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, MAIN_DB, OpenFlags, TransactionBehavior};

use crate::storage;

use super::{
    SunriseAccountDocument, SunriseAccountDocumentLoad, SunriseAccountError,
    document::{self, SourceRevision},
    reader,
};

pub(crate) struct SunriseSaveReceipt {
    pub(crate) backup: PathBuf,
    pub(crate) checkpoint_warning: Option<String>,
    before: Vec<u8>,
    committed: Vec<u8>,
}

pub(crate) fn rollback_save(
    path: &Path,
    receipt: &SunriseSaveReceipt,
) -> Result<(), SunriseAccountError> {
    super::snapshot::rollback(path, &receipt.committed, &receipt.before)
        .map_err(SunriseAccountError::Backup)
}

pub(crate) struct SunriseRestoreReceipt {
    pub(crate) safety_backup: PathBuf,
}

pub(crate) fn save_checked(
    document: &mut SunriseAccountDocument,
    require_closed: impl FnMut() -> Result<(), String>,
) -> Result<SunriseSaveReceipt, SunriseAccountError> {
    save_with_backup_checked(document, None, require_closed)
}

#[cfg(test)]
pub(super) fn save_with_backup(
    document: &mut SunriseAccountDocument,
    backup: Option<PathBuf>,
) -> Result<SunriseSaveReceipt, SunriseAccountError> {
    save_with_backup_checked(document, backup, || Ok(()))
}

fn save_with_backup_checked(
    document: &mut SunriseAccountDocument,
    backup: Option<PathBuf>,
    mut require_closed: impl FnMut() -> Result<(), String>,
) -> Result<SunriseSaveReceipt, SunriseAccountError> {
    require_closed().map_err(SunriseAccountError::WriteBlocked)?;
    let mut candidate = document.clone();
    candidate.prepare_persistence()?;

    let mut connection = Connection::open_with_flags(
        document.path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| SunriseAccountError::sqlite("open for writing", error))?;
    connection
        .busy_timeout(Duration::from_secs(2))
        .map_err(|error| SunriseAccountError::sqlite("configure", error))?;
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")
        .map_err(|error| SunriseAccountError::sqlite("configure", error))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| SunriseAccountError::sqlite("start a write transaction for", error))?;
    reader::validate_schema(&transaction)?;
    if document::database_revision(&transaction)? != document.revision() {
        return Err(SunriseAccountError::SourceChanged);
    }
    candidate.validate_native_edits(&transaction)?;
    let backup = if let Some(backup) = backup {
        create_verified_backup(document.path(), &backup, document.revision())?;
        backup
    } else {
        indexed_backup(
            document.path(),
            document.schema().backup_prefix(),
            true,
            |backup| create_verified_backup(document.path(), backup, document.revision()),
        )?
    };
    let before = super::snapshot::capture(&transaction).map_err(SunriseAccountError::Backup)?;
    // Backup and validation can take time. A process that started during either must not
    // receive an account mutation, even when its persisted revision has not changed yet.
    require_closed().map_err(SunriseAccountError::WriteBlocked)?;
    write_document(&transaction, &candidate)?;
    reader::load_connection(&transaction)?;
    let revision = document::database_revision(&transaction)?;
    candidate.capture_preserved_rows(&transaction)?;
    let committed = super::snapshot::capture(&transaction).map_err(SunriseAccountError::Backup)?;
    super::snapshot::verify_unedited_tables(
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
            "character_stacks",
            "pending_rewards",
            // Schema 7 deletes an item's objectives with the item.
            "item_objectives",
        ],
    )
    .map_err(SunriseAccountError::Backup)?;
    require_closed().map_err(SunriseAccountError::WriteBlocked)?;
    transaction
        .commit()
        .map_err(|error| SunriseAccountError::sqlite("commit", error))?;
    let checkpoint_warning = truncate_wal(&connection)
        .err()
        .map(|error| error.to_string());
    candidate.set_revision(revision);
    candidate.refresh_positions();
    *document = candidate;
    Ok(SunriseSaveReceipt {
        backup,
        checkpoint_warning,
        before,
        committed,
    })
}

#[cfg(test)]
pub(super) fn save_for_test(
    document: &mut SunriseAccountDocument,
    backup: PathBuf,
) -> Result<SunriseSaveReceipt, SunriseAccountError> {
    save_with_backup(document, Some(backup))
}

pub(crate) fn validate_backup(backup: &Path) -> Result<(), SunriseAccountError> {
    compatible_backup_revision(backup).map(|_| ())
}

#[cfg(test)]
pub(crate) fn restore_backup_safely(
    destination: &Path,
    backup: &Path,
) -> Result<SunriseRestoreReceipt, SunriseAccountError> {
    restore_backup_checked(destination, backup, || Ok(()))
}

/// Replaces the account with `backup` after keeping an integrity-checked safety snapshot of it.
/// Refused whenever `require_closed` reports the game running: before the snapshot, again before
/// the restore and last just before its commit, since a game started while the snapshot or the
/// tables are written must keep its database.
pub(crate) fn restore_backup_checked(
    destination: &Path,
    backup: &Path,
    require_closed: impl FnMut() -> Result<(), String>,
) -> Result<SunriseRestoreReceipt, SunriseAccountError> {
    restore_backup_safely_with_path(destination, backup, None, require_closed)
}

fn restore_backup_safely_with_path(
    destination: &Path,
    backup: &Path,
    safety_backup: Option<PathBuf>,
    mut require_closed: impl FnMut() -> Result<(), String>,
) -> Result<SunriseRestoreReceipt, SunriseAccountError> {
    require_closed().map_err(SunriseAccountError::WriteBlocked)?;
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
        super::snapshot::capture_path(&safety_backup).map_err(SunriseAccountError::Backup)?;
    require_closed().map_err(SunriseAccountError::WriteBlocked)?;
    super::snapshot::restore_checked(destination, &expected, backup, require_closed).map_err(
        |error| {
            SunriseAccountError::Backup(format!(
                "Could not restore the database: {error}. The recovery snapshot is at {}",
                safety_backup.display()
            ))
        },
    )?;
    Ok(SunriseRestoreReceipt { safety_backup })
}

fn compatible_backup_revision(backup: &Path) -> Result<SourceRevision, SunriseAccountError> {
    match document::load(backup)? {
        SunriseAccountDocumentLoad::Loaded(document) => Ok(document.revision()),
        _ => Err(SunriseAccountError::Backup(format!(
            "SQLite backup {} does not contain a compatible account snapshot",
            backup.display()
        ))),
    }
}

pub(super) fn create_integrity_checked_snapshot(
    source_path: &Path,
    backup: &Path,
) -> Result<(), SunriseAccountError> {
    let result = (|| {
        let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| SunriseAccountError::sqlite("open a recovery source for", error))?;
        source
            .busy_timeout(Duration::from_secs(2))
            .map_err(|error| {
                SunriseAccountError::sqlite("configure a recovery source for", error)
            })?;
        validate_integrity(&source, "the current investment.sqlite3")?;
        source
            .backup(MAIN_DB, backup, rusqlite::backup::NO_PROGRESS)
            .map_err(|error| SunriseAccountError::sqlite("create a recovery snapshot of", error))?;
        let snapshot = Connection::open_with_flags(backup, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| SunriseAccountError::sqlite("open a recovery snapshot for", error))?;
        validate_integrity(&snapshot, "the recovery snapshot")
    })();
    finish_backup_attempt(backup, result)
}

fn truncate_wal(connection: &Connection) -> Result<(), SunriseAccountError> {
    let (busy, log_frames, checkpointed_frames) = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| SunriseAccountError::sqlite("checkpoint", error))?;
    validate_checkpoint_status(busy, log_frames, checkpointed_frames)
}

fn validate_checkpoint_status(
    busy: i64,
    log_frames: i64,
    checkpointed_frames: i64,
) -> Result<(), SunriseAccountError> {
    if busy == 0 {
        Ok(())
    } else {
        Err(SunriseAccountError::Backup(format!(
            "could not truncate the SQLite write-ahead log because another database connection kept it busy ({checkpointed_frames} of {log_frames} frames checkpointed)"
        )))
    }
}

fn validate_integrity(
    connection: &Connection,
    description: &str,
) -> Result<(), SunriseAccountError> {
    let result = connection
        .query_row("PRAGMA quick_check(1);", [], |row| row.get::<_, String>(0))
        .map_err(|error| SunriseAccountError::sqlite("check the integrity of", error))?;
    if result.eq_ignore_ascii_case("ok") {
        Ok(())
    } else {
        Err(SunriseAccountError::Backup(format!(
            "Could not preserve {description} because SQLite integrity checking reported: {result}"
        )))
    }
}

fn create_verified_backup(
    source_path: &Path,
    backup: &Path,
    expected_revision: SourceRevision,
) -> Result<(), SunriseAccountError> {
    let result = (|| {
        let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| SunriseAccountError::sqlite("open a backup source for", error))?;
        source
            .busy_timeout(Duration::from_secs(2))
            .map_err(|error| SunriseAccountError::sqlite("configure a backup source for", error))?;
        source
            .backup(MAIN_DB, backup, rusqlite::backup::NO_PROGRESS)
            .map_err(|error| SunriseAccountError::sqlite("create a verified backup of", error))?;
        let loaded = document::load(backup)?;
        let SunriseAccountDocumentLoad::Loaded(loaded) = loaded else {
            return Err(SunriseAccountError::Backup(format!(
                "SQLite backup {} could not be validated",
                backup.display()
            )));
        };
        if loaded.revision() != expected_revision {
            return Err(SunriseAccountError::Backup(format!(
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
    result: Result<(), SunriseAccountError>,
) -> Result<(), SunriseAccountError> {
    match result {
        Ok(()) => Ok(()),
        Err(operation_error) => match storage::remove_file_if_present(backup) {
            Ok(()) => Err(operation_error),
            Err(cleanup_error) => Err(SunriseAccountError::Backup(format!(
                "{operation_error}. The incomplete SQLite backup at {} could not be removed: {cleanup_error}",
                backup.display()
            ))),
        },
    }
}

pub(super) fn indexed_backup(
    source: &Path,
    prefix: &str,
    automatic: bool,
    write: impl FnOnce(&Path) -> Result<(), SunriseAccountError>,
) -> Result<PathBuf, SunriseAccountError> {
    let root = crate::backups::root().ok_or_else(|| {
        SunriseAccountError::Backup(
            "could not locate Sundial's local backup folder for investment.sqlite3".to_owned(),
        )
    })?;
    crate::backups::create(&root, source, prefix, "sqlite3", automatic, |path, _| {
        write(path).map_err(|error| error.to_string())
    })
    .map_err(SunriseAccountError::Backup)
}

// Snapshot rows before replacing positional collections, so opaque columns follow their exact
// item identity rather than being reset to defaults during an equip, move or removal.
pub(super) type NativeRow = std::collections::BTreeMap<String, super::snapshot::Cell>;

pub(super) fn rows(
    connection: &Connection,
    table: &str,
) -> Result<Vec<NativeRow>, SunriseAccountError> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table}"))
        .map_err(|error| SunriseAccountError::sqlite("preserve native rows from", error))?;
    let names = statement
        .column_names()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mapped = statement
        .query_map([], |row| {
            names
                .iter()
                .enumerate()
                .map(|(i, name)| Ok((name.clone(), super::snapshot::Cell::from_value(row.get(i)?))))
                .collect()
        })
        .map_err(|error| SunriseAccountError::sqlite("preserve native rows from", error))?;
    mapped
        .collect::<Result<_, _>>()
        .map_err(|error| SunriseAccountError::sqlite("preserve native rows from", error))
}

/// Preserved rows by the integer columns that name them. An account holds thousands of items and
/// sockets and each written row looks up the one it replaces, so a save indexes the rows once
/// instead of searching them per row. Of rows that share a key, the first stands.
pub(super) struct Preserved<'a>(std::collections::BTreeMap<Vec<i64>, &'a NativeRow>);

impl<'a> Preserved<'a> {
    pub(super) fn new(rows: &'a [NativeRow], columns: &[&str]) -> Self {
        let mut index = std::collections::BTreeMap::new();
        for row in rows {
            let key = columns
                .iter()
                .map(|column| match row.get(*column) {
                    Some(super::snapshot::Cell::Integer(value)) => Some(*value),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            if let Some(key) = key {
                index.entry(key).or_insert(row);
            }
        }
        Self(index)
    }

    /// The row whose columns hold `key`, in the order they were named, or an empty row.
    pub(super) fn get(&self, key: &[i64]) -> NativeRow {
        self.0
            .get(key)
            .map_or_else(NativeRow::default, |row| (*row).clone())
    }
}

pub(super) fn put(row: &mut NativeRow, name: &str, value: impl Into<rusqlite::types::Value>) {
    row.insert(name.into(), super::snapshot::Cell::from_value(value.into()));
}

pub(super) fn insert(
    connection: &Connection,
    table: &str,
    row: NativeRow,
) -> Result<(), SunriseAccountError> {
    let names = row
        .keys()
        .map(|name| format!("\"{}\"", name.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
    let placeholders = vec!["?"; row.len()].join(",");
    connection
        .execute(
            &format!("INSERT INTO {table} ({names}) VALUES ({placeholders})"),
            rusqlite::params_from_iter(row.values().map(super::snapshot::Cell::value)),
        )
        .map_err(|error| SunriseAccountError::sqlite("write native rows to", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDirectory;
    use std::fs;

    #[test]
    fn failed_backup_cleanup_preserves_the_primary_error_and_names_the_residue() {
        let directory = TestDirectory::new("sqlite-backup-cleanup-failure");
        let residue = directory.0.join("incomplete.sqlite3");
        fs::create_dir(&residue).unwrap();

        let error = finish_backup_attempt(
            &residue,
            Err(SunriseAccountError::Backup(
                "injected backup failure".to_owned(),
            )),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("injected backup failure"));
        assert!(error.contains(&residue.display().to_string()));
    }
}
