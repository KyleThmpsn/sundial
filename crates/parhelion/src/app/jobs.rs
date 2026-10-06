//! Catalog, runtime, build and install background job coordination.
use super::*;
use sundial::package_authoring::account::{AuthoredGrantReport, AuthoredProfileSyncReport};

/// How long an install or uninstall waits for model previews to finish reading packages.
pub(super) const PREVIEW_READ_WAIT: Duration = Duration::from_secs(60);
pub(super) const PREVIEW_READ_BUSY: &str =
    "A model preview is still reading packages. Close it and try again";

#[cfg(test)]
mod tests;

/// Keep the request identity with its worker, including when it exits without a result.
pub(super) struct RuntimeGraphJob {
    key: RuntimeGraphKey,
    receiver: Receiver<Result<(WeaponRuntimeGraph, bool), String>>,
    worker: thread::JoinHandle<()>,
}

impl PackageAuthoringApp {
    pub(super) fn start_replacement_review(&mut self) {
        self.replacement_review = None;
        self.replacement_receiver = None;
        let Some(Ok(build)) = &self.latest_build else {
            return;
        };
        let staged = build.run_directory.clone();
        let target = self.packages.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(crate::install::preview_replacement(&target, &staged));
        });
        self.replacement_receiver = Some(receiver);
    }

    pub(super) fn poll_replacement_review(&mut self) {
        let Some(receiver) = &self.replacement_receiver else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => {
                self.replacement_review = Some(result);
                self.replacement_receiver = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.replacement_review = Some(Err(
                    "Account review stopped unexpectedly. Review the installation again.".into(),
                ));
                self.replacement_receiver = None;
            }
        }
    }

    pub(super) fn start_catalog_load(&mut self, ctx: &egui::Context) {
        if self.catalog_receiver.is_some() || self.catalog_worker.is_some() {
            self.catalog_reload_pending = true;
            return;
        }
        self.catalog_load_requested = true;
        self.catalog_reload_pending = false;
        let packages = self.packages.clone();
        let Some(install) = packages
            .parent()
            .filter(|parent| parent.is_dir())
            .map(Path::to_path_buf)
        else {
            self.catalog_progress = None;
            return;
        };
        let _ = sundial::investment::configure_authoring_fonts(ctx, &install);
        let force_rebuild = std::mem::take(&mut self.catalog_force_rebuild);
        let (sender, receiver) = mpsc::channel();
        self.catalog_receiver = Some(receiver);
        self.catalog_progress = Some(CatalogLoadProgress {
            message: "Starting the shared Sundial catalog…",
            completed: 0,
            total: 0,
        });
        let ctx = ctx.clone();
        self.catalog_worker = Some(thread::spawn(move || {
            let progress_sender = sender.clone();
            let progress_ctx = ctx.clone();
            let result = InvestmentCatalog::load(&install, force_rebuild, move |progress| {
                let _ = progress_sender.send(CatalogEvent::Progress(progress));
                progress_ctx.request_repaint();
            });
            let _ = sender.send(CatalogEvent::Finished(Box::new(result)));
            ctx.request_repaint();
        }));
    }

    pub(super) fn poll_catalog(&mut self) {
        let Some(receiver) = &self.catalog_receiver else {
            return;
        };
        let mut events = Vec::new();
        let mut disconnected = false;
        loop {
            match receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        let mut worker_finished = false;
        for event in events {
            match event {
                CatalogEvent::Progress(progress) => {
                    if !self.catalog_reload_pending {
                        self.catalog_progress = Some(progress);
                    }
                }
                CatalogEvent::Finished(result) => {
                    worker_finished = true;
                    if self.catalog_reload_pending {
                        drop(result);
                    } else {
                        match *result {
                            Ok(catalog) => self.install_catalog(catalog),
                            Err(error) => {
                                self.drop_loaded_catalog();
                                self.log.push(LogEntry::error(format!(
                                    "Could not load the weapon catalog: {error}"
                                )));
                            }
                        }
                    }
                }
            }
        }
        if worker_finished || disconnected {
            self.catalog_receiver = None;
            self.catalog_progress = None;
            if let Some(worker) = self.catalog_worker.take()
                && worker.join().is_err()
            {
                self.log.push(LogEntry::error("Catalog loading crashed"));
            }
            if disconnected && !worker_finished {
                self.log
                    .push(LogEntry::error("Catalog loading stopped without a result"));
            }
            if self.catalog_reload_pending {
                self.catalog_reload_pending = false;
                self.catalog_load_requested = false;
            }
        }
    }

    pub(super) fn runtime_graph_key(&self) -> Option<RuntimeGraphKey> {
        // Gear keeps its base item's runtime unchanged, so there is no weapon graph to read.
        if !self.recipe.kind.is_weapon() {
            return None;
        }
        let fallback_item_hash = self
            .recipe
            .donor
            .item_hash
            .parse_u32()
            .ok()
            .filter(|hash| *hash != 0)?;
        let pattern_index = self.recipe.overrides.weapon_pattern_index.or_else(|| {
            self.donor_summaries
                .iter()
                .find(|donor| donor.hash == fallback_item_hash)
                .and_then(|donor| donor.weapon_pattern_index)
        });
        // An appearance from another weapon family makes the build move that family's rig and
        // animations onto this runtime, so the editor has to read the same entity the build
        // will write rather than the untouched gameplay one.
        let group = |item_hash: u32| {
            self.donor_summaries
                .iter()
                .find(|donor| donor.hash == item_hash)
                .and_then(|donor| donor.weapon_translation_group)
        };
        // Animations from the base weapon's family keep its rig, as the build does.
        let keeps_base_rig = self
            .recipe
            .overrides
            .animation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .is_some_and(|hash| group(hash).is_some() && group(hash) == group(fallback_item_hash));
        let appearance_rig = self
            .recipe
            .presentation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .filter(|hash| *hash != 0 && group(*hash) != group(fallback_item_hash))
            .filter(|_| !keeps_base_rig)
            .map(|hash| {
                (
                    self.donor_summaries
                        .iter()
                        .find(|donor| donor.hash == hash)
                        .and_then(|donor| donor.weapon_pattern_index),
                    hash,
                )
            });
        Some(
            RuntimeGraphKey::new(
                pattern_index,
                fallback_item_hash,
                self.recipe
                    .runtime_component_donors
                    .iter()
                    .filter_map(|component| {
                        let donor_item_hash = component.donor.item_hash.parse_u32().ok()?;
                        Some((
                            component.binding_hash.parse_u32().ok()?,
                            self.donor_summaries
                                .iter()
                                .find(|donor| donor.hash == donor_item_hash)
                                .and_then(|donor| donor.weapon_pattern_index),
                            donor_item_hash,
                        ))
                    })
                    .filter(|(binding_hash, _, donor_hash)| {
                        !matches!(*binding_hash, 0 | u32::MAX) && *donor_hash != 0
                    }),
            )
            .with_appearance_rig(appearance_rig),
        )
    }

    pub(super) fn ensure_runtime_graph(&mut self, ctx: &egui::Context) {
        // The installer replaces the packages a runtime read would open.
        if self.install_receiver.is_some() {
            return;
        }
        let key = self.runtime_graph_key();
        if self.runtime_graph_target != key {
            self.runtime_graph_target = key.clone();
            self.runtime_graph = None;
            self.runtime_graph_error = None;
            self.runtime_value_text.clear();
        }
        let Some(key) = key else {
            return;
        };
        if self.runtime_graph_job.is_some()
            || self
                .runtime_graph
                .as_ref()
                .is_some_and(|(loaded, _)| loaded == &key)
            || self
                .runtime_graph_error
                .as_ref()
                .is_some_and(|(failed, _)| failed == &key)
        {
            return;
        }
        let packages = self.packages.clone();
        let (sender, receiver) = mpsc::channel();
        let worker_key = key.clone();
        let ctx = ctx.clone();
        let worker = thread::spawn(move || {
            let result = load_effective_runtime_graph(&packages, &worker_key);
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.runtime_graph_job = Some(RuntimeGraphJob {
            key,
            receiver,
            worker,
        });
    }

    pub(super) fn poll_runtime_graph(&mut self) {
        let Some(job) = &self.runtime_graph_job else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("Weapon data loading stopped without a result".to_owned())
            }
        };
        let RuntimeGraphJob { key, worker, .. } =
            self.runtime_graph_job.take().expect("job was checked");
        // Join even stale jobs: their package handles must be released before installation.
        let completion = worker.join();
        if self.runtime_graph_target.as_ref() != Some(&key) {
            return;
        }
        let result = if completion.is_err() {
            Err("Weapon data loading crashed".to_owned())
        } else {
            result
        };
        match result {
            Ok((graph, carries_appearance_rig)) => {
                self.runtime_rig_appearance = carries_appearance_rig
                    .then(|| key.appearance_rig.map(|(_, item_hash)| item_hash))
                    .flatten();
                self.runtime_graph = Some((key, Arc::new(graph)));
                self.runtime_graph_error = None;
            }
            Err(error) => {
                self.runtime_rig_appearance = None;
                self.runtime_graph = None;
                self.runtime_graph_error = Some((key, error));
            }
        }
    }

    pub(super) fn reset_catalog_load(&mut self) {
        self.perk_workbench.stop_optional_reads();
        self.drop_loaded_catalog();
        if self.catalog_receiver.is_some() || self.catalog_worker.is_some() {
            self.catalog_reload_pending = true;
            self.catalog_load_requested = true;
        } else {
            self.catalog_progress = None;
            self.catalog_load_requested = false;
            self.catalog_reload_pending = false;
        }
    }

    /// Makes a loaded catalog the app's: bases for every item kind, perk choices, then a default
    /// base for the open recipe.
    pub(super) fn install_catalog(&mut self, catalog: InvestmentCatalog) {
        self.donor_summaries = catalog.weapon_donors();
        // Only stock subclasses can be a base or an ability source, not ones a build installed.
        self.subclasses = catalog.subclasses(crate::package_profile::is_stock_item_definition);
        // On a Specific Ability names every stock ability by the nodes that equip it.
        super::custom_perks::workbench::remember_abilities(&self.subclasses);
        super::ability_names::remember(&self.subclasses, catalog.ability_rows());
        self.gear_donors = ItemKind::ALL
            .into_iter()
            .filter(|kind| !kind.is_weapon())
            .map(|kind| {
                // Shaders are plugs, not inventory items, so their bases come from plug metadata.
                let donors = match kind {
                    ItemKind::Shader => catalog.shader_donors(),
                    ItemKind::Subclass => catalog
                        .gear_donors(kind.bucket_hashes())
                        .into_iter()
                        .filter(|donor| {
                            self.subclasses
                                .iter()
                                .any(|subclass| subclass.hash == donor.hash)
                        })
                        .collect(),
                    _ => catalog.gear_donors(kind.bucket_hashes()),
                };
                (kind, donors)
            })
            .collect();
        self.ornament_appearances = catalog.weapon_ornament_appearances(&self.donor_summaries);
        self.library_donors = self
            .donor_summaries
            .iter()
            .chain(self.gear_donors.values().flatten())
            .cloned()
            .collect();
        self.library_state.refresh_donors(&self.library_donors);
        self.sandbox_perk_choices = catalog
            .weapon_sandbox_perk_choices_from(crate::package_profile::is_stock_item_definition);
        self.trait_choices = catalog.weapon_trait_choices();
        self.log.push(LogEntry::info(format!(
            "Loaded {} weapon donors",
            self.donor_summaries.len()
        )));
        self.catalog = Some(catalog);
        self.catalog_revision = self.catalog_revision.wrapping_add(1);
        self.bind_default_donor();
    }

    pub(super) fn bind_default_donor(&mut self) {
        if self
            .recipe
            .donor
            .item_hash
            .parse_u32()
            .is_ok_and(|hash| hash != 0)
        {
            return;
        }
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let default = if self.recipe.kind.is_weapon() {
            self.donor_summaries
                .iter()
                .filter(|summary| summary.collection_backed)
                .find_map(|summary| {
                    let donor = catalog.weapon_donor(summary.hash)?;
                    weapon_authoring_capabilities(&donor)
                        .is_authorable()
                        .then(|| (summary.hash, summary.name.clone()))
                })
        } else if self.recipe.kind == ItemKind::Subclass {
            // No subclass is in Collections, so the first stock one serves.
            self.gear_donors_for(ItemKind::Subclass)
                .first()
                .map(|summary| (summary.hash, summary.name.clone()))
        } else {
            // A Legendary base from Collections, so the new item has a Collections page and a
            // rarity the reader can raise or lower.
            let donors = self.gear_donors_for(self.recipe.kind);
            donors
                .iter()
                .filter(|summary| summary.collection_backed)
                .find(|summary| summary.rarity == sundial::investment::WeaponRarity::Legendary)
                .or_else(|| donors.iter().find(|summary| summary.collection_backed))
                .map(|summary| (summary.hash, summary.name.clone()))
        };
        if let Some((hash, name)) = default {
            self.recipe.set_donor(hash, name);
            if !self.recipe_dirty && self.recipe_path.is_none() {
                self.recipe_baseline = self.recipe.clone();
            }
            self.advance_recipe_revision();
        }
    }

    pub(super) fn start_build(&mut self) {
        let snapshot = match self.save_edits_for_build().and_then(|()| self.snapshot()) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.latest_build = Some(Err(error.clone()));
                self.build_blocker = None;
                self.build_status_open = true;
                self.log
                    .push(LogEntry::error(format!("Build blocked: {error}")));
                return;
            }
        };
        self.perk_workbench.stop_optional_reads();
        let (sender, receiver) = mpsc::channel();
        let started = Instant::now();
        self.build_invalidated = false;
        self.observed_recipe.clone_from(&self.recipe);
        self.advance_recipe_revision();
        self.build_started = Some(started);
        self.build_activity = build_status::Activity::default();
        self.install_status = build_status::InstallStatus::default();
        thread::spawn(move || {
            let result = build_and_stage_snapshot_reporting(&snapshot, |progress| {
                let _ = sender.send(BuildWorkerEvent::Progress(
                    TimedBuildProgress::from_progress(progress, started.elapsed()),
                ));
            });
            let _ = sender.send(BuildWorkerEvent::Finished {
                result,
                elapsed: started.elapsed(),
            });
        });
        self.build_progress = Some(TimedBuildProgress {
            phase: BuildPhase::InspectingSource,
            current_artifact: Some("Checking Recipes".to_owned()),
            completed: 0,
            total: 2,
            elapsed: Duration::ZERO,
        });
        self.latest_build = None;
        self.build_blocker = None;
        self.latest_install = None;
        self.replacement_review = None;
        self.replacement_receiver = None;
        self.build_status_open = true;
        self.build_dialog_step = BuildDialogStep::Build;
        self.build_receiver = Some(receiver);
        self.log.push(LogEntry::info(
            "Started package compilation in a background worker",
        ));
    }

    pub(super) fn poll_build(&mut self) {
        loop {
            let event = match self.build_receiver.as_ref().map(Receiver::try_recv) {
                Some(Ok(event)) => event,
                Some(Err(TryRecvError::Empty)) | None => return,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.log
                        .push(LogEntry::error("The build stopped without a result"));
                    self.latest_build = Some(Err("The build stopped without a result".to_owned()));
                    let elapsed = self.build_elapsed(Instant::now());
                    if let Some(progress) = &mut self.build_progress {
                        progress.elapsed = elapsed;
                    }
                    self.build_started = None;
                    self.build_receiver = None;
                    return;
                }
            };
            match event {
                BuildWorkerEvent::Progress(progress) => {
                    if let Some(message) = self.build_activity.progress(
                        progress.elapsed,
                        progress.phase.label(),
                        progress.current_artifact.as_deref(),
                        (progress.completed, progress.total),
                        matches!(
                            progress.phase,
                            BuildPhase::InspectingSource
                                | BuildPhase::LoadingCatalog
                                | BuildPhase::CompilingProject
                                | BuildPhase::BuildingPayloads
                        ),
                        true,
                    ) {
                        self.log.push(LogEntry::info(message));
                    }
                    self.build_progress = Some(progress);
                }
                BuildWorkerEvent::Finished {
                    result: Ok(report),
                    elapsed,
                } => {
                    self.build_progress = Some(TimedBuildProgress {
                        phase: BuildPhase::Complete,
                        current_artifact: None,
                        completed: 1,
                        total: 1,
                        elapsed,
                    });
                    self.log.push(LogEntry::info(format!(
                        "Build complete: {}",
                        report.run_directory.display()
                    )));
                    self.latest_build = Some(if std::mem::take(&mut self.build_invalidated) {
                        let error =
                            "Recipes changed during the build. Build & Stage again before installing."
                                .to_owned();
                        self.build_activity.push(elapsed, error.clone());
                        self.log.push(LogEntry::error(&error));
                        Err(error)
                    } else {
                        Ok(report)
                    });
                    self.build_started = None;
                    self.build_receiver = None;
                    return;
                }
                BuildWorkerEvent::Finished {
                    result: Err(failure),
                    elapsed,
                } => {
                    if let Some(progress) = &mut self.build_progress {
                        progress.elapsed = elapsed;
                    }
                    self.build_blocker = failure.recipe;
                    let error = failure.message;
                    self.build_activity
                        .push(elapsed, format!("Build failed: {error}"));
                    self.log
                        .push(LogEntry::error(format!("Build failed: {error}")));
                    self.latest_build = Some(Err(error));
                    self.build_started = None;
                    self.build_receiver = None;
                    return;
                }
            }
        }
    }

    pub(super) fn build_elapsed(&self, now: Instant) -> Duration {
        self.build_started.map_or_else(
            || {
                self.build_progress
                    .as_ref()
                    .map_or(Duration::ZERO, |progress| progress.elapsed)
            },
            |started| now.saturating_duration_since(started),
        )
    }

    pub(super) fn start_install(&mut self) {
        let Some(Ok(confirmed_replacement)) = self.replacement_review.clone() else {
            self.log.push(LogEntry::error(
                "Review the account changes before installing",
            ));
            return;
        };
        if self.install_receiver.is_some() {
            return;
        }
        if self.account_resync_receiver.is_some() {
            self.log.push(LogEntry::error(
                "Wait for the account resync to finish before installing",
            ));
            return;
        }
        if self.catalog_receiver.is_some()
            || self.catalog_worker.is_some()
            || self.catalog_reload_pending
        {
            self.log.push(LogEntry::error(
                "Wait for the catalog scan to finish before installing",
            ));
            return;
        }
        self.perk_workbench.stop_optional_reads();
        if self.installed.busy()
            || self.build_check.busy()
            || self.runtime_graph_job.is_some()
            || self.runtime_donors.busy()
            || self.runtime_dependencies.busy()
            || self.perk_workbench.busy()
            || self.technical_markers_busy()
        {
            self.log.push(LogEntry::error(
                "Wait for weapon data to finish loading before installing",
            ));
            return;
        }
        if self.importer_busy() {
            self.log.push(LogEntry::error(
                "Wait for the D2 Importer to finish before installing",
            ));
            return;
        }
        let Some(Ok(build)) = self.latest_build.as_ref() else {
            self.log
                .push(LogEntry::error("Build & Stage before installing"));
            return;
        };
        let staged_run_directory = build.run_directory.clone();
        let target_packages_directory = self.packages.clone();
        let backup_root = PathBuf::from(self.backup_root.trim());
        let limit_package_backups = self.limit_package_backups;
        let package_backup_retention = self.package_backup_retention;
        let backup_recipe_snapshots = self.backup_recipe_snapshots;
        // The shared icon runtime can hold package files open on Windows. Drop the complete
        // catalog before the installer replaces any package, and keep reload paused until the
        // worker finishes.
        self.drop_loaded_catalog();
        self.catalog_progress = None;
        self.catalog_load_requested = true;
        let (sender, receiver) = mpsc::channel();
        let (progress_sender, progress_receiver) = mpsc::channel();
        let started = Instant::now();
        self.install_status = build_status::InstallStatus {
            receiver: Some(progress_receiver),
            started: Some(started),
            ..Default::default()
        };
        thread::spawn(move || {
            // Model previews pause once the install starts, but a read already running keeps
            // package files open until it returns, so the install waits for it.
            if !sundial::ui::model_preview::wait_for_package_reads(PREVIEW_READ_WAIT) {
                let _ = sender.send(Err(PREVIEW_READ_BUSY.to_owned()));
                return;
            }
            let mut request =
                InstallRequest::new(staged_run_directory, target_packages_directory, backup_root);
            request.limit_package_backups = limit_package_backups;
            request.package_backup_retention = package_backup_retention;
            request.backup_recipe_snapshots = backup_recipe_snapshots;
            request.confirmed_replacement = Some(confirmed_replacement);
            let result = install_staged_packages_with_progress(&request, |progress| {
                let _ = progress_sender.send((progress, started.elapsed()));
            })
            .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.latest_install = None;
        self.install_receiver = Some(receiver);
        self.build_dialog_step = BuildDialogStep::Install;
        self.build_status_open = true;
        self.log.push(LogEntry::info(
            "Started verified package installation in a background worker",
        ));
    }

    fn log_profile_sync(
        &mut self,
        context: &str,
        sync: Option<&Result<AuthoredProfileSyncReport, String>>,
    ) {
        match sync {
            Some(Ok(sync)) => {
                self.log.push(LogEntry::info(format!(
                    "Synchronized {}/{} authored collection unlocks in {}. Backup: {}",
                    sync.newly_set_unlocks,
                    sync.total_unlocks,
                    sync.settings_path.display(),
                    sync.backup_path.as_ref().map_or_else(
                        || "not needed".to_owned(),
                        |path| path.display().to_string()
                    ),
                )));
            }
            Some(Err(error)) => {
                self.log.push(LogEntry::error(format!(
                    "{context}Collections could not be updated: {error}"
                )));
            }
            None => {}
        }
    }

    fn log_item_grants(
        &mut self,
        context: &str,
        grants: Option<&Result<AuthoredGrantReport, String>>,
    ) {
        fn item_count(count: usize) -> String {
            format!("{count} {}", if count == 1 { "item" } else { "items" })
        }
        match grants {
            Some(Ok(grants)) => {
                if let Some(backup) = &grants.backup_path {
                    self.log.push(LogEntry::info(format!(
                        "Added {} to {}. Backup: {}",
                        item_count(grants.added.len()),
                        grants.account_path.display(),
                        backup.display()
                    )));
                }
                if !grants.full.is_empty() {
                    self.log.push(LogEntry::error(format!(
                        "{} not added because a bucket is full",
                        item_count(grants.full.len())
                    )));
                }
                if !grants.equipped.is_empty() {
                    self.log.push(LogEntry::info(format!(
                        "Equipped the authored subclass on {} {}",
                        grants.equipped.len(),
                        if grants.equipped.len() == 1 {
                            "character"
                        } else {
                            "characters"
                        }
                    )));
                }
            }
            Some(Err(error)) => {
                self.log.push(LogEntry::error(format!(
                    "{context}authored items could not be added: {error}"
                )));
            }
            None => {}
        }
    }

    /// Runs the install's account step again for the installed generation, in a worker.
    pub(super) fn start_account_resync(&mut self) {
        if self.account_resync_receiver.is_some()
            || self.install_receiver.is_some()
            || self.uninstall.open
        {
            return;
        }
        let target = self.packages.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(crate::install::resync_account(
                &target,
                sundial::package_authoring::destiny_is_running,
            ));
        });
        self.account_resync_receiver = Some(receiver);
        self.log.push(LogEntry::info(
            "Started the account resync in a background worker",
        ));
    }

    pub(super) fn poll_account_resync(&mut self) {
        let Some(receiver) = &self.account_resync_receiver else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("The account resync stopped without a result".to_owned())
            }
        };
        self.account_resync_receiver = None;
        // A disconnected worker can also have written part of the account. The
        // host refreshes only when it is safe to replace its current document.
        self.account_changed = true;
        match result {
            Ok(report) => {
                self.log.push(LogEntry::info(format!(
                    "Resynced the account from the installed packages: {} authored unlocks",
                    report.authored_unlocks
                )));
                self.log_profile_sync("Account resync: ", Some(&report.profile_sync));
                self.log_item_grants("Account resync: ", report.item_grants.as_ref());
            }
            Err(error) => {
                self.log
                    .push(LogEntry::error(format!("Account resync failed: {error}")));
            }
        }
    }

    pub(super) fn poll_install(&mut self) {
        self.install_status.poll(&mut self.log);
        let Some(receiver) = &self.install_receiver else {
            return;
        };
        let mut finished = false;
        let mut installed = false;
        match receiver.try_recv() {
            Ok(Ok(report)) => {
                self.installed.request();
                if let Some(path) = &report.cleaned_account {
                    self.log.push(LogEntry::info(format!(
                        "Applied the reviewed account changes to {}. Original account and packages are backed up in {} (excluded from automatic pruning)",
                        path.display(),
                        report.backup_directory.display()
                    )));
                }
                self.log_profile_sync("Packages installed, but ", report.profile_sync.as_ref());
                self.log_item_grants("Packages installed, but ", report.item_grants.as_ref());
                let cache_status = report.invalidated_sunrise_cache.as_ref().map_or_else(
                    || "no Sunrise build-data cache was present".to_owned(),
                    |cache| {
                        if let Some(quarantine) = &cache.retained_quarantine_path {
                            format!(
                                "Sunrise build-data cache invalidated. Cache backup: {}. Retained quarantine: {}",
                                cache.backup_path.display(),
                                quarantine.display()
                            )
                        } else {
                            format!(
                                "Sunrise build-data cache invalidated. Cache backup: {}",
                                cache.backup_path.display()
                            )
                        }
                    },
                );
                let header_cache_status = if report.invalidated_package_header_caches.is_empty() {
                    "no client package-header cache was present".to_owned()
                } else {
                    format!(
                        "{} client package-header cache(s) invalidated",
                        report.invalidated_package_header_caches.len()
                    )
                };
                self.log.push(LogEntry::info(format!(
                    "Installed {} authored packages to {}. Backup: {}. {}. {}",
                    report.artifacts.len(),
                    report.target_packages_directory.display(),
                    report.backup_directory.display(),
                    cache_status,
                    header_cache_status,
                )));
                if let Some(recipes) = &report.recipe_backup_directory {
                    self.log.push(LogEntry::info(format!(
                        "Backed up staged recipe snapshots to {}",
                        recipes.display()
                    )));
                }
                if !report.removed_obsolete_packages.is_empty() {
                    self.log.push(LogEntry::info(format!(
                        "Removed {} obsolete authored runtime package(s). Originals are in the installation backup",
                        report.removed_obsolete_packages.len()
                    )));
                }
                if !report.pruned_backup_directories.is_empty() {
                    self.log.push(LogEntry::info(format!(
                        "Removed {} old automatic package {}",
                        report.pruned_backup_directories.len(),
                        if report.pruned_backup_directories.len() == 1 {
                            "backup"
                        } else {
                            "backups"
                        }
                    )));
                }
                if let Some(error) = &report.backup_prune_warning {
                    self.log.push(LogEntry::error(format!(
                        "Packages installed, but old backups could not be removed: {error}"
                    )));
                }
                self.latest_install = Some(Ok(report));
                self.install_receiver = None;
                finished = true;
                installed = true;
            }
            Ok(Err(error)) => {
                self.log
                    .push(LogEntry::error(format!("Installation failed: {error}")));
                self.latest_install = Some(Err(error));
                self.install_receiver = None;
                finished = true;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                let error = "The installation stopped without a result".to_owned();
                self.log.push(LogEntry::error(&error));
                self.latest_install = Some(Err(error));
                self.install_receiver = None;
                finished = true;
            }
        }
        if finished {
            self.install_status.poll(&mut self.log);
            self.install_status.finish();
            if let Some(Err(error)) = &self.latest_install {
                self.install_status.activity.push(
                    self.install_status.elapsed,
                    format!("Installation failed: {error}"),
                );
            }
            if installed {
                self.packages_changed = true;
                self.catalog_force_rebuild = true;
                self.log.push(LogEntry::info(
                    "Queued a forced rebuild of Sundial's shared Shadowkeep catalog cache",
                ));
            }
            self.reset_catalog_load();
        }
    }
}
