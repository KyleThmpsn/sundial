use std::{
    collections::{HashMap, HashSet},
    env, fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{AbilityChoice, Catalog as Manifest, CatalogProgress, ItemDef},
    game_settings, storage,
};

#[path = "startup.rs"]
mod startup;
use startup::StartupApp;

#[path = "settings.rs"]
mod settings;
use settings::*;

#[path = "equipment.rs"]
mod equipment;
use equipment::*;

const ROOT_SETTINGS_RELATIVE_PATH: &str = r"Sunrise\settings.json";
const BIN_X64_SETTINGS_RELATIVE_PATH: &str = r"bin\x64\Sunrise\settings.json";
const MAX_SETTINGS_BYTES: usize = 64 * 1024 - 1;
const PROJECT_URL: &str = "https://github.com/kylethmpsn/sundial";
const SUNRISE_URL: &str = "https://github.com/stanuwu/Sunrise";
const TIGER_PKG_URL: &str = "https://github.com/v4nguard/tiger-pkg";
const DISPLAY_VERSION: &str = concat!(
    "v",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);
const ARMOR_SLOTS: &[&str] = &["helmet", "gauntlets", "chest", "legs", "class_item"];
const WEAPON_SLOTS: &[&str] = &["kinetic", "energy", "heavy"];
const GENERATED_INSTANCE_SOID_START: u64 = 0x4000_0000_0000_0001;
const PLUG_PICKER_MIN_HEIGHT: f32 = 320.0;
const PLUG_PICKER_MAX_HEIGHT: f32 = 420.0;

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
    GameSettings,
    AdvancedJson,
    Paths,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsLayout {
    Root,
    BinX64,
}

impl SettingsLayout {
    const ALL: [Self; 2] = [Self::Root, Self::BinX64];

    const fn relative_path(self) -> &'static str {
        match self {
            Self::Root => ROOT_SETTINGS_RELATIVE_PATH,
            Self::BinX64 => BIN_X64_SETTINGS_RELATIVE_PATH,
        }
    }

    const fn preference_value(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::BinX64 => "bin_x64",
        }
    }

    fn from_preference(value: &str) -> Option<Self> {
        match value {
            "root" => Some(Self::Root),
            "bin_x64" => Some(Self::BinX64),
            _ => None,
        }
    }
}

enum SettingsPathResolution {
    Found(SettingsLayout, PathBuf),
    Missing,
    Ambiguous,
}

#[derive(Clone)]
struct InstallSelection {
    install_path: PathBuf,
    preferred_layout: Option<SettingsLayout>,
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
    manifest: Manifest,
    document: Value,
    persisted_document: Value,
    source_warning: Option<String>,
    class_armor_defaults: HashMap<u64, HashMap<String, Value>>,
    selected_character: usize,
    searches: HashMap<String, String>,
    plug_searches: HashMap<String, String>,
    browsing: HashSet<String>,
    allow_unsafe_plugs: bool,
    show_dummy_items: bool,
    view_mode: ViewMode,
    game_settings_tab: game_settings::Tab,
    key_binding_ui: game_settings::KeyBindingUiState,
    raw_json: String,
    logo: Option<egui::TextureHandle>,
    about_open: bool,
    reload_confirmation_open: bool,
    reset_defaults_confirmation_open: bool,
    exit_confirmation_open: bool,
    exit_confirmed: bool,
    dirty: bool,
    status: String,
    status_is_error: bool,
    pending_install_choice: Option<PathBuf>,
    pending_future_schema: Option<PendingFutureSchemaLoad>,
}

impl SundialApp {
    fn new(
        settings_path: PathBuf,
        settings_layout: SettingsLayout,
        install_path: PathBuf,
    ) -> Result<Self, String> {
        Self::new_with_progress(settings_path, settings_layout, install_path, |_| {})
    }

