//! Account bytes participate in the same durable journal as package replacement or removal.
use super::*;
use sundial::investment::{AuthoredAccountCleanup, AuthoredClientSettings};
const BACKUP_NAME: &str = "account-settings.json";
const CLIENT_SETTINGS_BACKUP: &str = "client-settings.json";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Source {
    #[default]
    Account,
    ClientSettings,
}

#[cfg(test)]
mod native_tests;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(in crate::install) struct AccountCleanupRecord {
    #[serde(default)]
    source: Source,
    relative_path: PathBuf,
    original: TransactionDigest,
    cleaned: TransactionDigest,
}

impl AccountCleanupRecord {
    pub(super) fn changes_source(&self) -> bool {
        self.original != self.cleaned
    }
    fn backup_name(&self) -> &'static str {
        match self.source {
            Source::Account => BACKUP_NAME,
            Source::ClientSettings => CLIENT_SETTINGS_BACKUP,
        }
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>, String> {
        match self.source {
            Source::Account => sundial::investment::read_authored_account_source(path),
            Source::ClientSettings => fs::read(path).map_err(|error| error.to_string()),
        }
    }

    fn replace(&self, path: &Path, expected: &[u8], updated: &[u8]) -> Result<(), String> {
        match self.source {
            Source::Account => {
                sundial::investment::replace_authored_account_source(path, expected, updated)
            }
            Source::ClientSettings => {
                if self.read(path)? != expected {
                    return Err("Sunrise settings changed after installation review".into());
                }
                sundial::package_authoring::replace_authoring_file(path, updated)
                    .map_err(|error| error.to_string())
            }
        }
    }
}

fn digest(bytes: &[u8]) -> TransactionDigest {
    TransactionDigest {
        byte_length: bytes.len() as u64,
        sha256: format!("{:X}", Sha256::digest(bytes)),
    }
}

fn target_path(record: &AccountCleanupRecord, packages: &Path) -> Result<PathBuf, InstallError> {
    if record.relative_path != Path::new("settings.json")
        && record.relative_path != Path::new("Sunrise/settings.json")
        && record.relative_path != Path::new("bin/x64/Sunrise/settings.json")
        && record.relative_path != Path::new("data/investment.sqlite3")
        && record.relative_path != Path::new("Sunrise/data/investment.sqlite3")
        && record.relative_path != Path::new("bin/x64/Sunrise/data/investment.sqlite3")
    {
        return Err(InstallError::validation(
            "Uninstall account record has an unsupported settings path",
        ));
    }
    let root = packages
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    let path = root.join(&record.relative_path);
    reject_symlink(&path, "uninstall account")?;
    let resolved = fs::canonicalize(&path).map_err(|e| InstallError::validation(e.to_string()))?;
    if !paths_equal(&resolved, &path) || !path_is_within(&resolved, root) {
        return Err(InstallError::validation(
            "The account settings path was redirected",
        ));
    }
    if record.source == Source::Account {
        sundial::investment::validate_authored_cleanup_backend(&path)
            .map_err(InstallError::validation)?;
    } else if path.file_name().is_none_or(|name| name != "settings.json") {
        return Err(InstallError::validation(
            "Client settings require a settings.json file",
        ));
    }
    Ok(path)
}

pub(super) fn prepare(
    cleanup: &AuthoredAccountCleanup,
    packages: &Path,
    backup: &Path,
) -> Result<AccountCleanupRecord, InstallError> {
    prepare_change(
        &cleanup.settings_path,
        &cleanup.original_bytes,
        &cleanup.cleaned_bytes,
        Source::Account,
        packages,
        backup,
    )
}

pub(super) fn prepare_settings(
    settings: &AuthoredClientSettings,
    packages: &Path,
    backup: &Path,
) -> Result<AccountCleanupRecord, InstallError> {
    prepare_change(
        &settings.settings_path,
        &settings.original_bytes,
        &settings.updated_bytes,
        Source::ClientSettings,
        packages,
        backup,
    )
}

