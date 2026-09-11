use crate::app::account_workspace as account;

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui;
use serde_json::Value;

use crate::{
    account_contract::EQUIPMENT_SLOTS as SLOTS,
    catalog::{Catalog as Manifest, CatalogProgress},
    game_settings,
    package_authoring::{PackageAuthoringPreferences, PackageAuthoringUtility},
    updates::{UpdateCheck, UpdateStatus},
};

mod activity_log;
mod chrome;
mod confirmations;
mod history;
mod json_workspace;
mod preferences_page;
mod recovery;
mod runtime_installation;
mod saving;
mod shortcuts;
mod update;
mod workspace_loading;

mod startup;
use startup::StartupApp;

mod background_tasks;
use background_tasks::CatalogTask;

mod diagnostics;

mod persistence_compatibility;
use persistence_compatibility::PersistenceCompatibility;

mod bootstrap;
use bootstrap::parse_args;

mod save_support;
use save_support::SaveAction;

mod change_review;

pub(crate) mod platform;
use platform::load_logo_texture;
#[cfg(target_os = "linux")]
use platform::{draw_linux_title_bar, load_linux_title_bar_texture};
#[cfg(windows)]
use platform::{set_windows_app_identity, set_windows_taskbar_icon};

mod preferences;
pub use preferences::PlugSelectionMode;
use preferences::{
    CharacterInventoryLayout, InstallSelection, Preferences, SettingsLayout,
    SettingsPathResolution, configure_destiny_symbol_fonts, draw_plug_selection_warning,
};

mod settings;
use settings::{
    catalog_path, detect_sunrise_version, encode_settings, load_workspace_json,
    missing_settings_message, prepare_settings, resolve_settings_path, validate_workspace_document,
};

mod workspace_save;

mod json_editor;
use json_editor::JsonEditorState;

mod equipment;
use equipment::class_name;

mod account_settings;

mod account_details;
mod account_workspace;
use account_workspace::{AccountSourceKind, WorkspaceDocument};

mod account_validation;

mod character_metadata;

mod inventory;

pub(crate) mod components;

pub(crate) mod authoring_bridge;
mod item_editor;

mod glyphs;

mod ui;

mod inspector;

mod inventory_page;

mod progression;

mod collections_page;

const PROJECT_URL: &str = "https://github.com/kylethmpsn/sundial";
const CREDITS_URL: &str = "https://github.com/kylethmpsn/sundial#credits-and-license";
const SUNRISE_URL: &str = "https://github.com/stanuwu/Sunrise";
const TIGER_PKG_URL: &str = "https://github.com/v4nguard/tiger-pkg";
const DISPLAY_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const ARMOR_SLOTS: &[&str] = &["helmet", "gauntlets", "chest", "legs", "class_item"];
const WEAPON_SLOTS: &[&str] = &["kinetic", "energy", "heavy"];
const ITEM_PICKER_MIN_HEIGHT: f32 = 320.0;
const ITEM_PICKER_MAX_HEIGHT: f32 = 420.0;
const PLUG_PICKER_MIN_HEIGHT: f32 = 320.0;
const PLUG_PICKER_MAX_HEIGHT: f32 = 420.0;
const MAIN_SIDEBAR_WIDTH: f32 = 168.0;
const DOCUMENT_HISTORY_LIMIT: usize = 50;
const CHANGE_REVIEW_LIMIT: usize = 120;
const INVENTORY_LAYOUT_PREVIEW_HASH: u64 = 0x26F9_5A00;
const WORKSPACE_REFRESH_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Characters,
    ProfileInventory,
    CharacterInventory,
    GameSettings,
    Progression,
    AdvancedJson,
    Preferences,
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
enum PreferencesTab {
    #[default]
    Editing,
    Interface,
    Installation,
    SavingRecovery,
}

impl PreferencesTab {
    const ALL: [Self; 4] = [
        Self::Editing,
        Self::Interface,
        Self::Installation,
        Self::SavingRecovery,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Interface => "Interface",
            Self::Editing => "Editing",
            Self::Installation => "Installation",
            Self::SavingRecovery => "Saving & Recovery",
        }
    }
}

struct InventoryLayoutPreviewItem {
    hash: u64,
    slot: &'static str,
    slot_label: &'static str,
    bucket_hash: u64,
    class_type: u64,
    power: i64,
}

