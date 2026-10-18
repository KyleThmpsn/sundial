use super::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    schema: u32,
    pub state: InstallTransactionState,
    pub context: InstallTransactionRecord,
    stock: Vec<ArtifactMetadata>,
    legacy: Vec<ArtifactMetadata>,
    original: Option<Folder>,
    incoming: Option<Folder>,
    token: String,
    #[serde(default)]
    pub(super) legacy_recovery: Option<legacy::Recovery>,
}

pub(super) struct Transaction {
    pub record: Record,
    pub sunrise: OriginalSunriseCache,
    pub headers: OriginalPackageHeaderCaches,
}

impl Record {
    fn install(&self) -> &Path {
        self.context
            .target_packages_directory
            .parent()
            .expect("validated installation")
    }
    fn live(&self) -> PathBuf {
        mod_content::directory(self.install())
    }
    fn temporary(&self, kind: &str) -> PathBuf {
        self.live()
            .parent()
            .unwrap()
            .join(format!(".parhelion-{kind}-{}", self.token))
    }
    fn journal(&self) -> PathBuf {
        mod_content::runtime_directory(self.install()).join(mod_content::JOURNAL)
    }
    fn write(&self) -> Result<(), InstallError> {
        write_json(&self.journal(), self)
    }
    fn migration(&self) -> bool {
        !self.legacy.is_empty() || self.legacy_recovery.is_some()
    }
    /// Whether the recorded runtime loads the mod folder. On Dawn or Sunrise before 0.6 the only
    /// operation is removing the folder, which changes nothing those runtimes read.
    fn loads_mods(&self) -> Result<bool, InstallError> {
        self.context
            .runtime
            .as_ref()
            .expect("a mod operation records its runtime")
            .uses_mod_loader()
            .map_err(InstallError::validation)
    }

    fn verify_finished_account(&self) -> Result<(), InstallError> {
        if let Some(legacy) = &self.legacy_recovery {
            legacy.verify_finished_account()?;
        }
        account::verify_committed(&self.context)
    }

    /// Studio and other editors do not take Parhelion's lock. Recheck their packages before
    /// publishing and before recording success. Removal and rollback never depend on them.
    fn verify_coexistence(&self, incoming_directory: &Path) -> Result<(), InstallError> {
        let Some(incoming) = &self.incoming else {
            return Ok(());
        };
        mod_content::ensure_resolved_environment(self.install())
            .map_err(InstallError::validation)?;
        let sources = artifacts(incoming)
            .into_iter()
            .map(|artifact| {
                let path = incoming_directory.join(package_path(&artifact)?);
                Ok((artifact, path))
            })
            .collect::<Result<Vec<_>, InstallError>>()?;
        reject_installed_conflicts(
            &self.context.target_packages_directory,
            &sources,
            &self.legacy,
        )?;
        check_other_mods(
            &self.context.target_packages_directory,
            &sources,
            &self.legacy,
        )?;
        Ok(())
    }
}

pub(super) struct Plan<'a> {
    pub target: &'a Path,
    pub backup_root: &'a Path,
    pub original: Option<Folder>,
    pub legacy: Vec<ArtifactMetadata>,
    pub stock: Vec<ArtifactMetadata>,
    pub incoming: Option<&'a [(ArtifactMetadata, PathBuf)]>,
    pub runtime: &'a RuntimeSnapshot,
}

