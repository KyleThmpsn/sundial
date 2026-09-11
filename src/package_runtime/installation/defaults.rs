//! Restore the exact selected DLL's defaults without normalizing its schema.
use super::*;

#[derive(Clone)]
pub(crate) struct SettingsResetPlan {
    install: PathBuf,
    copy: RuntimeCopy,
    before: Vec<u8>,
    defaults: Vec<u8>,
    pub schema: u64,
}

impl SettingsResetPlan {
    pub(crate) fn prepare(install: &Path, copy: &RuntimeCopy) -> Result<Self, String> {
        let current = RuntimeInspection::inspect(install);
        if !current.copies.contains(copy) {
            return Err("Runtime files changed. Refresh the comparison and try again.".into());
        }
        let bytes = fs::read(&copy.dll_path).map_err(|e| e.to_string())?;
        let defaults =
            embedded_defaults(&bytes).ok_or("This DLL has no readable bundled settings")?;
        let schema = crate::game_settings::schema_version(&defaults)
            .ok_or("The bundled settings have no schema version")?;
        if let Some(dawn) = &copy.dawn_runtime {
            dawn.validate(&defaults)?;
        }
        Ok(Self {
            install: install.to_owned(),
            copy: copy.clone(),
            schema,
            before: fs::read(&copy.settings_path)
                .map_err(|e| format!("Cannot back up settings: {e}"))?,
            defaults: serde_json::to_vec_pretty(&defaults).map_err(|e| e.to_string())?,
        })
    }

    pub(crate) fn copy(&self) -> &RuntimeCopy {
        &self.copy
    }

    pub(crate) fn apply(
        &self,
        mut require_closed: impl FnMut() -> Result<(), String>,
    ) -> Result<PathBuf, String> {
        require_closed()?;
        self.check_current()?;
        let root = fs::canonicalize(&self.install).map_err(|e| e.to_string())?;
        let settings = self
            .copy
            .location
            .directory(&root)
            .join("Sunrise/settings.json");
        backup::checked_path(&root, &settings)?;
        let directory = root.join(".sunrise/backups");
        backup::checked_directory(&root, &directory)?;
        let directory = directory.join(format!(
            "settings-defaults-{}",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        fs::create_dir(&directory).map_err(|e| e.to_string())?;
        let backup = directory.join("settings.json");
        crate::storage::replace_file(&backup, &self.before).map_err(|e| e.to_string())?;
        if fs::read(&backup).map_err(|e| e.to_string())? != self.before {
            return Err("Settings backup verification failed".into());
        }
        require_closed()?;
        self.check_current()?;
        crate::storage::replace_file_if_unchanged(
            &self.copy.settings_path,
            &self.defaults,
            &self.before,
        )
        .map_err(|e| e.to_string())?;
        Ok(backup)
    }

    fn check_current(&self) -> Result<(), String> {
        if !RuntimeInspection::inspect(&self.install)
            .copies
            .contains(&self.copy)
        {
            return Err("Runtime files changed. Open the restore confirmation again.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires SUNDIAL_DEFAULTS_DLL pointing to a Sunrise DLL, read only"]
    fn runtime_defaults_from_exact_native_dll() {
        let source = std::env::var_os("SUNDIAL_DEFAULTS_DLL").expect("DLL path");
        let bytes = fs::read(source).unwrap();
        let expected = embedded_defaults(&bytes).unwrap();
        let dir = tempfile::tempdir().unwrap();
        super::super::tests::fixture(dir.path());
        fs::write(dir.path().join("bin/x64/steam_api64.dll"), bytes).unwrap();
        let copy = RuntimeInspection::inspect(dir.path()).copies[1].clone();
        let plan = SettingsResetPlan::prepare(dir.path(), &copy).unwrap();
        let backup = plan.apply(|| Ok(())).unwrap();
        assert_eq!(fs::read(backup).unwrap(), plan.before);
        let actual: Value = serde_json::from_slice(&fs::read(copy.settings_path).unwrap()).unwrap();
        assert_eq!(actual, expected);
    }

    fn fixture(schema: u64) -> (tempfile::TempDir, SettingsResetPlan) {
        let dir = tempfile::tempdir().unwrap();
        super::super::tests::fixture(dir.path());
        let copy = RuntimeInspection::inspect(dir.path()).copies[1].clone();
        let plan = SettingsResetPlan {
            install: dir.path().to_owned(),
            before: fs::read(&copy.settings_path).unwrap(),
            copy,
            defaults: format!("{{\"version\":{schema},\"native\":true}}").into_bytes(),
            schema,
        };
        (dir, plan)
    }

    #[test]
    fn runtime_defaults_reset_preserves_other_copy_database_and_exact_backup() {
        for schema in [6, 8, 18] {
            let (dir, plan) = fixture(schema);
            let backup = plan.apply(|| Ok(())).unwrap();
            assert_eq!(fs::read(backup).unwrap(), plan.before);
            assert_eq!(fs::read(&plan.copy.settings_path).unwrap(), plan.defaults);
            assert_eq!(
                fs::read(dir.path().join("Sunrise/settings.json")).unwrap(),
                plan.before
            );
            assert_eq!(
                fs::read(dir.path().join("bin/x64/Sunrise/investment.sqlite3")).unwrap(),
                b"saved account"
            );
        }
    }

    #[test]
    fn runtime_defaults_reset_rejects_game_running_and_changed_settings_or_dll() {
        let (_dir, plan) = fixture(6);
        assert!(plan.apply(|| Err("Game running".into())).is_err());
        assert_eq!(fs::read(&plan.copy.settings_path).unwrap(), plan.before);
        fs::write(&plan.copy.settings_path, b"concurrent edit").unwrap();
        assert!(plan.apply(|| Ok(())).is_err());
        assert_eq!(
            fs::read(&plan.copy.settings_path).unwrap(),
            b"concurrent edit"
        );
        let (_dir, plan) = fixture(18);
        let mut calls = 0;
        assert!(
            plan.apply(|| {
                calls += 1;
                if calls == 2 {
                    fs::write(&plan.copy.dll_path, b"new DLL").unwrap();
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(&plan.copy.settings_path).unwrap(), plan.before);
    }
}
