//! Installation loading, catalog refresh, and external-change handling.
use crate::app::account_workspace as account;

use super::account_workspace::WorkspaceDocument;
use super::background_tasks::{CatalogTask, CatalogTaskEvent, CatalogTaskKind, PendingInstallLoad};
use super::persistence_compatibility::PersistenceCompatibility;
use super::preferences::{SettingsLayout, SettingsPathResolution};
use super::settings::{
    catalog_path, load_workspace_json, missing_settings_message,
    resolve_settings_path, validate_workspace_document,
};
use super::{
    PendingFutureSchemaLoad, SundialApp, WORKSPACE_REFRESH_POLL_INTERVAL, equipment,
    persistence_compatibility, platform, should_poll_pending_workspace_refresh,
    should_refresh_workspace_on_focus,
};
use crate::catalog::{Catalog as Manifest, CatalogProgress};
use crate::game_settings;
use eframe::egui;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::Instant;

impl SundialApp {
    pub(super) fn reload(&mut self) -> bool {
        match load_workspace_json(&self.settings_path) {
            Ok(json) => {
                let doc = WorkspaceDocument::load(json, &self.settings_path);
                self.install_reloaded_document(doc, false);
                true
            }
            Err(error) => {
                self.set_status(error, true);
                false
            }
        }
    }

    pub(super) fn refresh_after_focus_if_needed(&mut self, ctx: &egui::Context, focused: bool) {
        let now = Instant::now();
        if should_refresh_workspace_on_focus(
            self.window_was_focused,
            focused,
            self.has_unsaved_changes(),
        ) {
            self.workspace_refresh_pending = true;
            self.next_workspace_refresh_poll = now;
        }
        self.window_was_focused = focused;
        if !should_poll_pending_workspace_refresh(
            focused,
            self.has_unsaved_changes(),
            self.workspace_refresh_pending,
        ) {
            return;
        }
        if now < self.next_workspace_refresh_poll {
            ctx.request_repaint_after(self.next_workspace_refresh_poll - now);
            return;
        }
        match platform::destiny_is_running() {
            Ok(true) => {
                self.next_workspace_refresh_poll = now + WORKSPACE_REFRESH_POLL_INTERVAL;
                ctx.request_repaint_after(WORKSPACE_REFRESH_POLL_INTERVAL);
                return;
            }
            Ok(false) => {}
            Err(error) => {
                self.workspace_refresh_pending = false;
                self.set_status(
                    format!("Could not check whether Sunrise data should refresh: {error}"),
                    true,
                );
                return;
            }
        }
        let json = match load_workspace_json(&self.settings_path) {
            Ok(json) => json,
            Err(error) => {
                self.next_workspace_refresh_poll = now + WORKSPACE_REFRESH_POLL_INTERVAL;
                ctx.request_repaint_after(WORKSPACE_REFRESH_POLL_INTERVAL);
                self.set_status(format!("Could not refresh Sunrise data yet: {error}"), true);
                return;
            }
        };
        let document = WorkspaceDocument::load(json, &self.settings_path);
        self.workspace_refresh_pending = false;
        if document != self.persisted_document {
            self.install_reloaded_document(document, true);
        } else {
            self.refresh_runtime_inspection();
        }
    }

    pub(super) fn install_reloaded_document(
        &mut self,
        document: WorkspaceDocument,
        automatic: bool,
    ) {
        let blocked_reason = document.account_editing_blocked().map(str::to_owned);
        let warning = validate_workspace_document(&document).err();
        self.replace_loaded_document(document);
        let action = if automatic { "Refreshed" } else { "Reloaded" };
        if let Some(reason) = blocked_reason {
            self.set_status(
                format!("{action} Sunrise data, but account editing is blocked: {reason}"),
                true,
            );
        } else if let Some(warning) = warning {
            self.set_status(
                format!(
                    "{action} with an unexpected setting: {warning}. Correct invalid known settings before saving edited JSON; a safety copy of the loaded source will be created beside settings.json"
                ),
                true,
            );
        } else if automatic {
            self.set_status("Refreshed Sunrise data after returning to Sundial", false);
        } else {
            self.set_status("Reloaded Sunrise data", false);
        }
        if self.preferences.troubleshooting_logging {
            let _ = self.append_troubleshooting_snapshot();
        }
    }

