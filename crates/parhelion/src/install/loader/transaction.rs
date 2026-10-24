use super::*;
mod retention;
pub(in crate::install) use retention::completed_backup_owner;

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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    family: BTreeMap<String, Change>,
    token: String,
    #[serde(default)]
    pub(super) legacy_recovery: Option<legacy::Recovery>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Change {
    original: Option<Folder>,
    incoming: Option<Folder>,
    kept: bool,
}

/// Every member keeps its own manifest and package folders inside the backup.
struct Member<'a> {
    id: &'a str,
    original: Option<&'a Folder>,
    incoming: Option<&'a Folder>,
    kept: bool,
}

impl Member<'_> {
    fn live(&self, record: &Record) -> PathBuf {
        mod_content::member_directory(record.install(), self.id)
    }
    fn temporary(&self, record: &Record, kind: &str) -> PathBuf {
        if self.id == mod_content::ID {
            record.temporary(kind)
        } else {
            record
                .live()
                .parent()
                .unwrap()
                .join(format!(".{}-{kind}-{}", self.id, record.token))
        }
    }
    fn backup(&self, record: &Record) -> PathBuf {
        let backup = &record.context.backup_directory;
        if record.schema == 1 && self.id == mod_content::ID {
            backup.join("mod")
        } else {
            backup.join("mods").join(self.id)
        }
    }
}

pub(super) struct Transaction {
    pub record: Record,
    pub sunrise: OriginalSunriseCache,
    pub headers: OriginalPackageHeaderCaches,
    unjournaled: Unjournaled,
}

/// The new backup and stage folders, removed if the transaction ends before the journal names them.
#[derive(Default)]
struct Unjournaled(Vec<PathBuf>);

impl Drop for Unjournaled {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

impl Record {
    fn members(&self) -> impl Iterator<Item = Member<'_>> {
        std::iter::once(Member {
            id: mod_content::ID,
            original: self.original.as_ref(),
            incoming: self.incoming.as_ref(),
            kept: false,
        })
        .chain(self.family.iter().map(|(id, change)| Member {
            id,
            original: change.original.as_ref(),
            incoming: change.incoming.as_ref(),
            kept: change.kept,
        }))
    }
    fn verify_family(&self, incoming: bool) -> Result<(), InstallError> {
        for member in self.members() {
            verify_folder(
                &member.live(self),
                if incoming {
                    member.incoming
                } else {
                    member.original
                },
            )?;
        }
        let recorded = self.members().map(|m| m.id).collect::<BTreeSet<_>>();
        if mod_content::family_directories(self.install())
            .map_err(failure)?
            .keys()
            .any(|id| !recorded.contains(id.as_str()))
        {
            return Err(InstallError::validation(
                "A managed family mod appeared after review",
            ));
        }
        Ok(())
    }
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
    fn verify_coexistence(&self, published: bool) -> Result<(), InstallError> {
        if self.incoming.is_none() {
            return Ok(());
        }
        let mut sources = Vec::new();
        let mut members = Vec::new();
        for member in self.members().filter(|member| member.incoming.is_some()) {
            let directory = if published
                || member.kept
                || !member
                    .temporary(self, "stage")
                    .try_exists()
                    .map_err(failure)?
            {
                member.live(self)
            } else {
                member.temporary(self, "stage")
            };
            verify_folder(&directory, member.incoming)?;
            let manifest =
                mod_content::read_json(&directory.join(mod_content::MANIFEST)).map_err(failure)?;
            let receipt =
                mod_content::read_json(&directory.join(mod_content::RECEIPT)).map_err(failure)?;
            members.push(crate::mod_family::Member {
                manifest,
                receipt,
                kept: member.kept,
            });
            for artifact in artifacts(member.incoming.unwrap()) {
                let path = directory.join(package_path(&artifact)?);
                sources.push((artifact, path));
            }
        }
        reject_installed_conflicts(
            &self.context.target_packages_directory,
            &sources,
            &self.legacy,
        )?;
        check_other_mods(
            &self.context.target_packages_directory,
            &sources,
            &self.legacy,
            &members,
        )?;
        Ok(())
    }
}