impl InventoryLayoutPreviewItem {
    fn snapshot(&self) -> equipment::EquippedItemSnapshot {
        equipment::EquippedItemSnapshot {
            slot: self.slot,
            slot_label: self.slot_label,
            bucket_hash: self.bucket_hash,
            raw_item_text: "<preference preview>".to_owned(),
            definition_hash: Some(self.hash),
            definition_text: crate::hash::format_hash_hex(self.hash),
            instance_soid: Some(1),
            instance_soid_text: "0x0000000000000001".to_owned(),
            level: Some(self.power),
            quantity: Some(1),
            flags: Some(0),
            plugs: equipment::EquippedItemPlugs::NativeDefaults,
            issues: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CharacterInventorySourceFilter {
    #[default]
    All,
    Stored,
    Equipped,
}

impl CharacterInventorySourceFilter {
    const fn label(self) -> &'static str {
        match self {
            Self::All => "All items",
            Self::Stored => "Stored only",
            Self::Equipped => "Equipped only",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CharacterInventorySort {
    #[default]
    InventoryOrder,
    Name,
    PowerDescending,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CharacterInventoryLockFilter {
    #[default]
    All,
    Locked,
    Unlocked,
}

impl CharacterInventoryLockFilter {
    const fn label(self) -> &'static str {
        match self {
            Self::All => "Any lock state",
            Self::Locked => "Locked only",
            Self::Unlocked => "Unlocked only",
        }
    }
}

impl CharacterInventorySort {
    const fn label(self) -> &'static str {
        match self {
            Self::InventoryOrder => "Inventory order",
            Self::Name => "Name",
            Self::PowerDescending => "Power: high to low",
        }
    }
}

fn update_detached_window_state(open: &mut bool, generation: &mut u64, requested_open: bool) {
    if *open && !requested_open {
        *generation = generation.wrapping_add(1);
    }
    *open = requested_open;
}

fn should_refresh_workspace_on_focus(
    was_focused: bool,
    focused: bool,
    has_unsaved_changes: bool,
) -> bool {
    focused && !was_focused && !has_unsaved_changes
}

fn should_poll_pending_workspace_refresh(
    focused: bool,
    has_unsaved_changes: bool,
    refresh_pending: bool,
) -> bool {
    focused && !has_unsaved_changes && refresh_pending
}

fn has_save_work(document_changed: bool, raw_json_changed: bool) -> bool {
    document_changed || raw_json_changed
}

fn should_open_json_editor_window_on_selection(
    open_in_second_window: bool,
    selected_view: ViewMode,
) -> bool {
    open_in_second_window && selected_view == ViewMode::AdvancedJson
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ProgressionSection {
    #[default]
    Collections,
    Unlocks,
    Investment,
    Seasonal,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfirmationDialog {
    EnableParhelion,
    ReallyUnsafe,
    ReviewSave,
    DeleteEquipment,
    Reload,
    RestoreSqliteBackup,
    ResetSqliteDefaults,
    ResetDefaults,
    Exit,
}

#[derive(Clone)]
struct DocumentHistoryEntry {
    document: WorkspaceDocument,
    label: String,
}

#[derive(Clone)]
struct PendingEquipmentDelete {
    character_index: usize,
    slot: String,
    item_name: String,
}

#[derive(Clone)]
struct PendingFutureSchemaLoad {
    install_path: PathBuf,
    settings_path: PathBuf,
    settings_layout: SettingsLayout,
    schema_version: u64,
}

struct SundialApp {
    runtime_choice: runtime_installation::RuntimeChoice,
    settings_path: PathBuf,
    settings_layout: SettingsLayout,
    install_path: PathBuf,
    sunrise_version: String,
    manifest: Manifest,
    document: WorkspaceDocument,
    persisted_document: WorkspaceDocument,
    source_warning: Option<String>,
    persistence_compatibility: PersistenceCompatibility,
    class_armor_defaults: HashMap<u64, usize>,
    selected_character: usize,
    searches: HashMap<String, String>,
    plug_searches: HashMap<String, String>,
    character_inventory_query: String,
    character_inventory_source_filter: CharacterInventorySourceFilter,
    character_inventory_lock_filter: CharacterInventoryLockFilter,
    character_inventory_sort: CharacterInventorySort,
    armor_stats_adjuster: equipment::ArmorStatsAdjusterState,
    plug_selection_mode: PlugSelectionMode,
    preferences: Preferences,
    preferences_load_warning: Option<String>,
    troubleshooting_log_error: Option<String>,
    remember_plug_selection_mode_after_confirmation: bool,
    show_dummy_items: bool,
    view_mode: ViewMode,
    preferences_tab: PreferencesTab,
    progression_section: ProgressionSection,
    game_settings_tab: game_settings::Tab,
    key_binding_ui: game_settings::KeyBindingUiState,
    progression_ui: progression::UiState,
    collections_ui: collections_page::UiState,
    hash_inspection: inspector::HashInspectionState,
    raw_json: String,
    raw_json_document: Value,
    json_editor: JsonEditorState,
    json_editor_window_open: bool,
    json_editor_window_generation: u64,
    logo: Option<egui::TextureHandle>,
    #[cfg(target_os = "linux")]
    title_bar_icon: Option<egui::TextureHandle>,
    about_open: bool,
    update_check: UpdateCheck,
    confirmation: Option<ConfirmationDialog>,
    pending_save_action: Option<SaveAction>,
    pending_equipment_delete: Option<PendingEquipmentDelete>,
    pending_sqlite_restore: Option<PathBuf>,
    pending_sqlite_reset: Option<crate::persistence::sqlite_account::ResetPlan>,
    exit_confirmed: bool,
    dirty: bool,
    undo_history: Vec<DocumentHistoryEntry>,
    account_details: account_details::State,
    redo_history: Vec<DocumentHistoryEntry>,
    suppress_history_record: bool,
    status: String,
    status_is_error: bool,
    activity_log: activity_log::ActivityLog,
    activity_log_open: bool,
    window_was_focused: bool,
    workspace_refresh_pending: bool,
    next_workspace_refresh_poll: Instant,
    pending_install_choice: Option<PathBuf>,
    pending_future_schema: Option<PendingFutureSchemaLoad>,
    catalog_task: Option<CatalogTask>,
    package_authoring: Option<Box<dyn PackageAuthoringUtility>>,
    package_authoring_open: bool,
    package_authoring_busy: bool,
    package_authoring_dirty: bool,
    package_authoring_packages_changed: bool,
    destiny_symbol_font_install: Option<PathBuf>,
    destiny_symbol_font_error: Option<String>,
}

impl SundialApp {
    fn new(
        settings_path: PathBuf,
        settings_layout: SettingsLayout,
        install_path: PathBuf,
    ) -> Result<Self, String> {
        Self::new_with_progress(
            settings_path,
            settings_layout,
            install_path,
            Preferences::default(),
            |_| {},
        )
    }

    fn new_with_progress(
        settings_path: PathBuf,
        settings_layout: SettingsLayout,
        install_path: PathBuf,
        mut preferences: Preferences,
        report: impl FnMut(CatalogProgress),
    ) -> Result<Self, String> {
        let json_document = load_workspace_json(&settings_path)?;
        let cache = catalog_path().ok_or("Could not locate Sundial's local catalog folder")?;
        let manifest = Manifest::load_or_scan_with_progress(&install_path, cache, false, report)?;
        let sunrise_version = detect_sunrise_version(&install_path);
        let document = WorkspaceDocument::load(json_document, &settings_path);
        let source_warning = validate_workspace_document(&document).err();
        let persistence_compatibility = PersistenceCompatibility::inspect(&install_path);
        let class_armor_defaults = account::class_armor_default_characters(&document);
        let raw_json = encode_settings_for_editor(document.json())?;
        let raw_json_document = document.json().clone();
        let persisted_document = document.clone();
        preferences.normalize_for_runtime();
        let default_plug_selection_mode = preferences.default_plug_selection_mode;
        let mut app = Self {
            runtime_choice: runtime_installation::RuntimeChoice::inspect(&install_path),
            settings_path,
            settings_layout,
            install_path,
            sunrise_version,
            manifest,
            document,
            persisted_document,
            source_warning: source_warning.clone(),
            persistence_compatibility: persistence_compatibility.clone(),
            class_armor_defaults,
            selected_character: 0,
            searches: HashMap::new(),
            plug_searches: HashMap::new(),
            character_inventory_query: String::new(),
            character_inventory_source_filter: CharacterInventorySourceFilter::default(),
            character_inventory_lock_filter: CharacterInventoryLockFilter::default(),
            character_inventory_sort: CharacterInventorySort::default(),
            armor_stats_adjuster: equipment::ArmorStatsAdjusterState::default(),
            plug_selection_mode: default_plug_selection_mode,
            preferences,
            preferences_load_warning: None,
            troubleshooting_log_error: None,
            remember_plug_selection_mode_after_confirmation: false,
            show_dummy_items: false,
            view_mode: ViewMode::Characters,
            preferences_tab: PreferencesTab::default(),
            progression_section: ProgressionSection::default(),
            game_settings_tab: game_settings::Tab::Player,
            key_binding_ui: game_settings::KeyBindingUiState::default(),
            progression_ui: progression::UiState::default(),
            collections_ui: collections_page::UiState::default(),
            hash_inspection: inspector::HashInspectionState::default(),
            raw_json,
            raw_json_document,
            json_editor: JsonEditorState::default(),
            json_editor_window_open: false,
            json_editor_window_generation: 0,
            logo: None,
            #[cfg(target_os = "linux")]
            title_bar_icon: None,
            about_open: false,
            update_check: UpdateCheck::default(),
            confirmation: None,
            pending_save_action: None,
            pending_equipment_delete: None,
            pending_sqlite_restore: None,
            pending_sqlite_reset: None,
            exit_confirmed: false,
            dirty: false,
            undo_history: Vec::new(),
            account_details: account_details::State::default(),
            redo_history: Vec::new(),
            suppress_history_record: false,
            status: source_warning.as_ref().map_or_else(
                || {
                    if persistence_compatibility.detected() {
                        persistence_compatibility::WARNING_MESSAGE.to_owned()
                    } else {
                        "Ready".to_owned()
                    }
                },
                |warning| {
                    format!(
                        "Loaded with an unexpected setting: {warning}. A safety copy will be created beside settings.json before saving"
                    )
                },
            ),
            status_is_error: source_warning.is_some() || persistence_compatibility.detected(),
            activity_log: activity_log::ActivityLog::default(),
            activity_log_open: false,
            window_was_focused: true,
            workspace_refresh_pending: false,
            next_workspace_refresh_poll: Instant::now(),
            pending_install_choice: None,
            pending_future_schema: None,
            catalog_task: None,
            package_authoring: None,
            package_authoring_open: false,
            package_authoring_busy: false,
            package_authoring_dirty: false,
            package_authoring_packages_changed: false,
            destiny_symbol_font_install: None,
            destiny_symbol_font_error: None,
        };
        app.activity_log.enable_file();
        app.activity_log
            .push(app.status.clone(), app.status_is_error);
        if app.preferences.troubleshooting_logging {
            let report = app.build_troubleshooting_report();
            if let Err(error) = diagnostics::initialize_log(&report) {
                app.troubleshooting_log_error = Some(error);
            }
        }
        Ok(app)
    }

    fn ensure_destiny_symbol_font(&mut self, ctx: &egui::Context) {
        if self.destiny_symbol_font_install.as_ref() == Some(&self.install_path) {
            return;
        }

        self.destiny_symbol_font_error =
            configure_destiny_symbol_fonts(ctx, &self.install_path).err();
        self.destiny_symbol_font_install = Some(self.install_path.clone());
    }

    fn request_equipment_delete(&mut self, character_index: usize, slot: &str, item_name: &str) {
        self.pending_equipment_delete = Some(PendingEquipmentDelete {
            character_index,
            slot: slot.to_owned(),
            item_name: item_name.to_owned(),
        });
        self.confirmation = Some(ConfirmationDialog::DeleteEquipment);
    }

    fn select_view(&mut self, view: ViewMode) {
        if self.view_mode == view {
            if should_open_json_editor_window_on_selection(
                self.preferences.always_open_json_editor_in_second_window,
                view,
            ) && !self.json_editor_window_open
            {
                self.sync_raw_json_if_stale();
                self.json_editor.restore_location_next_draw();
                self.set_json_editor_window_open(true);
            }
            return;
        }
        if self.view_mode == ViewMode::AdvancedJson
            && !self.json_editor_window_open
            && self.json_editor.has_unapplied_changes()
            && !self.apply_raw_json()
        {
            return;
        }
        if view == ViewMode::AdvancedJson {
            self.sync_raw_json_if_stale();
            self.json_editor.restore_location_next_draw();
            if should_open_json_editor_window_on_selection(
                self.preferences.always_open_json_editor_in_second_window,
                view,
            ) {
                self.set_json_editor_window_open(true);
            }
        }
        if self.view_mode == ViewMode::Progression || view == ViewMode::Progression {
            self.progression_ui.reset_navigation();
        }
        self.view_mode = view;
    }

    fn set_status(&mut self, message: impl Into<String>, is_error: bool) {
        self.status = message.into();
        self.status_is_error = is_error;
        self.activity_log.push(self.status.clone(), is_error);
        if self.preferences.troubleshooting_logging
            && let Err(error) = diagnostics::append_status(&self.status, is_error)
        {
            self.troubleshooting_log_error = Some(error);
        }
    }

    fn build_troubleshooting_report(&self) -> String {
        let account_source = self.document.source_info();
        let catalog_stats = self.manifest.stats();
        let destiny_process_status = match platform::destiny_is_running() {
            Ok(true) => "running".to_owned(),
            Ok(false) => "not_running".to_owned(),
            Err(error) => format!("check_failed: {error}"),
        };
        diagnostics::build_report(&diagnostics::ReportContext {
            install_path: &self.install_path,
            settings_path: &self.settings_path,
            settings_layout: self.settings_layout.preference_value(),
            sunrise_version: &self.sunrise_version,
            settings_schema: game_settings::schema_version(&self.document),
            account_source: account_source.label,
            account_contract: account_source.contract,
            account_detail: &account_source.detail,
            account_database_path: &account_source.database_path,
            catalog: diagnostics::CatalogSummary {
                cache_path: &self.manifest.cache_path,
                loaded_from_cache: self.manifest.loaded_from_cache,
                items: catalog_stats.items,
                plugs: catalog_stats.plugs,
                icons: catalog_stats.icons,
                descriptions: catalog_stats.descriptions,
                unlock_flags: self.manifest.unlock_flag_definitions().len(),
                unlock_values: self.manifest.unlock_value_definitions().len(),
                progressions: self.manifest.progression_definitions().len(),
                objectives: self.manifest.objectives().len(),
                expressions: self.manifest.shared_expression_pool().len(),
                progression_error: self.manifest.progression_package_error(),
            },
            recent_activity: &self.activity_log.text(),
            current_status: &self.status,
            source_warning: self.source_warning.as_deref(),
            has_unsaved_changes: self.has_unsaved_changes(),
            destiny_process_status: &destiny_process_status,
        })
    }

    fn initialize_troubleshooting_log(&mut self) -> Result<PathBuf, String> {
        let report = self.build_troubleshooting_report();
        let result = diagnostics::initialize_log(&report);
        match &result {
            Ok(_) => self.troubleshooting_log_error = None,
            Err(error) => self.troubleshooting_log_error = Some(error.clone()),
        }
        result
    }

    fn append_troubleshooting_snapshot(&mut self) -> Result<PathBuf, String> {
        let report = self.build_troubleshooting_report();
        let result = diagnostics::append_snapshot(&report);
        match &result {
            Ok(_) => self.troubleshooting_log_error = None,
            Err(error) => self.troubleshooting_log_error = Some(error.clone()),
        }
        result
    }

    fn characters(&self) -> Option<&[Value]> {
        self.document
            .pointer("/state/characters")?
            .as_array()
            .map(Vec::as_slice)
    }

    fn character_count(&self) -> usize {
        account::character_count(&self.document)
    }

    fn draw_character_tabs(&mut self, ui: &mut egui::Ui) {
        let character_tabs = (0..self.character_count())
            .map(|index| {
                let class_type = account::character_metadata(&self.document, index)
                    .ok()
                    .map(|metadata| u64::from(metadata.class_type))
                    .unwrap_or(99);
                (
                    index,
                    format!("Character {} · {}", index + 1, class_name(class_type)),
                )
            })
            .collect::<Vec<_>>();
        ui.horizontal_wrapped(|ui| {
            for (index, label) in character_tabs {
                if ui
                    .selectable_label(self.selected_character == index, label)
                    .clicked()
                {
                    if self.selected_character != index {
                        self.progression_ui.invalidate_document();
                        self.collections_ui.reset_navigation();
                    }
                    self.selected_character = index;
                }
            }
        });
    }

    fn draw_progression_character_tabs(&mut self, ui: &mut egui::Ui) {
        if !self.document.uses_json_account() {
            self.draw_character_tabs(ui);
            ui.separator();
        }
    }

    fn open_package_authoring(&mut self, ctx: &egui::Context) {
        if self.package_authoring_open {
            self.set_status("Parhelion is already open", false);
            return;
        }
        if self.catalog_task.is_some() {
            self.set_status(
                "Wait for the current catalog operation to finish before opening Parhelion",
                true,
            );
            return;
        }
        let Some(package_authoring) = self.package_authoring.as_mut() else {
            self.set_status("This Sundial build does not include Parhelion", true);
            return;
        };

        self.manifest.suspend_package_access();
        let preferences = PackageAuthoringPreferences {
            show_parhelion_experimental_options: self
                .preferences
                .show_parhelion_experimental_options,
        };
        match package_authoring.open(ctx, &self.install_path, preferences) {
            Ok(()) => {
                self.package_authoring_open = true;
                // Opening starts or resumes Parhelion's catalog worker. Treat its
                // state as busy until the first hosted update reports otherwise.
                self.package_authoring_busy = true;
                self.package_authoring_dirty = false;
                self.package_authoring_packages_changed = false;
                self.set_status("Opened Parhelion", false);
            }
            Err(error) => {
                self.manifest.resume_package_access();
                self.set_status(format!("Could not open Parhelion: {error}"), true);
            }
        }
    }

    fn update_package_authoring(&mut self, ctx: &egui::Context) {
        if !self.package_authoring_open {
            return;
        }
        let Some(package_authoring) = self.package_authoring.as_mut() else {
            self.package_authoring_open = false;
            self.package_authoring_busy = false;
            self.package_authoring_dirty = false;
            self.manifest.resume_package_access();
            return;
        };
        let update = package_authoring.update(ctx);
        if update.open_sundial_preferences {
            self.select_view(ViewMode::Preferences);
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            ctx.request_repaint();
        }
        self.package_authoring_busy = update.busy;
        self.package_authoring_dirty = update.dirty;
        self.package_authoring_packages_changed |= update.packages_changed;
        let preference_save_error = update.preferences_changed.and_then(|preferences| {
            self.preferences.show_parhelion_experimental_options =
                preferences.show_parhelion_experimental_options;
            self.save_preferences().err()
        });
        if update.open {
            if let Some(error) = preference_save_error {
                self.set_status(
                    format!("Parhelion preferences changed, but could not be saved: {error}"),
                    true,
                );
            }
            return;
        }

        self.package_authoring_open = false;
        self.package_authoring_busy = false;
        self.package_authoring_dirty = false;
        if self.package_authoring_packages_changed {
            self.package_authoring_packages_changed = false;
            // Parhelion already rebuilt the shared cache after installation. Load that
            // result here, while still allowing the cache fingerprint to trigger one
            // scan if the package set changed again before the window closed.
            self.reload_catalog_after_authoring(ctx);
        } else {
            self.manifest.resume_package_access();
            self.set_status("Closed Parhelion", false);
        }
        if let Some(error) = preference_save_error {
            self.set_status(
                format!("Parhelion preferences changed, but could not be saved: {error}"),
                true,
            );
        }
    }
}

fn preserve_inactive_json_account_domains(defaults: &mut Value, source: &Value) {
    if let Some(server) = defaults.get_mut("server").and_then(Value::as_object_mut) {
        if let Some(value) = source.pointer("/server/entitlements") {
            server.insert("entitlements".into(), value.clone());
        } else {
            server.remove("entitlements");
        }
    }
    let Some(default_state) = defaults.get_mut("state").and_then(Value::as_object_mut) else {
        return;
    };
    let source_state = source.get("state").and_then(Value::as_object);
    for key in ["account", "characters", "unlocks", "investment"] {
        if let Some(value) = source_state.and_then(|state| state.get(key)) {
            default_state.insert(key.to_owned(), value.clone());
        } else {
            default_state.remove(key);
        }
    }
}

fn draw_json_account_source_notice(ui: &mut egui::Ui, source: AccountSourceKind) {
    let (title, message) = match source {
        AccountSourceKind::Json => return,
        AccountSourceKind::Sqlite => (
            "Account Data",
            "As of schema v18, most account data is stored in investment.sqlite3. Some settings, including player identity and runtime configuration, are still read from settings.json.",
        ),
        AccountSourceKind::Blocked => (
            "Account Database Unavailable",
            "Sundial couldn't load investment.sqlite3. Database-backed account editing is unavailable.",
        ),
    };
    let (background, border, foreground) = if ui.visuals().dark_mode {
        (
            egui::Color32::from_rgb(55, 40, 26),
            egui::Color32::from_rgb(102, 72, 39),
            egui::Color32::from_rgb(245, 215, 177),
        )
    } else {
        (
            egui::Color32::from_rgb(255, 240, 221),
            egui::Color32::from_rgb(220, 181, 131),
            egui::Color32::from_rgb(104, 60, 16),
        )
    };
    egui::Frame::NONE
        .fill(background)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.visuals_mut().override_text_color = Some(foreground);
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.horizontal_top(|ui| {
                ui.label(egui::RichText::new(egui_phosphor::regular::INFO).size(18.0));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.strong(title);
                    ui.add(egui::Label::new(message).wrap());
                });
            });
        });
    ui.add_space(8.0);
}

impl SundialApp {
    fn draw_active_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.json_editor_window_open
                && self.json_editor.has_unapplied_changes()
                && matches!(
                    self.view_mode,
                    ViewMode::Characters
                        | ViewMode::ProfileInventory
                        | ViewMode::CharacterInventory
                        | ViewMode::GameSettings
                        | ViewMode::Progression
                )
            {
                ui.heading("Finish the JSON edit");
                ui.label(
                    "The detached editor has unapplied changes. Resolve its validation errors or reset it before using guided settings.",
                );
                return;
            }
            match self.view_mode {
                ViewMode::Characters => {
                    self.draw_character_tabs(ui);
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt(("character_editor_scroll", self.selected_character))
                        .show(ui, |ui| {
                            let index = self.selected_character;
                            let character_editable = account::can_mutate_equipment(&self.document);
                            ui.add_enabled_ui(character_editable, |ui| {
                                self.draw_character_fields(ui, index, character_editable)
                            });
                            if !character_editable {
                                ui.label(
                                    egui::RichText::new(
                                        "Character and equipment controls are disabled for this settings schema.",
                                    )
                                    .weak(),
                                );
                            }
                            self.draw_equipment(ui, index);
                        });
                }
                ViewMode::ProfileInventory => self.draw_profile_inventory_page(ui),
                ViewMode::CharacterInventory => self.draw_character_inventory_page(ui),
                ViewMode::GameSettings => {
                    let dawn = self.runtime_choice.inspection.copies.iter_mut()
                        .find(|copy| copy.settings_path == self.settings_path)
                        .and_then(|copy| copy.dawn_runtime.as_mut());
                    let account_settings = account::account_settings_map(&self.document);
                    let bindings_editable = account::named_key_bindings_editable(&self.document);
                    let json_account = self.document.uses_json_account();
                    let mut runtime_document=self.document.runtime_view();
                    let edits = game_settings::draw_page(
                        ui,
                        game_settings::PageContext {
                            json_document: &mut runtime_document,
                            account_settings: account_settings.as_ref().map_err(String::as_str),
                            bindings_editable,
                            json_account,
                            extended_fov: self.preferences.experimental_extended_fov,
                            dawn,
                            tab: &mut self.game_settings_tab,
                            key_bindings: &mut self.key_binding_ui,
                        },
                    );
                    if edits.json_changed && let Err(error)=self.document.apply_runtime_view(runtime_document) {
                        self.set_status(error,true);
                        return;
                    }
                    let account_changed =
                        match account::apply_account_settings(
                            &mut self.document,
                            edits.account_commands,
                        ) {
                            Ok(changed) => changed,
                            Err(error) => {
                                self.set_status(
                                    format!("Game setting was not changed: {error}"),
                                    true,
                                );
                                false
                            }
                        };
                    if edits.json_changed || account_changed {
                        self.dirty = true;
                        self.set_status("Game setting updated. Click Save to write it", false);
                    }
                }
                ViewMode::Progression => self.draw_progression_page(ui),
                ViewMode::AdvancedJson => {
                    if self.json_editor_window_open {
                        ui.heading("All Settings");
                        ui.label("The JSON editor is open in a separate window.");
                        if ui.button("Dock in Main Window").clicked() {
                            self.set_json_editor_window_open(false);
                            self.json_editor.restore_location_next_draw();
                        }
                    } else {
                        self.sync_raw_json_if_stale();
                        draw_json_account_source_notice(ui, self.document.source_info().kind);
                        let response = json_editor::draw(
                            ui,
                            &mut self.raw_json,
                            &mut self.json_editor,
                            false,
                            self.dirty,
                        );
                        self.handle_json_editor_response(ctx, response);
                    }
                }
                ViewMode::Preferences => self.draw_preferences_page(ui, ctx),
            }
        });
    }

    fn prepare_frame(&mut self, ctx: &egui::Context) -> Option<String> {
        #[cfg(target_os = "linux")]
        {
            let title_bar_icon = self
                .title_bar_icon
                .get_or_insert_with(|| load_linux_title_bar_texture(ctx))
                .clone();
            if draw_linux_title_bar(ctx, &title_bar_icon) {
                if let Some(message) = self.package_authoring_exit_blocker() {
                    self.set_status(message, true);
                } else if self.has_unsaved_changes() {
                    self.confirmation = Some(ConfirmationDialog::Exit);
                } else {
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        self.ensure_destiny_symbol_font(ctx);
        self.update_check.start_if_needed(ctx);
        self.update_check.poll();
        self.poll_catalog_task();
        let available_update = match self.update_check.status() {
            UpdateStatus::Available(release) => Some(release.version.clone()),
            _ => None,
        };
        if ctx.input(|input| input.viewport().close_requested()) && !self.exit_confirmed {
            if let Some(message) = self.package_authoring_exit_blocker() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.set_status(message, true);
            } else if self.has_unsaved_changes() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirmation = Some(ConfirmationDialog::Exit);
            }
        }

        self.refresh_after_focus_if_needed(ctx, ctx.input(|input| input.focused));
        available_update
    }

    fn draw_supporting_windows(&mut self, ctx: &egui::Context) {
        if let Some(hash) = inspector::take_definition_request(ctx) {
            let context = inspector::take_definition_context(ctx, hash);
            self.hash_inspection.open_with_context(hash, context);
        }
        let mut inspector_document = self.document.progression_view(self.selected_character);
        let inspector_changed = inspector::draw_catalog_hash_window(
            ctx,
            &self.manifest,
            Some(&mut inspector_document),
            self.preferences.experimental_progression
                && self.document.account_editing_blocked().is_none()
                && !self.json_editor.has_unapplied_changes(),
            &mut self.hash_inspection,
            "global",
        );
        if inspector_changed {
            if let Err(error) = self
                .document
                .apply_progression_view(self.selected_character, inspector_document)
            {
                self.set_status(error, true);
                return;
            }
            self.dirty = true;
            self.progression_ui.invalidate_document();
            self.set_status("Progression state updated. Click Save to write it", false);
        }

        self.draw_json_editor_window(ctx);

        self.draw_about_window(ctx);
        self.draw_activity_log_window(ctx);

        self.draw_catalog_progress(ctx);
    }
}

