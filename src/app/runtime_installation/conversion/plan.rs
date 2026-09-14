use super::*;
use crate::persistence::sqlite_account::AccountDefaults;
use tempfile::TempDir;

pub(in crate::app::runtime_installation) struct Plan {
    pub document: WorkspaceDocument,
    pub target: RuntimeCopy,
    pub notes: Vec<String>,
    pub staged: TempDir,
    pub receipt: storage::Receipt,
    source_json: Vec<u8>,
    source_database: Option<Vec<u8>>,
    target_json: Option<Vec<u8>>,
    target_database: Option<Vec<u8>>,
}

pub(super) fn supported(document: &WorkspaceDocument, runtime: &RuntimeCopy) -> bool {
    matches!(
        (document.json()["version"].as_u64(), runtime.bundled_schema),
        (Some(6), Some(18))
    ) || (document.json()["version"] == 18 && runtime.dawn && runtime.bundled_schema == Some(6))
}

impl Plan {
    pub(super) fn prepare(
        install: &Path,
        settings_path: &Path,
        document: &WorkspaceDocument,
        persisted: &WorkspaceDocument,
        target: RuntimeCopy,
        defaults: Value,
        native_defaults: Option<&AccountDefaults>,
    ) -> Result<Self, String> {
        if !supported(document, &target) {
            return Err("Conversion supports Dawn v6 and Sunrise v18.".into());
        }
        document.verify_account_source_unchanged()?;
        if let Some(reason) = document.account_editing_blocked() {
            return Err(reason.into());
        }
        settings::validate_workspace_document(document)?;
        settings::verify_workspace_source_unchanged(
            settings_path,
            persisted.json(),
            persisted.uses_json_account(),
        )?;
        if defaults["version"].as_u64() != target.bundled_schema {
            return Err("The runtime defaults changed. Recheck Runtime Copies.".into());
        }
        let source_path = settings_path
            .strip_prefix(install)
            .map_err(|_| "The settings file is outside this installation.")?
            .to_owned();
        let target_path = target
            .settings_path
            .strip_prefix(install)
            .map_err(|_| "The target settings are outside this installation.")?
            .to_owned();
        let install = fs::canonicalize(install).map_err(storage::err)?;
        storage::check_settings_path(&install, &source_path)?;
        storage::check_settings_path(&install, &target_path)?;
        let staged = tempfile::tempdir().map_err(storage::err)?;
        let source_json = fs::read(settings_path).map_err(storage::err)?;
        storage::write(staged.path().join("source-settings.json"), &source_json)?;
        storage::write(
            staged.path().join("source-draft.json"),
            &serde_json::to_vec_pretty(document.json()).map_err(storage::err)?,
        )?;
        let target_json = storage::read_optional(&target.settings_path)?;
        if let Some(bytes) = &target_json {
            storage::write(staged.path().join("settings.before.json"), bytes)?;
        }
        let mut notes = vec![
            "Activity settings use the target runtime's defaults.".into(),
            "Settings and fields absent from the target format stay in the backup.".into(),
        ];
        let source_database = if let Some(native_document) = document.native_account() {
            storage::check_path(
                &install,
                &crate::persistence::investment_path(&install.join(&source_path)),
            )?;
            native_document
                .write_conversion_copy(&staged.path().join("source-draft.sqlite3"))
                .map_err(storage::err)?;
            Some(native::snapshot::read(
                &staged.path().join("source-draft.before.sqlite3"),
            )?)
        } else {
            None
        };
        let to_sqlite = target.bundled_schema == Some(18);
        let mut converted = if to_sqlite {
            native::conversion::from_json(
                document.json(),
                native_defaults.ok_or("The runtime has no SQLite defaults.")?,
                &staged.path().join("account.after.sqlite3"),
            )?;
            notes.push("Each character will receive the shared v6 progress.".into());
            defaults
        } else {
            native::conversion::to_json(
                document
                    .native_account()
                    .ok_or("The SQLite account is unavailable.")?,
                &defaults,
                &mut notes,
            )?
        };
        copy_settings(&mut converted, document.json());
        if to_sqlite {
            crate::game_settings::validate_non_account(&converted)?;
        } else {
            settings::validate_document(&converted)?;
            target
                .dawn_runtime
                .as_ref()
                .ok_or("Dawn is no longer detected.")?
                .validate(&converted)?;
        }
        storage::write(
            staged.path().join("settings.after.json"),
            settings::prepare_settings(&converted)?.encoded.as_bytes(),
        )?;
        let target_db = crate::persistence::investment_path(&install.join(&target_path));
        let target_database = if to_sqlite && target_db.try_exists().map_err(storage::err)? {
            crate::package_runtime::installation::checked_path(&install, &target_db)?;
            native::conversion::snapshot(
                &target_db,
                &staged.path().join("account.before.sqlite3"),
            )?;
            let snapshot = native::snapshot::read(&staged.path().join("account.before.sqlite3"))?;
            notes.push(
                "The existing SQLite account will be replaced. It will be backed up first.".into(),
            );
            Some(snapshot)
        } else {
            None
        };
        let receipt = storage::Receipt {
            version: 1,
            install,
            source_path,
            target_path,
            to_sqlite,
            had_json: target_json.is_some(),
            had_database: target_database.is_some(),
            files: storage::hashes(staged.path())?,
        };
        Ok(Self {
            document: document.clone(),
            target,
            notes,
            staged,
            receipt,
            source_json,
            source_database,
            target_json,
            target_database,
        })
    }

