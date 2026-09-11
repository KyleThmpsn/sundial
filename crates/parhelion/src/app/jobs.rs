//! Catalog, runtime, build and install background job coordination.
use super::*;

#[cfg(test)]
mod tests;

/// Keep the request identity with its worker, including when it exits without a result.
pub(super) struct RuntimeGraphJob {
    key: RuntimeGraphKey,
    receiver: Receiver<Result<WeaponRuntimeGraph, String>>,
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
                self.replacement_review = Some(Err("Account review stopped unexpectedly. Return to the build and review installation again.".into()));
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
                            Ok(catalog) => {
                                self.donor_summaries = catalog.weapon_donors();
                                self.sandbox_perk_choices = catalog
                                    .weapon_sandbox_perk_choices_from(
                                        crate::package_profile::is_stock_item_definition,
                                    );
                                self.trait_choices = catalog.weapon_trait_choices();
                                let supported = self
                                    .donor_summaries
                                    .iter()
                                    .filter(|donor| donor.collection_backed)
                                    .count();
                                self.log.push(LogEntry::info(format!(
                                    "Loaded {} weapon donors from Sundial ({} collection-backed)",
                                    self.donor_summaries.len(),
                                    supported
                                )));
                                self.catalog = Some(catalog);
                                self.bind_default_donor();
                            }
                            Err(error) => {
                                self.drop_loaded_catalog();
                                self.log.push(LogEntry::error(format!(
                                    "Could not load Sundial's investment catalog: {error}"
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
                self.log
                    .push(LogEntry::error("The Sundial catalog worker panicked"));
            }
            if disconnected && !worker_finished {
                self.log.push(LogEntry::error(
                    "The Sundial catalog worker stopped without a result",
                ));
            }
            if self.catalog_reload_pending {
                self.catalog_reload_pending = false;
                self.catalog_load_requested = false;
            }
        }
    }

    pub(super) fn runtime_graph_key(&self) -> Option<RuntimeGraphKey> {
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
        Some(RuntimeGraphKey::new(
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
        ))
    }

    pub(super) fn ensure_runtime_graph(&mut self, ctx: &egui::Context) {
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
                Err("The runtime-data worker stopped without a result".to_owned())
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
            Err("The runtime-data worker panicked while reading packages".to_owned())
        } else {
            result
        };
        match result {
            Ok(graph) => {
                self.runtime_graph = Some((key, Arc::new(graph)));
                self.runtime_graph_error = None;
            }
            Err(error) => {
                self.runtime_graph = None;
                self.runtime_graph_error = Some((key, error));
            }
        }
    }

    pub(super) fn reset_catalog_load(&mut self) {
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
        let default = self
            .donor_summaries
            .iter()
            .filter(|summary| summary.collection_backed)
            .find_map(|summary| {
                let donor = catalog.weapon_donor(summary.hash)?;
                weapon_authoring_capabilities(&donor)
                    .is_authorable()
                    .then(|| (summary.hash, summary.name.clone()))
            });
        if let Some((hash, name)) = default {
            self.recipe.set_donor(hash, name);
            if !self.recipe_dirty && self.recipe_path.is_none() {
                self.recipe_baseline = self.recipe.clone();
            }
        }
    }