impl eframe::App for SundialApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let document_before_frame = self.document.clone();
        let available_update = self.prepare_frame(ctx);

        self.draw_app_chrome(ctx, available_update.as_deref());
        self.draw_active_view(ctx);
        self.draw_supporting_windows(ctx);
        self.update_package_authoring(ctx);

        self.draw_pending_install_choice(ctx);
        self.draw_future_schema_confirmation(ctx);

        self.draw_reset_defaults_confirmation(ctx);
        self.draw_runtime_choice(ctx);
        self.draw_sqlite_reset_confirmation(ctx);
        self.draw_sqlite_restore_confirmation(ctx);
        self.draw_parhelion_confirmation(ctx);
        self.draw_unsafe_mode_confirmation(ctx);
        self.draw_save_review_confirmation(ctx);
        self.draw_delete_equipment_confirmation(ctx);
        self.draw_reload_confirmation(ctx);
        self.draw_exit_confirmation(ctx);

        self.handle_workspace_shortcuts(ctx);
        self.record_document_change(document_before_frame);
        self.draw_update_window(ctx);
    }
}

fn encode_settings_for_editor(document: &Value) -> Result<String, String> {
    encode_settings(document).map(|encoded| encoded.replace("\r\n", "\n"))
}

fn draw_future_schema_warning(ui: &mut egui::Ui, pending: &PendingFutureSchemaLoad) {
    ui.heading("Newer Sunrise settings detected");
    ui.add_space(6.0);
    ui.label(format!(
        "This settings.json uses schema version {}, which this Sundial release has not been tested with.",
        pending.schema_version
    ));
    ui.add_space(6.0);
    ui.colored_label(
        ui.visuals().warn_fg_color,
        "You can continue, but settings may have changed in this Sunrise version.",
    );
    ui.add_space(6.0);
    ui.label("Known fields remain editable where their layout is recognized. Sundial will preserve unrecognized JSON and create settings.json.bak beside the original before saving.");
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(pending.settings_path.display().to_string())
            .weak()
            .small(),
    );
}

