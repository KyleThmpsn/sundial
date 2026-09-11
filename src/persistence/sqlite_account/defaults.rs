//! Explicit account reset from the installed Sunrise resources, with a recoverable snapshot.
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::{SqliteAccountError, SqliteRestoreReceipt, package, reader, writer};

pub(crate) struct AccountDefaults {
    pub(crate) schema: String,
    pub(crate) rows: String,
    pub(crate) settings_schema: String,
    pub(crate) settings_rows: String,
}

pub(crate) struct ResetPlan {
    path: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}

impl ResetPlan {
    pub(crate) fn prepare(
        path: &Path,
        defaults: &AccountDefaults,
    ) -> Result<Self, SqliteAccountError> {
        let mut candidate = Connection::open_in_memory()
            .map_err(|error| SqliteAccountError::sqlite("prepare installed defaults for", error))?;
        candidate
            .execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|error| {
                SqliteAccountError::sqlite("configure installed defaults for", error)
            })?;
        let transaction = candidate.transaction().map_err(|error| {
            SqliteAccountError::sqlite("start default initialization for", error)
        })?;
        for sql in [
            &defaults.schema,
            &defaults.rows,
            &defaults.settings_schema,
            &defaults.settings_rows,
        ] {
            transaction.execute_batch(sql).map_err(|error| {
                SqliteAccountError::sqlite("initialize installed defaults for", error)
            })?;
        }
        package::validate(&transaction).map_err(SqliteAccountError::Backup)?;
        let after = package::capture(&transaction).map_err(SqliteAccountError::Backup)?;

        let mut current = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| {
                SqliteAccountError::sqlite("read the account before resetting", error)
            })?;
        let current = current.transaction().map_err(|error| {
            SqliteAccountError::sqlite("inspect the account before resetting", error)
        })?;
        // Invalid account rows can be reset, but an unreviewed database format cannot.
        let version: i64 = current
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| {
                SqliteAccountError::sqlite("read the reset source version from", error)
            })?;
        if version != super::contract::SCHEMA_VERSION {
            return Err(SqliteAccountError::InvalidSchema(format!(
                "Account reset requires SQLite schema {}, found {version}",
                super::contract::SCHEMA_VERSION
            )));
        }
        reader::validate_schema(&current)?;
        let before = package::capture(&current).map_err(SqliteAccountError::Backup)?;
        Ok(Self {
            path: path.to_path_buf(),
            before,
            after,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn apply(self) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
        self.apply_with_backup(None)
    }

    fn apply_with_backup(
        self,
        backup: Option<PathBuf>,
    ) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
        if package::capture_path(&self.path).map_err(SqliteAccountError::Backup)? != self.before {
            return Err(SqliteAccountError::SourceChanged);
        }
        let safety_backup = if let Some(backup) = backup {
            writer::create_integrity_checked_snapshot(&self.path, &backup)?;
            backup
        } else {
            writer::indexed_backup(&self.path, "investment-reset", false, |backup| {
                writer::create_integrity_checked_snapshot(&self.path, backup)
            })?
        };
        if package::capture_path(&safety_backup).map_err(SqliteAccountError::Backup)? != self.before
        {
            return Err(SqliteAccountError::SourceChanged);
        }
        package::restore_snapshot(&self.path, &self.before, &self.after).map_err(|error| {
            SqliteAccountError::Backup(format!(
                "Account defaults were not restored: {error}. The recovery snapshot is at {}",
                safety_backup.display()
            ))
        })?;
        Ok(SqliteRestoreReceipt { safety_backup })
    }
}

#[cfg(test)]
mod tests;
