//! Account bytes participate in the same durable journal as package replacement or removal.
use super::*;
use sundial::investment::AuthoredAccountCleanup;
const BACKUP_NAME: &str = "account-settings.json";

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(in crate::install) struct AccountCleanupRecord {
    relative_path: PathBuf,
    original: TransactionDigest,
    cleaned: TransactionDigest,
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
    sundial::investment::validate_authored_cleanup_backend(&path)
        .map_err(InstallError::validation)?;
    Ok(path)
}

pub(super) fn prepare(
    cleanup: &AuthoredAccountCleanup,
    packages: &Path,
    backup: &Path,
) -> Result<AccountCleanupRecord, InstallError> {
    let root = packages
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    reject_symlink(&cleanup.settings_path, "reviewed account")?;
    let path = fs::canonicalize(&cleanup.settings_path)
        .map_err(|e| InstallError::validation(e.to_string()))?;
    let relative_path = path
        .strip_prefix(root)
        .map_err(|_| InstallError::validation("Account settings are outside this installation"))?
        .to_path_buf();
    let record = AccountCleanupRecord {
        relative_path,
        original: digest(&cleanup.original_bytes),
        cleaned: digest(&cleanup.cleaned_bytes),
    };
    let path = target_path(&record, packages)?;
    reject_recovery_backup(&path, &record.original)?;
    let destination = backup.join(BACKUP_NAME);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|e| InstallError::validation(e.to_string()))?;
    file.write_all(&cleanup.original_bytes)
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
    let path = target_path(record, packages)?;
    reject_recovery_backup(&backup.join(BACKUP_NAME), &record.original)?;
    reject_recovery_backup(&path, &record.original)?;
    if digest(&cleanup.cleaned_bytes) != record.cleaned {
        return Err(InstallError::validation("Account proposal changed"));
    }
    if record.original != record.cleaned {
        sundial::package_authoring::replace_authoring_file(&path, &cleanup.cleaned_bytes)
            .map_err(|e| InstallError::validation(e.to_string()))?;
        reject_recovery_backup(&path, &record.cleaned)?;
    }
    Ok(())
}

pub(in crate::install) fn validate_account_record(
    transaction: &InstallTransactionRecord,
    packages: &Path,
) -> Result<(), InstallError> {
    if let Some(record) = &transaction.account_cleanup {
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
    if let Some(record) = &transaction.account_cleanup {
        let path = target_path(record, &transaction.target_packages_directory)?;
        reject_recovery_backup(
            &transaction.backup_directory.join(BACKUP_NAME),
            &record.original,
        )?;
        let current = digest_file(&path).map_err(|e| InstallError::validation(e.to_string()))?;
        if record.original != current && record.cleaned != current {
            return Err(InstallError::validation(
                "Account settings changed outside this package operation; recovery will not overwrite them",
            ));
        }
    }
    Ok(())
}

pub(super) fn verify_committed(transaction: &InstallTransactionRecord) -> Result<(), InstallError> {
    if let Some(record) = &transaction.account_cleanup {
        let path = target_path(record, &transaction.target_packages_directory)?;
        reject_recovery_backup(&path, &record.cleaned)?;
    }
    Ok(())
}

pub(in crate::install) fn recover_account(
    transaction: &InstallTransactionRecord,
    check: GameRunningCheck,
) -> Result<(), InstallError> {
    if let Some(record) = &transaction.account_cleanup {
        check_game_before_recovery(check)?;
        verify_account_recovery(transaction)?;
        let path = target_path(record, &transaction.target_packages_directory)?;
        let current = digest_file(&path).map_err(|e| InstallError::validation(e.to_string()))?;
        if record.original != current {
            restore_recovery_backup(
                &transaction.backup_directory.join(BACKUP_NAME),
                &path,
                &record.original,
            )?;
        }
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
