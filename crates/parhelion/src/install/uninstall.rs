//! Remove the complete recognized authored overlay set, never stock generations.
use super::*;
use sundial::investment::AuthoredAccountCleanup;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UninstallPlan {
    target: PathBuf,
    artifacts: Vec<ArtifactMetadata>,
    stock: Vec<ArtifactMetadata>,
    cleanup: Option<AuthoredAccountCleanup>,
    cleanup_error: Option<String>,
}

impl UninstallPlan {
    #[cfg(test)]
    pub(crate) fn preview_fixture() -> Self {
        Self {
            target: PathBuf::from("C:/Example Game/packages"),
            artifacts: AUTHORED_PACKAGES
                .iter()
                .map(|profile| ArtifactMetadata {
                    file_name: profile.file_name.into(),
                    byte_length: 100,
                    sha256: "0".repeat(64),
                })
                .collect(),
            stock: vec![],
            cleanup: Some(AuthoredAccountCleanup {
                settings_path: PathBuf::from("C:/Example Game/Sunrise/settings.json"),
                original_bytes: vec![],
                cleaned_bytes: vec![],
                removed_items: BTreeMap::from([(100, 2)]),
                resized_items: BTreeMap::new(),
                cleared_plugs: 1,
                cleared_unlocks: 3,
                removed_reward_rules: 0,
            }),
            cleanup_error: None,
        }
    }
    pub fn target(&self) -> &Path {
        &self.target
    }
    pub fn artifacts(&self) -> &[ArtifactMetadata] {
        &self.artifacts
    }
    pub fn account_cleanup(&self) -> Option<&AuthoredAccountCleanup> {
        self.cleanup.as_ref()
    }
    pub fn account_cleanup_error(&self) -> Option<&str> {
        self.cleanup_error.as_deref()
    }
    pub fn without_account_cleanup(mut self) -> Self {
        self.cleanup = None;
        self
    }
}

#[derive(Clone, Debug)]
pub struct UninstallReport {
    pub removed_files: Vec<PathBuf>,
    pub backup_directory: PathBuf,
    pub invalidated_sunrise_cache: Option<InvalidatedSunriseCache>,
    pub invalidated_package_header_caches: Vec<InvalidatedPackageHeaderCache>,
    pub cleaned_account: Option<PathBuf>,
}

/// Read-only review. Unknown signatures, aliases and incomplete sets fail closed.
pub fn preview_uninstall(packages: &Path) -> Result<UninstallPlan, InstallError> {
    let target = canonical_directory(packages, "target packages")?;
    validate_target_package_chain(&target)?;
    let mut artifacts = Vec::new();
    for profile in AUTHORED_PACKAGES {
        let path = target.join(profile.file_name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(InstallError::validation(error.to_string())),
            Ok(_) => artifacts.push(snapshot(&target, profile.file_name)?),
        }
    }
    if !artifacts.is_empty() {
        authored_packages_for_file_names(
            artifacts.iter().map(|artifact| artifact.file_name.as_str()),
        )
        .map_err(InstallError::validation)?;
    }
    let stock = discover_target_source_artifact_names(&target)?
        .iter()
        .map(|name| snapshot(&target, name))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(UninstallPlan {
        target,
        artifacts,
        stock,
        cleanup: None,
        cleanup_error: None,
    })
}

/// Reviews package ownership and the selected account without writing either.
pub fn preview_uninstall_with_account_cleanup(
    packages: &Path,
) -> Result<UninstallPlan, InstallError> {
    let mut plan = preview_uninstall(packages)?;
    if !plan.artifacts.is_empty() {
        match prepare_account_cleanup(&plan) {
            Ok(cleanup) => plan.cleanup = Some(cleanup),
            Err(error) => plan.cleanup_error = Some(error),
        }
    }
    Ok(plan)
}

fn prepare_account_cleanup(plan: &UninstallPlan) -> Result<AuthoredAccountCleanup, String> {
    let (hashes, unlocks) = identities::installed_identities(&plan.target)?;
    sundial::investment::preview_authored_account_cleanup(
        plan.target.parent().ok_or("Missing game root")?,
        &hashes,
        &unlocks,
    )
}

fn snapshot(target: &Path, name: &str) -> Result<ArtifactMetadata, InstallError> {
    let path = target.join(name);
    reject_symlink(&path, "uninstall package")?;
    let digest = digest_file(&path).map_err(|error| InstallError::validation(error.to_string()))?;
    Ok(ArtifactMetadata {
        file_name: name.into(),
        byte_length: digest.byte_length,
        sha256: digest.sha256,
    })
}

/// Execute an unchanged reviewed plan, including its optional, backed-up account cleanup.
pub fn uninstall_custom_packages(
    plan: &UninstallPlan,
    backup_root: &Path,
    check: GameRunningCheck,
) -> Result<UninstallReport, InstallError> {
    uninstall_inner(
        plan,
        backup_root,
        check,
        None,
        DEFAULT_CACHE_INVALIDATION_OPS,
    )
}