    fn new_with_progress(
        settings_path: PathBuf,
        settings_layout: SettingsLayout,
        install_path: PathBuf,
        report: impl FnMut(CatalogProgress),
    ) -> Result<Self, String> {
        let document = load_json(&settings_path)?;
        let cache = catalog_path().ok_or("Could not locate Sundial's local catalog folder")?;
        let manifest = Manifest::load_or_scan_with_progress(&install_path, cache, false, report)?;
        let source_warning = validate_document(&document).err();
        let class_armor_defaults = collect_class_armor_defaults(&document);
        let raw_json = serde_json::to_string_pretty(&document)
            .map_err(|e| format!("Could not display settings JSON: {e}"))?;
        let persisted_document = document.clone();
        Ok(Self {
            settings_path,
            settings_layout,
            install_path,
            manifest,
            document,
            persisted_document,
            source_warning: source_warning.clone(),
            class_armor_defaults,
            selected_character: 0,
            searches: HashMap::new(),
            plug_searches: HashMap::new(),
            browsing: HashSet::new(),
            allow_unsafe_plugs: false,
            show_dummy_items: false,
            view_mode: ViewMode::Characters,
            game_settings_tab: game_settings::Tab::Player,
            key_binding_ui: game_settings::KeyBindingUiState::default(),
            raw_json,
            logo: None,
            about_open: false,
            reload_confirmation_open: false,
            reset_defaults_confirmation_open: false,
            exit_confirmation_open: false,
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
        })
    }

    fn reload(&mut self) {
        match load_json(&self.settings_path) {
            Ok(doc) => {
                let warning = validate_document(&doc).err();
                self.class_armor_defaults = collect_class_armor_defaults(&doc);
                self.persisted_document = doc.clone();
                self.document = doc;
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

    fn save(&mut self) {
        if let Err(error) = verify_source_unchanged(&self.settings_path, &self.persisted_document) {
            self.set_status(format!("Not saved: {error}"), true);
            return;
        }
        let current_warning = validate_document(&self.document).err();
        let detected_warning = self
            .source_warning
            .clone()
            .or_else(|| current_warning.clone());
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
                    return;
                }
            }
        } else {
            None
        };
        match save_json(&self.settings_path, &self.document) {
            Ok(backup) => {
                self.persisted_document = self.document.clone();
                self.source_warning = current_warning;
                self.dirty = false;
                if let (Some(warning), Some(safety_backup)) = (detected_warning, safety_backup) {
                    self.set_status(
                        format!(
                            "Saved after detecting an unexpected setting ({warning}). The untouched source is at {}. Backup: {}",
                            safety_backup.display(),
                            backup.display()
                        ),
                        true,
                    );
                } else {
                    self.set_status(format!("Saved. Backup: {}", backup.display()), false);
                }
            }
            Err(error) => {
                let suffix = safety_backup.map_or_else(String::new, |path| {
                    format!(" The untouched source is at {}.", path.display())
                });
                self.set_status(format!("{error}{suffix}"), true);
            }
        }
    }