fn prepare_change(
    settings_path: &Path,
    original: &[u8],
    updated: &[u8],
    source: Source,
    packages: &Path,
    backup: &Path,
) -> Result<AccountCleanupRecord, InstallError> {
    let root = packages
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    reject_symlink(settings_path, "reviewed account or client settings")?;
    let path =
        fs::canonicalize(settings_path).map_err(|e| InstallError::validation(e.to_string()))?;
    let relative_path = path
        .strip_prefix(root)
        .map_err(|_| InstallError::validation("Account settings are outside this installation"))?
        .to_path_buf();
    let record = AccountCleanupRecord {
        source,
        relative_path,
        original: digest(original),
        cleaned: digest(updated),
    };
    let path = target_path(&record, packages)?;
    verify_source(&record, &path, &record.original)?;
    let destination = backup.join(record.backup_name());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|e| InstallError::validation(e.to_string()))?;
    file.write_all(original)
        .and_then(|_| file.sync_all())
        .map_err(|e| InstallError::validation(e.to_string()))?;
    reject_recovery_backup(&destination, &record.original)?;
    Ok(record)
}

pub(super) fn commit(
    cleanup: &AuthoredAccountCleanup,
    record: &AccountCleanupRecord,
    packages: &Path,
    backup: &Path,
) -> Result<(), InstallError> {
    commit_change(
        &cleanup.original_bytes,
        &cleanup.cleaned_bytes,
        record,
        packages,
        backup,
    )
}

pub(super) fn commit_settings(
    settings: &AuthoredClientSettings,
    record: &AccountCleanupRecord,
    packages: &Path,
    backup: &Path,
) -> Result<(), InstallError> {
    commit_change(
        &settings.original_bytes,
        &settings.updated_bytes,
        record,
        packages,
        backup,
    )
}

fn commit_change(
    original: &[u8],
    updated: &[u8],
    record: &AccountCleanupRecord,
    packages: &Path,
    backup: &Path,
) -> Result<(), InstallError> {
    let path = target_path(record, packages)?;
    reject_recovery_backup(&backup.join(record.backup_name()), &record.original)?;
    verify_source(record, &path, &record.original)?;
    if digest(updated) != record.cleaned || digest(original) != record.original {
        return Err(InstallError::validation("Account proposal changed"));
    }
    if record.original != record.cleaned {
        record
            .replace(&path, original, updated)
            .map_err(|e| InstallError::validation(e.to_string()))?;
        verify_source(record, &path, &record.cleaned)?;
    }
    Ok(())
}

pub(in crate::install) fn validate_account_record(
    transaction: &InstallTransactionRecord,
    packages: &Path,
) -> Result<(), InstallError> {
    if transaction
        .account_cleanup
        .as_ref()
        .is_some_and(|record| record.source != Source::Account)
        || transaction
            .client_settings
            .as_ref()
            .is_some_and(|record| record.source != Source::ClientSettings)
    {
        return Err(InstallError::validation(
            "The settings journal has an invalid source type",
        ));
    }
    if let (Some(account), Some(settings)) =
        (&transaction.account_cleanup, &transaction.client_settings)
        && paths_equal(
            &target_path(account, packages)?,
            &target_path(settings, packages)?,
        )
    {
        return Err(InstallError::validation(
            "Settings changes must share one journal entry per file",
        ));
    }
    for record in transaction
        .account_cleanup
        .iter()
        .chain(transaction.client_settings.iter())
    {
        if transaction.artifacts.is_empty() {
            return Err(InstallError::validation(
                "Account cleanup requires a package transaction",
            ));
        }
        target_path(record, packages)?;
        validate_transaction_digest(&record.original, "original account")?;
        validate_transaction_digest(&record.cleaned, "cleaned account")?;
    }
    Ok(())
}