pub(super) fn uninstall_inner(
    plan: &UninstallPlan,
    backup_root: &Path,
    check: GameRunningCheck,
    fail_after: Option<usize>,
    cache_ops: CacheInvalidationOps,
) -> Result<UninstallReport, InstallError> {
    let _lock = lock_installation(&plan.target)?;
    check_stopped(check)?;
    recover_interrupted_install_locked(&RecoveryRequest {
        target_packages_directory: plan.target.clone(),
        backup_root: backup_root.to_path_buf(),
        game_running_check: check,
    })?;
    verify_plan(plan)?;
    if plan.artifacts.is_empty() {
        return Err(InstallError::validation("No custom packages are installed"));
    }
    let root = resolve_path_for_comparison(backup_root)
        .map_err(|error| InstallError::validation(error.to_string()))?;
    if path_is_within(&root, &plan.target) {
        return Err(InstallError::validation(
            "The uninstall backup must be outside the packages directory",
        ));
    }
    let sunrise = validate_sunrise_build_cache(&plan.target)?;
    let headers = validate_package_header_caches(&plan.target)?;
    fs::create_dir_all(&root).map_err(|error| InstallError::validation(error.to_string()))?;
    let root = canonical_directory(&root, "uninstall backup root")?;
    if path_is_within(&root, &plan.target) {
        return Err(InstallError::validation(
            "Backup root changed during uninstall preparation",
        ));
    }
    // Not an automatic-install generation: retention must not silently discard this recovery set.
    let backup = root.join(format!("parhelion-uninstall-{}", unique_token()));
    create_private_directory(&backup)
        .map_err(|error| InstallError::validation(error.to_string()))?;
    let backup = canonical_directory(&backup, "uninstall backup")?;
    if !path_is_within(&backup, &root) || path_is_within(&backup, &plan.target) {
        return Err(InstallError::validation(
            "The resolved uninstall backup escaped its selected root",
        ));
    }
    let after_backup = |message| InstallError::after_backup(message, &backup);
    let originals = plan
        .artifacts
        .iter()
        .map(|artifact| {
            backup_one_original(
                &artifact.file_name,
                &plan.target.join(&artifact.file_name),
                &backup,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(after_backup)?;
    let sunrise = backup_sunrise_cache(&sunrise, &backup).map_err(after_backup)?;
    let headers = backup_package_header_caches(&headers, &backup).map_err(after_backup)?;
    verify_plan(plan)?;
    verify_targets_unchanged(&originals).map_err(after_backup)?;
    verify_sunrise_cache_unchanged(&sunrise).map_err(after_backup)?;
    verify_package_header_caches_unchanged(&headers).map_err(after_backup)?;
    let mut record = removal_record(plan, &backup);
    if let Some(cleanup) = &plan.cleanup {
        if prepare_account_cleanup(plan).map_err(InstallError::validation)? != *cleanup {
            return Err(after_backup(
                "Account data changed since review; review again".into(),
            ));
        }
        record.account_cleanup = Some(account::prepare(cleanup, &plan.target, &backup)?);
    }
    validate_install_transaction(&record, &plan.target)?;
    let journal = plan.target.join(INSTALL_TRANSACTION_FILE_NAME);
    write_install_transaction(&backup.join("uninstall-recovery.json"), &record)?;
    write_install_transaction(&journal, &record)?;
    let result = commit_removal(
        plan,
        &mut record,
        &journal,
        &sunrise,
        &headers,
        check,
        fail_after,
        cache_ops,
    );
    match result {
        Ok((invalidated_sunrise_cache, invalidated_package_header_caches)) => {
            remove_terminal_install_transaction(&journal);
            Ok(UninstallReport {
                removed_files: plan
                    .artifacts
                    .iter()
                    .map(|artifact| plan.target.join(&artifact.file_name))
                    .collect(),
                backup_directory: backup,
                invalidated_sunrise_cache,
                invalidated_package_header_caches,
                cleaned_account: plan
                    .cleanup
                    .as_ref()
                    .map(|cleanup| cleanup.settings_path.clone()),
            })
        }
        Err(error) => {
            let recovered = recover_interrupted_install_locked(&RecoveryRequest {
                target_packages_directory: plan.target.clone(),
                backup_root: root,
                game_running_check: check,
            });
            let message = match recovered {
                Ok(RecoveryOutcome::Recovered { .. }) => format!(
                    "Uninstall failed; the original custom package set was restored. {error}"
                ),
                Ok(_) => format!(
                    "The operation reached a terminal state, but finalization reported an error. Review the current package set before continuing. {error}"
                ),
                Err(recovery) => format!(
                    "Uninstall stopped. Recovery is still required: {recovery}. Original error: {error}"
                ),
            };
            Err(InstallError::after_backup(message, &backup))
        }
    }
}

fn removal_record(plan: &UninstallPlan, backup: &Path) -> InstallTransactionRecord {
    InstallTransactionRecord {
        schema: INSTALL_TRANSACTION_SCHEMA,
        state: InstallTransactionState::Pending,
        target_packages_directory: plan.target.clone(),
        backup_directory: backup.to_path_buf(),
        artifacts: plan
            .artifacts
            .iter()
            .enumerate()
            .map(|(index, artifact)| InstallTransactionArtifact {
                remove_target: true,
                file_name: artifact.file_name.clone(),
                authored: TransactionDigest::from(artifact),
                original: Some(TransactionDigest::from(artifact)),
                backup_file_name: Some(artifact.file_name.clone()),
                temporary_file_name: format!(
                    ".parhelion-install-uninstall-{}-{index}.tmp",
                    unique_token()
                ),
            })
            .collect(),
        account_cleanup: None,
    }
}

type RemovedCaches = (
    Option<InvalidatedSunriseCache>,
    Vec<InvalidatedPackageHeaderCache>,
);

#[allow(clippy::too_many_arguments)]
fn commit_removal(
    plan: &UninstallPlan,
    record: &mut InstallTransactionRecord,
    journal: &Path,
    sunrise: &OriginalSunriseCache,
    headers: &OriginalPackageHeaderCaches,
    check: GameRunningCheck,
    fail_after: Option<usize>,
    cache_ops: CacheInvalidationOps,
) -> Result<RemovedCaches, InstallError> {
    check_stopped(check)?;
    if let (Some(cleanup), Some(account)) = (&plan.cleanup, &record.account_cleanup) {
        account::commit(cleanup, account, &plan.target, &record.backup_directory)?;
    }
    for (index, artifact) in plan.artifacts.iter().enumerate().rev() {
        let path = plan.target.join(&artifact.file_name);
        if snapshot(&plan.target, &artifact.file_name)? != *artifact {
            return Err(InstallError::validation(
                "A custom package changed during uninstall",
            ));
        }
        fs::remove_file(&path).map_err(|error| InstallError::validation(error.to_string()))?;
        if fail_after == Some(plan.artifacts.len() - index) {
            return Err(InstallError::validation("Injected uninstall failure"));
        }
    }
    // Check the resulting set, not just the files we removed; detect concurrent additions.
    let after = preview_uninstall(&plan.target)?;
    if !after.artifacts.is_empty() || after.stock != plan.stock {
        return Err(InstallError::validation(
            "The package set changed during uninstall",
        ));
    }
    let sunrise = invalidate_sunrise_cache(sunrise, cache_ops).map_err(InstallError::validation)?;
    let headers =
        invalidate_package_header_caches(headers, cache_ops).map_err(InstallError::validation)?;
    account::verify_committed(record)?;
    record.state = InstallTransactionState::Committed;
    write_install_transaction(
        &record.backup_directory.join("uninstall-recovery.json"),
        record,
    )?;
    write_install_transaction(journal, record)?;
    Ok((sunrise, headers))
}

fn verify_plan(plan: &UninstallPlan) -> Result<(), InstallError> {
    let current = preview_uninstall(&plan.target)?;
    if current.artifacts != plan.artifacts || current.stock != plan.stock {
        return Err(InstallError::validation(
            "Packages changed since the uninstall review. Review the current set again; no files were removed.",
        ));
    }
    Ok(())
}

fn check_stopped(check: GameRunningCheck) -> Result<(), InstallError> {
    if check().map_err(InstallError::validation)? {
        return Err(InstallError::validation(
            "Close Destiny 2 before uninstalling custom packages",
        ));
    }
    Ok(())
}

pub(super) fn refresh_recovery_caches(
    record: &InstallTransactionRecord,
) -> Result<(), InstallError> {
    let sunrise = validate_sunrise_build_cache(&record.target_packages_directory)?;
    let headers = validate_package_header_caches(&record.target_packages_directory)?;
    let backup = record
        .backup_directory
        .join(format!("recovery-caches-{}", unique_token()));
    create_private_directory(&backup)
        .map_err(|error| InstallError::validation(error.to_string()))?;
    let sunrise = backup_sunrise_cache(&sunrise, &backup).map_err(InstallError::validation)?;
    let headers =
        backup_package_header_caches(&headers, &backup).map_err(InstallError::validation)?;
    invalidate_sunrise_cache(&sunrise, DEFAULT_CACHE_INVALIDATION_OPS)
        .map_err(InstallError::validation)?;
    invalidate_package_header_caches(&headers, DEFAULT_CACHE_INVALIDATION_OPS)
        .map_err(InstallError::validation)?;
    Ok(())
}
