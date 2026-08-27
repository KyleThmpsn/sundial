use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::persistence::json_account::ensure_schema_v8_preferences;
use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{Catalog as Manifest, CatalogProgress},
    game_settings, storage,
    updates::{RELEASES_URL, UpdateCheck, UpdateStatus},
};

mod startup;
use startup::StartupApp;

mod background_tasks;
use background_tasks::{CatalogTask, CatalogTaskEvent, CatalogTaskKind, PendingInstallLoad};

mod bootstrap;
use bootstrap::parse_args;

mod generated_files;
use generated_files::{
    GeneratedFileDecision, GeneratedFileKind, GeneratedFileSaveAction, PendingGeneratedFile,
    normalized_generated_document, settings_size_label, settings_size_note,
};

mod change_review;
use change_review::collect_change_summaries;

mod platform;
#[cfg(target_os = "linux")]
use platform::{draw_linux_title_bar, load_linux_title_bar_texture};
use platform::{load_logo_texture, open_directory};
#[cfg(windows)]
use platform::{set_windows_app_identity, set_windows_taskbar_icon};

mod preferences;
use preferences::{
    CharacterInventoryLayout, ColorTheme, InstallSelection, ItemCardWidth,
    MAX_AUTOMATIC_BACKUP_LIMIT, MIN_AUTOMATIC_BACKUP_LIMIT, PlugSelectionMode, Preferences,
    SettingsLayout, SettingsPathResolution, configure_destiny_symbol_fonts,
    draw_plug_selection_warning, normalized_automatic_backup_limit,
};

mod settings;
use settings::{
    backups_path, catalog_path, create_adjacent_backup, detect_sunrise_version, encode_settings,
    load_installed_sunrise_defaults, load_workspace_json, missing_settings_message,
    preferences_path, prepare_settings, prune_automatic_backups, repair_known_ability_pairs,
    resolve_settings_path, save_json, settings_path_for_install, validate_workspace_document,
    verify_workspace_source_unchanged,
};

mod workspace_save;
use workspace_save::save_changed_sources;

mod json_editor;
use json_editor::JsonEditorState;

mod equipment;
use equipment::class_name;

mod account_settings;

mod account_workspace;
use account_workspace::{AccountSourceKind, AccountWorkspace, WorkspaceDocument};

mod character_metadata;

mod inventory;

mod components;

mod item_editor;

mod glyphs;

mod ui;

mod inspector;

mod inventory_page;

mod progression;

mod collections_page;

const PROJECT_URL: &str = "https://github.com/kylethmpsn/sundial";
const CREDITS_URL: &str = "https://github.com/kylethmpsn/sundial#credits-and-licensing";
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

const SLOTS: &[(&str, &str, u64)] = &[
    ("kinetic", "Kinetic", 1_498_876_634),
    ("energy", "Energy", 2_465_295_065),
    ("heavy", "Power", 953_998_645),
    ("helmet", "Helmet", 3_448_274_439),
    ("gauntlets", "Gauntlets", 3_551_918_588),
    ("chest", "Chest", 14_239_492),
    ("legs", "Legs", 20_886_954),
    ("class_item", "Class item", 1_585_787_867),
    ("ghost", "Ghost", 4_023_194_814),
    ("vehicle", "Vehicle", 2_025_709_351),
    ("ship", "Ship", 284_967_655),
    ("subclass", "Subclass", 3_284_755_031),
    ("clan_banner", "Clan banner", 4_292_445_962),
    ("emblem", "Emblem", 4_274_335_291),
    ("emote", "Emote", 2_401_704_334),
    ("finisher", "Finisher", 3_683_254_069),
];

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
    Interface,
    Editing,
    Sunrise,
    SavingRecovery,
}