pub(in crate::install) fn verify_account_recovery(
    transaction: &InstallTransactionRecord,
) -> Result<(), InstallError> {
    for record in transaction
        .account_cleanup
        .iter()
        .chain(transaction.client_settings.iter())
    {
        let path = target_path(record, &transaction.target_packages_directory)?;
        reject_recovery_backup(
            &transaction.backup_directory.join(record.backup_name()),
            &record.original,
        )?;
        let current = digest(&record.read(&path).map_err(InstallError::validation)?);
        if record.original != current && record.cleaned != current {
            return Err(InstallError::validation(
                "Account settings changed outside this package operation. Recovery will not overwrite them",
            ));
        }
    }
    Ok(())
}

pub(super) fn verify_committed(transaction: &InstallTransactionRecord) -> Result<(), InstallError> {
    for record in transaction
        .account_cleanup
        .iter()
        .chain(transaction.client_settings.iter())
    {
        let path = target_path(record, &transaction.target_packages_directory)?;
        verify_source(record, &path, &record.cleaned)?;
    }
    Ok(())
}

pub(in crate::install) fn recover_account(
    transaction: &InstallTransactionRecord,
    check: GameRunningCheck,
) -> Result<(), InstallError> {
    // Validate both sources before restoring either one.
    verify_account_recovery(transaction)?;
    for record in transaction
        .account_cleanup
        .iter()
        .chain(transaction.client_settings.iter())
    {
        check_game_before_recovery(check)?;
        verify_account_recovery(transaction)?;
        let path = target_path(record, &transaction.target_packages_directory)?;
        let current = digest(&record.read(&path).map_err(InstallError::validation)?);
        if record.original != current {
            let expected = record.read(&path).map_err(InstallError::validation)?;
            if digest(&expected) != record.cleaned {
                return Err(InstallError::validation(
                    "The account changed before recovery",
                ));
            }
            let original = fs::read(transaction.backup_directory.join(record.backup_name()))
                .map_err(|e| InstallError::validation(e.to_string()))?;
            record
                .replace(&path, &expected, &original)
                .map_err(InstallError::validation)?;
            verify_source(record, &path, &record.original)?;
        }
    }
    Ok(())
}

fn verify_source(
    record: &AccountCleanupRecord,
    path: &Path,
    expected: &TransactionDigest,
) -> Result<(), InstallError> {
    let bytes = record.read(path).map_err(InstallError::validation)?;
    if digest(&bytes) != *expected {
        return Err(InstallError::validation("The account changed after review"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_commit_is_backed_up_and_rejects_stale_or_redirected_targets() {
        let directory = tempfile::tempdir().unwrap();
        let packages = directory.path().join("packages");
        let backup = directory.path().join("backup");
        fs::create_dir(&packages).unwrap();
        fs::create_dir(&backup).unwrap();
        let packages = fs::canonicalize(packages).unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, b"original").unwrap();
        let proposal = AuthoredAccountCleanup {
            settings_path: path.clone(),
            original_bytes: b"original".to_vec(),
            cleaned_bytes: b"cleaned".to_vec(),
            removed_items: BTreeMap::new(),
            resized_items: BTreeMap::new(),
            slot_moves: vec![],
            cleared_plugs: 0,
            cleared_unlocks: 0,
            removed_reward_rules: 0,
        };
        let record = prepare(&proposal, &packages, &backup).unwrap();
        assert_eq!(fs::read(backup.join(BACKUP_NAME)).unwrap(), b"original");
        fs::write(&path, b"concurrent edit").unwrap();
        assert!(commit(&proposal, &record, &packages, &backup).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"concurrent edit");
        fs::write(&path, b"original").unwrap();
        commit(&proposal, &record, &packages, &backup).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"cleaned");
        let mut invalid = record;
        invalid.relative_path = PathBuf::from("../unrelated.json");
        assert!(target_path(&invalid, &packages).is_err());
    }
}