pub(super) fn prepare(plan: Plan<'_>) -> Result<Transaction, InstallError> {
    let Plan {
        target,
        backup_root,
        original,
        legacy,
        stock,
        incoming,
        runtime,
    } = plan;
    // Only publishing needs a runtime that loads the mod folder. Removal works on any runtime.
    if incoming.is_some()
        && !runtime
            .uses_mod_loader()
            .map_err(InstallError::validation)?
    {
        return Err(InstallError::validation(
            "Installing to the mod folder requires Sunrise 0.6 or later. The runtime changed during review",
        ));
    }
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    let mods = mod_content::runtime_directory(install).join("mods");
    mod_content::checked_path(install, &mods).map_err(InstallError::validation)?;
    let root = resolve_path_for_comparison(backup_root).map_err(failure)?;
    if path_is_within(&root, target)
        || path_is_within(&root, &mod_content::runtime_directory(install))
    {
        return Err(InstallError::validation(
            "The mod backup must be outside packages and the Sunrise runtime directory",
        ));
    }
    let sunrise =
        validate_sunrise_build_cache_with(target, sundial::package_authoring::installed_runtime)?;
    let headers = validate_package_header_caches(target)?;
    if sunrise.runtime != *runtime {
        return Err(InstallError::validation(
            "Runtime changed before mod preparation",
        ));
    }
    fs::create_dir_all(&root).map_err(failure)?;
    let root = canonical_directory(&root, "mod backup root")?;
    let token = unique_token();
    let backup = root.join(format!("parhelion-mod-backup-{token}"));
    create_private_directory(&backup).map_err(failure)?;
    let backup = canonical_directory(&backup, "mod backup")?;
    if !path_is_within(&backup, &root)
        || path_is_within(&backup, target)
        || path_is_within(&backup, &mod_content::runtime_directory(install))
    {
        return Err(InstallError::validation(
            "The resolved mod backup escaped its selected location",
        ));
    }
    if let Some(folder) = &original {
        copy_folder(
            &mod_content::directory(install),
            &backup.join("mod"),
            folder,
        )?;
    }
    if !legacy.is_empty() {
        fs::create_dir(backup.join("legacy")).map_err(failure)?;
    }
    for artifact in &legacy {
        let copied = copy_file_create_new(
            &target.join(&artifact.file_name),
            &backup.join("legacy").join(&artifact.file_name),
        )
        .map_err(failure)?;
        if TransactionDigest::from(artifact) != copied {
            return Err(InstallError::validation(
                "Legacy package changed during backup",
            ));
        }
    }
    let sunrise = backup_sunrise_cache(&sunrise, &backup).map_err(InstallError::validation)?;
    let headers =
        backup_package_header_caches(&headers, &backup).map_err(InstallError::validation)?;
    fs::create_dir_all(&mods).map_err(failure)?;
    mod_content::checked_path(install, &mods).map_err(InstallError::validation)?;
    let mut record = Record {
        schema: 1,
        state: InstallTransactionState::Pending,
        stock,
        legacy,
        original,
        incoming: None,
        token,
        legacy_recovery: None,
        context: InstallTransactionRecord {
            schema: 1,
            state: InstallTransactionState::Pending,
            target_packages_directory: target.to_owned(),
            backup_directory: backup,
            artifacts: vec![],
            runtime: Some(runtime.clone()),
            account_cleanup: None,
            client_settings: None,
        },
    };
    if let Some(sources) = incoming {
        let staging = record.temporary("stage");
        create_private_directory(&staging).map_err(failure)?;
        fs::create_dir(staging.join("packages")).map_err(failure)?;
        fs::create_dir(staging.join("patches")).map_err(failure)?;
        let mut receipt = mod_content::Receipt {
            schema: 1,
            mod_id: mod_content::ID.into(),
            artifacts: vec![],
        };
        for (artifact, source) in sources {
            let path = package_path(artifact)?;
            let copied = copy_file_create_new(source, &staging.join(&path)).map_err(failure)?;
            if TransactionDigest::from(artifact) != copied {
                return Err(InstallError::validation(
                    "Package changed while staging its mod folder",
                ));
            }
            receipt.artifacts.push(mod_content::Artifact {
                path,
                byte_length: copied.byte_length,
                sha256: copied.sha256,
            });
        }
        receipt.validate().map_err(InstallError::validation)?;
        write_json(
            &staging.join(mod_content::MANIFEST),
            &mod_content::Manifest::default(),
        )?;
        write_json(&staging.join(mod_content::RECEIPT), &receipt)?;
        record.incoming = Some(snapshot_folder(&staging)?);
        sync_backup_directories(&staging, &mods).map_err(InstallError::validation)?;
    }
    // Account recovery uses the existing account journal contract. Its package rows serve only
    // as evidence that an account edit belongs to a nonempty content operation.
    let involved = record
        .incoming
        .as_ref()
        .or(record.original.as_ref())
        .map(artifacts)
        .unwrap_or_else(|| record.legacy.clone());
    record.context.artifacts = involved
        .iter()
        .map(|a| InstallTransactionArtifact {
            remove_target: false,
            file_name: a.file_name.clone(),
            authored: TransactionDigest::from(a),
            original: None,
            backup_file_name: None,
            temporary_file_name: format!(".parhelion-{}.tmp", record.token),
        })
        .collect();
    Ok(Transaction {
        record,
        sunrise,
        headers,
    })
}

