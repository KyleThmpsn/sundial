//! Only finished, disposable mod generations participate in automatic retention.
use super::*;

pub(in crate::install) fn completed_backup_owner(backup: &Path) -> Option<PathBuf> {
    let marker = backup.join("mod-recovery.json");
    let metadata = fs::symlink_metadata(&marker).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let record: Record = mod_content::read_json(&marker).ok()?;
    if record.state != InstallTransactionState::Committed
        || !paths_equal(&record.context.backup_directory, backup)
        || record.incoming.is_none()
        || record.retires_legacy()
        || record
            .members()
            .any(|m| m.original.is_some() && m.incoming.is_none())
        || record
            .context
            .account_cleanup
            .iter()
            .chain(record.context.client_settings.iter())
            .any(account::AccountCleanupRecord::changes_source)
    {
        return None;
    }
    let target = &record.context.target_packages_directory;
    // Validate the paths before deriving the journal and temporary member paths from them.
    target.parent()?;
    validate_record(&record, target, backup.parent()?).ok()?;
    if record.journal().try_exists().ok()?
        || target
            .join(INSTALL_TRANSACTION_FILE_NAME)
            .try_exists()
            .ok()?
    {
        return None;
    }
    // A committed record is written before cleanup. Leftover transaction folders keep its
    // recovery set protected even when the journal was moved or removed outside Parhelion.
    for member in record.members() {
        for kind in ["stage", "retired", "restore", "rollback"] {
            if member.temporary(&record, kind).try_exists().ok()? {
                return None;
            }
        }
    }
    Some(target.clone())
}