impl PreferencesTab {
    const ALL: [Self; 4] = [
        Self::Interface,
        Self::Editing,
        Self::Sunrise,
        Self::SavingRecovery,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Interface => "Interface",
            Self::Editing => "Editing",
            Self::Sunrise => "Sunrise",
            Self::SavingRecovery => "Saving & recovery",
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

fn has_save_work(
    document_changed: bool,
    raw_json_changed: bool,
    generated_file_retry_pending: bool,
) -> bool {
    document_changed || raw_json_changed || generated_file_retry_pending
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
    Unlocks,
    Investment,
    Collections,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfirmationDialog {
    ReallyUnsafe,
    ReviewSave,
    DeleteEquipment,
    Reload,
    RestoreSqliteBackup,
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
    settings_path: PathBuf,
    settings_layout: SettingsLayout,
    install_path: PathBuf,
    sunrise_version: String,
    manifest: Manifest,
    account_workspace: AccountWorkspace,
    document: WorkspaceDocument,
    persisted_document: WorkspaceDocument,
    source_warning: Option<String>,
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
    default_plug_selection_mode: PlugSelectionMode,
    show_safety_warnings: bool,
    review_changes_before_saving: bool,
    limit_automatic_backups: bool,
    automatic_backup_limit: u16,
    color_theme: ColorTheme,
    always_open_json_editor_in_second_window: bool,
    show_plug_hashes: bool,
    item_card_width: ItemCardWidth,
    character_inventory_layout: CharacterInventoryLayout,
    experimental_orbit_backdrops: bool,
    experimental_progression: bool,
    experimental_power_above_cap: bool,
    really_unsafe_warning_acknowledged: bool,
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
    pending_save_action: Option<GeneratedFileSaveAction>,
    pending_equipment_delete: Option<PendingEquipmentDelete>,
    pending_sqlite_restore: Option<PathBuf>,
    pending_generated_file: Option<PendingGeneratedFile>,
    generated_file_decisions: Vec<(GeneratedFileKind, GeneratedFileDecision)>,
    generated_file_retry_pending: bool,
    exit_confirmed: bool,
    dirty: bool,
    undo_history: Vec<DocumentHistoryEntry>,
    redo_history: Vec<DocumentHistoryEntry>,
    suppress_history_record: bool,
    status: String,
    status_is_error: bool,
    window_was_focused: bool,
    workspace_refresh_pending: bool,
    next_workspace_refresh_poll: Instant,
    pending_install_choice: Option<PathBuf>,
    pending_future_schema: Option<PendingFutureSchemaLoad>,
    catalog_task: Option<CatalogTask>,
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
        preferences: Preferences,
        report: impl FnMut(CatalogProgress),
    ) -> Result<Self, String> {
        let json_document = load_workspace_json(&settings_path)?;
        let cache = catalog_path().ok_or("Could not locate Sundial's local catalog folder")?;
        let manifest = Manifest::load_or_scan_with_progress(&install_path, cache, false, report)?;
        let sunrise_version = detect_sunrise_version(&install_path);
        let account_workspace = AccountWorkspace::json();
        let document = WorkspaceDocument::load(json_document, &settings_path);
        let source_warning = validate_workspace_document(&document).err();
        let class_armor_defaults = account_workspace.class_armor_default_characters(&document);
        let raw_json = encode_settings_for_editor(document.json())?;
        let raw_json_document = document.json().clone();
        let persisted_document = document.clone();
        let default_plug_selection_mode = if preferences.default_plug_selection_mode
            == PlugSelectionMode::AnyPlug
            && !preferences.really_unsafe_warning_acknowledged
        {
            PlugSelectionMode::Supported
        } else {
            preferences.default_plug_selection_mode
        };
        Ok(Self {
            settings_path,
            settings_layout,
            install_path,
            sunrise_version,
            manifest,
            account_workspace,
            document,
            persisted_document,
            source_warning: source_warning.clone(),
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
            default_plug_selection_mode,
            show_safety_warnings: preferences.show_safety_warnings,
            review_changes_before_saving: preferences.review_changes_before_saving,
            limit_automatic_backups: preferences.limit_automatic_backups,
            automatic_backup_limit: normalized_automatic_backup_limit(
                preferences.automatic_backup_limit,
            ),
            color_theme: preferences.color_theme,
            always_open_json_editor_in_second_window: preferences
                .always_open_json_editor_in_second_window,
            show_plug_hashes: preferences.show_plug_hashes,
            item_card_width: preferences.item_card_width,
            character_inventory_layout: preferences.character_inventory_layout,
            experimental_orbit_backdrops: preferences.experimental_orbit_backdrops,
            experimental_progression: preferences.experimental_progression,
            experimental_power_above_cap: preferences.experimental_power_above_cap,
            really_unsafe_warning_acknowledged: preferences
                .really_unsafe_warning_acknowledged,
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
            pending_generated_file: None,
            generated_file_decisions: Vec::new(),
            generated_file_retry_pending: false,
            exit_confirmed: false,
            dirty: false,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            suppress_history_record: false,
            status: source_warning.as_ref().map_or_else(
                || "Ready".to_owned(),
                |warning| {
                    format!(
                        "Loaded with an unexpected setting: {warning}. A safety copy will be created beside settings.json before saving"
                    )
                },
            ),
            status_is_error: source_warning.is_some(),
            window_was_focused: true,
            workspace_refresh_pending: false,
            next_workspace_refresh_poll: Instant::now(),
            pending_install_choice: None,
            pending_future_schema: None,
            catalog_task: None,
            destiny_symbol_font_install: None,
            destiny_symbol_font_error: None,
        })
    }

    fn ensure_destiny_symbol_font(&mut self, ctx: &egui::Context) {
        if self.destiny_symbol_font_install.as_ref() == Some(&self.install_path) {
            return;
        }

        self.destiny_symbol_font_error =
            configure_destiny_symbol_fonts(ctx, &self.install_path).err();
        self.destiny_symbol_font_install = Some(self.install_path.clone());
    }

    fn reload(&mut self) -> bool {
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

    fn refresh_after_focus_if_needed(&mut self, ctx: &egui::Context, focused: bool) {
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
        }
    }

    fn install_reloaded_document(&mut self, document: WorkspaceDocument, automatic: bool) {
        let blocked_reason = document.account_editing_blocked().map(str::to_owned);
        let warning = validate_workspace_document(&document).err();
        self.class_armor_defaults = self
            .account_workspace
            .class_armor_default_characters(&document);
        self.persisted_document = document.clone();
        self.document = document;
        self.progression_ui.invalidate_document();
        self.refresh_sunrise_version();
        self.source_warning.clone_from(&warning);
        self.selected_character = self
            .selected_character
            .min(self.character_count().saturating_sub(1));
        self.clear_picker_state();
        self.sync_raw_json();
        self.dirty = false;
        self.undo_history.clear();
        self.redo_history.clear();
        self.suppress_history_record = true;
        let action = if automatic { "Refreshed" } else { "Reloaded" };
        if let Some(reason) = blocked_reason {
            self.set_status(
                format!("{action} Sunrise data, but account editing is blocked: {reason}"),
                true,
            );
        } else if let Some(warning) = warning {
            self.set_status(
                format!(
                    "{action} with an unexpected setting: {warning}. A safety copy will be created beside settings.json before saving"
                ),
                true,
            );
        } else if automatic {
            self.set_status("Refreshed Sunrise data after returning to Sundial", false);
        } else {
            self.set_status("Reloaded Sunrise data", false);
        }
    }

    fn request_sqlite_backup_restore(&mut self) {
        #[cfg(feature = "sqlite-account")]
        {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Select a Sundial state.sqlite3 backup")
                .add_filter("SQLite database", &["sqlite3"]);
            if let Some(path) = backups_path() {
                dialog = dialog.set_directory(path);
            }
            let Some(path) = dialog.pick_file() else {
                return;
            };
            match self.document.validate_sqlite_backup(&path) {
                Ok(()) => {
                    self.pending_sqlite_restore = Some(path);
                    self.confirmation = Some(ConfirmationDialog::RestoreSqliteBackup);
                }
                Err(error) => self.set_status(
                    format!("Backup not selected: {error}. No files were changed"),
                    true,
                ),
            }
        }
        #[cfg(not(feature = "sqlite-account"))]
        self.set_status(
            "This Sundial build does not include SQLite account recovery",
            true,
        );
    }

    fn restore_selected_sqlite_backup(&mut self) {
        let Some(backup) = self.pending_sqlite_restore.take() else {
            return;
        };
        #[cfg(feature = "sqlite-account")]
        {
            match platform::destiny_is_running() {
                Ok(true) => {
                    self.set_status(
                        "Not restored: close Destiny 2 before replacing state.sqlite3, then try again",
                        true,
                    );
                    return;
                }
                Ok(false) => {}
                Err(error) => {
                    self.set_status(format!("Not restored: {error}"), true);
                    return;
                }
            }
            match self.document.restore_sqlite_backup_safely(&backup) {
                Ok(safety_backup) => {
                    if self.reload() {
                        let warning = self.source_warning.clone().map_or_else(
                            String::new,
                            |value| {
                                format!(
                                    " Reloaded with an unrelated settings.json warning: {value}."
                                )
                            },
                        );
                        self.set_status(
                            format!(
                                "Restored state.sqlite3 from {}. The replaced database is preserved at {}.{warning}",
                                backup.display(),
                                safety_backup.display()
                            ),
                            self.source_warning.is_some(),
                        );
                    } else {
                        let reload_error = self.status.clone();
                        self.set_status(
                            format!(
                                "Restored state.sqlite3 from {}, but Sundial could not reload the workspace: {reload_error}. The replaced database is preserved at {}",
                                backup.display(),
                                safety_backup.display()
                            ),
                            true,
                        );
                    }
                }
                Err(error) => self.set_status(format!("Not restored: {error}"), true),
            }
        }
        #[cfg(not(feature = "sqlite-account"))]
        let _ = backup;
    }

    fn save_with_generated_files(&mut self, action: GeneratedFileSaveAction) -> bool {
        if self.document.uses_json_account()
            && ensure_schema_v8_preferences(self.document.json_mut())
        {
            self.dirty = true;
        }
        let repaired_ability_pairs =
            match repair_known_ability_pairs(self.account_workspace, &mut self.document) {
                Ok(repaired) => repaired,
                Err(error) => {
                    self.set_status(format!("Not saved: {error}"), true);
                    return false;
                }
            };
        if repaired_ability_pairs > 0 {
            self.dirty = true;
        }
        let json_changed = self.document.json_changed_from(&self.persisted_document);
        let account_changed = self.document.account_changed_from(&self.persisted_document);
        if (json_changed || account_changed)
            && let Err(error) = self.document.verify_account_source_unchanged()
        {
            self.set_status(format!("Not saved: {error}"), true);
            return false;
        }
        if json_changed
            && let Err(error) = verify_workspace_source_unchanged(
                &self.settings_path,
                self.persisted_document.json(),
                self.persisted_document.uses_json_account(),
            )
        {
            self.set_status(format!("Not saved: {error}"), true);
            return false;
        }
        if account_changed {
            match platform::destiny_is_running() {
                Ok(true) => {
                    self.set_status(
                        "Not saved: close Destiny 2 before writing state.sqlite3, then try again",
                        true,
                    );
                    return false;
                }
                Ok(false) => {}
                Err(error) => {
                    self.set_status(format!("Not saved: {error}"), true);
                    return false;
                }
            }
        }
        let current_warning = validate_workspace_document(&self.document).err();
        let detected_warning = self
            .source_warning
            .clone()
            .or_else(|| current_warning.clone());
        let orbit_supported =
            orbit_map_generation_enabled(self.experimental_orbit_backdrops, &self.document);
        let generated_file_plans = match self.prepare_generated_files(orbit_supported, action) {
            Ok(Some(plans)) => plans,
            Ok(None) => return false,
            Err(error) => {
                self.set_status(format!("Not saved: {error}"), true);
                return false;
            }
        };
        let safety_backup = if json_changed && detected_warning.is_some() {
            match create_adjacent_backup(&self.settings_path) {
                Ok(path) => Some(path),
                Err(error) => {
                    self.set_status(
                        format!(
                            "Not saved: the file contains an unexpected setting and its safety copy could not be created: {error}"
                        ),
                        true,
                    );
                    return false;
                }
            }
        } else {
            None
        };
        let source_receipt = match save_changed_sources(
            &mut self.document,
            &self.persisted_document,
            &self.settings_path,
            json_changed,
            account_changed,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.generated_file_decisions.clear();
                let suffix = safety_backup.map_or_else(String::new, |path| {
                    format!(" The untouched JSON source is at {}.", path.display())
                });
                let rollback_note = match error.sqlite_rollback {
                    Some(Ok(())) => {
                        " The SQLite write was rolled back from its verified backup.".to_owned()
                    }
                    Some(Err(rollback_error)) => format!(
                        " CRITICAL: the JSON save failed after SQLite was written, and SQLite rollback also failed: {rollback_error}"
                    ),
                    None => String::new(),
                };
                self.set_status(
                    format!("Not saved: {}{suffix}{rollback_note}", error.message),
                    true,
                );
                return false;
            }
        };
        let json_result = source_receipt.json;
        #[cfg(feature = "sqlite-account")]
        let sqlite_receipt = source_receipt.sqlite;
        let automatic_backup_created = json_result.is_some();
        #[cfg(feature = "sqlite-account")]
        let automatic_backup_created = automatic_backup_created || sqlite_receipt.is_some();
        let (retention_note, retention_failed) = if automatic_backup_created {
            self.apply_backup_retention()
        } else {
            (String::new(), false)
        };

        let safe_to_close = json_result
            .as_ref()
            .is_none_or(|result| !result.exceeds_size_limit);
        self.persisted_document = self.document.clone();
        self.source_warning = current_warning;
        self.dirty = false;
        self.progression_ui.mark_saved();
        self.sync_raw_json();
        let repair_note = match repaired_ability_pairs {
            0 => String::new(),
            1 => " Corrected one invalid ability pairing.".to_owned(),
            count => format!(" Corrected {count} invalid ability pairings."),
        };
        let size_note = json_result
            .as_ref()
            .map_or_else(String::new, settings_size_note);
        let generated_file_note = match self.complete_generated_file_plans(generated_file_plans) {
            Ok(note) => {
                self.generated_file_retry_pending = false;
                note
            }
            Err(error) => {
                self.generated_file_decisions.clear();
                self.generated_file_retry_pending = true;
                self.set_status(
                    format!(
                        "Saved the selected data sources, but a package-generated Sunrise file could not be written: {error}.{retention_note}"
                    ),
                    true,
                );
                return false;
            }
        };
        self.generated_file_decisions.clear();
        let mut backups = Vec::new();
        if let Some(result) = &json_result {
            backups.push(format!("settings.json backup: {}", result.backup.display()));
        }
        #[cfg(feature = "sqlite-account")]
        if let Some(receipt) = &sqlite_receipt {
            backups.push(format!(
                "state.sqlite3 backup: {}",
                receipt.backup.display()
            ));
        }
        let backup_note = if backups.is_empty() {
            String::new()
        } else {
            format!(" {}.", backups.join(" · "))
        };
        let exceeds_size_limit = json_result
            .as_ref()
            .is_some_and(|result| result.exceeds_size_limit);
        if let (Some(warning), Some(safety_backup)) = (detected_warning, safety_backup) {
            self.set_status(
                format!(
                    "Saved after detecting an unexpected JSON setting ({warning}).{repair_note}{size_note}{generated_file_note} The untouched JSON source is at {}.{backup_note}{retention_note}",
                    safety_backup.display()
                ),
                true,
            );
        } else {
            self.set_status(
                format!(
                    "Saved.{repair_note}{size_note}{generated_file_note}{backup_note}{retention_note}"
                ),
                exceeds_size_limit || retention_failed,
            );
        }
        safe_to_close
    }

    fn save_all_edits_with_action(&mut self, action: GeneratedFileSaveAction) -> bool {
        if self.json_editor.has_unapplied_changes() && !self.apply_raw_json() {
            return false;
        }
        self.generated_file_decisions.clear();
        self.save_with_generated_files(action)
    }

    fn has_unsaved_changes(&self) -> bool {
        has_save_work(
            self.dirty,
            self.json_editor.has_unapplied_changes(),
            self.generated_file_retry_pending,
        )
    }

    fn request_save(&mut self, ctx: &egui::Context, action: GeneratedFileSaveAction) {
        if self.json_editor.has_unapplied_changes() && !self.apply_raw_json() {
            return;
        }
        let document_changed = self.document != self.persisted_document;
        if !has_save_work(document_changed, false, self.generated_file_retry_pending) {
            self.dirty = false;
            self.set_status("There are no changes to save", false);
            return;
        }
        if self.review_changes_before_saving && document_changed {
            self.pending_save_action = Some(action);
            self.confirmation = Some(ConfirmationDialog::ReviewSave);
        } else {
            self.perform_save_action(ctx, action);
        }
    }

    fn perform_save_action(&mut self, ctx: &egui::Context, action: GeneratedFileSaveAction) {
        let safe_to_close = self.save_all_edits_with_action(action);
        if action == GeneratedFileSaveAction::SaveAndExit
            && !self.has_unsaved_changes()
            && safe_to_close
        {
            self.exit_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn record_document_change(&mut self, previous: WorkspaceDocument) {
        if self.document == previous {
            self.suppress_history_record = false;
            return;
        }
        if self.suppress_history_record {
            self.suppress_history_record = false;
            return;
        }
        let label = if self.status.trim().is_empty() {
            "Settings change".to_owned()
        } else {
            self.status
                .split("; click Save")
                .next()
                .unwrap_or(&self.status)
                .trim()
                .to_owned()
        };
        self.undo_history.push(DocumentHistoryEntry {
            document: previous,
            label,
        });
        if self.undo_history.len() > DOCUMENT_HISTORY_LIMIT {
            self.undo_history.remove(0);
        }
        self.redo_history.clear();
    }

    fn restore_history_document(&mut self, mut entry: DocumentHistoryEntry, undo: bool) {
        let current = DocumentHistoryEntry {
            document: self.document.clone(),
            label: entry.label.clone(),
        };
        if undo {
            self.redo_history.push(current);
        } else {
            self.undo_history.push(current);
        }
        entry.document.rebase_account_revision_from(&self.document);
        self.document = entry.document;
        self.dirty = self.document != self.persisted_document;
        self.progression_ui.invalidate_document();
        self.clear_picker_state();
        self.sync_raw_json();
        self.armor_stats_adjuster = equipment::ArmorStatsAdjusterState::default();
        self.suppress_history_record = true;
        self.set_status(
            format!("{}: {}", if undo { "Undid" } else { "Redid" }, entry.label),
            false,
        );
    }

    fn undo(&mut self) {
        if let Some(entry) = self.undo_history.pop() {
            self.restore_history_document(entry, true);
        }
    }

    fn redo(&mut self) {
        if let Some(entry) = self.redo_history.pop() {
            self.restore_history_document(entry, false);
        }
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
        if view == ViewMode::Progression && !self.experimental_progression {
            return;
        }
        if self.view_mode == view {
            if should_open_json_editor_window_on_selection(
                self.always_open_json_editor_in_second_window,
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
                self.always_open_json_editor_in_second_window,
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

    fn reset_to_sunrise_defaults(&mut self) {
        self.generated_file_decisions.clear();
        self.reset_to_sunrise_defaults_with_generated_files();
    }

    fn reset_to_sunrise_defaults_with_generated_files(&mut self) {
        if let Err(error) = self.document.verify_account_source_unchanged() {
            self.set_status(format!("Defaults not restored: {error}"), true);
            return;
        }
        if let Err(error) = verify_workspace_source_unchanged(
            &self.settings_path,
            self.persisted_document.json(),
            self.persisted_document.uses_json_account(),
        ) {
            self.set_status(format!("Defaults not restored: {error}"), true);
            return;
        }
        let mut default_document = match load_installed_sunrise_defaults(&self.install_path) {
            Ok(document) => document,
            Err(error) => {
                self.set_status(error, true);
                return;
            }
        };
        if !self.document.uses_json_account() {
            preserve_inactive_json_account_domains(
                &mut default_document,
                self.persisted_document.json(),
            );
        }
        let orbit_supported =
            orbit_map_generation_enabled(self.experimental_orbit_backdrops, &default_document);
        let generated_file_plans = match self
            .prepare_generated_files(orbit_supported, GeneratedFileSaveAction::ResetDefaults)
        {
            Ok(Some(plans)) => plans,
            Ok(None) => return,
            Err(error) => {
                self.set_status(format!("Defaults not restored: {error}"), true);
                return;
            }
        };
        let adjacent_backup = match create_adjacent_backup(&self.settings_path) {
            Ok(path) => path,
            Err(error) => {
                self.set_status(
                    format!("Defaults not restored because the safety copy failed: {error}"),
                    true,
                );
                return;
            }
        };
        match save_json(&self.settings_path, &default_document) {
            Ok(result) => {
                let size_note = settings_size_note(&result);
                let (retention_note, retention_failed) = self.apply_backup_retention();
                let generated_file_result =
                    self.complete_generated_file_plans(generated_file_plans);
                self.document.replace_json(default_document.clone());
                self.progression_ui.invalidate_document();
                self.persisted_document.replace_json(default_document);
                self.refresh_sunrise_version();
                self.source_warning = validate_workspace_document(&self.document).err();
                self.class_armor_defaults = self
                    .account_workspace
                    .class_armor_default_characters(&self.document);
                self.selected_character = self
                    .selected_character
                    .min(self.character_count().saturating_sub(1));
                self.clear_picker_state();
                self.sync_raw_json();
                self.dirty = self.document != self.persisted_document;
                let generated_file_note = match generated_file_result {
                    Ok(note) => note,
                    Err(error) => {
                        self.generated_file_decisions.clear();
                        self.generated_file_retry_pending = true;
                        self.set_status(
                            format!(
                                "Restored the settings defaults, but a package-generated Sunrise file could not be written: {error}. Original: {}. Backup: {}.{retention_note}",
                                adjacent_backup.display(),
                                result.backup.display()
                            ),
                            true,
                        );
                        return;
                    }
                };
                self.generated_file_decisions.clear();
                self.generated_file_retry_pending = false;
                self.set_status(
                    format!(
                        "Restored the defaults bundled with the installed Project Sunrise.{size_note}{generated_file_note} Original: {}. Backup: {}.{retention_note}",
                        adjacent_backup.display(),
                        result.backup.display()
                    ),
                    result.exceeds_size_limit || retention_failed,
                );
            }
            Err(error) => {
                self.generated_file_decisions.clear();
                self.set_status(
                    format!(
                        "Defaults not restored: {error}. The untouched source is at {}",
                        adjacent_backup.display()
                    ),
                    true,
                );
            }
        }
    }

    fn set_status(&mut self, message: impl Into<String>, is_error: bool) {
        self.status = message.into();
        self.status_is_error = is_error;
    }

    fn apply_backup_retention(&self) -> (String, bool) {
        if !self.limit_automatic_backups {
            return (String::new(), false);
        }
        let result = backups_path()
            .ok_or_else(|| "Could not locate Sundial's backups folder".to_owned())
            .and_then(|root| {
                prune_automatic_backups(
                    &root,
                    usize::from(normalized_automatic_backup_limit(
                        self.automatic_backup_limit,
                    )),
                )
            });
        match result {
            Ok(0) => (String::new(), false),
            Ok(1) => (" Removed one older automatic backup.".to_owned(), false),
            Ok(removed) => (
                format!(" Removed {removed} older automatic backups."),
                false,
            ),
            Err(error) => (
                format!(" Automatic backup cleanup needs attention: {error}."),
                true,
            ),
        }
    }

    fn refresh_sunrise_version(&mut self) {
        self.sunrise_version = detect_sunrise_version(&self.install_path);
    }

    fn save_preferences(&self) -> Result<(), String> {
        let path = preferences_path().ok_or("Could not locate Sundial's preferences folder")?;
        let parent = path
            .parent()
            .ok_or("Sundial's preferences path has no parent folder")?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create Sundial's preferences folder: {e}"))?;
        let preferences = Preferences {
            install: Some(self.install_path.clone()),
            settings_layout: Some(self.settings_layout.preference_value().to_owned()),
            really_unsafe_warning_acknowledged: self.really_unsafe_warning_acknowledged,
            default_plug_selection_mode: self.default_plug_selection_mode,
            show_safety_warnings: self.show_safety_warnings,
            review_changes_before_saving: self.review_changes_before_saving,
            limit_automatic_backups: self.limit_automatic_backups,
            automatic_backup_limit: normalized_automatic_backup_limit(self.automatic_backup_limit),
            color_theme: self.color_theme,
            always_open_json_editor_in_second_window: self.always_open_json_editor_in_second_window,
            show_plug_hashes: self.show_plug_hashes,
            item_card_width: self.item_card_width,
            character_inventory_layout: self.character_inventory_layout,
            experimental_orbit_backdrops: self.experimental_orbit_backdrops,
            experimental_progression: self.experimental_progression,
            experimental_power_above_cap: self.experimental_power_above_cap,
        };
        let encoded = serde_json::to_vec_pretty(&preferences)
            .map_err(|e| format!("Could not encode Sundial's preferences: {e}"))?;
        storage::replace_file(&path, &encoded)
            .map_err(|e| format!("Could not save Sundial's preferences: {e}"))
    }

    fn clear_picker_state(&mut self) {
        self.searches.clear();
        self.plug_searches.clear();
        self.key_binding_ui.clear_pickers();
    }

    fn choose_install(&mut self, ctx: &egui::Context) {
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

    fn load_install(
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

    fn load_future_schema_install(
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

    fn begin_install_load(
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

    fn apply_install_load(&mut self, pending: PendingInstallLoad, manifest: Manifest) {
        let PendingInstallLoad {
            install_path,
            settings_path,
            settings_layout,
            document,
        } = pending;
        self.install_path = install_path;
        self.settings_path = settings_path;
        self.settings_layout = settings_layout;
        self.manifest = manifest;
        self.hash_inspection.close();
        self.progression_ui.reset_navigation();
        self.collections_ui.reset_navigation();
        let document = WorkspaceDocument::load(document, &self.settings_path);
        let warning = validate_workspace_document(&document).err();
        self.class_armor_defaults = self
            .account_workspace
            .class_armor_default_characters(&document);
        self.persisted_document = document.clone();
        self.document = document;
        self.progression_ui.invalidate_document();
        self.refresh_sunrise_version();
        self.source_warning.clone_from(&warning);
        self.selected_character = 0;
        self.clear_picker_state();
        self.sync_raw_json();
        self.dirty = false;
        match self.save_preferences() {
            Ok(()) => match warning {
                Some(warning) => self.set_status(
                    format!(
                        "Install loaded with an unexpected setting: {warning}. A safety copy will be created beside settings.json before saving"
                    ),
                    true,
                ),
                None => self.set_status("Shadowkeep install and Sunrise settings loaded", false),
            },
            Err(error) => self.set_status(
                format!("Install loaded, but its location could not be remembered: {error}"),
                true,
            ),
        }
    }

    fn start_catalog_task(
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

    fn poll_catalog_task(&mut self) {
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
                            self.set_status(format!("Catalog not rebuilt: {error}"), true);
                        }
                    }
                    break;
                }
            }
        }
    }

    fn rebuild_catalog(&mut self, ctx: &egui::Context) {
        self.set_status("Scanning installed Shadowkeep packages…", false);
        self.start_catalog_task(
            ctx,
            self.install_path.clone(),
            true,
            CatalogTaskKind::Rebuild,
        );
    }

    fn characters(&self) -> Option<&[Value]> {
        self.document
            .pointer("/state/characters")?
            .as_array()
            .map(Vec::as_slice)
    }

    fn character_count(&self) -> usize {
        self.account_workspace.character_count(&self.document)
    }

    fn draw_character_tabs(&mut self, ui: &mut egui::Ui) {
        let character_tabs = (0..self.character_count())
            .map(|index| {
                let class_type = self
                    .account_workspace
                    .character_metadata(&self.document, index)
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
                    self.selected_character = index;
                }
            }
        });
    }

    fn draw_app_chrome(&mut self, ctx: &egui::Context, available_update: Option<&str>) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.has_unsaved_changes() {
                    ui.label(
                        egui::RichText::new("Unsaved changes").color(ui.visuals().warn_fg_color),
                    );
                }
                let undo_label = self.undo_history.last().map(|entry| entry.label.clone());
                let redo_label = self.redo_history.last().map(|entry| entry.label.clone());
                let undo = ui
                    .add_enabled(undo_label.is_some(), egui::Button::new("Undo"))
                    .on_disabled_hover_text("Nothing to undo");
                let undo = if let Some(label) = undo_label.as_deref() {
                    undo.on_hover_text(format!("Undo: {label}"))
                } else {
                    undo
                };
                if undo.clicked() {
                    self.undo();
                }
                let redo = ui
                    .add_enabled(redo_label.is_some(), egui::Button::new("Redo"))
                    .on_disabled_hover_text("Nothing to redo");
                let redo = if let Some(label) = redo_label.as_deref() {
                    redo.on_hover_text(format!("Redo: {label}"))
                } else {
                    redo
                };
                if redo.clicked() {
                    self.redo();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(self.has_unsaved_changes(), egui::Button::new("Save"))
                        .clicked()
                    {
                        self.request_save(ctx, GeneratedFileSaveAction::Save);
                    }
                    if ui.button("Reload").clicked() {
                        if self.has_unsaved_changes() {
                            self.confirmation = Some(ConfirmationDialog::Reload);
                        } else {
                            self.reload();
                        }
                    }
                });
            });
        });

        egui::SidePanel::left("characters")
            .resizable(false)
            .exact_width(MAIN_SIDEBAR_WIDTH)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                for (view, label) in [
                    (ViewMode::Characters, "Characters & loadouts"),
                    (ViewMode::ProfileInventory, "Profile inventory"),
                    (ViewMode::CharacterInventory, "Character inventory"),
                    (ViewMode::GameSettings, "Game settings"),
                ] {
                    if ui.selectable_label(self.view_mode == view, label).clicked() {
                        self.select_view(view);
                    }
                }
                if self.experimental_progression
                    && ui
                        .selectable_label(self.view_mode == ViewMode::Progression, "Progression")
                        .clicked()
                {
                    self.select_view(ViewMode::Progression);
                }
                for (view, label) in [
                    (ViewMode::AdvancedJson, "All settings (JSON)"),
                    (ViewMode::Preferences, "Preferences"),
                ] {
                    if ui.selectable_label(self.view_mode == view, label).clicked() {
                        self.select_view(view);
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.horizontal(|ui| {
                        if ui.small_button("About").clicked() {
                            self.about_open = true;
                        }
                        if let Some(version) = available_update {
                            let update_text = egui::RichText::new("Update Available")
                                .color(ui.visuals().hyperlink_color);
                            if ui
                                .add(egui::Button::new(update_text).small())
                                .on_hover_text(format!(
                                    "Sundial {version} is available. Open GitHub Releases."
                                ))
                                .clicked()
                            {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(RELEASES_URL));
                            }
                        }
                    });
                });
            });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let color = if self.status_is_error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let account_source = self.document.source_info();
                if account_source.kind != AccountSourceKind::Json {
                    let source_color = if account_source.kind == AccountSourceKind::Blocked {
                        ui.visuals().error_fg_color
                    } else if ui.visuals().dark_mode {
                        egui::Color32::from_rgb(105, 156, 118)
                    } else {
                        egui::Color32::from_rgb(64, 122, 80)
                    };
                    ui.label(
                        egui::RichText::new(egui_phosphor::regular::DATABASE)
                            .size(16.0)
                            .color(source_color),
                    )
                    .on_hover_text(format!(
                        "{}\n\n{}\n\nPath: {}\nContract: {}",
                        account_source.label,
                        account_source.detail,
                        account_source.database_path.display(),
                        account_source.contract,
                    ));
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.colored_label(color, &self.status);
                });
            });
        });
    }

    fn draw_about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let logo = self
            .logo
            .get_or_insert_with(|| load_logo_texture(ctx))
            .clone();
        let update_status = self.update_check.status().clone();
        let mut retry_update_check = false;
        egui::Window::new("About Sundial")
            .open(&mut self.about_open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_width(430.0);
                ui.vertical_centered(|ui| {
                    ui.image((logo.id(), egui::vec2(64.0, 64.0)));
                    ui.heading("Sundial");
                    ui.label(egui::RichText::new(DISPLAY_VERSION).weak());
                    ui.add_space(8.0);
                    ui.label("A simple Project Sunrise settings editor.");
                    ui.hyperlink_to("github.com/kylethmpsn/sundial", PROJECT_URL);
                    ui.add_space(8.0);
                    match &update_status {
                        UpdateStatus::NotStarted => {
                            retry_update_check = ui.button("Check for updates").clicked();
                        }
                        UpdateStatus::Checking => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Checking for updates...");
                            });
                        }
                        UpdateStatus::Current => {
                            ui.label(egui::RichText::new("Sundial is up to date.").weak());
                        }
                        UpdateStatus::Available(version) => {
                            ui.colored_label(
                                ui.visuals().warn_fg_color,
                                format!("Sundial {version} is available."),
                            );
                            ui.hyperlink_to("Open GitHub Releases", RELEASES_URL);
                        }
                        UpdateStatus::Failed => {
                            ui.label(
                                egui::RichText::new("Could not check for updates.").weak(),
                            );
                            retry_update_check = ui.button("Try again").clicked();
                        }
                    }
                });
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label("Built for Project Sunrise 0.1 through 0.3.2.");
                ui.hyperlink_to("Project Sunrise on GitHub", SUNRISE_URL);
                ui.add_space(6.0);
                ui.label("Local Destiny package parsing is powered by tiger-pkg.");
                ui.hyperlink_to("tiger-pkg on GitHub", TIGER_PKG_URL);
                ui.add_space(6.0);
                ui.hyperlink_to("For additional credits, see the project README.", CREDITS_URL);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "This project is not affiliated with or endorsed by Bungie Inc. or Sony Interactive Entertainment. Destiny and related intellectual property are owned by Bungie Inc. and their respective rights holders.",
                    )
                    .weak(),
                );
            });
        if retry_update_check {
            self.update_check.retry(ctx);
        }
    }

    fn draw_catalog_progress(&self, ctx: &egui::Context) {
        let Some(task) = &self.catalog_task else {
            return;
        };
        let progress = task.progress;
        let title = task.kind.title();
        let path = match &task.kind {
            CatalogTaskKind::LoadInstall(pending) => &pending.install_path,
            CatalogTaskKind::Rebuild => &self.install_path,
        };
        egui::Modal::new("catalog_task_progress".into()).show(ctx, |ui| {
            ui.set_width(500.0);
            ui.heading(title);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.strong(progress.message);
            });
            ui.add_space(10.0);
            let mut bar = egui::ProgressBar::new(progress.fraction()).desired_width(480.0);
            if progress.total > 0 {
                bar = bar.show_percentage();
            } else {
                bar = bar.animate(true);
            }
            ui.add(bar);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(path.display().to_string())
                    .weak()
                    .small(),
            );
        });
    }

    fn sync_raw_json(&mut self) {
        if let Ok(raw_json) = encode_settings_for_editor(self.document.json()) {
            self.raw_json = raw_json;
            self.raw_json_document = self.document.json().clone();
            self.json_editor.mark_synced();
            self.json_editor.restore_location_next_draw();
        }
    }

    fn sync_raw_json_if_stale(&mut self) {
        if !self.json_editor.has_unapplied_changes()
            && self.raw_json_document != *self.document.json()
        {
            self.sync_raw_json();
        }
    }

    fn apply_raw_json(&mut self) -> bool {
        self.apply_raw_json_with_status(true)
    }

    fn apply_raw_json_silently(&mut self) -> bool {
        self.apply_raw_json_with_status(false)
    }

    fn apply_raw_json_with_status(&mut self, report_status: bool) -> bool {
        match serde_json::from_str::<Value>(&self.raw_json) {
            Ok(document) => {
                let changed = document != *self.document.json();
                self.raw_json_document = document.clone();
                self.document.replace_json(document);
                let warning = validate_workspace_document(&self.document).err();
                self.progression_ui.invalidate_document();
                self.selected_character = self
                    .selected_character
                    .min(self.character_count().saturating_sub(1));
                self.clear_picker_state();
                self.dirty |= changed;
                if report_status {
                    if let Some(warning) = warning {
                        self.set_status(
                            format!(
                                "Advanced JSON applied with an unexpected setting: {warning}. Saving will first create settings.json.bak beside the source"
                            ),
                            true,
                        );
                    } else {
                        self.set_status("Advanced JSON applied; click Save to write it", false);
                    }
                }
                self.json_editor.mark_synced();
                true
            }
            Err(error) => {
                if report_status {
                    self.set_status(
                        format!(
                            "JSON syntax error at line {}, column {}: {error}",
                            error.line(),
                            error.column()
                        ),
                        true,
                    );
                }
                false
            }
        }
    }

    fn set_json_editor_window_open(&mut self, open: bool) {
        update_detached_window_state(
            &mut self.json_editor_window_open,
            &mut self.json_editor_window_generation,
            open,
        );
    }

    fn handle_json_editor_response(
        &mut self,
        ctx: &egui::Context,
        response: json_editor::JsonEditorResponse,
    ) {
        if response.save {
            self.request_save(ctx, GeneratedFileSaveAction::Save);
        }
        if response.reset {
            self.sync_raw_json();
            self.set_status("JSON editor reset to current settings", false);
        }
        if response.toggle_window {
            self.set_json_editor_window_open(!self.json_editor_window_open);
            self.json_editor.restore_location_next_draw();
            if !self.json_editor_window_open {
                self.view_mode = ViewMode::AdvancedJson;
            }
        }
    }

    fn inventory_layout_preview_item(
        &self,
        ctx: &egui::Context,
    ) -> Option<InventoryLayoutPreviewItem> {
        let from_hash = |hash| self.inventory_layout_preview_item_for_hash(ctx, hash);
        from_hash(INVENTORY_LAYOUT_PREVIEW_HASH)
            .or_else(|| {
                self.account_workspace
                    .equipped_item_snapshots(&self.document, self.selected_character)
                    .ok()?
                    .into_iter()
                    .filter_map(|snapshot| snapshot.definition_hash)
                    .find_map(from_hash)
            })
            .or_else(|| {
                SLOTS
                    .iter()
                    .filter(|(slot, _, _)| {
                        WEAPON_SLOTS.contains(slot) || ARMOR_SLOTS.contains(slot)
                    })
                    .flat_map(|(_, _, bucket_hash)| {
                        self.manifest
                            .items_for_bucket(*bucket_hash)
                            .map(|item| item.hash)
                    })
                    .find_map(from_hash)
            })
    }

    fn inventory_layout_preview_item_for_hash(
        &self,
        ctx: &egui::Context,
        hash: u64,
    ) -> Option<InventoryLayoutPreviewItem> {
        let item = self.manifest.item(hash)?;
        let &(slot, slot_label, bucket_hash) = SLOTS.iter().find(|(slot, _, bucket_hash)| {
            *bucket_hash == item.bucket_hash
                && (WEAPON_SLOTS.contains(slot) || ARMOR_SLOTS.contains(slot))
        })?;
        (!item.name.trim().is_empty() && self.manifest.icon_texture(ctx, hash).is_some()).then(
            || InventoryLayoutPreviewItem {
                hash,
                slot,
                slot_label,
                bucket_hash,
                class_type: item.class_type,
                power: self.manifest.item_power_cap(hash).unwrap_or(136),
            },
        )
    }

    fn draw_inventory_layout_choices(
        &mut self,
        ui: &mut egui::Ui,
        selected: &mut CharacterInventoryLayout,
    ) -> bool {
        let previous = *selected;
        ui.horizontal(|ui| {
            ui.selectable_value(
                selected,
                CharacterInventoryLayout::Cards,
                "Sundial cards (default)",
            );
            ui.selectable_value(
                selected,
                CharacterInventoryLayout::Panoptes,
                "Panoptes grid",
            );
        });
        ui.label(
            egui::RichText::new(match selected {
                CharacterInventoryLayout::Cards => "Full item cards with inline editing controls",
                CharacterInventoryLayout::Panoptes => {
                    "Selected-item editor beside the equipped and inventory grid"
                }
            })
            .weak(),
        );
        ui.add_space(6.0);
        let preview = self.inventory_layout_preview_item(ui.ctx());
        self.draw_inventory_layout_preview(ui, preview.as_ref(), *selected);
        previous != *selected
    }

    fn draw_inventory_layout_preview(
        &mut self,
        ui: &mut egui::Ui,
        preview: Option<&InventoryLayoutPreviewItem>,
        layout: CharacterInventoryLayout,
    ) {
        let Some(preview) = preview else {
            ui.label(egui::RichText::new("Preview available after the catalog loads").weak());
            return;
        };
        let snapshot = preview.snapshot();
        let preview_id = match layout {
            CharacterInventoryLayout::Cards => "cards",
            CharacterInventoryLayout::Panoptes => "panoptes",
        };
        ui.push_id(
            ("inventory-layout-preview", preview_id),
            |ui| match layout {
                CharacterInventoryLayout::Cards => {
                    let width = ItemCardWidth::Standard
                        .dimensions()
                        .1
                        .min(ui.available_width());
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(width);
                                self.draw_equipment_slot_card(
                                    ui,
                                    0,
                                    equipment::EquipmentSlotCard {
                                        id_scope: "preferences-layout-preview",
                                        slot: preview.slot,
                                        label: preview.slot_label,
                                        bucket_hash: preview.bucket_hash,
                                        class_type: preview.class_type,
                                        editable: false,
                                        header_fill: None,
                                        snapshot: Some(&snapshot),
                                    },
                                );
                            },
                        );
                    });
                }
                CharacterInventoryLayout::Panoptes => {
                    let width = ui.available_width().min(880.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(width);
                                self.draw_panoptes_layout_preview(
                                    ui,
                                    &snapshot,
                                    preview.class_type,
                                );
                            },
                        );
                    });
                }
            },
        );
    }

    fn draw_preferences_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Preferences");
        ui.add_space(6.0);
        let mut reset_requested = false;
        ui.horizontal(|ui| {
            for tab in PreferencesTab::ALL {
                ui.selectable_value(&mut self.preferences_tab, tab, tab.label());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reset_requested = ui
                    .small_button("Reset preferences…")
                    .on_hover_text(
                        "Reset interface, editing, saving, and experimental preferences. Paths, catalog data, and backups are not changed.",
                    )
                    .clicked();
            });
        });
        ui.separator();

        let mut preferences_changed = false;
        let mut preferences_reset = false;
        if reset_requested {
            self.reset_preferences_to_defaults(ctx);
            preferences_changed = true;
            preferences_reset = true;
        }

        let selected_tab = self.preferences_tab;
        preferences_changed |= egui::ScrollArea::vertical()
            .id_salt(("preferences", selected_tab))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                match selected_tab {
                    PreferencesTab::Interface => self.draw_interface_preferences(ui, ctx),
                    PreferencesTab::Editing => self.draw_editing_preferences(ui),
                    PreferencesTab::Sunrise => {
                        self.draw_sunrise_preferences(ui, ctx);
                        false
                    }
                    PreferencesTab::SavingRecovery => self.draw_saving_recovery_preferences(ui),
                }
            })
            .inner;

        if preferences_changed {
            match self.save_preferences() {
                Ok(()) => self.set_status(
                    if preferences_reset {
                        "Preferences reset to defaults"
                    } else {
                        "Preferences saved"
                    },
                    false,
                ),
                Err(error) => self.set_status(
                    format!("Preferences changed, but could not be saved: {error}"),
                    true,
                ),
            }
        }
    }

    fn reset_preferences_to_defaults(&mut self, ctx: &egui::Context) {
        let defaults = Preferences::default();
        self.color_theme = defaults.color_theme;
        ctx.set_theme(defaults.color_theme.egui_theme());
        self.item_card_width = defaults.item_card_width;
        self.character_inventory_layout = defaults.character_inventory_layout;
        self.always_open_json_editor_in_second_window =
            defaults.always_open_json_editor_in_second_window;
        self.default_plug_selection_mode = defaults.default_plug_selection_mode;
        self.plug_selection_mode = defaults.default_plug_selection_mode;
        self.show_safety_warnings = defaults.show_safety_warnings;
        self.review_changes_before_saving = defaults.review_changes_before_saving;
        self.limit_automatic_backups = defaults.limit_automatic_backups;
        self.automatic_backup_limit = defaults.automatic_backup_limit;
        self.show_plug_hashes = defaults.show_plug_hashes;
        self.experimental_orbit_backdrops = defaults.experimental_orbit_backdrops;
        self.experimental_progression = defaults.experimental_progression;
        self.experimental_power_above_cap = defaults.experimental_power_above_cap;
        self.really_unsafe_warning_acknowledged = defaults.really_unsafe_warning_acknowledged;
        self.remember_plug_selection_mode_after_confirmation = false;
        if !self.experimental_progression && self.view_mode == ViewMode::Progression {
            self.select_view(ViewMode::Characters);
        }
    }

    fn draw_interface_preferences(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
        let mut preferences_changed = false;

        ui.strong("Appearance");
        let mut requested_theme = self.color_theme;
        ui.horizontal(|ui| {
            ui.label("Color theme:");
            ui.radio_value(&mut requested_theme, ColorTheme::Dark, "Dark (recommended)");
            ui.radio_value(&mut requested_theme, ColorTheme::Light, "Light");
        });
        if requested_theme != self.color_theme {
            self.color_theme = requested_theme;
            ctx.set_theme(requested_theme.egui_theme());
            preferences_changed = true;
        }
        let mut requested_card_width = self.item_card_width;
        ui.horizontal_wrapped(|ui| {
            ui.label("Item card width:");
            ui.radio_value(&mut requested_card_width, ItemCardWidth::Compact, "Compact");
            ui.radio_value(
                &mut requested_card_width,
                ItemCardWidth::Standard,
                "Standard",
            );
            ui.radio_value(&mut requested_card_width, ItemCardWidth::Wide, "Wide");
        });
        if requested_card_width != self.item_card_width {
            self.item_card_width = requested_card_width;
            preferences_changed = true;
        }

        ui.add_space(8.0);
        ui.label(egui::RichText::new("Character inventory layout").strong());
        let mut requested_inventory_layout = self.character_inventory_layout;
        if self.draw_inventory_layout_choices(ui, &mut requested_inventory_layout) {
            self.character_inventory_layout = requested_inventory_layout;
            preferences_changed = true;
        }
        if ui
            .checkbox(
                &mut self.always_open_json_editor_in_second_window,
                "Open All settings (JSON) in a second window",
            )
            .changed()
        {
            preferences_changed = true;
        }

        preferences_changed
    }

    fn draw_editing_preferences(&mut self, ui: &mut egui::Ui) -> bool {
        let mut preferences_changed = false;

        ui.strong("Item editing");
        ui.label("Choose the plug selection mode Sundial uses when it starts.");
        ui.add_space(4.0);

        let mut requested_mode = self.default_plug_selection_mode;
        ui.horizontal_wrapped(|ui| {
            ui.label("Default plug selection mode:");
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::Supported,
                PlugSelectionMode::Supported.label(),
            );
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::SocketAndGearType,
                PlugSelectionMode::SocketAndGearType.label(),
            );
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::MatchingSocketType,
                PlugSelectionMode::MatchingSocketType.label(),
            );
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::GearType,
                PlugSelectionMode::GearType.label(),
            );
            ui.radio_value(
                &mut requested_mode,
                PlugSelectionMode::AnyPlug,
                PlugSelectionMode::AnyPlug.label(),
            );
        });

        if requested_mode != self.default_plug_selection_mode {
            if requested_mode == PlugSelectionMode::AnyPlug
                && !self.really_unsafe_warning_acknowledged
            {
                self.remember_plug_selection_mode_after_confirmation = true;
                self.confirmation = Some(ConfirmationDialog::ReallyUnsafe);
            } else {
                self.default_plug_selection_mode = requested_mode;
                self.plug_selection_mode = requested_mode;
                preferences_changed = true;
            }
        }

        let warning_response = ui.checkbox(
            &mut self.show_safety_warnings,
            "Show plug-selection safety warnings",
        );
        preferences_changed |= warning_response.changed();

        let hash_response =
            ui.checkbox(&mut self.show_plug_hashes, "Show plug hashes on item cards");
        preferences_changed |= hash_response.changed();

        if self.show_safety_warnings {
            draw_plug_selection_warning(ui, self.default_plug_selection_mode);
        }

        ui.add_space(12.0);
        ui.strong("Experimental");
        let power_above_cap_response = ui.checkbox(
            &mut self.experimental_power_above_cap,
            "Allow Power above item caps",
        );
        preferences_changed |= power_above_cap_response.changed();
        ui.label(
            egui::RichText::new(
                "Allows manual Power values above an item's package-defined cap. Destiny may display the item at its cap, but the stored value may still affect overall character Power. Newly added items still start at their normal cap.",
            )
            .weak(),
        );
        ui.add_space(6.0);
        let progression_response =
            ui.checkbox(&mut self.experimental_progression, "Enable Progression");
        if progression_response.changed() {
            preferences_changed = true;
            if !self.experimental_progression && self.view_mode == ViewMode::Progression {
                self.select_view(ViewMode::Characters);
            }
        }
        ui.label(
            egui::RichText::new(
                "Shows package-backed Unlocks, Investment, and Collections editing.",
            )
            .weak(),
        );
        ui.add_space(6.0);
        let orbit_backdrops_response = ui.checkbox(
            &mut self.experimental_orbit_backdrops,
            "Enable Orbit backdrop selection",
        );
        preferences_changed |= orbit_backdrops_response.changed();
        ui.label(
            egui::RichText::new(
                "Adds package-backed Orbit backdrop selection to Game settings > Player. Requires an unmerged Sunrise PR.",
            )
            .weak(),
        );

        preferences_changed
    }

    fn draw_sunrise_preferences(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.strong("Installation and compatibility");
        ui.label("Select the Destiny 2 Shadowkeep installation. Sundial finds Project Sunrise's settings.json inside it automatically.");
        ui.add_space(10.0);
        let account_source = self.document.source_info();
        egui::Grid::new("preferences_sunrise_grid")
            .num_columns(3)
            .spacing([12.0, 10.0])
            .show(ui, |ui| {
                ui.label("Shadowkeep install");
                ui.monospace(self.install_path.display().to_string());
                if ui.button("Choose…").clicked() {
                    self.choose_install(ctx);
                }
                ui.end_row();
                ui.label("Active account source");
                ui.colored_label(
                    match account_source.kind {
                        AccountSourceKind::Json => ui.visuals().text_color(),
                        AccountSourceKind::Sqlite => ui.visuals().hyperlink_color,
                        AccountSourceKind::Blocked => ui.visuals().error_fg_color,
                    },
                    account_source.label,
                );
                ui.end_row();
                ui.label("Settings schema");
                ui.monospace(game_settings::schema_version(&self.document).map_or_else(
                    || "Missing or invalid".to_owned(),
                    |version| version.to_string(),
                ))
                .on_hover_text("Sundial uses this value to determine compatibility.");
                ui.end_row();
                ui.label("Detected Sunrise version");
                ui.monospace(&self.sunrise_version)
                    .on_hover_text("Shown for reference; this does not control compatibility.");
                ui.end_row();
                ui.label("Account contract");
                ui.monospace(account_source.contract);
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.colored_label(
            if account_source.kind == AccountSourceKind::Blocked {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            },
            &account_source.detail,
        );
        ui.label(
            egui::RichText::new(
                "Sundial saves account edits only to the active source. It never mirrors account data between state.sqlite3 and settings.json.",
            )
            .weak(),
        );
        ui.add_space(12.0);
        ui.strong("Catalog");
        ui.label(format!(
            "Local catalog cache: {}",
            self.manifest.cache_path.display()
        ));
        ui.label(if self.manifest.loaded_from_cache {
            "Loaded from local cache"
        } else {
            "Scanned from game packages"
        });
        let catalog_stats = self.manifest.stats();
        ui.label(format!(
            "{} items · {} plugs · {} icons · {} descriptions",
            catalog_stats.items,
            catalog_stats.plugs,
            catalog_stats.icons,
            catalog_stats.descriptions,
        ));
        if ui.button("Rebuild catalog from game files").clicked() {
            self.rebuild_catalog(ctx);
        }
        ui.add_space(6.0);
        ui.label("The first scan reads the installed packages. Later starts use the local cache unless the package files change.");
    }

    fn draw_saving_recovery_preferences(&mut self, ui: &mut egui::Ui) -> bool {
        let mut preferences_changed = false;

        ui.strong("Saving");
        let review_response = ui.checkbox(
            &mut self.review_changes_before_saving,
            "Review changes before saving",
        );
        preferences_changed |= review_response.changed();
        ui.label(
            egui::RichText::new(
                "Adds a confirmation step listing changed fields. Validation, conflict checks, backups, and SQLite safety always run.",
            )
            .weak(),
        );

        ui.add_space(12.0);
        ui.strong("Automatic backups");
        ui.label("Sundial creates a source-specific backup before every save.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let retention_response = ui.checkbox(&mut self.limit_automatic_backups, "Keep last");
            preferences_changed |= retention_response.changed();
            let limit_response = ui.add_enabled(
                self.limit_automatic_backups,
                egui::DragValue::new(&mut self.automatic_backup_limit)
                    .range(MIN_AUTOMATIC_BACKUP_LIMIT..=MAX_AUTOMATIC_BACKUP_LIMIT),
            );
            preferences_changed |= limit_response.changed();
            ui.label("automatic backups per source");
        });
        ui.label(
            egui::RichText::new(
                "Disabled by default. Recovery snapshots and manual settings.json.bak safety copies are never removed.",
            )
            .weak(),
        );

        ui.add_space(12.0);
        ui.strong("Recovery");
        let account_source = self.document.source_info();
        egui::Grid::new("preferences_recovery_paths_grid")
            .num_columns(3)
            .spacing([12.0, 10.0])
            .show(ui, |ui| {
                ui.label("Sunrise settings");
                ui.monospace(self.settings_path.display().to_string());
                if ui
                    .button("Reset to Sunrise defaults…")
                    .on_hover_text(
                        "Restore the settings bundled with this installed Sunrise version; the current settings.json is backed up first",
                    )
                    .clicked()
                {
                    self.confirmation = Some(ConfirmationDialog::ResetDefaults);
                }
                ui.end_row();
                ui.label("Sunrise account database");
                ui.monospace(account_source.database_path.display().to_string());
                if matches!(
                    account_source.kind,
                    AccountSourceKind::Sqlite | AccountSourceKind::Blocked
                ) {
                    if ui
                        .button("Restore backup…")
                        .on_hover_text(
                            "Restore a verified Sundial account backup; the current database is preserved first",
                        )
                        .clicked()
                    {
                        self.request_sqlite_backup_restore();
                    }
                } else {
                    ui.label("");
                }
                ui.end_row();
            });
        ui.add_space(8.0);
        if ui.button("Browse backups…").clicked() {
            match backups_path()
                .ok_or("Could not locate Sundial's backups folder".to_owned())
                .and_then(|path| {
                    fs::create_dir_all(&path)
                        .map_err(|error| format!("Could not create {}: {error}", path.display()))
                        .map(|()| path)
                })
                .and_then(|path| open_directory(&path))
            {
                Ok(()) => self.set_status("Opened the backups folder", false),
                Err(error) => self.set_status(error, true),
            }
        }

        preferences_changed
    }

    fn draw_json_editor_window(&mut self, ctx: &egui::Context) {
        if !self.json_editor_window_open {
            return;
        }

        self.sync_raw_json_if_stale();
        let account_source_kind = self.document.source_info().kind;
        let (response, close_requested) = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of((
                "sundial_json_editor",
                self.json_editor_window_generation,
            )),
            egui::ViewportBuilder::default()
                .with_title("Sundial: All settings (JSON)")
                .with_inner_size([960.0, 720.0])
                .with_min_inner_size([640.0, 420.0]),
            |child_ctx, class| {
                let close_requested = child_ctx.input(|input| input.viewport().close_requested());
                let mut response = json_editor::JsonEditorResponse::default();
                if class == egui::ViewportClass::Embedded {
                    egui::Window::new("All settings (JSON)")
                        .id(egui::Id::new("embedded_json_editor_window"))
                        .default_size([960.0, 720.0])
                        .show(child_ctx, |ui| {
                            draw_json_account_source_notice(ui, account_source_kind);
                            response = json_editor::draw(
                                ui,
                                &mut self.raw_json,
                                &mut self.json_editor,
                                true,
                            );
                        });
                } else {
                    egui::CentralPanel::default().show(child_ctx, |ui| {
                        draw_json_account_source_notice(ui, account_source_kind);
                        response =
                            json_editor::draw(ui, &mut self.raw_json, &mut self.json_editor, true);
                    });
                }
                (response, close_requested)
            },
        );

        self.handle_json_editor_response(ctx, response);
        if self.json_editor.has_unapplied_changes() {
            let _ = self.apply_raw_json_silently();
        }
        if close_requested {
            self.set_json_editor_window_open(false);
            self.json_editor.restore_location_next_draw();
            if self.json_editor.has_unapplied_changes() {
                self.view_mode = ViewMode::AdvancedJson;
            }
        }
    }
}