pub(super) fn commit(
    transaction: &mut Transaction,
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
    let record = &mut transaction.record;
    check_stopped(check)?;
    for path in [
        record.live(),
        record.temporary("stage"),
        record.temporary("retired"),
    ] {
        mod_content::checked_path(record.install(), &path).map_err(InstallError::validation)?;
    }
    verify_source_artifacts(&record.context.target_packages_directory, &record.stock)?;
    if record.incoming.is_some() {
        mod_content::ensure_resolved_environment(record.install())
            .map_err(InstallError::validation)?;
    }
    verify_folder(&record.live(), record.original.as_ref())?;
    if let Some(legacy) = &record.legacy_recovery {
        legacy.verify_files(&record.context.target_packages_directory, false)?;
        legacy.verify_account()?;
    }
    for artifact in &record.legacy {
        verify_file(
            &record
                .context
                .target_packages_directory
                .join(&artifact.file_name),
            &TransactionDigest::from(artifact),
        )?;
    }
    verify_sunrise_cache_unchanged(&transaction.sunrise).map_err(InstallError::validation)?;
    verify_package_header_caches_unchanged(&transaction.headers)
        .map_err(InstallError::validation)?;
    account::validate_account_record(&record.context, &record.context.target_packages_directory)?;
    write_json(
        &record.context.backup_directory.join("mod-recovery.json"),
        record,
    )?;
    sync_backup_directories(
        &record.context.backup_directory,
        record.context.backup_directory.parent().unwrap(),
    )
    .map_err(InstallError::validation)?;
    record.write()?;
    observer("journaled");
    check_stopped(check)?;
    sundial::package_authoring::verify_installed_runtime(
        record.install(),
        record.context.runtime.as_ref().unwrap(),
    )
    .map_err(InstallError::validation)?;
    verify_source_artifacts(&record.context.target_packages_directory, &record.stock)?;
    verify_legacy(record, false)?;
    record.verify_coexistence(&record.temporary("stage"))?;
    apply_account(&record.context)?;
    if let Some(legacy) = &record.legacy_recovery {
        legacy.apply_account(check)?;
    }
    if record.original.is_some() {
        verify_folder(&record.live(), record.original.as_ref())?;
        rename_absent(&record.live(), &record.temporary("retired"))?;
        observer("retired");
    }
    if record.incoming.is_some() {
        verify_folder(&record.temporary("stage"), record.incoming.as_ref())?;
        rename_absent(&record.temporary("stage"), &record.live())?;
        observer("published");
    }
    remove_legacy(record, false, observer)?;
    if !record.legacy.is_empty() {
        observer("legacy_removed");
    }
    verify_folder(&record.live(), record.incoming.as_ref())?;
    verify_source_artifacts(&record.context.target_packages_directory, &record.stock)?;
    record.verify_finished_account()?;
    let (sunrise, headers) = if record.loads_mods()? {
        (
            invalidate_sunrise_cache(&transaction.sunrise, DEFAULT_CACHE_INVALIDATION_OPS)
                .map_err(InstallError::validation)?,
            invalidate_package_header_caches(&transaction.headers, DEFAULT_CACHE_INVALIDATION_OPS)
                .map_err(InstallError::validation)?,
        )
    } else {
        (None, Vec::new())
    };
    observer("verified");
    record.verify_coexistence(&record.live())?;
    record.state = InstallTransactionState::Committed;
    record.write()?;
    observer("committed");
    write_json(
        &record.context.backup_directory.join("mod-recovery.json"),
        record,
    )?;
    cleanup(record)?;
    Ok((sunrise, headers))
}

