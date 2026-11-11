//! Installation loading, catalog refresh, and external-change handling.
use crate::app::account_workspace as account;

use super::account_workspace::WorkspaceDocument;
use super::background_tasks::{CatalogTask, CatalogTaskEvent, CatalogTaskKind, PendingInstallLoad};
use super::preferences::{SettingsLayout, SettingsPathResolution};
use super::runtime_state::RuntimeState;
use super::settings::{
    catalog_path, load_workspace_json, missing_settings_message, resolve_settings_path,
    validate_workspace_document,
};
use super::{
    PendingFutureSchemaLoad, SundialApp, WORKSPACE_REFRESH_POLL_INTERVAL, equipment, platform,
    runtime_state, should_poll_pending_workspace_refresh, should_refresh_workspace_on_focus,
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
    /// Inspect the destination before choosing its durable account backend. The same inspection
    /// stays with the loaded document so banners and save validation describe that source.
    pub(super) fn load_workspace_document(&mut self, json: Value) -> WorkspaceDocument {
        self.refresh_runtime_inspection();
        WorkspaceDocument::load(json, &self.settings_path, self.dawn_account_runtime())
    }

    pub(super) fn reload(&mut self) -> bool {
        match load_workspace_json(&self.settings_path) {
            Ok(json) => {
                let doc = self.load_workspace_document(json);
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
        if self.package_authoring_busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        // While Parhelion is open or a catalog is loading, a refresh could pair the account
        // Parhelion wrote with the previous build's catalog. The account follows the catalog.
        if self.package_authoring_open || self.catalog_task.is_some() {
            return;
        }
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
        let document = self.load_workspace_document(json);
        self.workspace_refresh_pending = false;
        if document != self.persisted_document {
            self.install_reloaded_document(document, true);
        }
    }

    pub(super) fn install_reloaded_document(
        &mut self,
        document: WorkspaceDocument,
        automatic: bool,
    ) {
        let runtime_name = if document.account_is_dawn() {
            "Dawn"
        } else {
            "Sunrise"
        };
        let blocked_reason = document.account_editing_blocked().map(str::to_owned);
        let warning = validate_workspace_document(&document).err();
        self.replace_loaded_document(document);
        let action = if automatic { "Refreshed" } else { "Reloaded" };
        if let Some(reason) = blocked_reason {
            self.set_status(
                format!("{action} {runtime_name} data, but account editing is blocked: {reason}"),
                true,
            );
        } else if let Some(warning) = warning {
            self.set_status(
                format!(
                    "{action} with an unexpected setting: {warning}. Correct invalid settings before saving JSON edits. A safety copy is made beside settings.json"
                ),
                true,
            );
        } else if automatic {
            self.set_status(format!("Refreshed {runtime_name} data"), false);
        } else {
            self.set_status(format!("Reloaded {runtime_name} data"), false);
        }
        if self.preferences.troubleshooting_logging {
            let _ = self.append_troubleshooting_snapshot();
        }
    }

    /// Loaded sources start a new history.
    pub(super) fn replace_loaded_document(&mut self, document: WorkspaceDocument) {
        self.class_armor_defaults = account::class_armor_default_characters(&document);
        self.persisted_document = document.clone();
        self.document = document;
        self.source_warning = validate_workspace_document(&self.document).err();
        self.selected_character = self
            .selected_character
            .min(self.character_count().saturating_sub(1));
        self.clear_picker_state();
        self.sync_raw_json();
        self.dirty = false;
        self.reset_document_history();
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
            .set_title("Select Project Sunrise or Dawn Install (Contains destiny2.exe)")
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
        self.begin_install_load(
            ctx,
            PendingInstallLoad {
                install_path: path,
                settings_path,
                settings_layout,
                accepted_future_schema: None,
            },
        );
    }

    pub(super) fn load_future_schema_install(
        &mut self,
        ctx: &egui::Context,
        pending: PendingFutureSchemaLoad,
    ) {
        self.begin_install_load(
            ctx,
            PendingInstallLoad {
                install_path: pending.install_path,
                settings_path: pending.settings_path,
                settings_layout: pending.settings_layout,
                accepted_future_schema: Some(pending.schema_version),
            },
        );
    }

    fn read_install_settings(&mut self, pending: &PendingInstallLoad) -> Option<Value> {
        let document = match load_workspace_json(&pending.settings_path) {
            Ok(document) => document,
            Err(error) => {
                self.set_status(error, true);
                return None;
            }
        };
        if let Some(schema_version) = game_settings::future_schema_version(&document)
            && Some(schema_version) != pending.accepted_future_schema
        {
            self.pending_future_schema = Some(PendingFutureSchemaLoad {
                install_path: pending.install_path.clone(),
                settings_path: pending.settings_path.clone(),
                settings_layout: pending.settings_layout,
                schema_version,
            });
            return None;
        }
        Some(document)
    }

    fn begin_install_load(&mut self, ctx: &egui::Context, pending: PendingInstallLoad) {
        if self.read_install_settings(&pending).is_none() {
            return;
        }
        self.start_catalog_task(
            ctx,
            pending.install_path.clone(),
            false,
            CatalogTaskKind::LoadInstall(pending),
        );
    }

    pub(super) fn apply_install_load(
        &mut self,
        pending: PendingInstallLoad,
        mut manifest: Manifest,
    ) -> bool {
        // Settings can change while the catalog loads, including the schema the user reviewed.
        let Some(document) = self.read_install_settings(&pending) else {
            return false;
        };
        self.install_path = pending.install_path;
        self.settings_path = pending.settings_path;
        self.settings_layout = pending.settings_layout;
        self.runtime_state = RuntimeState::inspect(&self.install_path);
        manifest.adopt_package_access(&self.manifest, self.package_authoring_open);
        self.manifest = manifest;
        self.armor_stats_adjuster = equipment::ArmorStatsAdjusterState::default();
        self.hash_inspection.reset();
        self.progression_ui.reset_navigation();
        self.collections_ui.reset_navigation();
        let document = self.load_workspace_document(document);
        let warning = validate_workspace_document(&document).err();
        self.selected_character = 0;
        self.replace_loaded_document(document);
        match warning {
            Some(warning) => self.set_status(
                format!(
                    "Install loaded with an unexpected setting: {warning}. Correct invalid settings before saving JSON edits. A safety copy is made beside settings.json"
                ),
                true,
            ),
            None if self.runtime_state.detected() => {
                self.set_status(runtime_state::WARNING_MESSAGE, true);
            }
            None => self.set_status("Sunrise install and settings loaded", false),
        }
        true
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
                                    "Install not loaded because settings changed during loading. Save or reload, then choose the install again.",
                                    true,
                                );
                            } else if self.apply_install_load(pending, manifest)
                                && let Err(error) = self.save_preferences()
                            {
                                self.set_status(
                                    format!("Install loaded, but its location could not be remembered: {error}"),
                                    true,
                                );
                            }
                        }
                        (CatalogTaskKind::Rebuild, Ok(mut manifest)) => {
                            // Previews and viewers keep the access they were handed, so the
                            // rebuilt catalog takes it over rather than leaving them paused.
                            manifest
                                .adopt_package_access(&self.manifest, self.package_authoring_open);
                            self.manifest = manifest;
                            self.armor_stats_adjuster =
                                equipment::ArmorStatsAdjusterState::default();
                            self.hash_inspection.reset();
                            self.progression_ui.reset_navigation();
                            self.collections_ui.reset_navigation();
                            self.clear_picker_state();
                            self.set_status(
                                "Catalog rebuilt from the installed game packages",
                                false,
                            );
                            self.refresh_account_after_authoring();
                        }
                        (CatalogTaskKind::LoadInstall(_), Err(error)) => {
                            self.set_status(format!("Install not loaded: {error}"), true);
                        }
                        (CatalogTaskKind::Rebuild, Err(error)) => {
                            self.manifest.resume_package_access();
                            self.set_status(format!("Catalog not rebuilt: {error}"), true);
                            self.refresh_account_after_authoring();
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

    /// Loads the account Parhelion changed once its catalog is in place, without waiting for
    /// focus or for the game to close. Parhelion refuses to change either while the game runs,
    /// so the account on disk is the one it wrote unless the game has since started. Unsaved
    /// Sundial edits are kept, and the usual refresh loads the change after they are resolved.
    pub(super) fn refresh_account_after_authoring(&mut self) {
        if !std::mem::take(&mut self.package_authoring_account_changed) {
            return;
        }
        self.workspace_refresh_pending = true;
        self.next_workspace_refresh_poll = Instant::now();
        if self.has_unsaved_changes() {
            return;
        }
        match load_workspace_json(&self.settings_path) {
            Ok(json) => {
                let document = self.load_workspace_document(json);
                self.workspace_refresh_pending = false;
                if document != self.persisted_document {
                    self.install_reloaded_document(document, true);
                }
            }
            Err(error) => self.set_status(
                format!("Could not load the account Parhelion changed: {error}"),
                true,
            ),
        }
    }

    pub(super) fn reload_catalog_after_authoring(&mut self, ctx: &egui::Context) {
        self.set_status("Loading the updated package catalog…", false);
        self.start_catalog_task(
            ctx,
            self.install_path.clone(),
            false,
            CatalogTaskKind::Rebuild,
        );
        if self.catalog_task.is_none() {
            self.refresh_account_after_authoring();
        }
    }
}
