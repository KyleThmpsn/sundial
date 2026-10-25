//! Explicit account reset from the installed Sunrise resources, with a recoverable snapshot.
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::{SqliteAccountError, SqliteRestoreReceipt, reader, snapshot, writer};

pub(crate) struct AccountDefaults {
    pub(crate) schema: String,
    pub(crate) rows: String,
    pub(crate) settings_schema: String,
    pub(crate) settings_rows: String,
    pub(crate) starter: Option<String>,
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
        if let Some(starter) = &defaults.starter {
            transaction.execute_batch(starter).map_err(|error| {
                SqliteAccountError::sqlite("initialize character starters for", error)
            })?;
        }
        super::validation::connection(&transaction).map_err(SqliteAccountError::Backup)?;
        let after = snapshot::capture(&transaction).map_err(SqliteAccountError::Backup)?;

        let mut current = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| {
                SqliteAccountError::sqlite("read the account before resetting", error)
            })?;
        let current = current.transaction().map_err(|error| {
            SqliteAccountError::sqlite("inspect the account before resetting", error)
        })?;
        // Reset can replace invalid rows within a supported schema. It cannot migrate formats.
        let version: i64 = current
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| {
                SqliteAccountError::sqlite("read the reset source version from", error)
            })?;
        let target_version: i64 = transaction
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| {
                SqliteAccountError::sqlite("read installed defaults version from", error)
            })?;
        if !matches!(version, 2 | 3) || version != target_version {
            return Err(SqliteAccountError::InvalidSchema(format!(
                "Account reset requires matching schemas. The account is schema {version} and installed defaults are schema {target_version}"
            )));
        }
        reader::validate_schema(&current)?;
        let before = snapshot::capture(&current).map_err(SqliteAccountError::Backup)?;
        Ok(Self {
            path: path.to_path_buf(),
            before,
            after,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn apply(self) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
        self.apply_checked(|| Ok(()))
    }

    /// Resets the account, refused whenever `require_closed` reports the game running: before the
    /// recovery snapshot, and again just before the reset commits.
    pub(crate) fn apply_checked(
        self,
        require_closed: impl FnMut() -> Result<(), String>,
    ) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
        self.apply_with(None, require_closed)
    }

    #[cfg(test)]
    fn apply_with_backup(
        self,
        backup: Option<PathBuf>,
    ) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
        self.apply_with(backup, || Ok(()))
    }

    fn apply_with(
        self,
        backup: Option<PathBuf>,
        mut require_closed: impl FnMut() -> Result<(), String>,
    ) -> Result<SqliteRestoreReceipt, SqliteAccountError> {
        require_closed().map_err(SqliteAccountError::WriteBlocked)?;
        if snapshot::capture_path(&self.path).map_err(SqliteAccountError::Backup)? != self.before {
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
        if snapshot::capture_path(&safety_backup).map_err(SqliteAccountError::Backup)?
            != self.before
        {
            return Err(SqliteAccountError::SourceChanged);
        }
        snapshot::restore_snapshot(&self.path, &self.before, &self.after, require_closed).map_err(
            |error| {
                SqliteAccountError::Backup(format!(
                    "Account defaults were not restored: {error}. The recovery snapshot is at {}",
                    safety_backup.display()
                ))
            },
        )?;
        Ok(SqliteRestoreReceipt { safety_backup })
    }
}

#[cfg(test)]
mod tests;