pub(in crate::install) fn recover(
    request: &RecoveryRequest,
) -> Result<Option<RecoveryOutcome>, InstallError> {
    let target = canonical_packages_directory(&request.target_packages_directory)?;
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    let journal = mod_content::runtime_directory(install).join(mod_content::JOURNAL);
    mod_content::checked_path(install, &journal).map_err(InstallError::validation)?;
    if !journal.try_exists().map_err(failure)? {
        return Ok(None);
    }
    let mut record: Record = mod_content::read_json(&journal).map_err(InstallError::validation)?;
    if target.join(INSTALL_TRANSACTION_FILE_NAME).exists() && record.legacy_recovery.is_none() {
        return Err(InstallError::validation(
            "Both legacy and mod-folder recovery journals are present. Review the installation before recovering either operation",
        ));
    }
    validate_record(&record, &target, &request.backup_root)?;
    check_stopped(request.game_running_check)?;
    sundial::package_authoring::verify_installed_runtime(
        install,
        record.context.runtime.as_ref().unwrap(),
    )
    .map_err(InstallError::validation)?;
    verify_source_artifacts(&target, &record.stock)?;
    if let Some(legacy) = &record.legacy_recovery {
        legacy.verify_files(&target, record.state == InstallTransactionState::Committed)?;
        legacy.verify_account()?;
    }
    if record.state == InstallTransactionState::Committed {
        verify_folder(&record.live(), record.incoming.as_ref())?;
        for artifact in &record.legacy {
            if target.join(&artifact.file_name).exists() {
                return Err(InstallError::validation(
                    "A legacy package reappeared after the upgrade",
                ));
            }
        }
        record.verify_finished_account()?;
        write_json(
            &record.context.backup_directory.join("mod-recovery.json"),
            &record,
        )?;
        cleanup(&record)?;
        return Ok(Some(RecoveryOutcome::CommittedTransaction));
    }
    if record.state == InstallTransactionState::Recovered {
        verify_folder(&record.live(), record.original.as_ref())?;
        verify_legacy(&record, false)?;
        cleanup(&record)?;
        return Ok(Some(RecoveryOutcome::Recovered {
            restored_files: vec![],
            removed_new_files: vec![],
        }));
    }
    // Validate every file and account before any rollback. Unknown concurrent content stays put.
    verify_source_artifacts(&target, &record.stock)?;
    verify_legacy(&record, true)?;
    account::verify_account_recovery(&record.context)?;
    if let Some(original) = &record.original {
        verify_folder(&record.context.backup_directory.join("mod"), Some(original))?;
    }
    for artifact in &record.legacy {
        verify_file(
            &record
                .context
                .backup_directory
                .join("legacy")
                .join(&artifact.file_name),
            &TransactionDigest::from(artifact),
        )?;
        let path = target.join(&artifact.file_name);
        if path.exists() {
            verify_file(&path, &TransactionDigest::from(artifact))?;
        }
    }
    for (kind, expected) in [
        ("stage", record.incoming.as_ref()),
        ("retired", record.original.as_ref()),
        ("rollback", record.incoming.as_ref()),
    ] {
        let path = record.temporary(kind);
        if path.exists() {
            verify_folder(&path, expected)?;
        }
    }
    let current = if record.live().exists() {
        Some(snapshot_folder(&record.live())?)
    } else {
        None
    };
    if current.is_some() && current != record.original && current != record.incoming {
        return Err(InstallError::validation(
            "Managed content changed after interruption. Recovery preserved the changed files",
        ));
    }
    if record.migration() {
        // Commit writes the account before any package moves. A migration whose account change
        // was not written yet moved nothing, so recovery restores the account and keeps every
        // legacy package where it is. Once it was written, the migration finishes forward.
        if record.legacy_recovery.is_none() && account::verify_committed(&record.context).is_err() {
            return roll_back_unstarted_migration(&mut record, request);
        }
        complete_migration(&mut record, request)?;
        return Ok(Some(RecoveryOutcome::CommittedTransaction));
    }
    check_stopped(request.game_running_check)?;
    let mut removed = vec![];
    if current != record.original {
        if current.is_some() {
            rename_absent(&record.live(), &record.temporary("rollback"))?;
            removed.push(record.live());
        }
        if let Some(original) = &record.original {
            if record.temporary("retired").exists() {
                rename_absent(&record.temporary("retired"), &record.live())?;
            } else {
                copy_folder(
                    &record.context.backup_directory.join("mod"),
                    &record.temporary("restore"),
                    original,
                )?;
                rename_absent(&record.temporary("restore"), &record.live())?;
            }
        }
    }
    account::recover_account(&record.context, request.game_running_check)?;
    uninstall::refresh_recovery_caches(&record.context, request.runtime_snapshot_check)?;
    verify_folder(&record.live(), record.original.as_ref())?;
    record.state = InstallTransactionState::Recovered;
    record.write()?;
    cleanup(&record)?;
    Ok(Some(RecoveryOutcome::Recovered {
        restored_files: vec![],
        removed_new_files: removed,
    }))
}

