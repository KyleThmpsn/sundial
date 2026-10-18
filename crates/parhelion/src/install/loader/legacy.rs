//! Recovers an older package transaction on a mod loader runtime. A pending one restores its
//! recorded account and a committed one keeps its account. Either way its packages are retired with
//! a verified backup, never restored into stock packages.
use super::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recovery {
    context: InstallTransactionRecord,
    files: BTreeMap<String, TransactionDigest>,
}

impl Recovery {
    pub(super) fn validate(&self, target: &Path, backup_root: &Path) -> Result<(), InstallError> {
        validate_install_transaction(&self.context, target)?;
        validate_pending_transaction_backup_directory(&self.context, backup_root)?;
        mod_content::checked_path(backup_root, &self.context.backup_directory)
            .map_err(InstallError::validation)?;
        if path_is_within(&self.context.backup_directory, target)
            || path_is_within(
                &self.context.backup_directory,
                &mod_content::runtime_directory(target.parent().unwrap()),
            )
            || !self.files.contains_key(INSTALL_TRANSACTION_FILE_NAME)
        {
            return Err(InstallError::validation(
                "Invalid legacy recovery backup or journal inventory",
            ));
        }
        for (name, digest) in &self.files {
            if name != INSTALL_TRANSACTION_FILE_NAME
                && !self
                    .context
                    .artifacts
                    .iter()
                    .any(|a| a.temporary_file_name == *name && a.authored == *digest)
            {
                return Err(InstallError::validation(
                    "Unexpected legacy recovery cleanup file",
                ));
            }
            validate_transaction_digest(digest, name)?;
        }
        Ok(())
    }