fn preserve_inactive_json_account_domains(defaults: &mut Value, source: &Value) {
    let Some(default_state) = defaults.get_mut("state").and_then(Value::as_object_mut) else {
        return;
    };
    let source_state = source.get("state").and_then(Value::as_object);
    for key in ["account", "characters"] {
        if let Some(value) = source_state.and_then(|state| state.get(key)) {
            default_state.insert(key.to_owned(), value.clone());
        } else {
            default_state.remove(key);
        }
    }
}

fn draw_json_account_source_notice(ui: &mut egui::Ui, source: AccountSourceKind) {
    let message = match source {
        AccountSourceKind::Json => return,
        AccountSourceKind::Sqlite => {
            "state.sqlite3 is the active account source. /state/account and /state/characters in this JSON are inactive legacy data; editing them changes settings.json only and will not change or sync the active account."
        }
        AccountSourceKind::Blocked => {
            "SQLite account loading is blocked. /state/account and /state/characters in this JSON are not a fallback and editing them will not unblock or change the active account source."
        }
    };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Account source notice:");
            ui.label(message);
        });
    });
    ui.add_space(6.0);
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
                    "The detached editor currently contains invalid JSON. Fix or reset it before using guided settings.",
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
                            let character_editable = self
                                .account_workspace
                                .can_mutate_equipment(&self.document);
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
                    let account_settings = self.account_workspace.account_settings_map(&self.document);
                    let bindings_editable = self
                        .account_workspace
                        .named_key_bindings_editable(&self.document);
                    let edits = game_settings::draw_page(
                        ui,
                        game_settings::PageContext {
                            json_document: self.document.json_mut(),
                            account_settings: account_settings.as_ref().map_err(String::as_str),
                            bindings_editable,
                            orbit_backdrops: self.manifest.orbit_backdrops(),
                            player_tools: game_settings::PlayerTools {
                            orbit_backdrops_enabled: self.experimental_orbit_backdrops,
                            },
                            tab: &mut self.game_settings_tab,
                            key_bindings: &mut self.key_binding_ui,
                        },
                    );
                    let account_changed =
                        match self.account_workspace.apply_account_settings(
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
                        self.set_status("Game setting updated; click Save to write it", false);
                    }
                }
                ViewMode::Progression => {
                    ui.heading("Progression");
                    ui.add_space(8.0);
                    let section_changed = ui
                        .horizontal(|ui| {
                            let mut changed = false;
                            changed |= ui
                                .selectable_value(
                                    &mut self.progression_section,
                                    ProgressionSection::Unlocks,
                                    "Unlocks",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut self.progression_section,
                                    ProgressionSection::Investment,
                                    "Investment",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut self.progression_section,
                                    ProgressionSection::Collections,
                                    "Collections",
                                )
                                .changed();
                            changed
                        })
                        .inner;
                    if section_changed {
                        self.progression_ui.reset_navigation();
                        self.collections_ui.reset_navigation();
                    }
                    ui.separator();

                    match self.progression_section {
                        ProgressionSection::Unlocks | ProgressionSection::Investment => {
                            let view = match self.progression_section {
                                ProgressionSection::Unlocks => progression::View::Unlocks,
                                ProgressionSection::Investment => progression::View::Investment,
                                ProgressionSection::Collections => unreachable!(),
                            };
                            if progression::draw_content(
                                ui,
                                self.document.json_mut(),
                                &self.manifest,
                                self.destiny_symbol_font_error.as_deref(),
                                &mut self.progression_ui,
                                view,
                            ) {
                                self.dirty = true;
                                self.set_status(
                                    "Progression updated; click Save to write it",
                                    false,
                                );
                            }
                        }
                        ProgressionSection::Collections => {
                            if collections_page::draw_content(
                                ui,
                                self.document.json_mut(),
                                &self.manifest,
                                &mut self.collections_ui,
                            ) {
                                self.dirty = true;
                                self.set_status(
                                    "Progression state updated; click Save to write it",
                                    false,
                                );
                            }
                        }
                    }
                }
                ViewMode::AdvancedJson => {
                    if self.json_editor_window_open {
                        ui.heading("All settings");
                        ui.label("The JSON editor is open in a separate window.");
                        if ui.button("Dock in main window").clicked() {
                            self.set_json_editor_window_open(false);
                            self.json_editor.restore_location_next_draw();
                        }
                    } else {
                        self.sync_raw_json_if_stale();
                        draw_json_account_source_notice(
                            ui,
                            self.document.source_info().kind,
                        );
                        let response = json_editor::draw(
                            ui,
                            &mut self.raw_json,
                            &mut self.json_editor,
                            false,
                        );
                        self.handle_json_editor_response(ctx, response);
                    }
                }
                ViewMode::Preferences => self.draw_preferences_page(ui, ctx),
            }
        });
    }

    fn prepare_frame(&mut self, ctx: &egui::Context) -> Option<String> {
        let undo_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
        let redo_shortcut = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        );
        let redo_windows_shortcut =
            egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Y);
        if ctx.input_mut(|input| input.consume_shortcut(&undo_shortcut)) {
            self.undo();
        } else if ctx.input_mut(|input| {
            input.consume_shortcut(&redo_shortcut) || input.consume_shortcut(&redo_windows_shortcut)
        }) {
            self.redo();
        }
        #[cfg(target_os = "linux")]
        {
            let title_bar_icon = self
                .title_bar_icon
                .get_or_insert_with(|| load_linux_title_bar_texture(ctx))
                .clone();
            if draw_linux_title_bar(ctx, &title_bar_icon) {
                if self.has_unsaved_changes() {
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
            UpdateStatus::Available(version) => Some(version.clone()),
            _ => None,
        };
        if ctx.input(|input| input.viewport().close_requested())
            && self.has_unsaved_changes()
            && !self.exit_confirmed
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirmation = Some(ConfirmationDialog::Exit);
        }

        self.refresh_after_focus_if_needed(ctx, ctx.input(|input| input.focused));
        available_update
    }

    fn draw_supporting_windows(&mut self, ctx: &egui::Context) {
        if let Some(hash) = inspector::take_definition_request(ctx) {
            let context = inspector::take_definition_context(ctx, hash);
            self.hash_inspection.open_with_context(hash, context);
        }
        let inspector_changed = inspector::draw_catalog_hash_window(
            ctx,
            &self.manifest,
            Some(self.document.json_mut()),
            self.experimental_progression,
            &mut self.hash_inspection,
            "global",
        );
        if inspector_changed {
            self.dirty = true;
            self.progression_ui.invalidate_document();
            self.set_status("Progression state updated; click Save to write it", false);
        }

        self.draw_json_editor_window(ctx);

        self.draw_about_window(ctx);

        self.draw_catalog_progress(ctx);
    }

    fn draw_pending_install_choice(&mut self, ctx: &egui::Context) {
        if let Some(install_path) = self.pending_install_choice.clone() {
            let mut selected = None;
            let mut cancel = false;
            let response = egui::Modal::new("choose_sunrise_settings".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                ui.heading("Choose Sunrise settings");
                ui.add_space(6.0);
                ui.label("Two existing settings.json files were found. Choose the one Project Sunrise uses for this installation.");
                ui.add_space(10.0);
                for layout in SettingsLayout::ALL {
                    let path = settings_path_for_install(&install_path, layout);
                    if ui
                        .button(format!("Use {}", layout.relative_path().display()))
                        .clicked()
                    {
                        selected = Some((layout, path.clone()));
                    }
                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .weak()
                            .small(),
                    );
                    ui.add_space(8.0);
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
            cancel |= response.should_close();
            if let Some((layout, path)) = selected {
                self.pending_install_choice = None;
                self.load_install(ctx, install_path, path, layout);
            } else if cancel {
                self.pending_install_choice = None;
            }
        }
    }

    fn draw_pending_generated_file(&mut self, ctx: &egui::Context) {
        if let Some(pending) = self.pending_generated_file.take() {
            let mut replace = false;
            let mut keep_existing = false;
            let response = egui::Modal::new("generated_file_replace_confirmation".into()).show(
                ctx,
                |ui| {
                    ui.set_width(760.0);
                    ui.heading(format!("Replace the existing {}?", pending.kind.label()));
                    ui.add_space(6.0);
                    ui.label(format!(
                        "Sunrise already has a different {}. Review the line diff before deciding; Sundial has not changed this file.",
                        pending.kind.file_name()
                    ));
                    ui.label(
                        egui::RichText::new(pending.path.display().to_string())
                            .weak()
                            .small(),
                    );
                    ui.add_space(8.0);
                    ui.label(format!(
                        "Existing: {} lines · Package-generated: {} lines",
                        normalized_generated_document(&pending.existing).lines().count(),
                        normalized_generated_document(&pending.generated)
                            .lines()
                            .count()
                    ));
                    egui::ScrollArea::vertical()
                        .id_salt("generated_file_diff")
                        .max_height(420.0)
                        .show(ui, |ui| {
                            ui.set_min_width(720.0);
                            for line in pending.diff.lines() {
                                let color = if line.starts_with('+') {
                                    ui.visuals().selection.bg_fill
                                } else if line.starts_with('-') {
                                    ui.visuals().error_fg_color
                                } else {
                                    ui.visuals().text_color()
                                };
                                ui.label(egui::RichText::new(line).monospace().color(color));
                            }
                        });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Yes, replace").clicked() {
                            replace = true;
                        }
                        if ui.button("No, keep existing").clicked() {
                            keep_existing = true;
                        }
                    });
                },
            );
            let cancel = response.should_close();
            if replace {
                match fs::read(&pending.path) {
                    Ok(raw)
                        if normalized_generated_document(&String::from_utf8_lossy(&raw))
                            == normalized_generated_document(&pending.existing) =>
                    {
                        self.resume_generated_file_action(
                            ctx,
                            pending.action,
                            pending.kind,
                            GeneratedFileDecision::Replace,
                        );
                    }
                    Ok(_) | Err(_) => {
                        self.set_status(
                            format!(
                                "The existing {} changed while the comparison was open; checking it again",
                                pending.kind.label()
                            ),
                            false,
                        );
                        self.resume_generated_file_action(
                            ctx,
                            pending.action,
                            pending.kind,
                            GeneratedFileDecision::Ask,
                        );
                    }
                }
            } else if keep_existing {
                self.resume_generated_file_action(
                    ctx,
                    pending.action,
                    pending.kind,
                    GeneratedFileDecision::KeepExisting,
                );
            } else if cancel {
                self.generated_file_decisions.clear();
                self.set_status(
                    format!(
                        "Save cancelled; the existing {} was not changed",
                        pending.kind.label()
                    ),
                    false,
                );
            } else {
                self.pending_generated_file = Some(pending);
            }
        }
    }

    fn draw_future_schema_confirmation(&mut self, ctx: &egui::Context) {
        if let Some(pending) = self.pending_future_schema.clone() {
            let mut proceed = false;
            let mut cancel = false;
            let response = egui::Modal::new("future_schema_warning".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                draw_future_schema_warning(ui, &pending);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Proceed with caution").clicked() {
                        proceed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.pending_future_schema = if !proceed && !cancel {
                Some(pending.clone())
            } else {
                None
            };
            if proceed {
                self.load_future_schema_install(ctx, pending);
            }
        }
    }

    fn draw_reset_defaults_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::ResetDefaults) {
            let mut reset = false;
            let mut cancel = false;
            let account_source = self.document.source_info().kind;
            let response = egui::Modal::new("restore_sunrise_defaults".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                ui.heading("Restore Sunrise defaults?");
                ui.add_space(6.0);
                ui.label(match account_source {
                    AccountSourceKind::Json => {
                        "This replaces the entire settings.json with the default bundled in your installed Project Sunrise version."
                    }
                    AccountSourceKind::Sqlite | AccountSourceKind::Blocked => {
                        "This restores bundled settings.json defaults while preserving its inactive legacy /state/account and /state/characters data. It does not change state.sqlite3."
                    }
                });
                ui.add_space(6.0);
                ui.label("Your current file will be preserved as settings.json.bak and as a timestamped Sundial backup. Any unsaved changes will be discarded.");
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(self.settings_path.display().to_string())
                        .weak()
                        .small(),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Restore defaults").clicked() {
                        reset = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.confirmation = (!reset && !cancel).then_some(ConfirmationDialog::ResetDefaults);
            if reset {
                self.reset_to_sunrise_defaults();
            }
        }
    }

    fn draw_sqlite_restore_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::RestoreSqliteBackup) {
            if let Some(backup) = self.pending_sqlite_restore.clone() {
                let mut restore = false;
                let mut cancel = false;
                let response =
                    egui::Modal::new("restore_sqlite_backup".into()).show(ctx, |ui| {
                        ui.set_width(560.0);
                        ui.heading("Restore this account database backup?");
                        ui.add_space(6.0);
                        ui.label("Sundial will replace state.sqlite3 with the selected compatible backup. Before replacement, it creates and integrity-checks a recovery snapshot of the current database.");
                        ui.add_space(6.0);
                        ui.label("Destiny 2 must be closed. Any unsaved Sundial changes will be discarded after the restored workspace reloads. settings.json is not changed or synchronized.");
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Selected backup").strong());
                        ui.label(
                            egui::RichText::new(backup.display().to_string())
                                .weak()
                                .small(),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("Restore account database").clicked() {
                                restore = true;
                            }
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                        });
                    });
                cancel |= response.should_close();
                if restore {
                    self.confirmation = None;
                    self.restore_selected_sqlite_backup();
                } else if cancel {
                    self.confirmation = None;
                    self.pending_sqlite_restore = None;
                    self.set_status("Database restore cancelled; no files were changed", false);
                }
            } else {
                self.confirmation = None;
            }
        }
    }

    fn draw_unsafe_mode_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::ReallyUnsafe) {
            let mut enable = false;
            let mut cancel = false;
            let account_source = self.document.source_info().kind;
            let response = egui::Modal::new("really_unsafe_confirmation".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                ui.heading("Really unsafe plug selection");
                ui.add_space(6.0);
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "This mode has a much higher chance of preventing the game from loading or causing Sunrise/Destiny 2 to crash.",
                );
                ui.add_space(8.0);
                ui.label("Even basic settings edits can theoretically cause problems, but this mode makes every discovered plug available in every socket. Saving arbitrary or incompatible combinations greatly increases the risk of leaving a character or the entire settings file unusable.");
                ui.add_space(8.0);
                ui.label("Every Sundial save creates timestamped backups for each source it changes in Sundial's local data folder.");
                ui.label(match account_source {
                    AccountSourceKind::Json => {
                        "If the game no longer loads, open Preferences > Recovery to restore bundled settings.json defaults. Sundial backs up the current file again first."
                    }
                    AccountSourceKind::Sqlite => {
                        "If an account edit prevents loading, open Preferences > Recovery to restore a verified state.sqlite3 backup. The current database is preserved again first."
                    }
                    AccountSourceKind::Blocked => {
                        "Account editing is currently blocked, so Sundial will not write the incompatible state.sqlite3."
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("I understand and enable").clicked() {
                        enable = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.confirmation = (!enable && !cancel).then_some(ConfirmationDialog::ReallyUnsafe);
            if enable {
                self.plug_selection_mode = PlugSelectionMode::AnyPlug;
                if self.remember_plug_selection_mode_after_confirmation {
                    self.default_plug_selection_mode = PlugSelectionMode::AnyPlug;
                }
                self.really_unsafe_warning_acknowledged = true;
                self.remember_plug_selection_mode_after_confirmation = false;
                if let Err(error) = self.save_preferences() {
                    self.set_status(
                        format!(
                            "Really unsafe mode enabled, but the preference could not be saved: {error}"
                        ),
                        true,
                    );
                }
            } else if cancel {
                self.remember_plug_selection_mode_after_confirmation = false;
            }
        }
    }

    fn draw_save_review_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::ReviewSave) {
            let mut changes = self
                .document
                .account_change_summaries(&self.persisted_document, CHANGE_REVIEW_LIMIT + 1);
            if self.document.account_changed_from(&self.persisted_document) && changes.is_empty() {
                changes.push("state.sqlite3: account data changed".to_owned());
            }
            if changes.len() <= CHANGE_REVIEW_LIMIT {
                changes.extend(collect_change_summaries(
                    self.persisted_document.json(),
                    self.document.json(),
                    CHANGE_REVIEW_LIMIT + 1 - changes.len(),
                ));
            }
            let truncated = changes.len() > CHANGE_REVIEW_LIMIT;
            changes.truncate(CHANGE_REVIEW_LIMIT);
            let total = changes.len();
            let mut confirm = false;
            let mut cancel = false;
            let action = self
                .pending_save_action
                .unwrap_or(GeneratedFileSaveAction::Save);
            let response = egui::Modal::new("review_settings_changes".into()).show(ctx, |ui| {
                ui.set_width(760.0);
                ui.heading(if action == GeneratedFileSaveAction::SaveAndExit {
                    "Review changes before saving and exiting"
                } else {
                    "Review changes before saving"
                });
                ui.add_space(6.0);
                let source_label = match (
                    self.document.json_changed_from(&self.persisted_document),
                    self.document.account_changed_from(&self.persisted_document),
                ) {
                    (true, true) => "settings.json and state.sqlite3",
                    (false, true) => "state.sqlite3",
                    _ => "settings.json",
                };
                ui.label(if truncated {
                    format!(
                        "Reviewing the first {total} changes that will be written to {source_label}."
                    )
                } else {
                    format!(
                        "Sundial will write {total} change{} to {source_label}.",
                        if total == 1 { "" } else { "s" }
                    )
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt("settings-change-review")
                    .max_height(430.0)
                    .show(ui, |ui| {
                        ui.set_min_width(720.0);
                        for change in &changes {
                            ui.label(egui::RichText::new(change).monospace().small());
                        }
                    });
                if truncated {
                    ui.label(
                        egui::RichText::new(
                            "The review is capped; additional changed fields may not be listed.",
                        )
                        .weak(),
                    );
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let save_label = if action == GeneratedFileSaveAction::SaveAndExit {
                        "Save and exit"
                    } else {
                        "Save changes"
                    };
                    if ui.button(save_label).clicked() {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            if confirm {
                self.confirmation = None;
                self.pending_save_action = None;
                self.perform_save_action(ctx, action);
            } else if cancel {
                self.confirmation = None;
                self.pending_save_action = None;
                self.set_status("Save cancelled; no files were changed", false);
            }
        }
    }

    fn draw_delete_equipment_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::DeleteEquipment) {
            let pending = self.pending_equipment_delete.clone();
            let mut delete = false;
            let mut cancel = false;
            if let Some(pending) = pending.as_ref() {
                let response = egui::Modal::new("delete_equipped_item".into()).show(ctx, |ui| {
                    ui.set_width(460.0);
                    ui.heading(format!("Delete {}?", pending.item_name));
                    ui.add_space(6.0);
                    ui.label(format!(
                        "This empties the {} slot and does not move the item to inventory.",
                        equipment::equipment_slot_label(&pending.slot)
                    ));
                    ui.label("You can Undo this change until the settings are saved.");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete item").clicked() {
                            delete = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
                cancel |= response.should_close();
            } else {
                cancel = true;
            }
            if delete {
                if let Some(pending) = self.pending_equipment_delete.take() {
                    self.empty_weapon(pending.character_index, &pending.slot);
                }
                self.confirmation = None;
            } else if cancel {
                self.pending_equipment_delete = None;
                self.confirmation = None;
            }
        }
    }

    fn draw_reload_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::Reload) {
            let mut discard = false;
            let mut cancel = false;
            let response = egui::Modal::new("reload_confirmation".into()).show(ctx, |ui| {
                ui.heading("Discard unsaved changes?");
                ui.add_space(6.0);
                ui.label("Reloading will discard changes that have not been saved.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Discard and reload").clicked() {
                        discard = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.confirmation = (!discard && !cancel).then_some(ConfirmationDialog::Reload);
            if discard {
                self.reload();
            }
        }
    }

    fn draw_exit_confirmation(&mut self, ctx: &egui::Context) {
        if self.confirmation == Some(ConfirmationDialog::Exit) {
            let mut save_and_exit = false;
            let mut discard_and_exit = false;
            let mut cancel = false;
            let response = egui::Modal::new("exit_confirmation".into()).show(ctx, |ui| {
                ui.heading("Unsaved changes");
                ui.add_space(6.0);
                ui.label("Save your changes before closing Sundial?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save and exit").clicked() {
                        save_and_exit = true;
                    }
                    if ui.button("Discard and exit").clicked() {
                        discard_and_exit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            cancel |= response.should_close();
            self.confirmation = (!save_and_exit && !discard_and_exit && !cancel)
                .then_some(ConfirmationDialog::Exit);
            if save_and_exit {
                self.request_save(ctx, GeneratedFileSaveAction::SaveAndExit);
            } else if discard_and_exit {
                self.exit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl eframe::App for SundialApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let document_before_frame = self.document.clone();
        let available_update = self.prepare_frame(ctx);

        self.draw_app_chrome(ctx, available_update.as_deref());
        self.draw_active_view(ctx);
        self.draw_supporting_windows(ctx);

        self.draw_pending_install_choice(ctx);
        self.draw_pending_generated_file(ctx);
        self.draw_future_schema_confirmation(ctx);

        self.draw_reset_defaults_confirmation(ctx);
        self.draw_sqlite_restore_confirmation(ctx);
        self.draw_unsafe_mode_confirmation(ctx);
        self.draw_save_review_confirmation(ctx);
        self.draw_delete_equipment_confirmation(ctx);
        self.draw_reload_confirmation(ctx);
        self.draw_exit_confirmation(ctx);

        self.record_document_change(document_before_frame);
    }
}

fn encode_settings_for_editor(document: &Value) -> Result<String, String> {
    encode_settings(document).map(|encoded| encoded.replace("\r\n", "\n"))
}

fn orbit_map_generation_enabled(preference_enabled: bool, document: &Value) -> bool {
    preference_enabled && document.pointer("/client/orbit_slice_set").is_some()
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
            return Err("Two Sunrise settings.json files were found; open Sundial and choose which one Project Sunrise uses".into());
        }
    };
    let app = SundialApp::new(settings_path, settings_layout, install_path)?;
    validate_for_check(&app.document)?;
    let prepared = prepare_settings(&app.document)?;
    let size_note = if prepared.exceeds_size_limit {
        format!(
            " (warning: still above {} after compaction)",
            settings_size_label(prepared.size_limit_bytes)
        )
    } else if prepared.compacted {
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

pub(crate) fn run() -> eframe::Result {
    let (install, check_only, preferences) = parse_args();
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
    #[cfg(not(target_os = "linux"))]
    let icon_bytes = include_bytes!("../assets/sundial-alt.png");
    let icon = eframe::icon_data::from_png_bytes(icon_bytes)
        .expect("embedded Sundial icon must be a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sundial")
            .with_app_id("io.github.kylethmpsn.Sundial")
            .with_decorations(!cfg!(target_os = "linux"))
            .with_inner_size([1_240.0, 880.0])
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
            Ok(Box::new(StartupApp::new(install, preferences)))
        }),
    )
}

#[cfg(test)]
mod tests;