    /// Loaded sources start a new history, including loads accepted midway through a frame.
    pub(super) fn replace_loaded_document(&mut self, document: WorkspaceDocument) {
        self.class_armor_defaults = account::class_armor_default_characters(&document);
        self.persisted_document = document.clone();
        self.document = document;
        self.progression_ui.invalidate_document();
        self.refresh_sunrise_version();
        self.source_warning = validate_workspace_document(&self.document).err();
        self.selected_character = self
            .selected_character
            .min(self.character_count().saturating_sub(1));
        self.clear_picker_state();
        self.sync_raw_json();
        self.dirty = false;
        self.undo_history.clear();
        self.redo_history.clear();
        self.suppress_history_record = true;
    }

    pub(super) fn refresh_sunrise_version(&mut self) {
        self.refresh_runtime_inspection();
    }

    pub(super) fn clear_picker_state(&mut self) {
        self.searches.clear();
        self.plug_searches.clear();
        self.key_binding_ui.clear_pickers();
    }

    pub(super) fn choose_install(&mut self, ctx: &egui::Context) {
        if self.package_authoring_open {
            self.set_status("Close Parhelion before choosing another installation", true);
            return;
        }
        if self.has_unsaved_changes() {
            self.set_status(
                "Save or reload your changes before choosing another installation",
                true,
            );
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_directory(&self.install_path)
            .pick_folder()
        else {
            return;
        };
        match resolve_settings_path(&path, None) {
            SettingsPathResolution::Found(layout, settings_path) => {
                self.load_install(ctx, path, settings_path, layout);
            }
            SettingsPathResolution::Missing => {
                self.set_status(missing_settings_message(&path), true);
            }
            SettingsPathResolution::Ambiguous => {
                self.pending_install_choice = Some(path);
            }
        }
    }

    pub(super) fn load_install(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        settings_path: PathBuf,
        settings_layout: SettingsLayout,
    ) {
        self.pending_future_schema = None;
        let document = match load_workspace_json(&settings_path) {
            Ok(document) => document,
            Err(error) => {
                self.set_status(error, true);
                return;
            }
        };
        if let Some(schema_version) = game_settings::future_schema_version(&document) {
            self.pending_future_schema = Some(PendingFutureSchemaLoad {
                install_path: path,
                settings_path,
                settings_layout,
                schema_version,
            });
            return;
        }
        self.begin_install_load(ctx, path, settings_path, settings_layout, document);
    }

    pub(super) fn load_future_schema_install(
        &mut self,
        ctx: &egui::Context,
        pending: PendingFutureSchemaLoad,
    ) {
        match load_workspace_json(&pending.settings_path) {
            Ok(document) => self.begin_install_load(
                ctx,
                pending.install_path,
                pending.settings_path,
                pending.settings_layout,
                document,
            ),
            Err(error) => self.set_status(error, true),
        }
    }

    pub(super) fn begin_install_load(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        settings_path: PathBuf,
        settings_layout: SettingsLayout,
        document: Value,
    ) {
        let install_path = path.clone();
        self.start_catalog_task(
            ctx,
            install_path,
            false,
            CatalogTaskKind::LoadInstall(PendingInstallLoad {
                install_path: path,
                settings_path,
                settings_layout,
                document,
            }),
        );
    }

    pub(super) fn apply_install_load(&mut self, pending: PendingInstallLoad, manifest: Manifest) {
        let PendingInstallLoad {
            install_path,
            settings_path,
            settings_layout,
            document,
        } = pending;
        self.install_path = install_path;
        self.settings_path = settings_path;
        self.settings_layout = settings_layout;
        self.persistence_compatibility = PersistenceCompatibility::inspect(&self.install_path);
        self.manifest = manifest;
        self.armor_stats_adjuster = equipment::ArmorStatsAdjusterState::default();
        self.hash_inspection.close();
        self.progression_ui.reset_navigation();
        self.collections_ui.reset_navigation();
        let document = WorkspaceDocument::load(document, &self.settings_path);
        let warning = validate_workspace_document(&document).err();
        self.selected_character = 0;
        self.replace_loaded_document(document);
        match self.save_preferences() {
            Ok(()) => match warning {
                Some(warning) => self.set_status(
                    format!(
                        "Install loaded with an unexpected setting: {warning}. Correct invalid known settings before saving edited JSON; a safety copy of the loaded source will be created beside settings.json"
                    ),
                    true,
                ),
                None if self.persistence_compatibility.detected() => {
                    self.set_status(persistence_compatibility::WARNING_MESSAGE, true);
                }
                None => self.set_status("Shadowkeep install and Sunrise settings loaded", false),
            },
            Err(error) => self.set_status(
                format!("Install loaded, but its location could not be remembered: {error}"),
                true,
            ),
        }
    }

    pub(super) fn start_catalog_task(
        &mut self,
        ctx: &egui::Context,
        install_path: PathBuf,
        force: bool,
        kind: CatalogTaskKind,
    ) {
        if self.catalog_task.is_some() {
            return;
        }
        let Some(cache) = catalog_path() else {
            self.manifest.resume_package_access();
            self.set_status("Could not locate Sundial's local catalog folder", true);
            return;
        };
        let (sender, receiver) = mpsc::channel();
        self.catalog_task = Some(CatalogTask {
            kind,
            receiver,
            progress: CatalogProgress {
                message: "Starting the local catalog…",
                completed: 0,
                total: 0,
            },
        });
        let ctx = ctx.clone();
        thread::spawn(move || {
            let progress_sender = sender.clone();
            let progress_ctx = ctx.clone();
            let result = Manifest::load_or_scan_with_progress(
                &install_path,
                cache,
                force,
                move |progress| {
                    let _ = progress_sender.send(CatalogTaskEvent::Progress(progress));
                    progress_ctx.request_repaint();
                },
            );
            let _ = sender.send(CatalogTaskEvent::Finished(Box::new(result)));
            ctx.request_repaint();
        });
    }

    pub(super) fn poll_catalog_task(&mut self) {
        loop {
            let event = match self
                .catalog_task
                .as_ref()
                .map(|task| task.receiver.try_recv())
            {
                Some(Ok(event)) => event,
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.catalog_task = None;
                    self.manifest.resume_package_access();
                    self.set_status("The background catalog task stopped unexpectedly", true);
                    break;
                }
            };
            match event {
                CatalogTaskEvent::Progress(progress) => {
                    if let Some(task) = &mut self.catalog_task {
                        task.progress = progress;
                    }
                }
                CatalogTaskEvent::Finished(result) => {
                    let Some(task) = self.catalog_task.take() else {
                        self.set_status("A catalog task finished without an active request", true);
                        break;
                    };
                    match (task.kind, *result) {
                        (CatalogTaskKind::LoadInstall(pending), Ok(manifest)) => {
                            if self.has_unsaved_changes() {
                                self.set_status(
                                    "Install not loaded because settings changed while its catalog was loading. Save or reload the current settings, then choose the installation again.",
                                    true,
                                );
                            } else {
                                self.apply_install_load(pending, manifest);
                            }
                        }
                        (CatalogTaskKind::Rebuild, Ok(manifest)) => {
                            self.manifest = manifest;
                            self.armor_stats_adjuster =
                                equipment::ArmorStatsAdjusterState::default();
                            self.hash_inspection.close();
                            self.progression_ui.reset_navigation();
                            self.collections_ui.reset_navigation();
                            self.clear_picker_state();
                            self.set_status(
                                "Catalog rebuilt from the installed game packages",
                                false,
                            );
                        }
                        (CatalogTaskKind::LoadInstall(_), Err(error)) => {
                            self.set_status(format!("Install not loaded: {error}"), true);
                        }
                        (CatalogTaskKind::Rebuild, Err(error)) => {
                            self.manifest.resume_package_access();
                            self.set_status(format!("Catalog not rebuilt: {error}"), true);
                        }
                    }
                    break;
                }
            }
        }
    }

    pub(super) fn rebuild_catalog(&mut self, ctx: &egui::Context) {
        if self.package_authoring_open {
            self.set_status("Close Parhelion before rebuilding the catalog", true);
            return;
        }
        self.set_status("Scanning installed Shadowkeep packages…", false);
        self.start_catalog_task(
            ctx,
            self.install_path.clone(),
            true,
            CatalogTaskKind::Rebuild,
        );
    }

    pub(super) fn reload_catalog_after_authoring(&mut self, ctx: &egui::Context) {
        self.set_status("Loading the updated package catalog…", false);
        self.start_catalog_task(
            ctx,
            self.install_path.clone(),
            false,
            CatalogTaskKind::Rebuild,
        );
    }
}
