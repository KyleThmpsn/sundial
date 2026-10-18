//! Sunrise 0.6 managed-directory publication and legacy-layout migration.
use super::*;
use sundial::package_authoring::mod_content;
mod legacy;
mod transaction;
mod validation;
pub(super) use legacy::recover_with_observer as recover_legacy_with_observer;
pub(super) use transaction::recover;
use validation::*;
pub(crate) use validation::{package_window, uses_mod_loader};

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Folder {
    files: BTreeMap<String, TransactionDigest>,
}

/// Packages an earlier layout left in stock `packages` on an installation whose runtime loads
/// Sunrise mods. Their patches add entries to stock packages, which the loader refuses, so they
/// are never moved into the mod folder. The next install retires them with a verified backup
/// while it publishes a build made for the mod folder, and removal retires them the same way.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeReview {
    target: PathBuf,
    artifacts: Vec<ArtifactMetadata>,
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

/// Whether installs publish to the mod folder, as on Sunrise 0.6 or later. Dawn and earlier
/// Sunrise load stock `packages` instead.
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
    runtime.uses_mod_loader().map_err(InstallError::validation)
}

/// The packages from an earlier layout that the next install or removal retires, or `None`
/// when the runtime does not load Sunrise mods or nothing remains in stock `packages`.
pub fn preview_upgrade(packages: &Path) -> Result<Option<UpgradeReview>, InstallError> {
    review_upgrade(packages)
}

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
            "An earlier package operation needs recovery. Recover it before building for the Sunrise mod folder",
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
    Ok(Some(UpgradeReview {
        target,
        artifacts: plan.artifacts,
    }))
}

/// The packages from an earlier layout an operation on the mod folder retires. They cannot share
/// the installation with a mod folder, and an interrupted earlier operation is recovered first.
fn retired_legacy(
    target: &Path,
    original: Option<&Folder>,
) -> Result<Vec<ArtifactMetadata>, InstallError> {
    if target
        .join(INSTALL_TRANSACTION_FILE_NAME)
        .try_exists()
        .map_err(failure)?
    {
        return Err(InstallError::validation(
            "An earlier package operation needs recovery. Recover it before installing",
        ));
    }
    let legacy = uninstall::preview_legacy(target)?.artifacts;
    if original.is_some() && !legacy.is_empty() {
        return Err(InstallError::validation(
            "Legacy and mod-folder packages are both present",
        ));
    }
    Ok(legacy)
}

/// Retires the mod folder before an install on Dawn or Sunrise before 0.6 publishes to stock
/// `packages`. Those runtimes don't load the folder, and left in place it would load beside the
/// new packages after a switch back to Sunrise 0.6. It is backed up first, and the install makes
/// any account change. Returns the backup.
pub(super) fn retire_inactive(
    validated: &ValidatedRun,
    check: GameRunningCheck,
) -> Result<Option<PathBuf>, InstallError> {
    let target = &validated.target_packages_directory;
    let Some(original) = managed(target)? else {
        return Ok(None);
    };
    if !uninstall::preview_legacy(target)?.artifacts.is_empty() {
        return Err(InstallError::validation(
            "Legacy and mod-folder packages are both present",
        ));
    }
    let mut transaction = transaction::prepare(transaction::Plan {
        target,
        backup_root: &validated.backup_root,
        original: Some(original),
        legacy: Vec::new(),
        stock: validated.source_artifacts.clone(),
        incoming: None,
        runtime: &validated.sunrise_build_cache.runtime,
    })?;
    let backup = transaction.record.context.backup_directory.clone();
    finish_commit(&mut transaction, check, &mut |_| {}, |_| Ok(()))?;
    Ok(Some(backup))
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
    let validated = validate_request_with_progress(request, progress)?;
    let target = &validated.target_packages_directory;
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    mod_content::ensure_resolved_environment(install).map_err(InstallError::validation)?;
    let original = managed(target)?;
    // Packages an earlier layout left in stock `packages` cannot load beside the mod, so this
    // transaction retires them, after backing them up, while it publishes the build.
    let legacy = retired_legacy(target, original.as_ref())?;
    let staged = |artifacts: &[ArtifactMetadata]| {
        artifacts
            .iter()
            .map(|a| (a.clone(), validated.staged_run_directory.join(&a.file_name)))
            .collect::<Vec<_>>()
    };
    let sources = staged(&validated.artifacts);
    validate_packages(target, &sources)?;
    reject_installed_conflicts(target, &sources, &legacy)?;
    let mod_overlaps = check_other_mods(target, &sources, &legacy)?;
    // The merge data is published beside its packages.
    let published = [sources.as_slice(), &staged(&validated.merge_data)].concat();
    if !legacy.is_empty() {
        clear_other_runtime_caches(target, validated.sunrise_build_cache.runtime.brand())?;
    }
    let mut transaction = transaction::prepare(transaction::Plan {
        target,
        backup_root: &validated.backup_root,
        original: original.clone(),
        legacy: legacy.clone(),
        stock: validated.source_artifacts.clone(),
        incoming: Some(&published),
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
            .chain(legacy.iter().map(|a| target.join(&a.file_name)))
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
        retired_mod_backup: None,
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
        || plan.artifacts.is_empty()
    {
        return Err(InstallError::validation(
            "The installed mod changed since removal review",
        ));
    }
    // Without a mod folder, the reviewed packages are an earlier layout's in stock `packages`.
    let legacy = if plan.mod_folder.is_none() {
        plan.artifacts.clone()
    } else {
        Vec::new()
    };
    let runtime = sundial::package_authoring::installed_runtime(
        plan.target
            .parent()
            .ok_or_else(|| InstallError::validation("Missing game root"))?,
    )
    .map_err(InstallError::validation)?;
    if !legacy.is_empty() {
        clear_other_runtime_caches(&plan.target, runtime.brand())?;
    }
    let mut transaction = transaction::prepare(transaction::Plan {
        target: &plan.target,
        backup_root: backups,
        original: plan.mod_folder.clone(),
        legacy: legacy.clone(),
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
        removed_files: if legacy.is_empty() {
            plan.artifacts
                .iter()
                .map(|a| directory.join(package_path(a).expect("validated package")))
                .collect()
        } else {
            legacy
                .iter()
                .map(|a| plan.target.join(&a.file_name))
                .collect()
        },
        backup_directory: backup,
        invalidated_sunrise_cache,
        invalidated_package_header_caches,
        cleaned_account: plan.cleanup.as_ref().map(|c| c.settings_path.clone()),
    })
}

/// Removes the packages an earlier layout left in stock `packages`, and the reviewed account
/// cleanup, in one transaction that retires them after backing them up.
pub(super) fn uninstall_legacy(
    plan: &uninstall::UninstallPlan,
    backups: &Path,
    check: GameRunningCheck,
) -> Result<UninstallReport, InstallError> {
    uninstall_with_observer(plan, backups, check, &mut |_| {})
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