fn validate_record(record: &Record, target: &Path, backup_root: &Path) -> Result<(), InstallError> {
    if record.schema != 1
        || !paths_equal(&record.context.target_packages_directory, target)
        || record.context.runtime.is_none()
        || record.token.is_empty()
        || !record
            .token
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || b == b'-')
    {
        return Err(InstallError::validation("Unsupported mod recovery record"));
    }
    let root = canonical_directory(backup_root, "mod recovery backup root")?;
    let backup = &record.context.backup_directory;
    if !path_is_within(backup, &root)
        || paths_equal(backup, &root)
        || path_is_within(backup, target)
        || path_is_within(backup, &mod_content::runtime_directory(record.install()))
    {
        return Err(InstallError::validation(
            "Mod recovery backup escaped the configured root",
        ));
    }
    mod_content::checked_path(&root, backup).map_err(InstallError::validation)?;
    for path in [
        record.live(),
        record.temporary("stage"),
        record.temporary("retired"),
        record.temporary("restore"),
        record.temporary("rollback"),
    ] {
        mod_content::checked_path(record.install(), &path).map_err(InstallError::validation)?;
    }
    for folder in record.original.iter().chain(record.incoming.iter()) {
        if folder.files.is_empty() || folder.files.len() > FOLDER_FILE_LIMIT {
            return Err(InstallError::validation("Invalid recovery inventory"));
        }
        for (name, digest) in &folder.files {
            if name != mod_content::MANIFEST && name != mod_content::RECEIPT {
                let file_name = name
                    .split_once('/')
                    .ok_or_else(|| InstallError::validation("Invalid mod recovery path"))?
                    .1;
                let artifact = ArtifactMetadata {
                    file_name: file_name.into(),
                    byte_length: digest.byte_length,
                    sha256: digest.sha256.clone(),
                };
                if package_path(&artifact)? != *name {
                    return Err(InstallError::validation("Invalid mod recovery path"));
                }
            }
            validate_transaction_digest(digest, "mod recovery file")?;
        }
    }
    let mut legacy_names = BTreeSet::new();
    for artifact in &record.legacy {
        package_path(artifact)?;
        validate_transaction_digest(&TransactionDigest::from(artifact), &artifact.file_name)?;
        if !legacy_names.insert(&artifact.file_name) {
            return Err(InstallError::validation("Repeated legacy cleanup package"));
        }
    }
    if let Some(legacy) = &record.legacy_recovery {
        legacy.validate(target, &root)?;
    }
    if record.migration() && record.original.is_some() {
        return Err(InstallError::validation(
            "Legacy migration cannot replace an existing managed mod",
        ));
    }
    validate_source_artifact_records(record.stock.clone())?;
    account::validate_account_record(&record.context, target)
}

fn cleanup(record: &Record) -> Result<(), InstallError> {
    if let Some(legacy) = &record.legacy_recovery {
        legacy.cleanup(&record.context.target_packages_directory)?;
    }
    for (kind, expected) in [
        ("stage", record.incoming.as_ref()),
        ("retired", record.original.as_ref()),
        ("restore", record.original.as_ref()),
        ("rollback", record.incoming.as_ref()),
    ] {
        let path = record.temporary(kind);
        if path.exists() {
            remove_folder(
                &path,
                expected
                    .ok_or_else(|| InstallError::validation("Unexpected temporary mod folder"))?,
            )?;
        }
    }
    fs::remove_file(record.journal()).map_err(failure)
}

fn complete_migration(record: &mut Record, request: &RecoveryRequest) -> Result<(), InstallError> {
    let target = &record.context.target_packages_directory;
    if !record.live().exists() && record.incoming.is_some() {
        verify_folder(&record.temporary("stage"), record.incoming.as_ref())?;
    }
    check_stopped(request.game_running_check)?;
    verify_source_artifacts(target, &record.stock)?;
    verify_legacy(record, true)?;
    record.verify_coexistence(&if record.live().exists() {
        record.live()
    } else {
        record.temporary("stage")
    })?;
    if let Some(legacy) = &record.legacy_recovery {
        legacy.verify_files(target, false)?;
        legacy.apply_account(request.game_running_check)?;
    }
    if !record.live().exists() && record.incoming.is_some() {
        rename_absent(&record.temporary("stage"), &record.live())?;
    }
    verify_folder(&record.live(), record.incoming.as_ref())?;
    remove_legacy(record, true, &mut |_| {})?;
    record.verify_finished_account()?;
    uninstall::refresh_recovery_caches(&record.context, request.runtime_snapshot_check)?;
    record.state = InstallTransactionState::Committed;
    record.write()?;
    write_json(
        &record.context.backup_directory.join("mod-recovery.json"),
        record,
    )?;
    cleanup(record)
}