fn check_install(selection: InstallSelection) -> Result<String, String> {
    let install_path = selection.install_path;
    let (settings_layout, settings_path) = match resolve_settings_path(
        &install_path,
        selection.preferred_layout,
    ) {
        SettingsPathResolution::Found(layout, path) => (layout, path),
        SettingsPathResolution::Missing => return Err(missing_settings_message(&install_path)),
        SettingsPathResolution::Ambiguous => {
            return Err("Multiple Sunrise settings.json files were found; open Sundial and choose which one Project Sunrise uses".into());
        }
    };
    let app = SundialApp::new(settings_path, settings_layout, install_path)?;
    validate_for_check(&app.document)?;
    if let Some(warning) = app.validation_warning_for_write(&app.document)? {
        return Err(warning);
    }
    let prepared = prepare_settings(&app.document)?;
    let size_note = if prepared.compacted {
        " (compacted from Sunrise's readable layout)".to_owned()
    } else {
        String::new()
    };
    let schema_version = game_settings::schema_version(&app.document)
        .ok_or("Validated settings are missing a schema version")?;
    let account_source = app.document.source_info();
    Ok(format!(
        "Valid: settings schema {}, detected Project Sunrise {}, {} characters from {}, {} compatible local catalog items loaded, save size {} bytes{}",
        schema_version,
        app.sunrise_version,
        app.character_count(),
        account_source.label,
        app.manifest.items.len(),
        prepared.encoded_bytes,
        size_note
    ))
}