pub(super) struct Plan<'a> {
    pub target: &'a Path,
    pub backup_root: &'a Path,
    pub original: Option<Folder>,
    pub original_family: BTreeMap<String, Folder>,
    pub members: &'a [crate::mod_family::Member],
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
        mut original_family,
        members,
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
    let backup = create_backup_directory(&root).map_err(failure)?;
    let mut unjournaled = Unjournaled(vec![
        mods.join(format!(".parhelion-stage-{token}")),
        backup.clone(),
    ]);
    let prepared = (|| {
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
            fs::create_dir_all(backup.join("mods")).map_err(failure)?;
            copy_folder(
                &mod_content::directory(install),
                &backup.join("mods").join(mod_content::ID),
                folder,
            )?;
        }
        original_family.remove(mod_content::ID);
        for (id, folder) in &original_family {
            if !members
                .iter()
                .any(|member| member.manifest.id == *id && member.kept)
            {
                let destination = backup.join("mods").join(id);
                fs::create_dir_all(destination.parent().unwrap()).map_err(failure)?;
                copy_folder(
                    &mod_content::member_directory(install, id),
                    &destination,
                    folder,
                )?;
            }
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
            schema: 2,
            state: InstallTransactionState::Pending,
            stock,
            legacy,
            original,
            incoming: None,
            family: original_family
                .into_iter()
                .map(|(id, original)| {
                    (
                        id,
                        Change {
                            original: Some(original),
                            incoming: None,
                            kept: false,
                        },
                    )
                })
                .collect(),
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
            let main = members.iter().find(|m| m.manifest.id == mod_content::ID);
            let staging = record.temporary("stage");
            create_private_directory(&staging).map_err(failure)?;
            fs::create_dir(staging.join("packages")).map_err(failure)?;
            fs::create_dir(staging.join("patches")).map_err(failure)?;
            let mut receipt = mod_content::Receipt {
                schema: 1,
                mod_id: mod_content::ID.into(),
                artifacts: vec![],
                content: None,
            };
            for (artifact, source) in sources {
                if main.is_some_and(|main| {
                    !main
                        .receipt
                        .artifacts
                        .iter()
                        .any(|a| a.path.rsplit('/').next() == Some(artifact.file_name.as_str()))
                }) {
                    continue;
                }
                let path =
                    if let Some(path) = mod_content::mission_script::path(&artifact.file_name) {
                        let parent = staging.join(&path).parent().unwrap().to_owned();
                        fs::create_dir_all(&parent).map_err(failure)?;
                        mod_content::checked_path(&staging, &parent)
                            .map_err(InstallError::validation)?;
                        path
                    } else if artifact.file_name == mod_content::map_diagnostics::FILE_NAME {
                        artifact.file_name.clone()
                    } else {
                        package_path(artifact)?
                    };
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
                &main.map_or_else(mod_content::Manifest::default, |m| m.manifest.clone()),
            )?;
            write_json(&staging.join(mod_content::RECEIPT), &receipt)?;
            record.incoming = Some(snapshot_folder(&staging)?);
            sync_backup_directories(&staging, &mods).map_err(InstallError::validation)?;
            for member in members.iter().filter(|m| m.manifest.id != mod_content::ID) {
                let id = &member.manifest.id;
                let change = record.family.entry(id.clone()).or_insert(Change {
                    original: None,
                    incoming: None,
                    kept: false,
                });
                if member.kept {
                    let directory = mod_content::member_directory(install, id);
                    let installed_manifest: mod_content::Manifest =
                        mod_content::read_json(&directory.join(mod_content::MANIFEST))
                            .map_err(failure)?;
                    let installed_receipt: mod_content::Receipt =
                        mod_content::read_json(&directory.join(mod_content::RECEIPT))
                            .map_err(failure)?;
                    if installed_manifest != member.manifest
                        || installed_receipt != member.receipt
                        || change.original.is_none()
                    {
                        return Err(InstallError::validation(
                            "A kept importer changed after staging",
                        ));
                    }
                    change.incoming = change.original.clone();
                    change.kept = true;
                    continue;
                }
                let staging = mods.join(format!(".{id}-stage-{}", record.token));
                unjournaled.0.push(staging.clone());
                create_private_directory(&staging).map_err(failure)?;
                fs::create_dir(staging.join("packages")).map_err(failure)?;
                for artifact in &member.receipt.artifacts {
                    let name = artifact.path.rsplit('/').next().unwrap();
                    let source = &sources
                        .iter()
                        .find(|(a, _)| a.file_name == name)
                        .ok_or_else(|| {
                            InstallError::validation(format!("Missing importer artifact {name}"))
                        })?
                        .1;
                    let expected = TransactionDigest {
                        byte_length: artifact.byte_length,
                        sha256: artifact.sha256.to_ascii_uppercase(),
                    };
                    if expected
                        != copy_file_create_new(source, &staging.join(&artifact.path))
                            .map_err(failure)?
                    {
                        return Err(InstallError::validation(
                            "Importer content changed during staging",
                        ));
                    }
                }
                write_json(&staging.join(mod_content::MANIFEST), &member.manifest)?;
                write_json(&staging.join(mod_content::RECEIPT), &member.receipt)?;
                change.incoming = Some(snapshot_folder(&staging)?);
                sync_backup_directories(&staging, &mods).map_err(InstallError::validation)?;
            }
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
            unjournaled: Unjournaled::default(),
        })
    })();
    prepared.map(|mut transaction| {
        transaction.unjournaled = unjournaled;
        transaction
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
    for member in record.members() {
        for path in [
            member.live(record),
            member.temporary(record, "stage"),
            member.temporary(record, "retired"),
        ] {
            mod_content::checked_path(record.install(), &path).map_err(InstallError::validation)?;
        }
    }
    verify_source_artifacts(&record.context.target_packages_directory, &record.stock)?;
    if record.incoming.is_some() {
        mod_content::ensure_resolved_environment(record.install())
            .map_err(InstallError::validation)?;
    }
    record.verify_family(false)?;
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
    // From here recovery owns both folders, even if the journal write itself fails.
    transaction.unjournaled.0.clear();
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
    record.verify_coexistence(false)?;
    apply_account(&record.context)?;
    if let Some(legacy) = &record.legacy_recovery {
        legacy.apply_account(check)?;
    }
    publish_family(record, check, observer)?;
    remove_legacy(record, false, observer)?;
    if !record.legacy.is_empty() {
        observer("legacy_removed");
    }
    record.verify_family(true)?;
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
    record.verify_coexistence(true)?;
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

fn publish_family(
    record: &Record,
    check: GameRunningCheck,
    observer: &mut dyn FnMut(&str),
) -> Result<(), InstallError> {
    record.verify_family(false)?;
    for member in record.members() {
        check_stopped(check)?;
        if member.kept {
            verify_folder(&member.live(record), member.incoming)?;
            observer(&format!("kept:{}", member.id));
            continue;
        }
        if member.original.is_some() {
            verify_folder(&member.live(record), member.original)?;
            rename_absent(&member.live(record), &member.temporary(record, "retired"))?;
            observer(&format!("retired:{}", member.id));
            observer("retired");
        }
        if member.incoming.is_some() {
            verify_folder(&member.temporary(record, "stage"), member.incoming)?;
            rename_absent(&member.temporary(record, "stage"), &member.live(record))?;
            observer(&format!("published:{}", member.id));
            observer("published");
        }
    }
    Ok(())
}

/// Inspect the complete family before recovery mutates any member. A known new generation may
/// be rolled back, but an editor's later changes are never silently replaced from a backup.
fn verify_pending_family(record: &Record) -> Result<(), InstallError> {
    let ids = record.members().map(|m| m.id).collect::<BTreeSet<_>>();
    if mod_content::family_directories(record.install())
        .map_err(failure)?
        .keys()
        .any(|id| !ids.contains(id.as_str()))
    {
        return Err(InstallError::validation(
            "A managed family mod appeared after interruption",
        ));
    }
    for member in record.members() {
        if member.kept {
            verify_folder(&member.live(record), member.original)?;
        } else if let Some(original) = member.original {
            verify_folder(&member.backup(record), Some(original))?;
        }
        for (kind, expected) in [
            ("stage", member.incoming),
            ("retired", member.original),
            ("rollback", member.incoming),
            ("restore", member.original),
        ] {
            let path = member.temporary(record, kind);
            if path.exists() {
                if kind == "restore" {
                    verify_subset(
                        &path,
                        expected.ok_or_else(|| {
                            InstallError::validation("Unexpected recovery folder")
                        })?,
                    )?;
                } else {
                    verify_folder(&path, expected)?;
                }
            }
        }
        let live = member.live(record);
        if live.exists() {
            let current = snapshot_folder(&live)?;
            if Some(&current) != member.original && Some(&current) != member.incoming {
                return Err(InstallError::validation(
                    "Managed content changed after interruption. Recovery preserved the changed files",
                ));
            }
        }
    }
    Ok(())
}

fn rollback_family(record: &Record, check: GameRunningCheck) -> Result<Vec<PathBuf>, InstallError> {
    let mut removed = Vec::new();
    for member in record.members().filter(|m| !m.kept) {
        check_stopped(check)?;
        let live = member.live(record);
        let current = live.exists().then(|| snapshot_folder(&live)).transpose()?;
        if current.as_ref() == member.original {
            continue;
        }
        if current.is_some() {
            rename_absent(&live, &member.temporary(record, "rollback"))?;
            removed.push(live.clone());
        }
        if let Some(original) = member.original {
            let retired = member.temporary(record, "retired");
            if retired.exists() {
                rename_absent(&retired, &live)?;
            } else {
                let restore = member.temporary(record, "restore");
                copy_folder(&member.backup(record), &restore, original)?;
                rename_absent(&restore, &live)?;
            }
        }
    }
    Ok(removed)
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
        record.verify_family(true)?;
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
        record.verify_family(false)?;
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
    verify_pending_family(&record)?;
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
    let removed = rollback_family(&record, request.game_running_check)?;
    account::recover_account(&record.context, request.game_running_check)?;
    uninstall::refresh_recovery_caches(&record.context, request.runtime_snapshot_check)?;
    record.verify_family(false)?;
    record.state = InstallTransactionState::Recovered;
    record.write()?;
    cleanup(&record)?;
    Ok(Some(RecoveryOutcome::Recovered {
        restored_files: vec![],
        removed_new_files: removed,
    }))
}

fn validate_record(record: &Record, target: &Path, backup_root: &Path) -> Result<(), InstallError> {
    if !matches!(record.schema, 1 | 2)
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
    for (id, change) in &record.family {
        if id == mod_content::ID
            || !mod_content::is_family_id(id)
            || (change.kept && (change.original.is_none() || change.original != change.incoming))
        {
            return Err(InstallError::validation(
                "Invalid managed family recovery record",
            ));
        }
    }
    for member in record.members() {
        for path in std::iter::once(member.live(record)).chain(
            ["stage", "retired", "restore", "rollback"].map(|kind| member.temporary(record, kind)),
        ) {
            mod_content::checked_path(record.install(), &path).map_err(InstallError::validation)?;
        }
    }
    for folder in record
        .members()
        .flat_map(|m| m.original.into_iter().chain(m.incoming))
    {
        if folder.files.is_empty() || folder.files.len() > FOLDER_FILE_LIMIT {
            return Err(InstallError::validation("Invalid recovery inventory"));
        }
        for (name, digest) in &folder.files {
            if name != mod_content::MANIFEST
                && name != mod_content::RECEIPT
                && name != mod_content::map_diagnostics::FILE_NAME
                && !mod_content::mission_script::valid_path(name)
            {
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
    if record.migration() && record.members().any(|m| m.original.is_some()) {
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
    for member in record.members() {
        for (kind, expected) in [
            ("stage", member.incoming),
            ("retired", member.original),
            ("restore", member.original),
            ("rollback", member.incoming),
        ] {
            let path = member.temporary(record, kind);
            if path.exists() {
                remove_folder(
                    &path,
                    expected.ok_or_else(|| {
                        InstallError::validation("Unexpected temporary mod folder")
                    })?,
                )?;
            }
        }
    }
    fs::remove_file(record.journal()).map_err(failure)
}

fn complete_migration(record: &mut Record, request: &RecoveryRequest) -> Result<(), InstallError> {
    let target = &record.context.target_packages_directory;
    check_stopped(request.game_running_check)?;
    verify_source_artifacts(target, &record.stock)?;
    verify_legacy(record, true)?;
    record.verify_coexistence(false)?;
    for member in record.members().filter(|m| m.incoming.is_some()) {
        if !member.live(record).exists() {
            verify_folder(&member.temporary(record, "stage"), member.incoming)?;
        }
    }
    if let Some(legacy) = &record.legacy_recovery {
        legacy.verify_files(target, false)?;
        legacy.apply_account(request.game_running_check)?;
    }
    for member in record.members().filter(|m| m.incoming.is_some()) {
        if !member.live(record).exists() {
            check_stopped(request.game_running_check)?;
            rename_absent(&member.temporary(record, "stage"), &member.live(record))?;
        }
    }
    record.verify_family(true)?;
    record.verify_coexistence(true)?;
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
    record.verify_family(false)?;
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
    let scripts = path.join("scripts");
    if scripts.exists() {
        for entry in fs::read_dir(&scripts).map_err(failure)? {
            let entry = entry.map_err(failure)?;
            mod_content::checked_path(path, &entry.path()).map_err(InstallError::validation)?;
            fs::remove_dir(entry.path()).map_err(failure)?;
        }
        fs::remove_dir(scripts).map_err(failure)?;
    }
    fs::remove_dir(path).map_err(failure)
}

/// Puts a retired mod folder back from its backup: copied beside it, checked against the backup's
/// files and moved into place. A folder already there is left alone.
pub(super) fn restore_retired(install: &Path, backup: &Path) -> Result<(), InstallError> {
    let record: Record =
        mod_content::read_json(&backup.join("mod-recovery.json")).map_err(failure)?;
    if !matches!(record.schema, 1 | 2) {
        return Err(InstallError::validation("Unsupported mod recovery record"));
    }
    for member in record.members().filter(|m| m.original.is_some()) {
        let directory = mod_content::member_directory(install, member.id);
        if directory.try_exists().map_err(failure)? {
            continue;
        }
        let source = member.backup(&record);
        let folder = member.original.unwrap();
        let staging = directory
            .parent()
            .ok_or_else(|| InstallError::validation("The mod folder has no parent"))?
            .join(format!(".{}-restore-{}", member.id, unique_token()));
        let restored = copy_folder(&source, &staging, folder)
            .and_then(|()| rename_absent(&staging, &directory));
        if restored.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        restored?;
    }
    Ok(())
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
    let limit = mod_content::metadata_byte_limit(path);
    if bytes.len() as u64 > limit {
        return Err(InstallError::validation(format!(
            "Mod metadata {} exceeds its {limit}-byte size limit",
            path.display()
        )));
    }
    sundial::package_authoring::replace_authoring_file(path, &bytes).map_err(failure)
}
