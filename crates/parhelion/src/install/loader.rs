//! Sunrise 0.6 managed-directory publication and legacy-layout migration.
use super::*;
use sundial::package_authoring::mod_content;
mod legacy;
mod transaction;
mod validation;
pub(super) use legacy::recover_with_observer as recover_legacy_with_observer;
pub(super) use transaction::completed_backup_owner;
pub(super) use transaction::recover;
pub(super) use validation::verify_file;
use validation::*;
pub(crate) use validation::{other_mods_change, package_window, uses_mod_loader};

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Folder {
    files: BTreeMap<String, TransactionDigest>,
}

/// The build the active runtime doesn't load, and the one kept for it. On a runtime that loads
/// Sunrise mods, these are packages an earlier layout or a Dawn install left in stock `packages`.
/// Their patches add entries to stock packages, which the loader refuses, so they are never moved
/// into the mod folder. The next install retires them with a verified backup while it publishes a
/// build made for the mod folder, and removal retires them the same way. On Dawn or Sunrise before
/// 0.6 it is Parhelion's mod folder, which the next install there retires, and the review appears
/// only when a build is kept for this runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeReview {
    target: PathBuf,
    artifacts: Vec<ArtifactMetadata>,
    kept: Option<kept::KeptBuild>,
}
impl UpgradeReview {
    pub fn target(&self) -> &Path {
        &self.target
    }
    /// The packages an earlier layout left in stock `packages`, empty on a runtime without the mod
    /// loader.
    pub fn artifacts(&self) -> &[ArtifactMetadata] {
        &self.artifacts
    }
    pub fn mod_directory(&self) -> PathBuf {
        mod_content::directory(self.target.parent().expect("reviewed installation"))
    }
    /// The build last installed for the active runtime, which can install again without a
    /// rebuild.
    pub fn kept(&self) -> Option<&kept::KeptBuild> {
        self.kept.as_ref()
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

/// What an upgrade review is doing at each step, in order, as Parhelion shows it.
pub const UPGRADE_REVIEW_STEPS: [&str; 5] = [
    "Reading the active game runtime…",
    "Checking for an interrupted package operation…",
    "Reading other Sunrise mods…",
    "Checking Parhelion's mod folder…",
    "Looking for packages from earlier builds…",
];

/// The packages from an earlier layout that the next install or removal retires, or `None`
/// when the runtime does not load Sunrise mods or nothing remains in stock `packages`.
#[cfg(test)]
pub fn preview_upgrade(packages: &Path) -> Result<Option<UpgradeReview>, InstallError> {
    review_upgrade(packages, &mut |_| {})
}

/// As [`preview_upgrade`], calling `progress` with the index in [`UPGRADE_REVIEW_STEPS`] of each
/// step as it starts.
pub fn preview_upgrade_with_progress(
    packages: &Path,
    progress: &mut dyn FnMut(usize),
) -> Result<Option<UpgradeReview>, InstallError> {
    review_upgrade(packages, progress)
}

fn review_upgrade(
    packages: &Path,
    progress: &mut dyn FnMut(usize),
) -> Result<Option<UpgradeReview>, InstallError> {
    progress(0);
    let target = canonical_packages_directory(packages)?;
    let runtime = sundial::package_authoring::installed_runtime(
        target
            .parent()
            .ok_or_else(|| InstallError::validation("Missing game root"))?,
    )
    .map_err(InstallError::validation)?;
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    if !runtime
        .uses_mod_loader()
        .map_err(InstallError::validation)?
    {
        // The mod folder doesn't load here. The review offers the build kept for this runtime in
        // its place, and otherwise the next install retires the folder without asking.
        if !mod_content::has_family(install).map_err(failure)? {
            return Ok(None);
        }
        return Ok(kept::find(&target, false)
            .map_err(InstallError::validation)?
            .map(|kept| UpgradeReview {
                target,
                artifacts: Vec::new(),
                kept: Some(kept),
            }));
    }
    progress(1);
    if target
        .join(INSTALL_TRANSACTION_FILE_NAME)
        .try_exists()
        .map_err(failure)?
    {
        return Err(InstallError::validation(
            "An earlier package operation needs recovery. Recover it before building for the Sunrise mod folder",
        ));
    }
    progress(2);
    mod_content::ensure_readable(install).map_err(InstallError::validation)?;
    mod_content::ensure_resolved_environment(install).map_err(InstallError::validation)?;
    progress(3);
    // With Parhelion's mod folder in place there is nothing to upgrade. The review runs at every
    // start, so it checks the folder's manifest, receipt and file list without hashing its
    // packages, which can run to gigabytes. Installation and removal verify them in full before
    // they change anything.
    if mod_content::has_family(install).map_err(failure)? {
        mod_content::files(install).map_err(InstallError::validation)?;
        return Ok(None);
    }
    progress(4);
    let plan = uninstall::preview_legacy(&target)?;
    if plan.artifacts.is_empty() {
        return Ok(None);
    }
    Ok(Some(UpgradeReview {
        kept: kept::find(&target, true).map_err(InstallError::validation)?,
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

/// The mod folder an install on Dawn or an earlier Sunrise retired. Dropped before
/// [`Retired::keep`], it puts the folder back from its backup, so an install that fails before it
/// commits leaves the installation as it was. Those runtimes ignore the folder, so it does no harm
/// there.
pub(super) struct Retired {
    install: PathBuf,
    backup: Option<PathBuf>,
}

impl Retired {
    /// The install is committing: the folder stays retired. Returns its backup.
    pub(super) fn keep(mut self) -> Option<PathBuf> {
        self.backup.take()
    }
}

impl Drop for Retired {
    fn drop(&mut self) {
        if let Some(backup) = &self.backup {
            // A folder that can't be put back stays in its backup, as it was before.
            let _ = transaction::restore_retired(&self.install, backup);
        }
    }
}

/// Retires the mod folder before an install on Dawn or Sunrise before 0.6 publishes to stock
/// `packages`. Left in place it would load beside the new packages after a switch back to
/// Sunrise 0.6. It is backed up first, and the install makes any account change.
pub(super) fn retire_inactive(
    validated: &ValidatedRun,
    check: GameRunningCheck,
) -> Result<Retired, InstallError> {
    let target = &validated.target_packages_directory;
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?
        .to_owned();
    let mut original_family = installation_family(validated)?;
    if original_family.is_empty() {
        return Ok(Retired {
            install,
            backup: None,
        });
    }
    let original = original_family.remove(mod_content::ID);
    if !uninstall::preview_legacy(target)?.artifacts.is_empty() {
        return Err(InstallError::validation(
            "Legacy and mod-folder packages are both present",
        ));
    }
    let mut transaction = transaction::prepare(transaction::Plan {
        target,
        backup_root: &validated.backup_root,
        original,
        original_family,
        members: &[],
        legacy: Vec::new(),
        stock: validated.source_artifacts.clone(),
        incoming: None,
        runtime: &validated.sunrise_build_cache.runtime,
    })?;
    let backup = transaction.record.context.backup_directory.clone();
    finish_commit(&mut transaction, check, &mut |_| {}, |_| Ok(()))?;
    Ok(Retired {
        install,
        backup: Some(backup),
    })
}

pub(super) fn preview_uninstall(target: &Path) -> Result<uninstall::UninstallPlan, InstallError> {
    preview_uninstall_with_packages(target, &[]).map(|(plan, _)| plan)
}

pub(super) fn preview_uninstall_with_packages(
    target: &Path,
    known: &[CheckedPackage],
) -> Result<(uninstall::UninstallPlan, Vec<CheckedPackage>), InstallError> {
    let (mut family, packages) = managed_family_with_packages(target, known)?;
    let original = family.remove(mod_content::ID);
    let mut plan = uninstall::preview_legacy(target)?;
    if let Some(original) = original {
        if !plan.artifacts.is_empty() {
            return Err(InstallError::validation(
                "Legacy and mod-folder packages are both present",
            ));
        }
        plan.artifacts = artifacts(&original);
        plan.artifacts.extend(family.values().flat_map(artifacts));
        plan.mod_folder = Some(original);
        plan.mod_family = family;
    }
    Ok((plan, packages))
}

/// Carries the reviewed folder inventory into preparation. Package evidence is reusable only
/// after current digests and stock sources pass the installed-family checks.
fn installation_family(validated: &ValidatedRun) -> Result<BTreeMap<String, Folder>, InstallError> {
    let mut known = validated.packages.clone();
    if let Some(review) = &validated.replacement_guard {
        known.extend_from_slice(review.packages());
    }
    let (family, _) = managed_family_with_packages(&validated.target_packages_directory, &known)?;
    if let Some(review) = &validated.replacement_guard {
        review
            .verify_family(&family)
            .map_err(InstallError::validation)?;
    }
    Ok(family)
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
    let mut original_family = installation_family(&validated)?;
    let original = original_family.remove(mod_content::ID);
    // Packages an earlier layout left in stock `packages` cannot load beside the mod, so this
    // transaction retires them, after backing them up, while it publishes the build.
    let legacy = retired_legacy(target, original.as_ref())?;
    let staged = |artifacts: &[ArtifactMetadata]| -> Result<Vec<_>, InstallError> {
        artifacts
            .iter()
            .map(|a| {
                crate::mod_family::source(
                    &validated.mods,
                    &validated.staged_run_directory,
                    target,
                    &a.file_name,
                )
                .map(|path| (a.clone(), path))
                .map_err(InstallError::validation)
            })
            .collect()
    };
    let sources = staged(&validated.artifacts)?;
    validate_packages(target, &sources, &validated.packages)?;
    reject_installed_conflicts(target, &sources, &legacy)?;
    let mod_overlaps = check_other_mods(target, &sources, &legacy, &validated.mods)?;
    let mod_reports = crate::mod_family::reports(&validated.mods, install, &validated.artifacts)
        .map_err(InstallError::validation)?;
    // The merge data is published beside its packages.
    let diagnostics = validated
        .map_diagnostics
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let published = [
        sources.as_slice(),
        &staged(&validated.merge_data)?,
        &staged(&diagnostics)?,
        &staged(&validated.mission_scripts)?,
    ]
    .concat();
    if !legacy.is_empty() {
        clear_other_runtime_caches(target, validated.sunrise_build_cache.runtime.brand())?;
    }
    let mut transaction = transaction::prepare(transaction::Plan {
        target,
        backup_root: &validated.backup_root,
        original: original.clone(),
        original_family: original_family.clone(),
        members: &validated.mods,
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
    let (pruned_backup_directories, backup_prune_warning) = if validated.limit_package_backups {
        match backups::prune_after_install(
            &validated.backup_root,
            validated.package_backup_retention,
            &backup,
        ) {
            Ok(report) => (report.removed_directories, None),
            Err(error) => (Vec::new(), Some(error)),
        }
    } else {
        (Vec::new(), None)
    };
    progress(InstallProgress::stage(InstallPhase::Complete));
    Ok(InstallReport {
        mods: mod_reports,
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
                let owner = validated.mods.iter().find(|m| {
                    m.receipt
                        .artifacts
                        .iter()
                        .any(|owned| owned.path.rsplit('/').next() == Some(a.file_name.as_str()))
                });
                let id = owner.map_or(mod_content::ID, |m| m.manifest.id.as_str());
                let kept = owner.is_some_and(|m| m.kept);
                let old = if id == mod_content::ID {
                    original.as_ref()
                } else {
                    original_family.get(id)
                };
                let replaced = !kept && old.is_some_and(|o| o.files.contains_key(&path));
                InstalledArtifact {
                    file_name: a.file_name.clone(),
                    target_path: mod_content::member_directory(install, id).join(&path),
                    byte_length: a.byte_length,
                    sha256: a.sha256.clone(),
                    replaced_existing_file: replaced,
                    backup_path: replaced.then(|| backup.join("mods").join(id).join(&path)),
                }
            })
            .collect(),
        removed_obsolete_packages: std::iter::once((mod_content::ID, original.as_ref()))
            .chain(
                original_family
                    .iter()
                    .map(|(id, folder)| (id.as_str(), Some(folder))),
            )
            .flat_map(|(id, folder)| {
                folder
                    .into_iter()
                    .flat_map(artifacts)
                    .map(move |artifact| (id, artifact))
            })
            .filter(|(id, old)| {
                !validated
                    .mods
                    .iter()
                    .find(|m| m.manifest.id == *id)
                    .map_or_else(
                        || {
                            *id == mod_content::ID
                                && validated.mods.is_empty()
                                && validated
                                    .artifacts
                                    .iter()
                                    .any(|a| a.file_name == old.file_name)
                        },
                        |m| {
                            m.receipt
                                .artifacts
                                .iter()
                                .any(|a| a.path.rsplit('/').next() == Some(old.file_name.as_str()))
                        },
                    )
            })
            .map(|(id, artifact)| {
                mod_content::member_directory(install, id)
                    .join(package_path(&artifact).expect("validated package"))
            })
            .chain(legacy.iter().map(|a| target.join(&a.file_name)))
            .collect(),
        recipe_backup_directory,
        pruned_backup_directories,
        backup_prune_warning,
        invalidated_sunrise_cache,
        invalidated_package_header_caches,
        profile_sync,
        item_grants,
        mod_overlaps,
        retired_mod_backup: None,
        kept_build_warning: None,
        other_build_kept: false,
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
        || current.mod_family != plan.mod_family
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
        original_family: plan.mod_family.clone(),
        members: &[],
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
    lock.finish(check, sundial::package_authoring::installed_runtime)?;
    Ok(UninstallReport {
        removed_files: if legacy.is_empty() {
            plan.artifacts
                .iter()
                .map(|a| {
                    let path = package_path(a).expect("validated package");
                    let id = plan
                        .mod_family
                        .iter()
                        .find(|(_, folder)| folder.files.contains_key(&path))
                        .map_or(mod_content::ID, |(id, _)| id.as_str());
                    mod_content::member_directory(plan.target.parent().unwrap(), id).join(path)
                })
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
