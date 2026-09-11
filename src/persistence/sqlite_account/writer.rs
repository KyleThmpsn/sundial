mod collections;
use collections::write_document;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, MAIN_DB, OpenFlags, TransactionBehavior};

use crate::storage;

use super::{
    SqliteAccountDocument, SqliteAccountDocumentLoad, SqliteAccountError,
    document::{self, SourceRevision},
    reader,
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
            "character_stacks",
            "pending_rewards",
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

pub(super) fn matching(rows: &[NativeRow], keys: &[(&str, i64)]) -> NativeRow {
    rows.iter()
        .find(|row| {
            keys.iter()
                .all(|(key, value)| row.get(*key) == Some(&super::package::Cell::Integer(*value)))
        })
        .cloned()
        .unwrap_or_default()
}

pub(super) fn put(row: &mut NativeRow, name: &str, value: impl Into<rusqlite::types::Value>) {
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
