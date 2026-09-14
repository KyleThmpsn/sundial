//! Restoring after a gameplay test is explicit and keeps the current account as well.
use super::*;

pub(in crate::app::runtime_installation) struct Plan {
    pub receipt: storage::Receipt,
    pub folder: PathBuf,
    json: Option<Vec<u8>>,
    database: Option<Vec<u8>>,
}

impl Plan {
    pub(super) fn prepare(install: &Path, folder: &Path) -> Result<Self, String> {
        let receipt = storage::load_backup(install, folder)?;
        let target = receipt.install.join(&receipt.target_path);
        let json = storage::read_optional(&target)?;
        let database = if receipt.to_sqlite {
            read_database(&receipt)?
        } else {
            None
        };
        Ok(Self {
            receipt,
            folder: folder.to_owned(),
            json,
            database,
        })
    }

    pub(super) fn apply(
        &self,
        mut check: impl FnMut() -> Result<(), String>,
    ) -> Result<Option<PathBuf>, String> {
        check()?;
        if storage::load_backup(&self.receipt.install, &self.folder)? != self.receipt {
            return Err("The conversion backup changed after review.".into());
        }
        let target = self.receipt.install.join(&self.receipt.target_path);
        let _lock = settings::lock_settings(&target)?;
        self.verify()?;
        let before = if self.receipt.had_json {
            Some(fs::read(self.folder.join("settings.before.json")).map_err(storage::err)?)
        } else {
            None
        };
        let database_before = if self.receipt.had_database {
            Some(native::snapshot::read(
                &self.folder.join("account.before.sqlite3"),
            )?)
        } else {
            None
        };
        if self.json == before && self.database == database_before {
            return Ok(None);
        }
        let safety = self.back_up_current()?;
        let result = (|| {
            check()?;
            self.verify()?;
            if self.json != before {
                match (&self.json, before) {
                    (Some(expected), Some(before)) => {
                        crate::storage::replace_file_if_unchanged(&target, &before, expected)
                            .map_err(storage::err)?
                    }
                    (Some(_), None) => fs::remove_file(&target).map_err(storage::err)?,
                    (None, Some(before)) => storage::create_json(&target, &before)?,
                    (None, None) => {}
                }
            }
            if self.receipt.to_sqlite && self.database != database_before {
                check()?;
                let database = crate::persistence::investment_path(&target);
                storage::check_path(&self.receipt.install, &database)?;
                match (&self.database, database_before) {
                    (Some(expected), Some(_)) => native::conversion::restore(
                        &database,
                        expected,
                        &self.folder.join("account.before.sqlite3"),
                    )?,
                    (Some(expected), None) => storage::remove_database(&database, expected)?,
                    (None, Some(_)) => storage::create_database(
                        &self.folder.join("account.before.sqlite3"),
                        &database,
                    )?,
                    (None, None) => {}
                }
            }
            Ok(())
        })();
        result.map_err(|error: String| format!("{error} Current files were backed up to {}. The original conversion backup is unchanged.", safety.display()))?;
        Ok(Some(safety))
    }

    fn verify(&self) -> Result<(), String> {
        storage::check_settings_path(&self.receipt.install, &self.receipt.target_path)?;
        let target = self.receipt.install.join(&self.receipt.target_path);
        if storage::read_optional(&target)? != self.json
            || (self.receipt.to_sqlite && read_database(&self.receipt)? != self.database)
        {
            return Err("The account changed after review. Review the restore again.".into());
        }
        Ok(())
    }

    fn back_up_current(&self) -> Result<PathBuf, String> {
        let directory = self.receipt.install.join(".sunrise/backups");
        crate::package_runtime::installation::checked_directory(&self.receipt.install, &directory)?;
        let folder = tempfile::Builder::new()
            .prefix("before-account-restore-")
            .tempdir_in(directory)
            .map_err(storage::err)?
            .keep();
        if let Some(json) = &self.json {
            storage::write(folder.join("settings.json"), json)?;
        }
        if let Some(expected) = &self.database {
            let database = crate::persistence::investment_path(
                &self.receipt.install.join(&self.receipt.target_path),
            );
            native::conversion::snapshot(&database, &folder.join("investment.sqlite3"))?;
            if native::snapshot::read(&folder.join("investment.sqlite3"))? != *expected {
                return Err("The account changed while making the restore backup.".into());
            }
        }
        storage::write(folder.join("restore.json"), &serde_json::to_vec_pretty(&serde_json::json!({
            "conversion_backup": self.folder, "settings_path": self.receipt.target_path,
            "had_json": self.json.is_some(), "had_database": self.database.is_some(), "files": storage::hashes(&folder)?
        })).map_err(storage::err)?)?;
        Ok(folder)
    }
}

fn read_database(receipt: &storage::Receipt) -> Result<Option<Vec<u8>>, String> {
    let path = crate::persistence::investment_path(&receipt.install.join(&receipt.target_path));
    storage::check_path(&receipt.install, &path)?;
    if path.try_exists().map_err(storage::err)? {
        native::snapshot::read(&path).map(Some)
    } else {
        Ok(None)
    }
}