    pub(super) fn apply(
        &self,
        mut check: impl FnMut() -> Result<(), String>,
    ) -> Result<PathBuf, String> {
        check()?;
        self.verify()?;
        let root = &self.receipt.install;
        let destination = root.join(&self.receipt.target_path);
        crate::package_runtime::installation::checked_directory(
            root,
            destination.parent().ok_or("Missing settings folder.")?,
        )?;
        let _source_lock = settings::lock_settings(&root.join(&self.receipt.source_path))?;
        let _target_lock = if self.receipt.source_path != self.receipt.target_path {
            Some(settings::lock_settings(&destination)?)
        } else {
            None
        };
        self.verify()?;
        let backup = storage::save_backup(&self.receipt, self.staged.path())?;
        let result = check()
            .and_then(|()| self.verify())
            .and_then(|()| storage::install(&self.receipt, &backup, &mut check));
        if let Err(error) = result {
            return Err(format!("{error} Backup: {}", backup.display()));
        }
        Ok(backup)
    }

    fn verify(&self) -> Result<(), String> {
        let root = &self.receipt.install;
        storage::check_settings_path(root, &self.receipt.source_path)?;
        storage::check_settings_path(root, &self.receipt.target_path)?;
        let inspection = RuntimeInspection::inspect(root);
        let current = inspection.launch_copy().ok_or("The runtime is missing.")?;
        if current.location != self.target.location
            || current.dll_hash != self.target.dll_hash
            || current.dll_hash.is_none()
        {
            return Err("The runtime changed. Review the conversion again.".into());
        }
        if self.target.dawn {
            let dawn = crate::game_settings::dawn::Runtime::inspect(&current.dll_path);
            let after: Value = serde_json::from_slice(
                &fs::read(self.staged.path().join("settings.after.json")).map_err(storage::err)?,
            )
            .map_err(storage::err)?;
            dawn.validate(&after)?;
        }
        if fs::read(root.join(&self.receipt.source_path)).map_err(storage::err)? != self.source_json
            || storage::read_optional(&root.join(&self.receipt.target_path))? != self.target_json
        {
            return Err("Settings changed after review. Review the conversion again.".into());
        }
        if let Some(expected) = &self.source_database {
            if native::snapshot::read(&crate::persistence::investment_path(
                &root.join(&self.receipt.source_path),
            ))? != *expected
            {
                return Err("The source account changed after review.".into());
            }
        }
        if self.receipt.to_sqlite {
            let path = crate::persistence::investment_path(&root.join(&self.receipt.target_path));
            let current = if path.try_exists().map_err(storage::err)? {
                Some(native::snapshot::read(&path)?)
            } else {
                None
            };
            if current != self.target_database {
                return Err("The target account changed after review.".into());
            }
        }
        Ok(())
    }
}

fn copy_settings(target: &mut Value, source: &Value) {
    for key in ["core", "steam", "client", "server", "experiments"] {
        if let (Some(target), Some(source)) = (target.get_mut(key), source.get(key)) {
            copy_common(target, source, key == "server");
        }
    }
    if target["version"] == 18 {
        if let Some(value) = source.pointer("/state/investment/complete_exotic_catalysts") {
            target["complete_exotic_catalysts"] = value.clone();
        }
    }
}

fn copy_common(target: &mut Value, source: &Value, server: bool) {
    if let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) {
        for (key, value) in target {
            if server && key == "entitlements" {
                continue;
            }
            if let Some(source) = source.get(key) {
                if value.is_object() {
                    copy_common(value, source, false);
                } else if !value.is_array()
                    && (value.is_boolean() == source.is_boolean())
                    && (value.is_number() == source.is_number())
                    && (value.is_string() == source.is_string())
                {
                    *value = source.clone();
                }
            }
        }
    }
}