fn validate_for_check(document: &WorkspaceDocument) -> Result<(), String> {
    if let Some(reason) = document.account_editing_blocked() {
        return Err(format!("Account source is incompatible: {reason}"));
    }
    validate_workspace_document(document).map_err(|error| format!("Invalid settings: {error}"))
}

pub(crate) fn run(package_authoring: Box<dyn PackageAuthoringUtility>) -> eframe::Result {
    let update_startup = crate::updates::startup()
        .map_err(|error| eframe::Error::AppCreation(Box::new(std::io::Error::other(error))))?;
    let (install, check_only, loaded_preferences) = parse_args();
    let preferences = loaded_preferences.preferences;
    let preferences_warning = loaded_preferences.warning;
    if let Some(warning) = &preferences_warning {
        eprintln!("Sundial: {warning}");
    }
    if check_only {
        let Some(selection) = install else {
            eprintln!("Sundial: --check requires a saved install or --install <folder>");
            std::process::exit(2);
        };
        match check_install(selection) {
            Ok(summary) => println!("{summary}"),
            Err(error) => {
                eprintln!("Sundial: {error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    #[cfg(windows)]
    set_windows_app_identity();
    #[cfg(target_os = "linux")]
    let icon_bytes = include_bytes!("../assets/linux/io.github.kylethmpsn.Sundial-window.png");
    #[cfg(windows)]
    let icon_bytes = include_bytes!("../assets/sundial-alt.png");
    let icon = eframe::icon_data::from_png_bytes(icon_bytes)
        .expect("embedded Sundial icon must be a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sundial")
            .with_app_id("io.github.kylethmpsn.Sundial")
            .with_decorations(!cfg!(target_os = "linux"))
            .with_inner_size([1_240.0, 960.0])
            .with_min_inner_size([720.0, 520.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "Sundial",
        options,
        Box::new(move |cc| {
            #[cfg(windows)]
            set_windows_taskbar_icon(cc);
            cc.egui_ctx.set_theme(preferences.color_theme.egui_theme());
            ui::configure_contrast(&cc.egui_ctx);
            let app = StartupApp::new(
                install,
                preferences,
                preferences_warning,
                Some(package_authoring),
            );
            if let Some(startup) = update_startup {
                startup.window_created().map_err(std::io::Error::other)?;
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests;