    pub(super) fn start_build(&mut self) {
        let snapshot = match self.save_edits_for_build().and_then(|()| self.snapshot()) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.latest_build = Some(Err(error.clone()));
                self.build_status_open = true;
                self.log
                    .push(LogEntry::error(format!("Build blocked: {error}")));
                return;
            }
        };
        let (sender, receiver) = mpsc::channel();
        let started = Instant::now();
        self.build_started = Some(started);
        self.build_activity = build_status::Activity::default();
        self.install_status = build_status::InstallStatus::default();
        thread::spawn(move || {
            let result = build_and_stage_snapshot_with_progress(&snapshot, |progress| {
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
                    self.log.push(LogEntry::error(
                        "The package build worker stopped without a result",
                    ));
                    self.latest_build = Some(Err(
                        "The package build worker stopped without a result".to_owned(),
                    ));
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
                    let message = build_status::progress_message(
                        progress.phase.label(),
                        progress.current_artifact.as_deref(),
                        progress.completed,
                        progress.total,
                    );
                    let changed = self.build_progress.as_ref().is_none_or(|previous| {
                        previous.phase != progress.phase
                            || previous.current_artifact != progress.current_artifact
                            || previous.completed != progress.completed
                            || previous.total != progress.total
                    });
                    if changed {
                        let same_operation = self.build_progress.as_ref().is_some_and(|previous| {
                            previous.phase == progress.phase
                                && previous.current_artifact == progress.current_artifact
                                && previous.total == progress.total
                                && previous.completed <= progress.completed
                        });
                        if same_operation {
                            self.build_activity.update_last(progress.elapsed, message);
                        } else {
                            self.build_activity.push(progress.elapsed, message.clone());
                            self.log.push(LogEntry::info(message));
                        }
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
                    self.latest_build = Some(Ok(report));
                    self.build_started = None;
                    self.build_receiver = None;
                    return;
                }
                BuildWorkerEvent::Finished {
                    result: Err(error),
                    elapsed,
                } => {
                    if let Some(progress) = &mut self.build_progress {
                        progress.elapsed = elapsed;
                    }
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
        if self.catalog_receiver.is_some()
            || self.catalog_worker.is_some()
            || self.catalog_reload_pending
        {
            self.log.push(LogEntry::error(
                "Installation is waiting for the Sundial catalog scan to finish",
            ));
            return;
        }
        if self.runtime_graph_job.is_some()
            || self.runtime_donors.busy()
            || self.runtime_dependencies.busy()
            || self.perk_workbench.busy()
        {
            self.log.push(LogEntry::error(
                "Installation is waiting for the runtime-data scan to finish",
            ));
            return;
        }
        let Some(Ok(build)) = self.latest_build.as_ref() else {
            self.log.push(LogEntry::error(
                "Installation requires a successfully validated staged build",
            ));
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

    pub(super) fn poll_install(&mut self) {
        self.install_status.poll(&mut self.log);
        let Some(receiver) = &self.install_receiver else {
            return;
        };
        let mut finished = false;
        let mut installed = false;
        match receiver.try_recv() {
            Ok(Ok(report)) => {
                if let Some(path) = &report.cleaned_account {
                    self.log.push(LogEntry::info(format!("Applied the reviewed account changes to {}. Original account and packages are backed up in {} (excluded from automatic pruning)", path.display(), report.backup_directory.display())));
                }
                match &report.profile_sync {
                    Some(Ok(sync)) => {
                        self.log.push(LogEntry::info(format!(
                            "Synchronized {}/{} authored collection unlocks in {}; backup: {}",
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
                        let error = format!(
                            "Packages were installed, but authored collection unlocks were not synchronized: {error}"
                        );
                        self.log.push(LogEntry::error(&error));
                    }
                    None => {}
                }
                let cache_status = report.invalidated_sunrise_cache.as_ref().map_or_else(
                    || "no Sunrise build-data cache was present".to_owned(),
                    |cache| {
                        if let Some(quarantine) = &cache.retained_quarantine_path {
                            format!(
                                "Sunrise build-data cache invalidated; cache backup: {}; retained quarantine: {}",
                                cache.backup_path.display(),
                                quarantine.display()
                            )
                        } else {
                            format!(
                                "Sunrise build-data cache invalidated; cache backup: {}",
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
                    "Installed {} authored packages to {}; backup: {}; {}; {}",
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
                        "Removed {} obsolete authored runtime package(s); originals are in the installation backup",
                        report.removed_obsolete_packages.len()
                    )));
                }
                if !report.pruned_backup_directories.is_empty() {
                    self.log.push(LogEntry::info(format!(
                        "Removed {} old automatic package backup(s)",
                        report.pruned_backup_directories.len()
                    )));
                }
                if let Some(error) = &report.backup_prune_warning {
                    self.log.push(LogEntry::error(format!(
                        "Packages were installed, but old backup pruning failed: {error}"
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
                let error = "The package installation worker stopped without a result".to_owned();
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
