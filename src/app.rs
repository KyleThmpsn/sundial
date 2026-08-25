use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::mpsc::{self, TryRecvError},
    thread,
};

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{Catalog as Manifest, CatalogProgress},
    game_settings, orbit_map, storage, unnamed_plugs,
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
    GeneratedFileDecision, GeneratedFileKind, GeneratedFilePlan, GeneratedFileSaveAction,
    PendingGeneratedFile, generated_file_diff, normalized_generated_document, settings_size_label,
    settings_size_note,
};

mod platform;
#[cfg(target_os = "linux")]
use platform::{draw_linux_title_bar, load_linux_title_bar_texture};
use platform::{load_logo_texture, open_directory};
#[cfg(windows)]
use platform::{set_windows_app_identity, set_windows_taskbar_icon};

mod preferences;
use preferences::{
    CharacterInventoryLayout, ColorTheme, InstallSelection, ItemCardWidth, PlugSelectionMode,
    Preferences, SettingsLayout, SettingsPathResolution, configure_destiny_symbol_fonts,
    draw_plug_selection_warning,
};

mod settings;
use settings::{
    backups_path, catalog_path, create_adjacent_backup, detect_sunrise_version, encode_settings,
    load_installed_sunrise_defaults, load_json, missing_settings_message, preferences_path,
    prepare_settings, repair_known_ability_pairs, resolve_settings_path, save_json,
    settings_path_for_install, validate_document, verify_source_unchanged,
};

mod json_editor;
use json_editor::JsonEditorState;

mod equipment;
use equipment::{class_name, collect_class_armor_defaults};

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

