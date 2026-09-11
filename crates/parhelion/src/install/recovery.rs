use super::*;

/// Reconcile and safely restore an interrupted package installation, if one is recorded.
pub fn recover_interrupted_install(
    request: &RecoveryRequest,
) -> Result<RecoveryOutcome, InstallError> {
    let _lock = lock_installation(&request.target_packages_directory)?;
    recover_interrupted_install_locked(request)
}

/// Caller holds the installation lock through recovery and any following mutation.
pub(super) fn recover_interrupted_install_locked(
    request: &RecoveryRequest,
) -> Result<RecoveryOutcome, InstallError> {
    let target_packages_directory =
        canonical_directory(&request.target_packages_directory, "target packages")?;
    let journal_path = target_packages_directory.join(INSTALL_TRANSACTION_FILE_NAME);
    match fs::symlink_metadata(&journal_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RecoveryOutcome::NoTransaction);
        }
        Err(error) => {
            return Err(InstallError::validation(format!(
                "Could not inspect package-install recovery record {}: {error}",
                journal_path.display()
            )));
        }
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(InstallError::validation(format!(
                "Package-install recovery record is not a regular file: {}",
                journal_path.display()
            )));
        }
        Ok(_) => {}
    }

    let mut transaction = read_install_transaction(&journal_path)?;
    validate_install_transaction(&transaction, &target_packages_directory)?;

    if transaction.state == InstallTransactionState::Committed {
        cleanup_transaction_temporary_files(&transaction, &target_packages_directory);
        remove_terminal_install_transaction(&journal_path);
        return Ok(RecoveryOutcome::CommittedTransaction);
    }
    if transaction.state == InstallTransactionState::Recovered {
        cleanup_transaction_temporary_files(&transaction, &target_packages_directory);
        remove_terminal_install_transaction(&journal_path);
        return Ok(RecoveryOutcome::Recovered {
            restored_files: Vec::new(),
            removed_new_files: Vec::new(),
        });
    }

    let backup_root = resolve_path_for_comparison(&request.backup_root).map_err(|error| {
        InstallError::validation(format!(
            "Could not resolve backup root {} during package recovery: {error}",
            request.backup_root.display()
        ))
    })?;
    validate_pending_transaction_backup_directory(&transaction, &backup_root)?;
    check_game_before_recovery(request.game_running_check)?;
    account::verify_account_recovery(&transaction)?;
    let recovery = recover_pending_transaction(
        &transaction,
        &target_packages_directory,
        request.game_running_check,
    )?;
    account::recover_account(&transaction, request.game_running_check)?;
    if transaction
        .artifacts
        .iter()
        .any(|artifact| artifact.remove_target)
    {
        uninstall::refresh_recovery_caches(&transaction)?;
    }
    transaction.state = InstallTransactionState::Recovered;
    write_install_transaction(&journal_path, &transaction)?;
    cleanup_transaction_temporary_files(&transaction, &target_packages_directory);
    remove_terminal_install_transaction(&journal_path);
    Ok(RecoveryOutcome::Recovered {
        restored_files: recovery.restored_files,
        removed_new_files: recovery.removed_new_files,
    })
}

