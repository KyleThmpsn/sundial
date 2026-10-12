//! Sunrise 0.6 managed-directory publication and legacy-layout migration.
use super::*;
use sundial::package_authoring::mod_content;
mod legacy;
mod transaction;
mod validation;
pub(super) use legacy::recover_with_observer as recover_legacy_with_observer;
pub(super) use transaction::recover;
pub(crate) use validation::package_window;
use validation::*;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Folder {
    files: BTreeMap<String, TransactionDigest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeReview {
    target: PathBuf,
    runtime: RuntimeSnapshot,
    artifacts: Vec<ArtifactMetadata>,
    stock: Vec<ArtifactMetadata>,
}
impl UpgradeReview {
    pub fn target(&self) -> &Path {
        &self.target
    }
    pub fn artifacts(&self) -> &[ArtifactMetadata] {
        &self.artifacts
    }
    pub fn mod_directory(&self) -> PathBuf {
        mod_content::directory(self.target.parent().expect("reviewed installation"))
    }
    fn sources(&self) -> Vec<(ArtifactMetadata, PathBuf)> {
        self.artifacts
            .iter()
            .map(|a| (a.clone(), self.target.join(&a.file_name)))
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct UpgradeReport {
    pub mod_directory: PathBuf,
    pub backup_directory: PathBuf,
    pub package_count: usize,
}

fn failure(error: impl fmt::Display) -> InstallError {
    InstallError::validation(error.to_string())
}
fn check_stopped(check: GameRunningCheck) -> Result<(), InstallError> {
    if check().map_err(InstallError::validation)? {
        return Err(InstallError::validation(
            "Close Destiny 2 before changing installed content",
        ));
    }
    Ok(())
}

pub(super) fn selected(
    target: &Path,
    snapshot: RuntimeSnapshotCheck,
) -> Result<bool, InstallError> {
    let target = canonical_packages_directory(target)?;
    let runtime = snapshot(
        target
            .parent()
            .ok_or_else(|| InstallError::validation("Missing game root"))?,
    )
    .map_err(InstallError::validation)?;
    let loader = runtime
        .uses_mod_loader()
        .map_err(InstallError::validation)?;
    if !loader
        && mod_content::directory(
            target
                .parent()
                .ok_or_else(|| InstallError::validation("Missing game root"))?,
        )
        .exists()
    {
        return Err(InstallError::validation(
            "Installed Parhelion content requires Sunrise 0.6 or later",
        ));
    }
    Ok(loader)
}

/// The upgrade review, refused when Sunrise would not load the moved content beside the other
/// mods installed.
pub fn preview_upgrade(packages: &Path) -> Result<Option<UpgradeReview>, InstallError> {
    let review = review_upgrade(packages)?;
    if let Some(review) = &review {
        check_other_mods(&review.target, &review.sources())?;
    }
    Ok(review)
}

/// The legacy content an upgrade would move, without checking other mods. Removing legacy
/// content moves it first, and other mods do not matter to content about to be removed.
fn review_upgrade(packages: &Path) -> Result<Option<UpgradeReview>, InstallError> {
    let target = canonical_packages_directory(packages)?;
    let runtime = sundial::package_authoring::installed_runtime(
        target
            .parent()
            .ok_or_else(|| InstallError::validation("Missing game root"))?,
    )
    .map_err(InstallError::validation)?;
    if !runtime
        .uses_mod_loader()
        .map_err(InstallError::validation)?
    {
        if mod_content::directory(target.parent().unwrap()).exists() {
            return Err(InstallError::validation(
                "The installed Parhelion mod requires Sunrise 0.6 or later. Restore a compatible runtime before editing its content",
            ));
        }
        return Ok(None);
    }
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    if target
        .join(INSTALL_TRANSACTION_FILE_NAME)
        .try_exists()
        .map_err(failure)?
    {
        return Err(InstallError::validation(
            "An earlier package operation needs recovery. Recover it to reconcile its packages into the Sunrise mod folder",
        ));
    }
    mod_content::ensure_readable(install).map_err(InstallError::validation)?;
    mod_content::ensure_resolved_environment(install).map_err(InstallError::validation)?;
    if managed(&target)?.is_some() {
        return Ok(None);
    }
    let plan = uninstall::preview_legacy(&target)?;
    if plan.artifacts.is_empty() {
        return Ok(None);
    }
    let sources = plan
        .artifacts
        .iter()
        .map(|a| (a.clone(), target.join(&a.file_name)))
        .collect::<Vec<_>>();
    validate_packages(&target, &sources)?;
    Ok(Some(UpgradeReview {
        target,
        runtime,
        artifacts: plan.artifacts,
        stock: plan.stock,
    }))
}

pub fn upgrade_installation(
    plan: &UpgradeReview,
    backups: &Path,
    check: GameRunningCheck,
) -> Result<UpgradeReport, InstallError> {
    check_other_mods(&plan.target, &plan.sources())?;
    upgrade_with_observer(plan, backups, check, &mut |_| {})
}

pub(super) fn upgrade_with_observer(
    plan: &UpgradeReview,
    backups: &Path,
    check: GameRunningCheck,
    observer: &mut dyn FnMut(&str),
) -> Result<UpgradeReport, InstallError> {
    check_stopped(check)?;
    let lock = lock_installation(&plan.target)?;
    recover_interrupted_install_locked(&RecoveryRequest {
        target_packages_directory: plan.target.clone(),
        backup_root: backups.into(),
        game_running_check: check,
        runtime_snapshot_check: sundial::package_authoring::installed_runtime,
    })?;
    if review_upgrade(&plan.target)?.as_ref() != Some(plan) {
        return Err(InstallError::validation(
            "The installation changed since upgrade review. Review it again",
        ));
    }
    let sources = plan
        .artifacts
        .iter()
        .map(|a| (a.clone(), plan.target.join(&a.file_name)))
        .collect::<Vec<_>>();
    let mut transaction = transaction::prepare(transaction::Plan {
        target: &plan.target,
        backup_root: backups,
        original: None,
        legacy: plan.artifacts.clone(),
        stock: plan.stock.clone(),
        incoming: Some(&sources),
        runtime: &plan.runtime,
    })?;
    let backup = transaction.record.context.backup_directory.clone();
    finish_commit(&mut transaction, check, observer, |_| Ok(()))?;
    lock.finish(check, sundial::package_authoring::installed_runtime)?;
    Ok(UpgradeReport {
        mod_directory: plan.mod_directory(),
        backup_directory: backup,
        package_count: plan.artifacts.len(),
    })
}

pub(super) fn preview_uninstall(target: &Path) -> Result<uninstall::UninstallPlan, InstallError> {
    let original = managed(target)?;
    let mut plan = uninstall::preview_legacy(target)?;
    if let Some(original) = original {
        if !plan.artifacts.is_empty() {
            return Err(InstallError::validation(
                "Legacy and mod-folder packages are both present",
            ));
        }
        plan.artifacts = artifacts(&original);
        plan.mod_folder = Some(original);
    }
    Ok(plan)
}

pub(super) fn install_with_observer(
    request: &InstallRequest,
    progress: Observer<'_>,
    observer: &mut dyn FnMut(&str),
) -> Result<InstallReport, InstallError> {
    let _lease = crate::workflow::staging_retention::lease_for_read(&request.staged_run_directory)
        .map_err(InstallError::validation)?;
    if review_upgrade(&request.target_packages_directory)?.is_some() {
        return Err(InstallError::validation(
            "Upgrade the existing Parhelion installation to the Sunrise mod folder before installing a new build",
        ));
    }
    let validated = validate_request_with_progress(request, progress)?;
    let target = &validated.target_packages_directory;
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    mod_content::ensure_resolved_environment(install).map_err(InstallError::validation)?;
    let original = managed(target)?;
    let sources = validated
        .artifacts
        .iter()
        .map(|a| (a.clone(), validated.staged_run_directory.join(&a.file_name)))
        .collect::<Vec<_>>();
    validate_packages(target, &sources)?;
    reject_installed_conflicts(target, &sources)?;
    let mod_overlaps = check_other_mods(target, &sources)?;
    let mut transaction = transaction::prepare(transaction::Plan {
        target,
        backup_root: &validated.backup_root,
        original: original.clone(),
        legacy: vec![],
        stock: validated.source_artifacts.clone(),
        incoming: Some(&sources),
        runtime: &validated.sunrise_build_cache.runtime,
    })?;
    let backup = transaction.record.context.backup_directory.clone();
    let recipe_backup_directory =
        backup_recipe_snapshots(&validated, &backup).map_err(InstallError::validation)?;
    if let Some(review) = &validated.replacement_guard {
        replacement::verify_account(target, Some(review)).map_err(InstallError::validation)?;
        if let Some(cleanup) = review.account_cleanup() {
            transaction.record.context.account_cleanup =
                Some(account::prepare(cleanup, target, &backup)?);
        }
        if let Some(settings) = review.client_settings() {
            transaction.record.context.client_settings =
                Some(account::prepare_settings(settings, target, &backup)?);
        }
    }
    progress(InstallProgress::stage(InstallPhase::Installing));
    let (invalidated_sunrise_cache, invalidated_package_header_caches) = finish_commit(
        &mut transaction,
        request.game_running_check,
        observer,
        |context| {
            replacement::verify_account(target, validated.replacement_guard.as_ref())
                .map_err(InstallError::validation)?;
            if let Some(review) = &validated.replacement_guard {
                if let (Some(cleanup), Some(record)) =
                    (review.account_cleanup(), &context.account_cleanup)
                {
                    account::commit(cleanup, record, target, &backup)?;
                }
                if let (Some(settings), Some(record)) =
                    (review.client_settings(), &context.client_settings)
                {
                    account::commit_settings(settings, record, target, &backup)?;
                }
            }
            Ok(())
        },
    )?;
    let directory = mod_content::directory(install);
    let profile_sync = (!validated.authored_unlocks.is_empty())
        .then(|| synchronize_authored_collection_unlocks(install, &validated.authored_unlocks));
    let item_grants = (!validated.authored_grants.is_empty())
        .then(|| identities::grant_items(install, target, &validated.authored_grants));
    progress(InstallProgress::stage(InstallPhase::Complete));
    Ok(InstallReport {
        manifest_schema: MANIFEST_SCHEMA,
        staged_run_directory: validated.staged_run_directory.clone(),
        target_packages_directory: target.clone(),
        mod_directory: Some(directory.clone()),
        backup_directory: backup.clone(),
        artifacts: validated
            .artifacts
            .iter()
            .map(|a| {
                let path = package_path(a).expect("validated package");
                let replaced = original
                    .as_ref()
                    .is_some_and(|o| o.files.contains_key(&path));
                InstalledArtifact {
                    file_name: a.file_name.clone(),
                    target_path: directory.join(&path),
                    byte_length: a.byte_length,
                    sha256: a.sha256.clone(),
                    replaced_existing_file: replaced,
                    backup_path: replaced.then(|| backup.join("mod").join(path)),
                }
            })
            .collect(),
        removed_obsolete_packages: original
            .as_ref()
            .map(artifacts)
            .unwrap_or_default()
            .iter()
            .filter(|old| {
                !validated
                    .artifacts
                    .iter()
                    .any(|a| a.file_name == old.file_name)
            })
            .map(|a| directory.join(package_path(a).expect("validated package")))
            .collect(),
        recipe_backup_directory,
        pruned_backup_directories: vec![],
        backup_prune_warning: Some(
            "Mod recovery backups are retained separately from automatic package backup cleanup"
                .into(),
        ),
        invalidated_sunrise_cache,
        invalidated_package_header_caches,
        profile_sync,
        item_grants,
        mod_overlaps,
        cleaned_account: validated
            .replacement_guard
            .as_ref()
            .filter(|r| r.changes_account())
            .and_then(|r| r.account_cleanup())
            .map(|c| c.settings_path.clone()),
    })
}

pub(super) fn uninstall_with_observer(
    plan: &uninstall::UninstallPlan,
    backups: &Path,
    check: GameRunningCheck,
    observer: &mut dyn FnMut(&str),
) -> Result<UninstallReport, InstallError> {
    check_stopped(check)?;
    let lock = lock_installation(&plan.target)?;
    recover_interrupted_install_locked(&RecoveryRequest {
        target_packages_directory: plan.target.clone(),
        backup_root: backups.into(),
        game_running_check: check,
        runtime_snapshot_check: sundial::package_authoring::installed_runtime,
    })?;
    let current = preview_uninstall(&plan.target)?;
    if current.mod_folder != plan.mod_folder
        || current.artifacts != plan.artifacts
        || current.stock != plan.stock
        || plan.mod_folder.is_none()
    {
        return Err(InstallError::validation(
            "The installed mod changed since removal review",
        ));
    }
    let runtime = sundial::package_authoring::installed_runtime(
        plan.target
            .parent()
            .ok_or_else(|| InstallError::validation("Missing game root"))?,
    )
    .map_err(InstallError::validation)?;
    let mut transaction = transaction::prepare(transaction::Plan {
        target: &plan.target,
        backup_root: backups,
        original: plan.mod_folder.clone(),
        legacy: vec![],
        stock: plan.stock.clone(),
        incoming: None,
        runtime: &runtime,
    })?;
    let backup = transaction.record.context.backup_directory.clone();
    if let Some(cleanup) = &plan.cleanup {
        if uninstall::prepare_account_cleanup(plan).map_err(InstallError::validation)? != *cleanup {
            return Err(InstallError::validation(
                "Account data changed since removal review",
            ));
        }
        transaction.record.context.account_cleanup =
            Some(account::prepare(cleanup, &plan.target, &backup)?);
    }
    let (invalidated_sunrise_cache, invalidated_package_header_caches) =
        finish_commit(&mut transaction, check, observer, |context| {
            if let (Some(cleanup), Some(record)) = (&plan.cleanup, &context.account_cleanup) {
                account::commit(cleanup, record, &plan.target, &backup)?;
            }
            Ok(())
        })?;
    let directory = mod_content::directory(plan.target.parent().unwrap());
    lock.finish(check, sundial::package_authoring::installed_runtime)?;
    Ok(UninstallReport {
        removed_files: plan
            .artifacts
            .iter()
            .map(|a| directory.join(package_path(a).expect("validated package")))
            .collect(),
        backup_directory: backup,
        invalidated_sunrise_cache,
        invalidated_package_header_caches,
        cleaned_account: plan.cleanup.as_ref().map(|c| c.settings_path.clone()),
    })
}

pub(super) fn uninstall_legacy(
    plan: &uninstall::UninstallPlan,
    backups: &Path,
    check: GameRunningCheck,
) -> Result<UninstallReport, InstallError> {
    check_stopped(check)?;
    let upgrade = review_upgrade(&plan.target)?.ok_or_else(|| {
        InstallError::validation("The legacy installation changed since removal review")
    })?;
    if upgrade.artifacts != plan.artifacts || upgrade.stock != plan.stock {
        return Err(InstallError::validation(
            "The legacy installation changed since removal review",
        ));
    }
    // Migration is independently recoverable. Removal then uses the regular mod transaction.
    upgrade_with_observer(&upgrade, backups, check, &mut |_| {})?;
    let mut migrated = preview_uninstall(&plan.target)?;
    migrated.cleanup = plan.cleanup.clone();
    uninstall_with_observer(&migrated, backups, check, &mut |_| {})
}

fn finish_commit(
    transaction: &mut transaction::Transaction,
    check: GameRunningCheck,
    observer: &mut dyn FnMut(&str),
    apply_account: impl FnOnce(&InstallTransactionRecord) -> Result<(), InstallError>,
) -> Result<
    (
        Option<InvalidatedSunriseCache>,
        Vec<InvalidatedPackageHeaderCache>,
    ),
    InstallError,
> {
    match transaction::commit(transaction, check, observer, apply_account) {
        Ok(value) => Ok(value),
        Err(error) => {
            let context = &transaction.record.context;
            let recovered = recover(&RecoveryRequest {
                target_packages_directory: context.target_packages_directory.clone(),
                backup_root: context.backup_directory.parent().unwrap().into(),
                game_running_check: check,
                runtime_snapshot_check: sundial::package_authoring::installed_runtime,
            });
            let detail = match recovered {
                Ok(Some(RecoveryOutcome::Recovered { .. })) => {
                    "The previous content was restored".into()
                }
                Ok(Some(RecoveryOutcome::CommittedTransaction)) => {
                    "The verified mod operation completed during recovery".into()
                }
                Ok(_) => "Review the current content before continuing".into(),
                Err(e) => format!("Recovery is still required: {e}"),
            };
            Err(InstallError::after_backup(
                format!("{error}. {detail}"),
                &context.backup_directory,
            ))
        }
    }
}