fn update_detached_window_state(open: &mut bool, generation: &mut u64, requested_open: bool) {
    if *open && !requested_open {
        *generation = generation.wrapping_add(1);
    }
    *open = requested_open;
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
    Reload,
    ResetDefaults,
    Exit,
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
    document: Value,
    persisted_document: Value,
    source_warning: Option<String>,
    class_armor_defaults: HashMap<u64, HashMap<String, Value>>,
    selected_character: usize,
    searches: HashMap<String, String>,
    plug_searches: HashMap<String, String>,
    armor_stats_adjuster: equipment::ArmorStatsAdjusterState,
    plug_selection_mode: PlugSelectionMode,
    default_plug_selection_mode: PlugSelectionMode,
    show_safety_warnings: bool,
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
    pending_generated_file: Option<PendingGeneratedFile>,
    generated_file_decisions: Vec<(GeneratedFileKind, GeneratedFileDecision)>,
    exit_confirmed: bool,
    dirty: bool,
    status: String,
    status_is_error: bool,
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
        let document = load_json(&settings_path)?;
        let cache = catalog_path().ok_or("Could not locate Sundial's local catalog folder")?;
        let manifest = Manifest::load_or_scan_with_progress(&install_path, cache, false, report)?;
        let source_warning = validate_document(&document).err();
        let sunrise_version = detect_sunrise_version(&install_path);
        let class_armor_defaults = collect_class_armor_defaults(&document);
        let raw_json = encode_settings_for_editor(&document)?;
        let raw_json_document = document.clone();
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
            document,
            persisted_document,
            source_warning: source_warning.clone(),
            class_armor_defaults,
            selected_character: 0,
            searches: HashMap::new(),
            plug_searches: HashMap::new(),
            armor_stats_adjuster: equipment::ArmorStatsAdjusterState::default(),
            plug_selection_mode: default_plug_selection_mode,
            default_plug_selection_mode,
            show_safety_warnings: preferences.show_safety_warnings,
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
            progression_section: ProgressionSection::default(),
            game_settings_tab: game_settings::Tab::Player,
            key_binding_ui: game_settings::KeyBindingUiState::default(),
            progression_ui: progression::UiState::default(),
            collections_ui: collections_page::UiState::default(),
            hash_inspection: inspector::HashInspectionState::default(),
            raw_json,
            raw_json_document,
            json_editor: JsonEditorState::default(),
            json_editor_window_open: preferences.always_open_json_editor_in_second_window,
            json_editor_window_generation: 0,
            logo: None,
            #[cfg(target_os = "linux")]
            title_bar_icon: None,
            about_open: false,
            update_check: UpdateCheck::default(),
            confirmation: None,
            pending_generated_file: None,
            generated_file_decisions: Vec::new(),
            exit_confirmed: false,
            dirty: false,
            status: source_warning.as_ref().map_or_else(
                || "Ready".to_owned(),
                |warning| {
                    format!(
                        "Loaded with an unexpected setting: {warning}. A safety copy will be created beside settings.json before saving"
                    )
                },
            ),
            status_is_error: source_warning.is_some(),
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

    fn reload(&mut self) {
        match load_json(&self.settings_path) {
            Ok(doc) => {
                let warning = validate_document(&doc).err();
                self.class_armor_defaults = collect_class_armor_defaults(&doc);
                self.persisted_document = doc.clone();
                self.document = doc;
                self.progression_ui.invalidate_document();
                self.refresh_sunrise_version();
                self.source_warning.clone_from(&warning);
                self.selected_character = self
                    .selected_character
                    .min(self.character_count().saturating_sub(1));
                self.clear_picker_state();
                self.sync_raw_json();
                self.dirty = false;
                if let Some(warning) = warning {
                    self.set_status(
                        format!(
                            "Reloaded with an unexpected setting: {warning}. A safety copy will be created beside settings.json before saving"
                        ),
                        true,
                    );
                } else {
                    self.set_status("Reloaded settings.json", false);
                }
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn save_with_generated_files(&mut self, action: GeneratedFileSaveAction) -> bool {
        if game_settings::ensure_schema_v8_preferences(&mut self.document) {
            self.dirty = true;
        }
        if let Err(error) = verify_source_unchanged(&self.settings_path, &self.persisted_document) {
            self.set_status(format!("Not saved: {error}"), true);
            return false;
        }
        let repaired_ability_pairs = repair_known_ability_pairs(&mut self.document);
        if repaired_ability_pairs > 0 {
            self.dirty = true;
        }
        let current_warning = validate_document(&self.document).err();
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
        let safety_backup = if detected_warning.is_some() {
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
        match save_json(&self.settings_path, &self.document) {
            Ok(result) => {
                let safe_to_close = !result.exceeds_size_limit;
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
                let size_note = settings_size_note(&result);
                let generated_file_note = match self
                    .complete_generated_file_plans(generated_file_plans)
                {
                    Ok(note) => note,
                    Err(error) => {
                        self.generated_file_decisions.clear();
                        self.set_status(
                            format!(
                                "Saved settings, but a package-generated Sunrise file could not be written: {error}. Backup: {}",
                                result.backup.display()
                            ),
                            true,
                        );
                        return false;
                    }
                };
                self.generated_file_decisions.clear();
                if let (Some(warning), Some(safety_backup)) = (detected_warning, safety_backup) {
                    self.set_status(
                        format!(
                            "Saved after detecting an unexpected setting ({warning}).{repair_note}{size_note}{generated_file_note} The untouched source is at {}. Backup: {}",
                            safety_backup.display(),
                            result.backup.display()
                        ),
                        true,
                    );
                } else {
                    self.set_status(
                        format!(
                            "Saved.{repair_note}{size_note}{generated_file_note} Backup: {}",
                            result.backup.display()
                        ),
                        result.exceeds_size_limit,
                    );
                }
                safe_to_close
            }
            Err(error) => {
                self.generated_file_decisions.clear();
                let suffix = safety_backup.map_or_else(String::new, |path| {
                    format!(" The untouched source is at {}.", path.display())
                });
                self.set_status(format!("{error}{suffix}"), true);
                false
            }
        }
    }

    fn save_all_edits(&mut self) -> bool {
        self.save_all_edits_with_action(GeneratedFileSaveAction::Save)
    }

    fn save_all_edits_with_action(&mut self, action: GeneratedFileSaveAction) -> bool {
        if self.json_editor.has_unapplied_changes() && !self.apply_raw_json() {
            return false;
        }
        self.generated_file_decisions.clear();
        self.save_with_generated_files(action)
    }

    fn prepare_generated_files(
        &mut self,
        orbit_supported: bool,
        action: GeneratedFileSaveAction,
    ) -> Result<Option<Vec<GeneratedFilePlan>>, String> {
        let mut files = Vec::new();
        if orbit_supported {
            files.push((
                GeneratedFileKind::OrbitMap,
                orbit_map::path(&self.settings_path)?,
                orbit_map::document(self.manifest.orbit_map_entries()),
            ));
        }
        let mut plans = Vec::with_capacity(files.len());
        for (kind, path, generated) in files {
            let decision = self
                .generated_file_decisions
                .iter()
                .rev()
                .find_map(|(saved_kind, decision)| (*saved_kind == kind).then_some(*decision))
                .unwrap_or(GeneratedFileDecision::Ask);
            match decision {
                GeneratedFileDecision::Replace => {
                    plans.push(GeneratedFilePlan::Write(kind, generated));
                }
                GeneratedFileDecision::KeepExisting => {
                    plans.push(GeneratedFilePlan::KeepExisting(kind, path));
                }
                GeneratedFileDecision::Ask => match fs::read(&path) {
                    Ok(raw) => {
                        let existing = String::from_utf8_lossy(&raw).into_owned();
                        if normalized_generated_document(&existing)
                            == normalized_generated_document(&generated)
                        {
                            plans.push(GeneratedFilePlan::Current(kind, path));
                        } else {
                            let diff = generated_file_diff(kind.file_name(), &existing, &generated);
                            self.pending_generated_file = Some(PendingGeneratedFile {
                                kind,
                                path: path.clone(),
                                existing,
                                generated,
                                diff,
                                action,
                            });
                            self.set_status(
                                format!(
                                    "Save paused: {} differs from Sundial's package-generated {}",
                                    path.display(),
                                    kind.label()
                                ),
                                false,
                            );
                            return Ok(None);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        plans.push(GeneratedFilePlan::Write(kind, generated));
                    }
                    Err(error) => {
                        return Err(format!("Could not read {}: {error}", path.display()));
                    }
                },
            }
        }
        Ok(Some(plans))
    }

    fn complete_generated_file_plans(
        &self,
        plans: Vec<GeneratedFilePlan>,
    ) -> Result<String, String> {
        let mut note = String::new();
        for plan in plans {
            match plan {
                GeneratedFilePlan::Current(kind, path) => {
                    note.push_str(&format!(" {} unchanged: {}.", kind.label(), path.display()))
                }
                GeneratedFilePlan::KeepExisting(kind, path) => note.push_str(&format!(
                    " Existing {} kept: {}.",
                    kind.label(),
                    path.display()
                )),
                GeneratedFilePlan::Write(kind, document) => {
                    let path = match kind {
                        GeneratedFileKind::OrbitMap => {
                            orbit_map::save(&self.settings_path, &document)?
                        }
                    };
                    note.push_str(&format!(" {}: {}.", kind.label(), path.display()));
                }
            }
        }
        Ok(note)
    }

    fn resume_generated_file_action(
        &mut self,
        ctx: &egui::Context,
        action: GeneratedFileSaveAction,
        kind: GeneratedFileKind,
        decision: GeneratedFileDecision,
    ) {
        self.generated_file_decisions
            .retain(|(saved_kind, _)| *saved_kind != kind);
        if decision != GeneratedFileDecision::Ask {
            self.generated_file_decisions.push((kind, decision));
        }
        match action {
            GeneratedFileSaveAction::Save => {
                let _ = self.save_with_generated_files(action);
            }
            GeneratedFileSaveAction::SaveAndExit => {
                let safe_to_close = self.save_with_generated_files(action);
                if !self.has_unsaved_changes() && safe_to_close {
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            GeneratedFileSaveAction::ResetDefaults => {
                self.reset_to_sunrise_defaults_with_generated_files();
            }
        }
    }

    fn has_unsaved_changes(&self) -> bool {
        self.dirty || self.json_editor.has_unapplied_changes()
    }

    fn select_view(&mut self, view: ViewMode) {
        if view == ViewMode::Progression && !self.experimental_progression {
            return;
        }
        if self.view_mode == view {
            if view == ViewMode::AdvancedJson
                && self.always_open_json_editor_in_second_window
                && !self.json_editor_window_open
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
            if self.always_open_json_editor_in_second_window {
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
        if let Err(error) = verify_source_unchanged(&self.settings_path, &self.persisted_document) {
            self.set_status(format!("Defaults not restored: {error}"), true);
            return;
        }
        let default_document = match load_installed_sunrise_defaults(&self.install_path) {
            Ok(document) => document,
            Err(error) => {
                self.set_status(error, true);
                return;
            }
        };
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
                let generated_file_result =
                    self.complete_generated_file_plans(generated_file_plans);
                self.document = default_document;
                self.progression_ui.invalidate_document();
                self.persisted_document = self.document.clone();
                self.refresh_sunrise_version();
                self.source_warning = validate_document(&self.document).err();
                self.class_armor_defaults = collect_class_armor_defaults(&self.document);
                self.selected_character = self
                    .selected_character
                    .min(self.character_count().saturating_sub(1));
                self.clear_picker_state();
                self.sync_raw_json();
                self.dirty = false;
                let generated_file_note = match generated_file_result {
                    Ok(note) => note,
                    Err(error) => {
                        self.generated_file_decisions.clear();
                        self.set_status(
                            format!(
                                "Restored the settings defaults, but a package-generated Sunrise file could not be written: {error}. Original: {}. Backup: {}",
                                adjacent_backup.display(),
                                result.backup.display()
                            ),
                            true,
                        );
                        return;
                    }
                };
                self.generated_file_decisions.clear();
                self.set_status(
                    format!(
                        "Restored the defaults bundled with the installed Project Sunrise.{size_note}{generated_file_note} Original: {}. Backup: {}",
                        adjacent_backup.display(),
                        result.backup.display()
                    ),
                    result.exceeds_size_limit,
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
        let document = match load_json(&settings_path) {
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
        match load_json(&pending.settings_path) {
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
        let warning = validate_document(&document).err();
        self.install_path = install_path;
        self.settings_path = settings_path;
        self.settings_layout = settings_layout;
        self.manifest = manifest;
        self.hash_inspection.close();
        self.progression_ui.reset_navigation();
        self.collections_ui.reset_navigation();
        self.class_armor_defaults = collect_class_armor_defaults(&document);
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

    fn characters_mut(&mut self) -> Option<&mut Vec<Value>> {
        self.document
            .pointer_mut("/state/characters")?
            .as_array_mut()
    }

    fn character_count(&self) -> usize {
        self.characters().map_or(0, <[Value]>::len)
    }

    fn draw_character_tabs(&mut self, ui: &mut egui::Ui) {
        let character_tabs = self
            .characters()
            .map(|characters| {
                characters
                    .iter()
                    .enumerate()
                    .map(|(index, character)| {
                        let class_type =
                            character.get("class").and_then(Value::as_u64).unwrap_or(99);
                        (
                            index,
                            format!("Character {} · {}", index + 1, class_name(class_type)),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
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
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(self.has_unsaved_changes(), egui::Button::new("Save"))
                        .clicked()
                    {
                        let _ = self.save_all_edits();
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
            ui.colored_label(color, &self.status);
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
                let definition_manifest_version = unnamed_plugs::manifest_version();
                ui.label(format!(
                    "Thanks to Nox for their research on unnamed plugs. Stat values are verified from manifest {definition_manifest_version}."
                ));
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
        if let Ok(raw_json) = encode_settings_for_editor(&self.document) {
            self.raw_json = raw_json;
            self.raw_json_document = self.document.clone();
            self.json_editor.mark_synced();
            self.json_editor.restore_location_next_draw();
        }
    }

    fn sync_raw_json_if_stale(&mut self) {
        if !self.json_editor.has_unapplied_changes() && self.raw_json_document != self.document {
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
                let warning = validate_document(&document).err();
                let changed = document != self.document;
                self.raw_json_document = document.clone();
                self.document = document;
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

    fn handle_json_editor_response(&mut self, response: json_editor::JsonEditorResponse) {
        if response.save {
            let _ = self.save_all_edits();
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

    fn draw_preferences_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Preferences");
        ui.add_space(8.0);
        let mut preferences_changed = false;
        let mut preferences_reset = false;

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
        let mut requested_inventory_layout = self.character_inventory_layout;
        ui.horizontal_wrapped(|ui| {
            ui.label("Character inventory layout:");
            ui.radio_value(
                &mut requested_inventory_layout,
                CharacterInventoryLayout::Cards,
                "Sundial cards (default)",
            );
            ui.radio_value(
                &mut requested_inventory_layout,
                CharacterInventoryLayout::Panoptes,
                "Panoptes slot grid",
            );
        });
        ui.label(
            egui::RichText::new(
                "Changes how equipped and stored items are presented on Characters & loadouts. The full Character inventory manager and stored data are unchanged.",
            )
            .weak(),
        );
        if requested_inventory_layout != self.character_inventory_layout {
            self.character_inventory_layout = requested_inventory_layout;
            preferences_changed = true;
        }
        if ui
            .checkbox(
                &mut self.always_open_json_editor_in_second_window,
                "Always open the JSON editor in a second window",
            )
            .changed()
        {
            self.set_json_editor_window_open(self.always_open_json_editor_in_second_window);
            self.json_editor.restore_location_next_draw();
            if !self.json_editor_window_open && self.json_editor.has_unapplied_changes() {
                self.view_mode = ViewMode::AdvancedJson;
            }
            preferences_changed = true;
        }

        ui.add_space(12.0);
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
        ui.add_space(8.0);
        if ui
            .button("Reset preferences to defaults")
            .on_hover_text(
                "Reset appearance, item-editing, and experimental preferences. Paths, catalog data, and backups are not changed.",
            )
            .clicked()
        {
            let defaults = Preferences::default();
            self.color_theme = defaults.color_theme;
            ctx.set_theme(defaults.color_theme.egui_theme());
            self.item_card_width = defaults.item_card_width;
            self.character_inventory_layout = defaults.character_inventory_layout;
            self.always_open_json_editor_in_second_window =
                defaults.always_open_json_editor_in_second_window;
            self.set_json_editor_window_open(defaults.always_open_json_editor_in_second_window);
            self.json_editor.restore_location_next_draw();
            if !self.json_editor_window_open && self.json_editor.has_unapplied_changes() {
                self.view_mode = ViewMode::AdvancedJson;
            }
            self.default_plug_selection_mode = defaults.default_plug_selection_mode;
            self.plug_selection_mode = defaults.default_plug_selection_mode;
            self.show_safety_warnings = defaults.show_safety_warnings;
            self.show_plug_hashes = defaults.show_plug_hashes;
            self.experimental_orbit_backdrops = defaults.experimental_orbit_backdrops;
            self.experimental_progression = defaults.experimental_progression;
            self.experimental_power_above_cap = defaults.experimental_power_above_cap;
            self.really_unsafe_warning_acknowledged =
                defaults.really_unsafe_warning_acknowledged;
            self.remember_plug_selection_mode_after_confirmation = false;
            preferences_changed = true;
            preferences_reset = true;
        }

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

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(10.0);
        ui.strong("Paths and catalog");
        ui.label("Select the Destiny 2 Shadowkeep installation. Sundial finds Project Sunrise's settings.json inside it automatically.");
        ui.add_space(10.0);
        egui::Grid::new("preferences_paths_grid")
            .num_columns(3)
            .spacing([12.0, 10.0])
            .show(ui, |ui| {
                ui.label("Shadowkeep install");
                ui.monospace(self.install_path.display().to_string());
                if ui.button("Choose…").clicked() {
                    self.choose_install(ctx);
                }
                ui.end_row();
                ui.label("Sunrise settings");
                ui.monospace(self.settings_path.display().to_string());
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
            });
        ui.add_space(12.0);
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

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(10.0);
        ui.strong("Recovery");
        ui.label("Restore the exact default settings bundled with this installed Project Sunrise version. Your current settings are backed up first.");
        ui.horizontal(|ui| {
            if ui.button("Restore Sunrise defaults…").clicked() {
                self.confirmation = Some(ConfirmationDialog::ResetDefaults);
            }
            if ui.button("Open backups folder…").clicked() {
                match backups_path()
                    .ok_or("Could not locate Sundial's backups folder".to_owned())
                    .and_then(|path| open_directory(&path))
                {
                    Ok(()) => self.set_status("Opened the backups folder", false),
                    Err(error) => self.set_status(error, true),
                }
            }
        });
    }

    fn draw_json_editor_window(&mut self, ctx: &egui::Context) {
        if !self.json_editor_window_open {
            return;
        }

        self.sync_raw_json_if_stale();
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
                            response = json_editor::draw(
                                ui,
                                &mut self.raw_json,
                                &mut self.json_editor,
                                true,
                            );
                        });
                } else {
                    egui::CentralPanel::default().show(child_ctx, |ui| {
                        response =
                            json_editor::draw(ui, &mut self.raw_json, &mut self.json_editor, true);
                    });
                }
                (response, close_requested)
            },
        );

        self.handle_json_editor_response(response);
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

impl eframe::App for SundialApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        self.draw_app_chrome(ctx, available_update.as_deref());

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
                            let character_editable =
                                inventory::schema_mode(&self.document).can_mutate_equipment();
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
                    if game_settings::draw_page(
                        ui,
                        &mut self.document,
                        self.manifest.orbit_backdrops(),
                        game_settings::PlayerTools {
                            orbit_backdrops_enabled: self.experimental_orbit_backdrops,
                        },
                        &mut self.game_settings_tab,
                        &mut self.key_binding_ui,
                    ) {
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
                                &mut self.document,
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
                                &mut self.document,
                                &self.manifest,
                                &mut self.collections_ui,
                            ) {
                                self.dirty = true;
                                self.set_status(
                                    "Collection acquisition state updated; click Save to write it",
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
                        let response = json_editor::draw(
                            ui,
                            &mut self.raw_json,
                            &mut self.json_editor,
                            false,
                        );
                        self.handle_json_editor_response(response);
                    }
                }
                ViewMode::Preferences => self.draw_preferences_page(ui, ctx),
            }
        });

        if let Some(hash) = inspector::take_definition_request(ctx) {
            self.hash_inspection.open(hash);
        }
        inspector::draw_catalog_hash_window(
            ctx,
            &self.manifest,
            Some(&self.document),
            &mut self.hash_inspection,
            "global",
        );

        self.draw_json_editor_window(ctx);

        self.draw_about_window(ctx);

        self.draw_catalog_progress(ctx);

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

        if self.confirmation == Some(ConfirmationDialog::ResetDefaults) {
            let mut reset = false;
            let mut cancel = false;
            let response = egui::Modal::new("restore_sunrise_defaults".into()).show(ctx, |ui| {
                ui.set_width(500.0);
                ui.heading("Restore Sunrise defaults?");
                ui.add_space(6.0);
                ui.label("This replaces the entire settings.json with the default bundled in your installed Project Sunrise version.");
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

        if self.confirmation == Some(ConfirmationDialog::ReallyUnsafe) {
            let mut enable = false;
            let mut cancel = false;
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
                ui.label("Every Sundial save creates a timestamped backup in Sundial's local data folder.");
                ui.label("If the game no longer loads, open Preferences > Recovery. Sundial backs up the current file again before restoring the defaults bundled with Project Sunrise.");
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
                let safe_to_close =
                    self.save_all_edits_with_action(GeneratedFileSaveAction::SaveAndExit);
                if !self.has_unsaved_changes() && safe_to_close {
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            } else if discard_and_exit {
                self.exit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
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
    Ok(format!(
        "Valid: settings schema {}, detected Project Sunrise {}, {} characters, {} compatible local catalog items loaded, save size {} bytes{}",
        schema_version,
        app.sunrise_version,
        app.character_count(),
        app.manifest.items.len(),
        prepared.encoded_bytes,
        size_note
    ))
}

fn validate_for_check(document: &Value) -> Result<(), String> {
    validate_document(document).map_err(|error| format!("Invalid settings: {error}"))
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