    pub(super) fn verify_files(&self, target: &Path, terminal: bool) -> Result<(), InstallError> {
        for (name, digest) in &self.files {
            let path = target.join(name);
            mod_content::checked_path(target, &path).map_err(InstallError::validation)?;
            if path.try_exists().map_err(failure)? {
                verify_file(&path, digest)?;
            } else if name == INSTALL_TRANSACTION_FILE_NAME && !terminal {
                return Err(InstallError::validation(
                    "The legacy recovery journal disappeared",
                ));
            }
        }
        for artifact in &self.context.artifacts {
            mod_content::checked_path(target, &target.join(&artifact.temporary_file_name))
                .map_err(InstallError::validation)?;
            if !self.files.contains_key(&artifact.temporary_file_name)
                && target
                    .join(&artifact.temporary_file_name)
                    .try_exists()
                    .map_err(failure)?
            {
                return Err(InstallError::validation(
                    "A legacy temporary file appeared during recovery",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn verify_account(&self) -> Result<(), InstallError> {
        if self.context.state == InstallTransactionState::Committed {
            account::verify_committed(&self.context)
        } else {
            account::verify_account_recovery(&self.context)
        }
    }

    pub(super) fn apply_account(&self, check: GameRunningCheck) -> Result<(), InstallError> {
        if self.context.state == InstallTransactionState::Committed {
            account::verify_committed(&self.context)
        } else {
            account::recover_account(&self.context, check)
        }
    }

    pub(super) fn verify_finished_account(&self) -> Result<(), InstallError> {
        if self.context.state == InstallTransactionState::Committed {
            account::verify_committed(&self.context)
        } else {
            account::verify_recovered(&self.context)
        }
    }

    pub(super) fn cleanup(&self, target: &Path) -> Result<(), InstallError> {
        self.verify_files(target, true)?;
        // The durable mod journal remains until every old temporary and journal is removed.
        for (name, digest) in &self.files {
            let path = target.join(name);
            if path.try_exists().map_err(failure)? {
                verify_file(&path, digest)?;
                fs::remove_file(path).map_err(failure)?;
            }
        }
        Ok(())
    }
}

pub(in crate::install) fn recover_with_observer(
    request: &RecoveryRequest,
    observer: &mut dyn FnMut(&str),
) -> Result<Option<RecoveryOutcome>, InstallError> {
    let target = canonical_packages_directory(&request.target_packages_directory)?;
    let journal = target.join(INSTALL_TRANSACTION_FILE_NAME);
    if !journal.try_exists().map_err(failure)? {
        return Ok(None);
    }
    let root = target.parent().unwrap();
    let runtime = (request.runtime_snapshot_check)(root).map_err(InstallError::validation)?;
    if !runtime
        .uses_mod_loader()
        .map_err(InstallError::validation)?
    {
        return Ok(None);
    }
    check_stopped(request.game_running_check)?;
    mod_content::checked_path(root, &journal).map_err(InstallError::validation)?;
    mod_content::ensure_resolved_environment(root).map_err(InstallError::validation)?;
    verify_folder(&mod_content::directory(root), None)?;
    let recovery = capture(&target)?;
    if recovery
        .context
        .runtime
        .as_ref()
        .is_some_and(|old| old.brand() != runtime.brand())
        && (recovery.context.account_cleanup.is_some()
            || recovery.context.client_settings.is_some())
    {
        return Err(InstallError::validation(
            "The interrupted operation belongs to a different runtime's account. Its files and backups were preserved for review",
        ));
    }
    let backups = canonical_directory(&request.backup_root, "legacy recovery backup root")?;
    recovery.validate(&target, &backups)?;
    recovery.verify_account()?;
    // The interrupted operation's packages patch stock packages with added entries, which the mod
    // loader refuses, so recovery retires them with a verified backup rather than publishing
    // them. Checking its sources still confirms every recorded backup is intact.
    let (_, present) = sources(&target, &recovery.context)?;
    validate_target_package_chain(&target)?;
    let stock = discover_target_source_artifact_names(&target)?
        .iter()
        .map(|name| metadata(&target.join(name), name))
        .collect::<Result<Vec<_>, _>>()?;
    let mut transaction = transaction::prepare(transaction::Plan {
        target: &target,
        backup_root: &backups,
        original: None,
        legacy: present,
        stock,
        incoming: None,
        runtime: &runtime,
    })?;
    recovery.verify_files(&target, false)?;
    let copied = copy_file_create_new(
        &journal,
        &transaction
            .record
            .context
            .backup_directory
            .join("legacy-install-transaction.json"),
    )
    .map_err(failure)?;
    if recovery.files[INSTALL_TRANSACTION_FILE_NAME] != copied {
        return Err(InstallError::validation(
            "The legacy journal changed while backing it up",
        ));
    }
    transaction.record.legacy_recovery = Some(recovery);
    transaction::commit(
        &mut transaction,
        request.game_running_check,
        observer,
        |_| Ok(()),
    )?;
    Ok(Some(RecoveryOutcome::CommittedTransaction))
}

fn capture(target: &Path) -> Result<Recovery, InstallError> {
    let journal = target.join(INSTALL_TRANSACTION_FILE_NAME);
    let mut bytes = Vec::new();
    File::open(&journal)
        .map_err(failure)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    if bytes.len() > 1024 * 1024 {
        return Err(InstallError::validation(
            "The legacy recovery journal exceeds its size limit",
        ));
    }
    let context: InstallTransactionRecord =
        sundial::package_authoring::read_json(bytes.as_slice()).map_err(failure)?;
    validate_install_transaction(&context, target)?;
    let digest = TransactionDigest {
        byte_length: bytes.len() as u64,
        sha256: hex::encode_upper(Sha256::digest(&bytes)),
    };
    verify_file(&journal, &digest)?;
    let mut files = BTreeMap::from([(INSTALL_TRANSACTION_FILE_NAME.into(), digest)]);
    for artifact in &context.artifacts {
        let path = target.join(&artifact.temporary_file_name);
        if path.try_exists().map_err(failure)? {
            verify_file(&path, &artifact.authored)?;
            files.insert(
                artifact.temporary_file_name.clone(),
                artifact.authored.clone(),
            );
        }
    }
    Ok(Recovery { context, files })
}

type Sources = Vec<(ArtifactMetadata, PathBuf)>;

fn sources(
    target: &Path,
    context: &InstallTransactionRecord,
) -> Result<(Sources, Vec<ArtifactMetadata>), InstallError> {
    let mut sources = Vec::new();
    let mut present = Vec::new();
    for artifact in &context.artifacts {
        reconcile_transaction_target(target, artifact)?;
        let target_path = target.join(&artifact.file_name);
        if target_path.try_exists().map_err(failure)? {
            present.push(metadata(&target_path, &artifact.file_name)?);
        }
        let source = recovered_source(context, artifact, target_path)?;
        if let Some(path) = source {
            sources.push((metadata(&path, &artifact.file_name)?, path));
        }
    }
    // Unrecorded authored files could be another generation. Do not silently adopt them.
    for profile in all_authored_packages_in(&[target]) {
        if target
            .join(profile.file_name)
            .try_exists()
            .map_err(failure)?
            && !context
                .artifacts
                .iter()
                .any(|a| a.file_name == profile.file_name)
        {
            return Err(InstallError::validation(
                "An unrecorded authored package appeared during legacy recovery",
            ));
        }
    }
    Ok((sources, present))
}

fn recovered_source(
    context: &InstallTransactionRecord,
    artifact: &InstallTransactionArtifact,
    target: PathBuf,
) -> Result<Option<PathBuf>, InstallError> {
    if context.state == InstallTransactionState::Committed {
        if artifact.remove_target {
            if target.try_exists().map_err(failure)? {
                return Err(InstallError::validation(
                    "A removed legacy package reappeared after commit",
                ));
            }
            return Ok(None);
        }
        verify_file(&target, &artifact.authored)?;
        return Ok(Some(target));
    }
    if let Some(original) = &artifact.original {
        let path = context
            .backup_directory
            .join(artifact.backup_file_name.as_ref().unwrap());
        verify_file(&path, original)?;
        return Ok(Some(path));
    }
    Ok(None)
}

fn metadata(path: &Path, name: &str) -> Result<ArtifactMetadata, InstallError> {
    reject_symlink(path, "legacy recovery source")?;
    let digest = digest_file(path).map_err(failure)?;
    Ok(ArtifactMetadata {
        file_name: name.into(),
        byte_length: digest.byte_length,
        sha256: digest.sha256,
    })
}