    fn reset_to_sunrise_defaults(&mut self) {
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
            Ok(backup) => {
                self.document = default_document;
                self.persisted_document = self.document.clone();
                self.source_warning = validate_document(&self.document).err();
                self.class_armor_defaults = collect_class_armor_defaults(&self.document);
                self.selected_character = self
                    .selected_character
                    .min(self.character_count().saturating_sub(1));
                self.clear_picker_state();
                self.sync_raw_json();
                self.dirty = false;
                self.set_status(
                    format!(
                        "Restored the defaults bundled with the installed Project Sunrise. Original: {}. Backup: {}",
                        adjacent_backup.display(),
                        backup.display()
                    ),
                    false,
                );
            }
            Err(error) => self.set_status(
                format!(
                    "Defaults not restored: {error}. The untouched source is at {}",
                    adjacent_backup.display()
                ),
                true,
            ),
        }
    }

    fn set_status(&mut self, message: impl Into<String>, is_error: bool) {
        self.status = message.into();
        self.status_is_error = is_error;
    }

    fn save_paths(&self) -> Result<(), String> {
        let path = preferences_path().ok_or("Could not locate Sundial's preferences folder")?;
        let parent = path
            .parent()
            .ok_or("Sundial's preferences path has no parent folder")?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create Sundial's preferences folder: {e}"))?;
        let preferences = serde_json::json!({
            "install": self.install_path,
            "settings_layout": self.settings_layout.preference_value(),
        });
        let encoded = serde_json::to_vec_pretty(&preferences)
            .map_err(|e| format!("Could not encode Sundial's preferences: {e}"))?;
        storage::replace_file(&path, &encoded)
            .map_err(|e| format!("Could not save Sundial's preferences: {e}"))
    }

    fn clear_picker_state(&mut self) {
        self.searches.clear();
        self.plug_searches.clear();
        self.browsing.clear();
        self.key_binding_ui.clear_pickers();
    }

    fn choose_install(&mut self) {
        if self.dirty {
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
                self.load_install(path, settings_path, layout);
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
        self.finish_install_load(path, settings_path, settings_layout, document);
    }

    fn load_future_schema_install(&mut self, pending: PendingFutureSchemaLoad) {
        match load_json(&pending.settings_path) {
            Ok(document) => self.finish_install_load(
                pending.install_path,
                pending.settings_path,
                pending.settings_layout,
                document,
            ),
            Err(error) => self.set_status(error, true),
        }
    }

    fn finish_install_load(
        &mut self,
        path: PathBuf,
        settings_path: PathBuf,
        settings_layout: SettingsLayout,
        document: Value,
    ) {
        let Some(cache) = catalog_path() else {
            self.set_status("Could not locate Sundial's local catalog folder", true);
            return;
        };
        match Manifest::load_or_scan(&path, cache, false) {
            Ok(manifest) => {
                let warning = validate_document(&document).err();
                self.install_path = path;
                self.settings_path = settings_path;
                self.settings_layout = settings_layout;
                self.manifest = manifest;
                self.class_armor_defaults = collect_class_armor_defaults(&document);
                self.persisted_document = document.clone();
                self.document = document;
                self.source_warning.clone_from(&warning);
                self.selected_character = 0;
                self.clear_picker_state();
                self.sync_raw_json();
                self.dirty = false;
                match self.save_paths() {
                    Ok(()) => match warning {
                        Some(warning) => self.set_status(
                            format!(
                                "Install loaded with an unexpected setting: {warning}. A safety copy will be created beside settings.json before saving"
                            ),
                            true,
                        ),
                        None => self.set_status(
                            "Shadowkeep install and Sunrise settings loaded",
                            false,
                        ),
                    },
                    Err(error) => self.set_status(
                        format!(
                            "Install loaded, but its location could not be remembered: {error}"
                        ),
                        true,
                    ),
                }
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn rebuild_catalog(&mut self) {
        let Some(cache) = catalog_path() else {
            self.set_status("Could not locate Sundial's local catalog folder", true);
            return;
        };
        self.set_status("Scanning installed Shadowkeep packages…", false);
        match Manifest::load_or_scan(&self.install_path, cache, true) {
            Ok(manifest) => {
                self.manifest = manifest;
                self.clear_picker_state();
                self.set_status("Catalog rebuilt from the installed game packages", false);
            }
            Err(error) => self.set_status(error, true),
        }
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

    fn sync_raw_json(&mut self) {
        if let Ok(raw_json) = serde_json::to_string_pretty(&self.document) {
            self.raw_json = raw_json;
        }
    }

    fn apply_raw_json(&mut self) {
        match serde_json::from_str::<Value>(&self.raw_json) {
            Ok(document) => {
                let warning = validate_document(&document).err();
                self.document = document;
                self.selected_character = self
                    .selected_character
                    .min(self.character_count().saturating_sub(1));
                self.clear_picker_state();
                self.dirty = true;
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
            Err(error) => self.set_status(
                format!(
                    "JSON syntax error at line {}, column {}: {error}",
                    error.line(),
                    error.column()
                ),
                true,
            ),
        }
    }
}

impl eframe::App for SundialApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|input| input.viewport().close_requested())
            && self.dirty
            && !self.exit_confirmed
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.exit_confirmation_open = true;
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.dirty {
                    ui.label(egui::RichText::new("Unsaved changes").color(egui::Color32::YELLOW));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(self.dirty, egui::Button::new("Save"))
                        .clicked()
                    {
                        self.save();
                    }
                    if ui.button("Reload").clicked() {
                        if self.dirty {
                            self.reload_confirmation_open = true;
                        } else {
                            self.reload();
                        }
                    }
                });
            });
        });

        egui::SidePanel::left("characters")
            .resizable(false)
            .default_width(175.0)
            .show(ctx, |ui| {
                ui.heading("Editor");
                ui.add_space(6.0);
                if ui
                    .selectable_label(
                        self.view_mode == ViewMode::Characters,
                        "Characters & loadouts",
                    )
                    .clicked()
                {
                    self.view_mode = ViewMode::Characters;
                }
                if ui
                    .selectable_label(self.view_mode == ViewMode::GameSettings, "Game settings")
                    .clicked()
                {
                    self.view_mode = ViewMode::GameSettings;
                }
                if ui
                    .selectable_label(
                        self.view_mode == ViewMode::AdvancedJson,
                        "All settings (JSON)",
                    )
                    .clicked()
                {
                    if self.view_mode != ViewMode::AdvancedJson {
                        self.sync_raw_json();
                    }
                    self.view_mode = ViewMode::AdvancedJson;
                }
                if ui
                    .selectable_label(self.view_mode == ViewMode::Paths, "Paths")
                    .clicked()
                {
                    self.view_mode = ViewMode::Paths;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if ui.button("About").clicked() {
                        self.about_open = true;
                    }
                });
            });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let color = if self.status_is_error {
                egui::Color32::LIGHT_RED
            } else {
                ui.visuals().text_color()
            };
            ui.colored_label(color, &self.status);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.view_mode {
                ViewMode::Characters => {
                    let character_tabs = self
                        .characters()
                        .map(|characters| {
                            characters
                                .iter()
                                .enumerate()
                                .map(|(index, character)| {
                                    let class_type = character
                                        .get("class")
                                        .and_then(Value::as_u64)
                                        .unwrap_or(99);
                                    (index, format!("Character {} · {}", index + 1, class_name(class_type)))
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
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let index = self.selected_character;
                        self.draw_character_fields(ui, index);
                        self.draw_equipment(ui, index);
                    });
                }
                ViewMode::GameSettings => {
                    if game_settings::draw_page(
                        ui,
                        &mut self.document,
                        &mut self.game_settings_tab,
                        &mut self.key_binding_ui,
                    ) {
                        self.dirty = true;
                        self.set_status("Game setting updated; click Save to write it", false);
                    }
                }
                ViewMode::AdvancedJson => {
                    ui.horizontal(|ui| {
                        ui.heading("All settings");
                        if ui.button("Apply JSON").clicked() {
                            self.apply_raw_json();
                        }
                        if ui.button("Reset editor").clicked() {
                            self.sync_raw_json();
                            self.set_status("JSON editor reset to current settings", false);
                        }
                    });
                    ui.label("Edit any setting below, then Apply JSON. Save writes applied changes to disk.");
                    ui.add_space(6.0);
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.raw_json)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(40),
                        );
                    });
                }
                ViewMode::Paths => {
                    ui.heading("Paths");
                    ui.label("Select the Destiny 2 Shadowkeep installation. Sundial finds Project Sunrise's settings.json inside it automatically.");
                    ui.add_space(12.0);
                    egui::Grid::new("paths_grid")
                        .num_columns(3)
                        .spacing([12.0, 10.0])
                        .show(ui, |ui| {
                            ui.label("Shadowkeep install");
                            ui.monospace(self.install_path.display().to_string());
                            if ui.button("Choose…").clicked() {
                                self.choose_install();
                            }
                            ui.end_row();
                            ui.label("Sunrise settings");
                            ui.monospace(self.settings_path.display().to_string());
                            ui.label("Detected automatically");
                            ui.end_row();
                            ui.label("Settings schema");
                            ui.monospace(
                                game_settings::schema_version(&self.document)
                                    .map_or_else(|| "Missing or invalid".to_owned(), |version| version.to_string()),
                            );
                            ui.end_row();
                        });
                    ui.add_space(14.0);
                    ui.label(format!("Local catalog: {}", self.manifest.cache_path.display()));
                    ui.label(if self.manifest.loaded_from_cache {
                        "Loaded from local cache"
                    } else {
                        "Scanned from game packages"
                    });
                    ui.label(format!(
                        "{} local catalog items",
                        self.manifest.items.len()
                    ));
                    if ui.button("Rebuild catalog from game files").clicked() {
                        self.rebuild_catalog();
                    }
                    ui.add_space(8.0);
                    ui.label("The first scan reads the installed packages. Later starts use the local cache unless the package files change.");
                    ui.add_space(18.0);
                    ui.separator();
                    ui.add_space(10.0);
                    ui.heading("Restore defaults");
                    ui.label("Restore the exact default settings bundled with this installed Project Sunrise version. Your current settings are backed up first.");
                    if ui.button("Restore Sunrise defaults…").clicked() {
                        self.reset_defaults_confirmation_open = true;
                    }
                }
            }
        });

        if self.about_open {
            let logo = self
                .logo
                .get_or_insert_with(|| load_logo_texture(ctx))
                .clone();
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
                    });
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label("Built for Project Sunrise 0.1, 0.2, and 0.2.1.");
                    ui.hyperlink_to("Project Sunrise on GitHub", SUNRISE_URL);
                    ui.add_space(6.0);
                    ui.label("Local Destiny package parsing is powered by tiger-pkg.");
                    ui.hyperlink_to("tiger-pkg on GitHub", TIGER_PKG_URL);
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(
                            "This project is not affiliated with or endorsed by Bungie Inc. or Sony Interactive Entertainment. Destiny and related intellectual property are owned by Bungie Inc. and their respective rights holders.",
                        )
                        .small()
                        .weak(),
                    );
                });
        }

        if let Some(install_path) = self.pending_install_choice.clone() {
            let mut open = true;
            let mut selected = None;
            egui::Window::new("Choose Sunrise settings")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.set_width(500.0);
                    ui.label("Two existing settings.json files were found. Choose the one Project Sunrise uses for this installation.");
                    ui.add_space(10.0);
                    for layout in SettingsLayout::ALL {
                        let path = settings_path_for_install(&install_path, layout);
                        if ui
                            .button(format!("Use {}", layout.relative_path()))
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
                });
            if let Some((layout, path)) = selected {
                self.pending_install_choice = None;
                self.load_install(install_path, path, layout);
            } else if !open {
                self.pending_install_choice = None;
            }
        }

        if let Some(pending) = self.pending_future_schema.clone() {
            let mut open = true;
            let mut proceed = false;
            let mut cancel = false;
            egui::Window::new("Newer Sunrise settings detected")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
            self.pending_future_schema = if open && !proceed && !cancel {
                Some(pending.clone())
            } else {
                None
            };
            if proceed {
                self.load_future_schema_install(pending);
            }
        }

        if self.reset_defaults_confirmation_open {
            let mut open = true;
            let mut reset = false;
            let mut cancel = false;
            egui::Window::new("Restore Sunrise defaults?")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.set_width(500.0);
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
            self.reset_defaults_confirmation_open = open && !reset && !cancel;
            if reset {
                self.reset_to_sunrise_defaults();
            }
        }

        if self.reload_confirmation_open {
            let mut open = true;
            let mut discard = false;
            let mut cancel = false;
            egui::Window::new("Discard unsaved changes?")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
            self.reload_confirmation_open = open && !discard && !cancel;
            if discard {
                self.reload();
            }
        }

        if self.exit_confirmation_open {
            let mut open = true;
            let mut save_and_exit = false;
            let mut discard_and_exit = false;
            let mut cancel = false;
            egui::Window::new("Unsaved changes")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
            self.exit_confirmation_open = open && !save_and_exit && !discard_and_exit && !cancel;
            if save_and_exit {
                self.save();
                if !self.dirty {
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

fn load_logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/sundial.png"))
        .expect("embedded Sundial logo must be a valid PNG");
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    ctx.load_texture("sundial-logo", image, egui::TextureOptions::LINEAR)
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
        egui::Color32::from_rgb(255, 190, 80),
        "You can continue, but settings may have changed in this Sunrise version.",
    );
    ui.add_space(6.0);
    ui.label("Sundial will preserve unrecognized JSON and create settings.json.bak beside the original before saving.");
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(pending.settings_path.display().to_string())
            .weak()
            .small(),
    );
}

fn parse_args() -> (Option<InstallSelection>, bool) {
    let mut install = saved_install();
    let mut check_only = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--install" => {
                if let Some(value) = args.next() {
                    install = Some(InstallSelection {
                        install_path: value.into(),
                        preferred_layout: None,
                    });
                }
            }
            "--check" => check_only = true,
            _ => {}
        }
    }
    (install, check_only)
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
    let encoded_size = encode_settings(&app.document)?.len() + 1;
    Ok(format!(
        "Valid: {} characters, {} compatible local catalog items loaded, save size {} bytes",
        app.character_count(),
        app.manifest.items.len(),
        encoded_size
    ))
}

fn validate_for_check(document: &Value) -> Result<(), String> {
    validate_document(document).map_err(|error| format!("Invalid settings: {error}"))
}

pub(crate) fn run() -> eframe::Result {
    let (install, check_only) = parse_args();
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
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/sundial.png"))
        .expect("embedded Sundial icon must be a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([920.0, 760.0])
            .with_min_inner_size([720.0, 520.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "Sundial",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(StartupApp::new(install)))
        }),
    )
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