pub(super) fn build_install_transaction(
    validated: &ValidatedRun,
    backup_directory: &Path,
    originals: &[OriginalArtifact],
    prepared: &[PreparedArtifact],
) -> Result<InstallTransactionRecord, String> {
    if originals.len() != prepared.len()
        || originals.len() != validated.artifacts.len() + validated.obsolete_artifacts.len()
    {
        return Err("Internal error: installation artifact sets disagree".to_owned());
    }
    for original in originals {
        if let (Some(backup_path), Some(expected)) = (&original.backup_path, &original.digest) {
            let metadata = fs::symlink_metadata(backup_path).map_err(|error| {
                format!(
                    "Could not inspect package backup {} before recording recovery: {error}",
                    backup_path.display()
                )
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!(
                    "Package backup is not a regular file: {}",
                    backup_path.display()
                ));
            }
            let current = digest_file(backup_path).map_err(|error| {
                format!(
                    "Could not verify package backup {} before recording recovery: {error}",
                    backup_path.display()
                )
            })?;
            if &current != expected {
                return Err(format!(
                    "Package backup changed before recovery metadata was recorded: {}",
                    backup_path.display()
                ));
            }
        }
    }
    let artifacts = originals
        .iter()
        .zip(prepared)
        .map(|(original, prepared)| {
            let temporary_file_name = prepared
                .temporary_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    format!(
                        "Install temporary path has no UTF-8 file name: {}",
                        prepared.temporary_path.display()
                    )
                })?;
            let backup_file_name = original
                .backup_path
                .as_ref()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .map(str::to_owned);
            Ok(InstallTransactionArtifact {
                remove_target: prepared.remove_target,
                file_name: original.file_name.clone(),
                authored: TransactionDigest::from(&prepared.manifest),
                original: original.digest.as_ref().map(TransactionDigest::from),
                backup_file_name,
                temporary_file_name: temporary_file_name.to_owned(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let transaction = InstallTransactionRecord {
        schema: INSTALL_TRANSACTION_SCHEMA,
        state: InstallTransactionState::Pending,
        target_packages_directory: validated.target_packages_directory.clone(),
        backup_directory: backup_directory.to_path_buf(),
        artifacts,
        account_cleanup: None,
        client_settings: None,
    };
    validate_install_transaction(&transaction, &validated.target_packages_directory)
        .map_err(|error| error.message)?;
    Ok(transaction)
}

pub(super) fn read_install_transaction(
    path: &Path,
) -> Result<InstallTransactionRecord, InstallError> {
    let file = File::open(path).map_err(|error| {
        InstallError::validation(format!(
            "Could not open package-install recovery record {}: {error}",
            path.display()
        ))
    })?;
    sundial::package_authoring::read_json(file).map_err(|error| {
        InstallError::validation(format!(
            "Could not parse package-install recovery record {}: {error}",
            path.display()
        ))
    })
}

pub(super) fn write_install_transaction(
    path: &Path,
    transaction: &InstallTransactionRecord,
) -> Result<(), InstallError> {
    let bytes = serde_json::to_vec_pretty(transaction).map_err(|error| {
        InstallError::validation(format!(
            "Could not serialize package-install recovery record: {error}"
        ))
    })?;
    sundial::package_authoring::replace_authoring_file(path, &bytes).map_err(|error| {
        InstallError::validation(format!(
            "Could not durably write package-install recovery record {}: {error}",
            path.display()
        ))
    })
}

pub(super) fn validate_install_transaction(
    transaction: &InstallTransactionRecord,
    target_packages_directory: &Path,
) -> Result<(), InstallError> {
    account::validate_account_record(transaction, target_packages_directory)?;
    if transaction.schema != INSTALL_TRANSACTION_SCHEMA {
        return Err(InstallError::validation(format!(
            "Unsupported package-install recovery schema {}; expected {}",
            transaction.schema, INSTALL_TRANSACTION_SCHEMA
        )));
    }
    let recorded_target =
        fs::canonicalize(&transaction.target_packages_directory).map_err(|error| {
            InstallError::validation(format!(
                "Could not resolve recorded package target {}: {error}",
                transaction.target_packages_directory.display()
            ))
        })?;
    if !paths_equal(&recorded_target, target_packages_directory) {
        return Err(InstallError::validation(
            "Package-install recovery record belongs to a different target directory",
        ));
    }
    let actual_names = transaction
        .artifacts
        .iter()
        .map(|artifact| artifact.file_name.as_str())
        .collect::<Vec<_>>();
    authored_packages_for_file_names(actual_names).map_err(|error| {
        InstallError::validation(format!(
            "Package-install recovery record has an invalid recipe-selected artifact set: {error}"
        ))
    })?;

    let mut temporary_names = BTreeSet::new();
    let uninstall = transaction
        .artifacts
        .iter()
        .all(|artifact| artifact.remove_target);
    for artifact in &transaction.artifacts {
        if artifact.remove_target
            && (artifact.original.as_ref() != Some(&artifact.authored)
                || (!uninstall
                    && authored_package_for_file_name(&artifact.file_name)
                        .is_none_or(|profile| profile.required_output)))
        {
            return Err(InstallError::validation(
                "Invalid package removal recovery metadata",
            ));
        }
        validate_plain_transaction_file_name(&artifact.file_name, "artifact")?;
        validate_transaction_digest(&artifact.authored, &artifact.file_name)?;
        if let Some(original) = &artifact.original {
            validate_transaction_digest(original, &artifact.file_name)?;
        }
        match (&artifact.original, &artifact.backup_file_name) {
            (Some(_), Some(backup_file_name)) if backup_file_name == &artifact.file_name => {}
            (None, None) => {}
            _ => {
                return Err(InstallError::validation(format!(
                    "Package-install recovery record has inconsistent backup metadata for {}",
                    artifact.file_name
                )));
            }
        }
        validate_plain_transaction_file_name(&artifact.temporary_file_name, "temporary")?;
        if !artifact
            .temporary_file_name
            .starts_with(".parhelion-install-")
            || !artifact.temporary_file_name.ends_with(".tmp")
        {
            return Err(InstallError::validation(format!(
                "Package-install recovery record has an invalid temporary file name: {}",
                artifact.temporary_file_name
            )));
        }
        if !temporary_names.insert(artifact.temporary_file_name.as_str()) {
            return Err(InstallError::validation(format!(
                "Package-install recovery record repeats temporary file {}",
                artifact.temporary_file_name
            )));
        }
    }
    Ok(())
}

pub(super) fn rollback_transaction(
    transaction: &InstallTransactionRecord,
    game_running_check: GameRunningCheck,
) -> RollbackReport {
    if let Err(error) = account::verify_account_recovery(transaction) {
        return RollbackReport {
            errors: vec![error.to_string()],
            ..Default::default()
        };
    }
    let mut report = match recover_pending_transaction(
        transaction,
        &transaction.target_packages_directory,
        game_running_check,
    ) {
        Ok(report) => report,
        Err(error) => RollbackReport {
            errors: vec![error.message],
            ..Default::default()
        },
    };
    if report.succeeded()
        && let Err(error) = account::recover_account(transaction, game_running_check)
    {
        report.errors.push(error.to_string());
    }
    report
}

pub(super) fn validate_pending_transaction_backup_directory(
    transaction: &InstallTransactionRecord,
    backup_root: &Path,
) -> Result<(), InstallError> {
    let backup_directory = fs::canonicalize(&transaction.backup_directory).map_err(|error| {
        InstallError::validation(format!(
            "Could not resolve recorded package backup {}: {error}",
            transaction.backup_directory.display()
        ))
    })?;
    if !path_is_within(&backup_directory, backup_root)
        || paths_equal(&backup_directory, backup_root)
    {
        return Err(InstallError::validation(format!(
            "Recorded package backup is outside the selected backup root: {}",
            backup_directory.display()
        )));
    }
    if !paths_equal(&backup_directory, &transaction.backup_directory) {
        return Err(InstallError::validation(
            "Recorded package backup path is not canonical",
        ));
    }
    Ok(())
}

pub(super) fn validate_plain_transaction_file_name(
    file_name: &str,
    label: &str,
) -> Result<(), InstallError> {
    let mut components = Path::new(file_name).components();
    if file_name.is_empty()
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(InstallError::validation(format!(
            "Package-install recovery record {label} name is not a plain file name: {file_name}"
        )));
    }
    Ok(())
}

pub(super) fn validate_transaction_digest(
    digest: &TransactionDigest,
    file_name: &str,
) -> Result<(), InstallError> {
    if digest.byte_length == 0 || !is_sha256(&digest.sha256) {
        return Err(InstallError::validation(format!(
            "Package-install recovery record has an invalid digest for {file_name}"
        )));
    }
    Ok(())
}

pub(super) fn check_game_before_recovery(
    game_running_check: GameRunningCheck,
) -> Result<(), InstallError> {
    let game_is_running = game_running_check().map_err(|error| {
        InstallError::validation(format!(
            "Could not check whether the game is running before package recovery: {error}"
        ))
    })?;
    if game_is_running {
        return Err(InstallError::validation(
            "The game is running; close it before recovering an interrupted package installation",
        ));
    }
    Ok(())
}

pub(super) fn recover_pending_transaction(
    transaction: &InstallTransactionRecord,
    target_packages_directory: &Path,
    game_running_check: GameRunningCheck,
) -> Result<RollbackReport, InstallError> {
    let mut states = Vec::with_capacity(transaction.artifacts.len());
    for artifact in &transaction.artifacts {
        states.push(reconcile_transaction_target(
            target_packages_directory,
            artifact,
        )?);
        if let (Some(original), Some(backup_file_name)) =
            (&artifact.original, &artifact.backup_file_name)
        {
            let backup_path = transaction.backup_directory.join(backup_file_name);
            reject_recovery_backup(&backup_path, original)?;
        }
    }

    // Reconcile the complete set a second time immediately before mutation. This ensures an
    // externally changed target blocks recovery before any member of the set is restored.
    for (artifact, expected_state) in transaction.artifacts.iter().zip(&states) {
        let current_state = reconcile_transaction_target(target_packages_directory, artifact)?;
        if &current_state != expected_state {
            return Err(InstallError::validation(format!(
                "Package {} changed while interrupted-install recovery was being prepared",
                artifact.file_name
            )));
        }
    }
    check_game_before_recovery(game_running_check)?;

    let mut report = RollbackReport::default();
    for (artifact, state) in transaction.artifacts.iter().zip(states).rev() {
        if state == ReconciledTarget::Original {
            continue;
        }
        let target_path = target_packages_directory.join(&artifact.file_name);
        if artifact.remove_target {
            if !matches!(fs::symlink_metadata(&target_path), Err(error) if error.kind() == io::ErrorKind::NotFound)
            {
                return Err(InstallError::validation(
                    "A removed package reappeared during uninstall recovery; no overwrite was attempted",
                ));
            }
        } else {
            verify_recovery_target_is_authored(&target_path, &artifact.authored)?;
        }
        match (&artifact.original, &artifact.backup_file_name) {
            (Some(original), Some(backup_file_name)) => {
                let backup_path = transaction.backup_directory.join(backup_file_name);
                restore_recovery_backup(&backup_path, &target_path, original)?;
                report.restored_files.push(target_path);
            }
            (None, None) => {
                fs::remove_file(&target_path).map_err(|error| {
                    InstallError::validation(format!(
                        "Could not remove interrupted newly installed package {}: {error}",
                        target_path.display()
                    ))
                })?;
                report.removed_new_files.push(target_path);
            }
            _ => unreachable!("validated transaction backup metadata"),
        }
    }
    for artifact in &transaction.artifacts {
        verify_recovered_target(target_packages_directory, artifact)?;
    }
    Ok(report)
}

pub(super) fn reconcile_transaction_target(
    target_packages_directory: &Path,
    artifact: &InstallTransactionArtifact,
) -> Result<ReconciledTarget, InstallError> {
    let target_path = target_packages_directory.join(&artifact.file_name);
    let metadata = match fs::symlink_metadata(&target_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound && artifact.remove_target => {
            return Ok(ReconciledTarget::Authored);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound && artifact.original.is_none() => {
            return Ok(ReconciledTarget::Original);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(InstallError::validation(format!(
                "Refusing interrupted-install recovery because {} is missing",
                target_path.display()
            )));
        }
        Err(error) => {
            return Err(InstallError::validation(format!(
                "Could not inspect {} during interrupted-install recovery: {error}",
                target_path.display()
            )));
        }
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(InstallError::validation(format!(
            "Refusing interrupted-install recovery because {} is not a regular file",
            target_path.display()
        )));
    }
    let current = digest_file(&target_path).map_err(|error| {
        InstallError::validation(format!(
            "Could not digest {} during interrupted-install recovery: {error}",
            target_path.display()
        ))
    })?;
    if artifact
        .original
        .as_ref()
        .is_some_and(|original| original == &current)
    {
        Ok(ReconciledTarget::Original)
    } else if artifact.authored == current {
        Ok(ReconciledTarget::Authored)
    } else {
        Err(InstallError::validation(format!(
            "Refusing interrupted-install recovery because {} matches neither the recorded original nor authored package digest",
            target_path.display()
        )))
    }
}

pub(super) fn reject_recovery_backup(
    backup_path: &Path,
    expected: &TransactionDigest,
) -> Result<(), InstallError> {
    let metadata = fs::symlink_metadata(backup_path).map_err(|error| {
        InstallError::validation(format!(
            "Could not inspect recorded package backup {}: {error}",
            backup_path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(InstallError::validation(format!(
            "Recorded package backup is not a regular file: {}",
            backup_path.display()
        )));
    }
    let digest = digest_file(backup_path).map_err(|error| {
        InstallError::validation(format!(
            "Could not verify recorded package backup {}: {error}",
            backup_path.display()
        ))
    })?;
    if expected != &digest {
        return Err(InstallError::validation(format!(
            "Recorded package backup no longer matches its original digest: {}",
            backup_path.display()
        )));
    }
    Ok(())
}

pub(super) fn verify_recovery_target_is_authored(
    target_path: &Path,
    expected: &TransactionDigest,
) -> Result<(), InstallError> {
    let digest = digest_file(target_path).map_err(|error| {
        InstallError::validation(format!(
            "Could not recheck {} immediately before recovery: {error}",
            target_path.display()
        ))
    })?;
    if expected != &digest {
        return Err(InstallError::validation(format!(
            "Refusing to overwrite {} because it changed during interrupted-install recovery",
            target_path.display()
        )));
    }
    Ok(())
}

pub(super) fn restore_recovery_backup(
    backup_path: &Path,
    target_path: &Path,
    expected: &TransactionDigest,
) -> Result<(), InstallError> {
    reject_recovery_backup(backup_path, expected)?;
    sundial::package_authoring::replace_file_from_path_atomically(backup_path, target_path)
        .map_err(|error| {
            InstallError::validation(format!(
                "Could not restore interrupted package {}: {error}",
                target_path.display()
            ))
        })?;
    let restored = digest_file(target_path).map_err(|error| {
        InstallError::validation(format!(
            "Could not verify restored package {}: {error}",
            target_path.display()
        ))
    })?;
    if expected != &restored {
        return Err(InstallError::validation(format!(
            "Restored package failed digest verification: {}",
            target_path.display()
        )));
    }
    Ok(())
}

pub(super) fn cleanup_transaction_temporary_files(
    transaction: &InstallTransactionRecord,
    target_packages_directory: &Path,
) {
    for artifact in &transaction.artifacts {
        remove_file_if_present(&target_packages_directory.join(&artifact.temporary_file_name));
    }
}

pub(super) fn remove_terminal_install_transaction(journal_path: &Path) {
    remove_file_if_present(journal_path);
}