fn roll_back_unstarted_migration(
    record: &mut Record,
    request: &RecoveryRequest,
) -> Result<Option<RecoveryOutcome>, InstallError> {
    check_stopped(request.game_running_check)?;
    verify_legacy(record, false)?;
    verify_folder(&record.live(), record.original.as_ref())?;
    account::recover_account(&record.context, request.game_running_check)?;
    uninstall::refresh_recovery_caches(&record.context, request.runtime_snapshot_check)?;
    record.state = InstallTransactionState::Recovered;
    record.write()?;
    cleanup(record)?;
    Ok(Some(RecoveryOutcome::Recovered {
        restored_files: vec![],
        removed_new_files: vec![],
    }))
}

fn remove_legacy(
    record: &Record,
    allow_missing: bool,
    observer: &mut dyn FnMut(&str),
) -> Result<(), InstallError> {
    verify_legacy(record, allow_missing)?;
    for artifact in &record.legacy {
        let path = record
            .context
            .target_packages_directory
            .join(&artifact.file_name);
        if path.try_exists().map_err(failure)? {
            verify_file(&path, &TransactionDigest::from(artifact))?;
            fs::remove_file(path).map_err(failure)?;
            observer("legacy_file_removed");
        }
    }
    #[cfg(unix)]
    if !record.legacy.is_empty() {
        File::open(&record.context.target_packages_directory)
            .and_then(|directory| directory.sync_all())
            .map_err(failure)?;
    }
    Ok(())
}

fn remove_folder(path: &Path, expected: &Folder) -> Result<(), InstallError> {
    verify_subset(path, expected)?;
    for (name, digest) in &expected.files {
        let file = path.join(name);
        if file.exists() {
            verify_file(&file, digest)?;
            fs::remove_file(file).map_err(failure)?;
        }
    }
    for name in ["packages", "patches"] {
        let sub = path.join(name);
        if sub.exists() {
            fs::remove_dir(sub).map_err(failure)?;
        }
    }
    fs::remove_dir(path).map_err(failure)
}

fn copy_folder(source: &Path, destination: &Path, expected: &Folder) -> Result<(), InstallError> {
    verify_folder(source, Some(expected))?;
    if destination.exists() {
        verify_subset(destination, expected)?;
    } else {
        create_private_directory(destination).map_err(failure)?;
    }
    for (name, digest) in &expected.files {
        let path = destination.join(name);
        if path.exists() {
            verify_file(&path, digest)?;
            continue;
        }
        fs::create_dir_all(path.parent().unwrap()).map_err(failure)?;
        if *digest != copy_file_create_new(&source.join(name), &path).map_err(failure)? {
            return Err(InstallError::validation(
                "Managed content changed during backup",
            ));
        }
    }
    verify_folder(destination, Some(expected))
}

fn verify_subset(path: &Path, expected: &Folder) -> Result<(), InstallError> {
    let actual = snapshot_folder(path)?;
    if actual
        .files
        .iter()
        .any(|(name, digest)| expected.files.get(name) != Some(digest))
    {
        return Err(InstallError::validation(
            "Temporary mod content changed during recovery",
        ));
    }
    Ok(())
}

fn verify_legacy(record: &Record, allow_missing: bool) -> Result<(), InstallError> {
    for profile in all_authored_packages_in(&[record.context.target_packages_directory.as_path()]) {
        let path = record
            .context
            .target_packages_directory
            .join(profile.file_name);
        let expected = record
            .legacy
            .iter()
            .find(|a| a.file_name == profile.file_name);
        mod_content::checked_path(&record.context.target_packages_directory, &path)
            .map_err(InstallError::validation)?;
        if path.exists() {
            let expected = expected.ok_or_else(|| {
                InstallError::validation(
                    "Unexpected legacy package appeared during the mod operation",
                )
            })?;
            verify_file(&path, &TransactionDigest::from(expected))?;
        } else if !allow_missing && expected.is_some() {
            return Err(InstallError::validation(
                "Legacy package disappeared during the mod operation",
            ));
        }
    }
    Ok(())
}

fn rename_absent(source: &Path, target: &Path) -> Result<(), InstallError> {
    if target.try_exists().map_err(failure)? {
        return Err(InstallError::validation(
            "A mod destination appeared during publication",
        ));
    }
    sundial::package_authoring::move_authoring_path(source, target)
        .map_err(InstallError::validation)?;
    #[cfg(unix)]
    File::open(target.parent().unwrap())
        .and_then(|f| f.sync_all())
        .map_err(failure)?;
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), InstallError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(failure)?;
    sundial::package_authoring::replace_authoring_file(path, &bytes).map_err(failure)
}
