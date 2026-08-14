#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::{HashMap, HashSet},
    env, fs, io,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use eframe::egui;
use serde_json::Value;

mod catalog;
mod class_items;
mod dummy_items;
mod game_settings;
mod storage;
use catalog::{AbilityChoice, Catalog as Manifest, CatalogProgress, ItemDef};

const ROOT_SETTINGS_RELATIVE_PATH: &str = r"Sunrise\settings.json";
const BIN_X64_SETTINGS_RELATIVE_PATH: &str = r"bin\x64\Sunrise\settings.json";
const MAX_SETTINGS_BYTES: usize = 64 * 1024 - 1;
const PROJECT_URL: &str = "https://github.com/kylethmpsn/sundial";
const SUNRISE_URL: &str = "https://github.com/stanuwu/Sunrise";
const TIGER_PKG_URL: &str = "https://github.com/v4nguard/tiger-pkg";
const DISPLAY_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const ARMOR_SLOTS: &[&str] = &["helmet", "gauntlets", "chest", "legs", "class_item"];

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
    key_binding_search: String,
    raw_json: String,
    logo: Option<egui::TextureHandle>,
    about_open: bool,
    reload_confirmation_open: bool,
    exit_confirmation_open: bool,
    exit_confirmed: bool,
    dirty: bool,
    status: String,
    status_is_error: bool,
    pending_install_choice: Option<PathBuf>,
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
            key_binding_search: String::new(),
            raw_json,
            logo: None,
            about_open: false,
            reload_confirmation_open: false,
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
        let Some(cache) = catalog_path() else {
            self.set_status("Could not locate Sundial's local catalog folder", true);
            return;
        };
        match load_json(&settings_path).and_then(|document| {
            Manifest::load_or_scan(&path, cache, false).map(|manifest| (manifest, document))
        }) {
            Ok((manifest, document)) => {
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

    fn select_item(&mut self, character: usize, slot: &str, item: &ItemDef) {
        let Some(equipment) = self
            .characters_mut()
            .and_then(|chars| chars.get_mut(character))
            .and_then(|ch| ch.get_mut("equipment"))
            .and_then(Value::as_object_mut)
        else {
            self.set_status("The selected character has no equipment object", true);
            return;
        };
        let Some(equipped) = equipment.get_mut(slot).and_then(Value::as_object_mut) else {
            self.set_status(format!("Missing equipment slot: {slot}"), true);
            return;
        };
        equipped.insert(
            "definition_hash".into(),
            Value::String(format_hash(item.hash)),
        );
        equipped.insert(
            "plugs".into(),
            Value::Array(default_plug_values(&item.default_plugs)),
        );
        self.dirty = true;
        self.set_status(format!("Equipped {}", item.name), false);
    }

    fn select_plug(
        &mut self,
        character: usize,
        slot: &str,
        socket_index: usize,
        default_plugs: &[Option<String>],
        hash: Option<u64>,
    ) {
        let Some(plugs_value) = self
            .characters_mut()
            .and_then(|chars| chars.get_mut(character))
            .and_then(|ch| ch.pointer_mut(&format!("/equipment/{slot}/plugs")))
        else {
            self.set_status(format!("Missing plugs value for {slot}"), true);
            return;
        };
        let Some(plugs) = materialize_authored_plugs(plugs_value, default_plugs) else {
            self.set_status(format!("Invalid plugs value for {slot}"), true);
            return;
        };
        while plugs.len() <= socket_index {
            plugs.push(Value::Null);
        }
        plugs[socket_index] = hash.map(format_hash).map_or(Value::Null, Value::String);
        self.dirty = true;
        self.set_status(format!("Updated {slot} socket {}", socket_index + 1), false);
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

    fn draw_character_fields(&mut self, ui: &mut egui::Ui, index: usize) {
        let Some(character) = self.characters().and_then(|chars| chars.get(index)) else {
            return;
        };
        let soid = character
            .get("soid")
            .and_then(parse_unsigned_value)
            .map_or_else(|| "Unknown".to_owned(), format_hash);
        let mut race = character.get("race").and_then(Value::as_u64).unwrap_or(0);
        let mut gender = character.get("gender").and_then(Value::as_u64).unwrap_or(0);
        let mut class_type = character.get("class").and_then(Value::as_u64).unwrap_or(0);
        let mut movement = character
            .get("movement_ability")
            .and_then(Value::as_u64)
            .unwrap_or(4);
        let mut grenade = character
            .get("grenade_ability")
            .and_then(Value::as_u64)
            .unwrap_or(7);
        let mut super_ability = character
            .get("super_ability")
            .and_then(Value::as_u64)
            .unwrap_or(10);
        let mut melee = character
            .get("melee_ability")
            .and_then(Value::as_u64)
            .unwrap_or(11);
        let mut class_ability = character
            .get("class_ability")
            .and_then(Value::as_u64)
            .unwrap_or(2);
        let original_class_type = class_type;
        let mut current_subclass_hash = character
            .pointer("/equipment/subclass/definition_hash")
            .and_then(parse_unsigned_value);
        let mut abilities = current_subclass_hash
            .and_then(|hash| self.manifest.get_for_bucket(hash, 3_284_755_031))
            .map(|item| item.abilities.clone())
            .unwrap_or_default();
        let mut attunement_index = selected_attunement_index(&abilities, super_ability, melee);
        let all_subclasses: Vec<ItemDef> = self
            .manifest
            .items
            .iter()
            .filter(|item| item.bucket_hash == 3_284_755_031)
            .cloned()
            .collect();
        let mut subclasses: Vec<ItemDef> = all_subclasses
            .iter()
            .filter(|item| item.class_type == class_type)
            .cloned()
            .collect();
        let mut selected_subclass = None::<ItemDef>;

        ui.heading(format!("Character {}", index + 1));
        ui.label(egui::RichText::new(soid).monospace().weak());
        ui.add_space(8.0);
        egui::Grid::new("character_fields")
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                ui.label("Class");
                combo_u64(
                    ui,
                    "class",
                    &mut class_type,
                    &[(0, "Titan"), (1, "Hunter"), (2, "Warlock")],
                );
                if class_type != original_class_type {
                    subclasses = all_subclasses
                        .iter()
                        .filter(|item| item.class_type == class_type)
                        .cloned()
                        .collect();
                    if let Some(subclass) = subclasses
                        .iter()
                        .find(|item| item.name == default_subclass_name(class_type))
                        .cloned()
                        .or_else(|| subclasses.first().cloned())
                    {
                        current_subclass_hash = Some(subclass.hash);
                        abilities = subclass.abilities.clone();
                        (movement, grenade, super_ability, melee, class_ability) =
                            default_ability_values(class_type, &abilities);
                        attunement_index =
                            selected_attunement_index(&abilities, super_ability, melee);
                        selected_subclass = Some(subclass);
                    }
                }
                ui.end_row();
                ui.label("Race");
                combo_u64(
                    ui,
                    "race",
                    &mut race,
                    &[(0, "Human"), (1, "Awoken"), (2, "Exo")],
                );
                ui.end_row();
                ui.label("Gender");
                combo_u64(ui, "gender", &mut gender, &[(0, "Male"), (1, "Female")]);
                ui.end_row();
                ui.label("Subclass");
                let selected_name = current_subclass_hash
                    .and_then(|hash| subclasses.iter().find(|item| item.hash == hash))
                    .map_or("Unknown subclass", |item| item.name.as_str());
                egui::ComboBox::from_id_salt("subclass")
                    .selected_text(selected_name)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for subclass in &subclasses {
                            let selected = current_subclass_hash == Some(subclass.hash);
                            if ui.selectable_label(selected, &subclass.name).clicked() && !selected
                            {
                                current_subclass_hash = Some(subclass.hash);
                                abilities = subclass.abilities.clone();
                                (movement, grenade, super_ability, melee, class_ability) =
                                    default_ability_values(class_type, &abilities);
                                attunement_index =
                                    selected_attunement_index(&abilities, super_ability, melee);
                                selected_subclass = Some(subclass.clone());
                            }
                        }
                    });
                ui.end_row();
                ui.label("Attunement");
                let previous_attunement = attunement_index;
                let selected_attunement = abilities
                    .attunements
                    .get(attunement_index)
                    .map_or("No attunement data", |attunement| attunement.name.as_str());
                egui::ComboBox::from_id_salt("attunement")
                    .selected_text(selected_attunement)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (choice_index, attunement) in abilities.attunements.iter().enumerate() {
                            ui.selectable_value(
                                &mut attunement_index,
                                choice_index,
                                &attunement.name,
                            );
                        }
                    });
                ui.end_row();
                if let Some(attunement) = abilities.attunements.get(attunement_index) {
                    let current_pair_is_valid = attunement.melee.entry == melee
                        && attunement
                            .super_abilities
                            .iter()
                            .any(|choice| choice.entry == super_ability);
                    if attunement_index != previous_attunement || !current_pair_is_valid {
                        melee = attunement.melee.entry;
                        super_ability = attunement
                            .super_abilities
                            .first()
                            .map_or(10, |choice| choice.entry);
                    }
                }
                if let Some(attunement) = abilities.attunements.get(attunement_index) {
                    ui.label("Attunement perks");
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                attunement
                                    .perks
                                    .iter()
                                    .map(|choice| choice.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" • "),
                            )
                            .weak(),
                        )
                        .wrap(),
                    );
                    ui.end_row();
                }
                for (label, id, value, choices) in [
                    (
                        "Movement ability",
                        "movement_ability",
                        &mut movement,
                        &abilities.movement,
                    ),
                    (
                        "Grenade ability",
                        "grenade_ability",
                        &mut grenade,
                        &abilities.grenade,
                    ),
                ] {
                    ui.label(label);
                    ability_combo(ui, id, value, choices);
                    ui.end_row();
                }
                if let Some(attunement) = abilities.attunements.get(attunement_index) {
                    ui.label("Super ability");
                    ui.label(
                        attunement
                            .super_abilities
                            .first()
                            .map_or("Unknown super", |choice| choice.name.as_str()),
                    );
                    ui.end_row();
                    ui.label("Melee ability");
                    ui.label(&attunement.melee.name);
                    ui.end_row();
                } else {
                    for (label, id, value, choices) in [
                        (
                            "Super ability",
                            "super_ability",
                            &mut super_ability,
                            &abilities.super_ability,
                        ),
                        (
                            "Melee ability",
                            "melee_ability",
                            &mut melee,
                            &abilities.melee,
                        ),
                    ] {
                        ui.label(label);
                        ability_combo(ui, id, value, choices);
                        ui.end_row();
                    }
                }
                ui.label("Class ability").on_hover_text(
                    "Dodge, Barricade, and Rift remain independent choices. Attunement perks may modify their behavior.",
                );
                ability_combo(
                    ui,
                    "class_ability",
                    &mut class_ability,
                    &abilities.class_ability,
                );
                ui.end_row();
            });

        let mut changed = false;
        let armor_template = (class_type != original_class_type)
            .then(|| self.class_armor_defaults.get(&class_type).cloned())
            .flatten();
        {
            let Some(character) = self.characters_mut().and_then(|chars| chars.get_mut(index))
            else {
                return;
            };
            let Some(object) = character.as_object_mut() else {
                return;
            };
            for (key, new_value) in [("race", race), ("gender", gender), ("class", class_type)] {
                let old = object.get(key).and_then(Value::as_u64);
                if old != Some(new_value) {
                    object.insert(key.into(), Value::from(new_value));
                    changed = true;
                }
            }
            for (key, new_value) in [
                ("movement_ability", movement),
                ("grenade_ability", grenade),
                ("super_ability", super_ability),
                ("melee_ability", melee),
                ("class_ability", class_ability),
            ] {
                if object.get(key).and_then(Value::as_u64) != Some(new_value) {
                    object.insert(key.into(), Value::from(new_value));
                    changed = true;
                }
            }
            if let Some(template) = armor_template.as_ref() {
                changed |= restore_class_armor(object, template);
            }
        }
        self.dirty |= changed;
        if let Some(subclass) = selected_subclass {
            self.select_item(index, "subclass", &subclass);
        }
    }

    fn draw_equipment(&mut self, ui: &mut egui::Ui, character_index: usize) {
        let class_type = self
            .characters()
            .and_then(|chars| chars.get(character_index))
            .and_then(|ch| ch.get("class"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        ui.add_space(14.0);
        ui.heading("Equipped loadout");
        ui.label("Search by item name or 0x hash. Choosing an item also installs its package-default plugs.");
        ui.checkbox(
            &mut self.allow_unsafe_plugs,
            "Allow any plug matching the socket type (unsafe)",
        );
        ui.checkbox(&mut self.show_dummy_items, "Show dummy items")
            .on_hover_text(
                "Includes display-only definitions that cannot normally be obtained in the game.",
            );
        if self.allow_unsafe_plugs {
            ui.colored_label(
                egui::Color32::from_rgb(255, 190, 80),
                "Warning: unsupported plug combinations may break items, corrupt the loadout, or crash Sunrise/Destiny 2.",
            );
        }
        ui.add_space(6.0);

        for &(slot, label, bucket) in SLOTS {
            if slot == "subclass" {
                continue;
            }
            let current_hash_value = self
                .characters()
                .and_then(|chars| chars.get(character_index))
                .and_then(|ch| ch.pointer(&format!("/equipment/{slot}/definition_hash")))
                .cloned();
            let current_hash = current_hash_value.as_ref().and_then(parse_unsigned_value);
            let current_hash_text = current_hash.map_or_else(
                || {
                    current_hash_value
                        .as_ref()
                        .and_then(Value::as_str)
                        .unwrap_or("<missing>")
                        .to_owned()
                },
                format_hash,
            );
            let current = current_hash
                .and_then(|hash| self.manifest.get_for_bucket(hash, bucket))
                .cloned();
            let valid = current.as_ref().is_some_and(|item| {
                item.bucket_hash == bucket
                    && (item.class_type == 3 || item.class_type == class_type)
            });
            ui.push_id((character_index, slot), |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(label).strong());
                        ui.add_space(6.0);
                        match &current {
                            Some(item) => {
                                ui.label(&item.name);
                                ui.label(
                                    egui::RichText::new(&current_hash_text).monospace().weak(),
                                );
                            }
                            None => {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    format!("Unknown item {current_hash_text}"),
                                );
                            }
                        }
                        if !valid {
                            ui.colored_label(egui::Color32::LIGHT_RED, "invalid for slot/class");
                        }
                    });
                    let key = format!("{character_index}:{slot}");
                    let picker_response = {
                        let query = self.searches.entry(key.clone()).or_default();
                        ui.add(
                            egui::TextEdit::singleline(query)
                                .hint_text("Click to browse, or type an item name or hex hash…")
                                .desired_width(ui.available_width()),
                        )
                    };
                    if picker_response.clicked() || picker_response.changed() {
                        self.browsing.insert(key.clone());
                    }
                    let query_value = self.searches.get(&key).cloned().unwrap_or_default();
                    let is_browsing = self.browsing.contains(&key);
                    if !query_value.trim().is_empty() || is_browsing {
                        let candidates = if is_browsing {
                            self.manifest
                                .browse(bucket, class_type, self.show_dummy_items)
                        } else {
                            self.manifest.search(
                                &query_value,
                                bucket,
                                class_type,
                                self.show_dummy_items,
                            )
                        };
                        let needle = query_value.to_lowercase();
                        let results: Vec<ItemDef> = candidates
                            .into_iter()
                            .filter(|item| {
                                query_value.trim().is_empty()
                                    || item.name.to_lowercase().contains(&needle)
                                    || format_hash(item.hash).to_lowercase().contains(&needle)
                            })
                            .take(500)
                            .cloned()
                            .collect();
                        if results.is_empty() {
                            ui.label(
                                egui::RichText::new("No compatible installed items found").weak(),
                            );
                        } else {
                            egui::Frame::popup(ui.style())
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    egui::ScrollArea::vertical()
                                        .max_height(400.0)
                                        .auto_shrink([false, true])
                                        .show(ui, |ui| {
                                            for item in results {
                                                if ui
                                                    .selectable_label(false, item.label())
                                                    .clicked()
                                                {
                                                    self.select_item(character_index, slot, &item);
                                                    self.searches
                                                        .insert(key.clone(), String::new());
                                                    self.browsing.remove(&key);
                                                }
                                            }
                                        });
                                });
                        }
                    }

                    if let Some(item) = &current {
                        let plugs_value = self
                            .characters()
                            .and_then(|chars| chars.get(character_index))
                            .and_then(|ch| ch.pointer(&format!("/equipment/{slot}/plugs")));
                        let (current_plugs, native_defaults) =
                            displayed_plugs(plugs_value, &item.default_plugs);
                        if !item.sockets.is_empty() || !current_plugs.is_empty() {
                            let title = if native_defaults {
                                format!("Plugs ({}, native defaults)", current_plugs.len())
                            } else {
                                format!("Plugs ({})", current_plugs.len())
                            };
                            ui.collapsing(title, |ui| {
                                let socket_count = item.sockets.len().max(current_plugs.len());
                                for socket_index in 0..socket_count {
                                    let current_hash = current_plugs
                                        .get(socket_index)
                                        .and_then(parse_unsigned_value);
                                    let allowed = item
                                        .sockets
                                        .get(socket_index)
                                        .map(|socket| {
                                            if self.allow_unsafe_plugs {
                                                self.manifest
                                                    .socket_type_options(socket.socket_type)
                                                    .to_vec()
                                            } else {
                                                self.manifest.socket_options(socket).to_vec()
                                            }
                                        })
                                        .unwrap_or_default();
                                    let current_label = current_hash.map_or_else(
                                        || "None".to_owned(),
                                        |hash| self.manifest.plug_label(hash),
                                    );
                                    let plug_search_key = format!(
                                        "plug-search:{character_index}:{slot}:{socket_index}"
                                    );
                                    let mut plug_query = self
                                        .plug_searches
                                        .get(&plug_search_key)
                                        .cloned()
                                        .unwrap_or_default();
                                    let searchable = allowed.len() > 12;
                                    ui.horizontal(|ui| {
                                        ui.label(format!("Socket {}", socket_index + 1));
                                        let popup_id = ui.make_persistent_id(format!(
                                            "plug-browser:{character_index}:{slot}:{socket_index}"
                                        ));
                                        let button = ui.add_sized(
                                            [500.0, ui.spacing().interact_size.y],
                                            egui::Button::new(current_label),
                                        );
                                        if button.clicked() {
                                            ui.memory_mut(|memory| memory.toggle_popup(popup_id));
                                        }
                                        let mut selection = None::<Option<u64>>;
                                        egui::popup::popup_below_widget(
                                            ui,
                                            popup_id,
                                            &button,
                                            egui::PopupCloseBehavior::CloseOnClickOutside,
                                            |ui| {
                                                ui.set_min_width(500.0);
                                                if searchable {
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut plug_query)
                                                            .hint_text(
                                                                "Search plug name or hex hash…",
                                                            )
                                                            .desired_width(480.0),
                                                    );
                                                    ui.separator();
                                                }
                                                egui::ScrollArea::vertical()
                                                    .max_height(if searchable {
                                                        340.0
                                                    } else {
                                                        240.0
                                                    })
                                                    .show(ui, |ui| {
                                                        if ui
                                                            .selectable_label(
                                                                current_hash.is_none(),
                                                                "None",
                                                            )
                                                            .clicked()
                                                        {
                                                            selection = Some(None);
                                                        }
                                                        if let Some(hash) = current_hash {
                                                            if !allowed.contains(&hash)
                                                                && ui
                                                                    .selectable_label(
                                                                        true,
                                                                        format!(
                                                                            "{}  (custom/current)",
                                                                            self.manifest
                                                                                .plug_label(hash)
                                                                        ),
                                                                    )
                                                                    .clicked()
                                                            {
                                                                selection = Some(Some(hash));
                                                            }
                                                        }
                                                        let needle =
                                                            plug_query.trim().to_lowercase();
                                                        let mut visible = 0usize;
                                                        for &hash in &allowed {
                                                            let label =
                                                                self.manifest.plug_label(hash);
                                                            if !needle.is_empty()
                                                                && !label
                                                                    .to_lowercase()
                                                                    .contains(&needle)
                                                                && !format_hash(hash)
                                                                    .to_lowercase()
                                                                    .contains(&needle)
                                                            {
                                                                continue;
                                                            }
                                                            visible += 1;
                                                            if ui
                                                                .selectable_label(
                                                                    current_hash == Some(hash),
                                                                    label,
                                                                )
                                                                .clicked()
                                                            {
                                                                selection = Some(Some(hash));
                                                            }
                                                        }
                                                        if searchable && visible == 0 {
                                                            ui.label(
                                                                egui::RichText::new(
                                                                    "No matching plugs found",
                                                                )
                                                                .weak(),
                                                            );
                                                        }
                                                    });
                                            },
                                        );
                                        if let Some(hash) = selection {
                                            self.select_plug(
                                                character_index,
                                                slot,
                                                socket_index,
                                                &item.default_plugs,
                                                hash,
                                            );
                                            ui.memory_mut(egui::Memory::close_popup);
                                        }
                                    });
                                    if searchable {
                                        self.plug_searches.insert(plug_search_key, plug_query);
                                    }
                                }
                            });
                        }
                    }
                });
            });
            ui.add_space(5.0);
        }
    }
}

enum StartupEvent {
    Progress(CatalogProgress),
    Finished(Box<Result<SundialApp, String>>),
}

struct StartupApp {
    editor: Option<SundialApp>,
    receiver: Option<Receiver<StartupEvent>>,
    install_path: Option<PathBuf>,
    progress: CatalogProgress,
    error: Option<String>,
    logo: Option<egui::TextureHandle>,
    pending_settings_choice: Option<PathBuf>,
}

impl StartupApp {
    fn new(selection: Option<InstallSelection>) -> Self {
        let mut app = Self {
            editor: None,
            receiver: None,
            install_path: selection
                .as_ref()
                .map(|selection| selection.install_path.clone()),
            progress: CatalogProgress {
                message: "Waiting for a Shadowkeep installation…",
                completed: 0,
                total: 0,
            },
            error: None,
            logo: None,
            pending_settings_choice: None,
        };
        if let Some(selection) = selection {
            app.begin_loading(selection.install_path, selection.preferred_layout);
        }
        app
    }

    fn begin_loading(&mut self, install_path: PathBuf, preferred_layout: Option<SettingsLayout>) {
        self.install_path = Some(install_path.clone());
        self.receiver = None;
        self.error = None;
        self.pending_settings_choice = None;
        match resolve_settings_path(&install_path, preferred_layout) {
            SettingsPathResolution::Found(layout, settings_path) => {
                self.begin_loading_at(install_path, settings_path, layout);
            }
            SettingsPathResolution::Missing => {
                self.error = Some(missing_settings_message(&install_path));
            }
            SettingsPathResolution::Ambiguous => {
                self.pending_settings_choice = Some(install_path);
            }
        }
    }

    fn begin_loading_at(
        &mut self,
        install_path: PathBuf,
        settings_path: PathBuf,
        settings_layout: SettingsLayout,
    ) {
        let (sender, receiver) = mpsc::channel();
        self.install_path = Some(install_path.clone());
        self.receiver = Some(receiver);
        self.error = None;
        self.pending_settings_choice = None;
        self.progress = CatalogProgress {
            message: "Starting the local catalog…",
            completed: 0,
            total: 0,
        };
        thread::spawn(move || {
            let progress_sender = sender.clone();
            let result = SundialApp::new_with_progress(
                settings_path,
                settings_layout,
                install_path,
                move |progress| {
                    let _ = progress_sender.send(StartupEvent::Progress(progress));
                },
            );
            let _ = sender.send(StartupEvent::Finished(Box::new(result)));
        });
    }

    fn choose_install(&mut self) {
        let mut dialog =
            rfd::FileDialog::new().set_title("Select the Destiny 2 Shadowkeep installation");
        if let Some(path) = self.install_path.as_ref().filter(|path| path.is_dir()) {
            dialog = dialog.set_directory(path);
        }
        if let Some(path) = dialog.pick_folder() {
            self.begin_loading(path, None);
        }
    }

    fn receive_events(&mut self) {
        let mut events = Vec::new();
        if let Some(receiver) = &self.receiver {
            events.extend(receiver.try_iter());
        }
        for event in events {
            match event {
                StartupEvent::Progress(progress) => self.progress = progress,
                StartupEvent::Finished(result) => match *result {
                    Ok(mut editor) => {
                        editor.logo.clone_from(&self.logo);
                        if let Err(error) = editor.save_paths() {
                            editor.set_status(
                                format!(
                                    "Loaded successfully, but the install location could not be remembered: {error}"
                                ),
                                true,
                            );
                        }
                        self.editor = Some(editor);
                        self.receiver = None;
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.receiver = None;
                    }
                },
            }
        }
    }

    fn draw_startup(&mut self, ctx: &egui::Context) {
        let logo = self
            .logo
            .get_or_insert_with(|| load_logo_texture(ctx))
            .clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            let top_space = ((ui.available_height() - 440.0) / 2.0).max(16.0);
            ui.add_space(top_space);
            ui.vertical_centered(|ui| {
                egui::Frame::group(ui.style())
                    .inner_margin(28.0)
                    .show(ui, |ui| {
                        ui.set_width(500.0_f32.min(ui.available_width()));
                        ui.vertical_centered(|ui| {
                            ui.image((logo.id(), egui::vec2(72.0, 72.0)));
                            ui.heading("Sundial");
                            ui.label(egui::RichText::new(DISPLAY_VERSION).weak());
                            ui.add_space(18.0);

                            if let Some(install_path) = self.pending_settings_choice.clone() {
                                ui.heading("Choose Sunrise settings");
                                ui.add_space(6.0);
                                ui.label("Two existing settings.json files were found. Choose the one Project Sunrise uses for this installation.");
                                ui.add_space(14.0);
                                for layout in SettingsLayout::ALL {
                                    let path = settings_path_for_install(&install_path, layout);
                                    if ui
                                        .add_sized(
                                            [400.0, 34.0],
                                            egui::Button::new(format!(
                                                "Use {}",
                                                layout.relative_path()
                                            )),
                                        )
                                        .clicked()
                                    {
                                        self.begin_loading_at(
                                            install_path.clone(),
                                            path.clone(),
                                            layout,
                                        );
                                    }
                                    ui.label(
                                        egui::RichText::new(path.display().to_string())
                                            .weak()
                                            .small(),
                                    );
                                    ui.add_space(8.0);
                                }
                                if ui.button("Choose another folder").clicked() {
                                    self.choose_install();
                                }
                                return;
                            }

                            if let Some(error) = self.error.clone() {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    "Could not load that installation",
                                );
                                ui.add_space(6.0);
                                ui.label(error);
                                ui.add_space(16.0);
                                ui.horizontal(|ui| {
                                    if ui.button("Choose another folder").clicked() {
                                        self.choose_install();
                                    }
                                    if let Some(path) = self.install_path.clone() {
                                        if ui.button("Try again").clicked() {
                                            self.begin_loading(path, None);
                                        }
                                    }
                                });
                                return;
                            }

                            if self.receiver.is_none() {
                                ui.heading("Choose your Shadowkeep installation");
                                ui.add_space(6.0);
                                ui.label("Select the Destiny 2 Shadowkeep installation you use with Project Sunrise to begin.");
                                ui.add_space(10.0);
                                ui.label(
                                    egui::RichText::new(
                                        "Sundial will read the installed packages once to build its local item catalog. Nothing is downloaded.",
                                    )
                                    .weak(),
                                );
                                ui.add_space(18.0);
                                if ui
                                    .add_sized(
                                        [240.0, 36.0],
                                        egui::Button::new("Choose Shadowkeep folder…"),
                                    )
                                    .clicked()
                                {
                                    self.choose_install();
                                }
                                return;
                            }

                            ui.spinner();
                            ui.strong(self.progress.message);
                            ui.add_space(10.0);
                            let progress = if self.progress.total > 0 {
                                self.progress.completed as f32 / self.progress.total as f32
                            } else {
                                0.0
                            };
                            let mut bar = egui::ProgressBar::new(progress)
                                .desired_width(400.0)
                                .corner_radius(egui::CornerRadius::same(3));
                            if self.progress.total > 0 {
                                bar = bar.show_percentage();
                            } else {
                                bar = bar.animate(true);
                            }
                            ui.add(bar);
                            if let Some(path) = &self.install_path {
                                ui.add_space(10.0);
                                ui.label(
                                    egui::RichText::new(path.display().to_string())
                                        .weak()
                                        .small(),
                                );
                            }
                        });
                    });
            });
        });
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
                        &mut self.key_binding_search,
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
                    ui.label("Built for Project Sunrise 0.1.");
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

impl eframe::App for StartupApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.receive_events();
        if let Some(editor) = &mut self.editor {
            editor.update(ctx, frame);
        } else {
            self.draw_startup(ctx);
            ctx.request_repaint_after(Duration::from_millis(50));
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

fn collect_class_armor_defaults(document: &Value) -> HashMap<u64, HashMap<String, Value>> {
    let mut defaults = HashMap::new();
    let Some(characters) = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
    else {
        return defaults;
    };
    for character in characters {
        let Some(class_type) = character.get("class").and_then(Value::as_u64) else {
            continue;
        };
        let Some(equipment) = character.get("equipment").and_then(Value::as_object) else {
            continue;
        };
        let armor = ARMOR_SLOTS
            .iter()
            .filter_map(|slot| {
                equipment
                    .get(*slot)
                    .cloned()
                    .map(|item| ((*slot).into(), item))
            })
            .collect();
        defaults.entry(class_type).or_insert(armor);
    }
    defaults
}

fn restore_class_armor(
    character: &mut serde_json::Map<String, Value>,
    defaults: &HashMap<String, Value>,
) -> bool {
    let Some(equipment) = character
        .get_mut("equipment")
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let mut changed = false;
    for &slot in ARMOR_SLOTS {
        let Some(replacement) = defaults.get(slot) else {
            continue;
        };
        let Some(replacement) = replacement.as_object() else {
            continue;
        };
        let Some(existing) = equipment.get(slot).and_then(Value::as_object) else {
            continue;
        };
        let mut merged = existing.clone();
        for (key, value) in replacement {
            if key != "instance_soid" {
                merged.insert(key.clone(), value.clone());
            }
        }
        let merged = Value::Object(merged);
        if equipment.get(slot) != Some(&merged) {
            equipment.insert(slot.into(), merged);
            changed = true;
        }
    }
    changed
}

fn combo_u64(ui: &mut egui::Ui, id: &str, value: &mut u64, choices: &[(u64, &str)]) {
    let selected = choices
        .iter()
        .find(|(candidate, _)| candidate == value)
        .map_or("Invalid", |(_, name)| *name);
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(160.0)
        .show_ui(ui, |ui| {
            for &(candidate, name) in choices {
                ui.selectable_value(value, candidate, name);
            }
        });
}

fn ability_combo(ui: &mut egui::Ui, id: &str, value: &mut u64, choices: &[AbilityChoice]) {
    let selected = choices
        .iter()
        .find(|choice| choice.entry == *value)
        .map_or_else(
            || format!("Unknown entry {}", *value),
            |choice| choice.name.clone(),
        );
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(260.0)
        .show_ui(ui, |ui| {
            for choice in choices {
                ui.selectable_value(value, choice.entry, &choice.name);
            }
            if choices.is_empty() {
                ui.label("No named choices found for this subclass");
            }
        });
}

const fn default_subclass_name(class_type: u64) -> &'static str {
    match class_type {
        0 => "Sunbreaker",
        1 => "Nightstalker",
        2 => "Dawnblade",
        _ => "",
    }
}

fn selected_attunement_index(
    abilities: &catalog::AbilityOptions,
    super_ability: u64,
    melee: u64,
) -> usize {
    let paths = &abilities.attunements;
    paths
        .iter()
        .position(|path| {
            path.melee.entry == melee
                && path
                    .super_abilities
                    .iter()
                    .any(|choice| choice.entry == super_ability)
        })
        .or_else(|| {
            if super_ability == 10 {
                None
            } else {
                paths.iter().position(|path| {
                    path.super_abilities
                        .iter()
                        .chain(path.perks.iter())
                        .any(|choice| choice.entry == super_ability)
                })
            }
        })
        .or_else(|| paths.iter().position(|path| path.melee.entry == melee))
        .or_else(|| {
            paths.iter().position(|path| {
                path.super_abilities
                    .iter()
                    .any(|choice| choice.entry == super_ability)
            })
        })
        .unwrap_or(0)
}

fn default_ability_values(
    class_type: u64,
    abilities: &catalog::AbilityOptions,
) -> (u64, u64, u64, u64, u64) {
    let pick = |choices: &[AbilityChoice], preferred: u64| {
        choices
            .iter()
            .find(|choice| choice.entry == preferred)
            .or_else(|| choices.first())
            .map_or(preferred, |choice| choice.entry)
    };
    let movement = match class_type {
        1 => 6,     // Hunter: Triple Jump
        0 | 2 => 5, // Titan: Strafe Lift; Warlock: Burst Glide
        _ => 4,
    };
    (
        pick(&abilities.movement, movement),
        pick(&abilities.grenade, 7),
        pick(&abilities.super_ability, 10),
        pick(&abilities.melee, 11),
        pick(&abilities.class_ability, 2),
    )
}

const fn class_name(class_type: u64) -> &'static str {
    match class_type {
        0 => "Titan",
        1 => "Hunter",
        2 => "Warlock",
        _ => "Invalid class",
    }
}

fn format_hash(hash: u64) -> String {
    format!("0x{hash:08X}")
}

fn parse_hash(text: &str) -> Option<u64> {
    let digits = text
        .trim()
        .strip_prefix("0x")
        .or_else(|| text.trim().strip_prefix("0X"))?;
    if digits.is_empty() || digits.len() > 16 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

fn parse_unsigned_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(parse_hash))
}

fn default_plug_values(defaults: &[Option<String>]) -> Vec<Value> {
    defaults
        .iter()
        .map(|plug| plug.clone().map_or(Value::Null, Value::String))
        .collect()
}

fn displayed_plugs(plugs: Option<&Value>, defaults: &[Option<String>]) -> (Vec<Value>, bool) {
    match plugs {
        Some(Value::Array(plugs)) => (plugs.clone(), false),
        Some(Value::Null) => (default_plug_values(defaults), true),
        _ => (Vec::new(), false),
    }
}

fn materialize_authored_plugs<'a>(
    plugs: &'a mut Value,
    defaults: &[Option<String>],
) -> Option<&'a mut Vec<Value>> {
    if plugs.is_null() {
        *plugs = Value::Array(default_plug_values(defaults));
    }
    plugs.as_array_mut()
}

fn load_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!(
                "No Project Sunrise settings.json was found in the selected installation. Expected: {}. Choose the Destiny 2 Shadowkeep folder containing destiny2.exe and the bin folder, and confirm Project Sunrise is installed there",
                path.display()
            )
        } else {
            format!("Could not read {}: {error}", path.display())
        }
    })?;
    serde_json::from_str(&raw).map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

fn verify_source_unchanged(path: &Path, expected: &Value) -> Result<(), String> {
    let current = load_json(path)?;
    if current == *expected {
        Ok(())
    } else {
        Err("settings.json changed outside Sundial after it was loaded. Reload before saving so newer data is not overwritten".into())
    }
}

fn save_json(path: &Path, document: &Value) -> Result<PathBuf, String> {
    let backup_root = preferences_path()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .ok_or("Could not locate the local backup folder")?
        .join("backups");
    save_json_with_backup_root(path, document, &backup_root)
}

fn save_json_with_backup_root(
    path: &Path,
    document: &Value,
    backup_root: &Path,
) -> Result<PathBuf, String> {
    let mut encoded = encode_settings(document)?;
    encoded.push('\n');
    if encoded.len() > MAX_SETTINGS_BYTES {
        return Err(format!(
            "The encoded settings would be {} bytes; Sunrise requires less than {} bytes",
            encoded.len(),
            MAX_SETTINGS_BYTES + 1
        ));
    }

    fs::create_dir_all(backup_root)
        .map_err(|e| format!("Could not create {}: {e}", backup_root.display()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Could not create backup timestamp: {e}"))?
        .as_nanos();
    let backup = backup_root.join(format!("settings-{timestamp}-{}.json", std::process::id()));
    create_backup(path, &backup)?;

    storage::replace_file(path, encoded.as_bytes())
        .map_err(|e| format!("Could not safely replace {}: {e}", path.display()))?;
    let verification = load_json(path).and_then(|saved| {
        if saved == *document {
            Ok(())
        } else {
            Err("the saved document did not match the requested settings".to_owned())
        }
    });
    if let Err(error) = verification {
        let restore = fs::read(&backup)
            .and_then(|contents| storage::replace_file(path, &contents))
            .map_err(|restore_error| restore_error.to_string());
        return match restore {
            Ok(()) => Err(format!(
                "Could not verify the saved settings ({error}); the original file was restored"
            )),
            Err(restore_error) => Err(format!(
                "Could not verify the saved settings ({error}), and restoring the backup failed: {restore_error}. The backup is at {}",
                backup.display()
            )),
        };
    }
    Ok(backup)
}

fn create_backup(source: &Path, destination: &Path) -> Result<(), String> {
    let mut source_file = fs::File::open(source)
        .map_err(|e| format!("Could not open {} for backup: {e}", source.display()))?;
    let mut backup_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| format!("Could not create {}: {e}", destination.display()))?;
    if let Err(error) =
        io::copy(&mut source_file, &mut backup_file).and_then(|_| backup_file.sync_all())
    {
        drop(backup_file);
        let _ = fs::remove_file(destination);
        return Err(format!(
            "Could not create {}: {error}",
            destination.display()
        ));
    }
    Ok(())
}

fn create_adjacent_backup(source: &Path) -> Result<PathBuf, String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("{} has no file name", source.display()))?
        .to_string_lossy();
    let destination = source.with_file_name(format!("{file_name}.bak"));
    let source_contents = fs::read(source)
        .map_err(|e| format!("Could not read {} for backup: {e}", source.display()))?;

    if destination.exists() {
        let existing = fs::read(&destination)
            .map_err(|e| format!("Could not read {}: {e}", destination.display()))?;
        if existing == source_contents {
            return Ok(destination);
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("Could not create backup timestamp: {e}"))?
            .as_nanos();
        let archived = source.with_file_name(format!("{file_name}.bak.previous-{timestamp}"));
        create_backup(&destination, &archived)?;
        storage::replace_file(&destination, &source_contents).map_err(|e| {
            format!(
                "Could not update {} after preserving its previous contents at {}: {e}",
                destination.display(),
                archived.display()
            )
        })?;
    } else {
        create_backup(source, &destination)?;
    }

    let copied = fs::read(&destination)
        .map_err(|e| format!("Could not verify {}: {e}", destination.display()))?;
    if copied != source_contents {
        return Err(format!(
            "The safety copy at {} did not match the source",
            destination.display()
        ));
    }
    Ok(destination)
}

fn encode_settings(document: &Value) -> Result<String, String> {
    fn write_value(value: &Value, indent: usize, output: &mut String) -> Result<(), String> {
        match value {
            Value::Object(object) if !object.is_empty() => {
                output.push_str("{\n");
                for (index, (key, child)) in object.iter().enumerate() {
                    output.push_str(&" ".repeat(indent + 2));
                    output.push_str(
                        &serde_json::to_string(key)
                            .map_err(|e| format!("Could not encode setting name: {e}"))?,
                    );
                    output.push_str(": ");
                    write_value(child, indent + 2, output)?;
                    if index + 1 != object.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&" ".repeat(indent));
                output.push('}');
            }
            Value::Array(_) => output.push_str(
                &serde_json::to_string(value)
                    .map_err(|e| format!("Could not encode settings array: {e}"))?,
            ),
            _ => output.push_str(
                &serde_json::to_string(value)
                    .map_err(|e| format!("Could not encode setting: {e}"))?,
            ),
        }
        Ok(())
    }

    let mut output = String::new();
    write_value(document, 0, &mut output)?;
    Ok(output)
}

fn validate_document(document: &Value) -> Result<(), String> {
    game_settings::validate(document)?;
    validate_characters(document)
}

fn validate_characters(document: &Value) -> Result<(), String> {
    const MAX_CHARACTERS: usize = 3;
    const MAX_PLUGS: usize = 12;
    const NO_DEFINITION_HASH: u64 = 0x811C_9DC5;

    let Some(characters_value) = document.pointer("/state/characters") else {
        return Ok(());
    };
    let characters = characters_value
        .as_array()
        .ok_or("state.characters must be an array")?;
    if characters.len() > MAX_CHARACTERS {
        return Err(format!(
            "state.characters cannot contain more than {MAX_CHARACTERS} characters"
        ));
    }
    for (character_index, character) in characters.iter().enumerate() {
        let number = character_index + 1;
        let character = character
            .as_object()
            .ok_or_else(|| format!("Character {number} must be an object"))?;
        character
            .get("soid")
            .and_then(parse_unsigned_value)
            .filter(|soid| *soid != 0)
            .ok_or_else(|| format!("Character {number} has an invalid SOID"))?;

        let optional_bounded = |key: &str, label: &str, maximum: u64| {
            let Some(value) = character.get(key) else {
                return Ok(());
            };
            value
                .as_u64()
                .filter(|value| *value <= maximum)
                .map(|_| ())
                .ok_or_else(|| format!("Character {number} has an invalid {label}"))
        };
        optional_bounded("class", "class", 2)?;
        optional_bounded("race", "race", 2)?;
        optional_bounded("gender", "gender", 1)?;
        optional_bounded("level", "level (expected 0 to 255)", u8::MAX.into())?;
        for (key, label) in [
            ("movement_ability", "movement ability"),
            ("grenade_ability", "grenade ability"),
            ("super_ability", "super ability"),
            ("melee_ability", "melee ability"),
            ("class_ability", "class ability"),
        ] {
            optional_bounded(key, label, 63)?;
        }

        let Some(equipment_value) = character.get("equipment") else {
            continue;
        };
        let equipment = equipment_value
            .as_object()
            .ok_or_else(|| format!("Character {number} equipment must be an object"))?;
        for slot in equipment.keys() {
            if !SLOTS.iter().any(|(known, _, _)| known == slot) {
                return Err(format!(
                    "Character {number} has an unknown equipment slot: {slot}"
                ));
            }
        }
        for &(slot, label, _) in SLOTS {
            let Some(equipped_value) = equipment.get(slot) else {
                continue;
            };
            if equipped_value.is_null() {
                continue;
            }
            let equipped = equipped_value
                .as_object()
                .ok_or_else(|| format!("Character {number} {label} must be an object or null"))?;
            equipped
                .get("definition_hash")
                .and_then(parse_unsigned_value)
                .filter(|hash| u32::try_from(*hash).is_ok() && *hash != NO_DEFINITION_HASH)
                .ok_or_else(|| {
                    format!("Character {number} {label} has an invalid definition hash")
                })?;
            equipped
                .get("instance_soid")
                .and_then(parse_unsigned_value)
                .filter(|soid| *soid != 0)
                .ok_or_else(|| {
                    format!("Character {number} {label} has an invalid instance SOID")
                })?;
            equipped
                .get("level")
                .and_then(Value::as_i64)
                .filter(|level| (0..=i64::from(i32::MAX)).contains(level))
                .ok_or_else(|| format!("Character {number} {label} has an invalid item level"))?;
            equipped
                .get("quantity")
                .and_then(Value::as_i64)
                .filter(|quantity| (1..=i64::from(i32::MAX)).contains(quantity))
                .ok_or_else(|| format!("Character {number} {label} has an invalid quantity"))?;

            match equipped.get("plugs") {
                Some(Value::Null) => {}
                Some(Value::Array(plugs)) => {
                    if plugs.len() > MAX_PLUGS {
                        return Err(format!(
                            "Character {number} {label} cannot contain more than {MAX_PLUGS} plugs"
                        ));
                    }
                    for plug in plugs {
                        if !plug.is_null()
                            && !parse_unsigned_value(plug).is_some_and(|hash| {
                                u32::try_from(hash).is_ok() && hash != NO_DEFINITION_HASH
                            })
                        {
                            return Err(format!(
                                "Character {number} {label} contains an invalid plug hash"
                            ));
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "Character {number} {label} plugs must be null or an array"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn preferences_path() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Sundial").join("paths.json"))
}

fn catalog_path() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Sundial").join("catalog").join("d2sk-86657.json"))
}

fn settings_path_for_install(install: &Path, layout: SettingsLayout) -> PathBuf {
    install.join(layout.relative_path())
}

fn resolve_settings_path(
    install: &Path,
    preferred_layout: Option<SettingsLayout>,
) -> SettingsPathResolution {
    if let Some(layout) = preferred_layout {
        let path = settings_path_for_install(install, layout);
        if path.is_file() {
            return SettingsPathResolution::Found(layout, path);
        }
    }

    let existing = SettingsLayout::ALL
        .into_iter()
        .filter_map(|layout| {
            let path = settings_path_for_install(install, layout);
            path.is_file().then_some((layout, path))
        })
        .collect::<Vec<_>>();
    match existing.as_slice() {
        [] => SettingsPathResolution::Missing,
        [(layout, path)] => SettingsPathResolution::Found(*layout, path.clone()),
        _ => SettingsPathResolution::Ambiguous,
    }
}

fn missing_settings_message(install: &Path) -> String {
    let root = settings_path_for_install(install, SettingsLayout::Root);
    let bin_x64 = settings_path_for_install(install, SettingsLayout::BinX64);
    format!(
        "No Project Sunrise settings.json was found in the selected installation. Checked {} and {}. Choose the Destiny 2 Shadowkeep folder containing destiny2.exe and confirm Project Sunrise is installed there",
        root.display(),
        bin_x64.display()
    )
}

fn saved_install() -> Option<InstallSelection> {
    let path = preferences_path()?;
    let raw = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    let install_path = value
        .get("install")
        .and_then(Value::as_str)
        .map(PathBuf::from)?;
    let preferred_layout = value
        .get("settings_layout")
        .and_then(Value::as_str)
        .and_then(SettingsLayout::from_preference);
    Some(InstallSelection {
        install_path,
        preferred_layout,
    })
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
    let encoded_size = encode_settings(&app.document)?.len() + 1;
    Ok(format!(
        "Valid: {} characters, {} compatible local catalog items loaded, save size {} bytes",
        app.character_count(),
        app.manifest.items.len(),
        encoded_size
    ))
}

fn main() -> eframe::Result {
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
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "sundial-save-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn hashes_are_strict_hex_and_normalized() {
        assert_eq!(parse_hash("0xE516CF40"), Some(0xE516_CF40));
        assert_eq!(parse_hash("0Xe516cf40"), Some(0xE516_CF40));
        assert_eq!(format_hash(0x123), "0x00000123");
        assert_eq!(parse_hash("E516CF40"), None);
        assert_eq!(parse_hash("0xnope"), None);
        assert_eq!(parse_unsigned_value(&Value::from(42)), Some(42));
        assert_eq!(
            parse_unsigned_value(&Value::String("0x0000002A".into())),
            Some(42)
        );
    }

    #[test]
    fn sunrise_native_plugs_are_displayed_and_materialized_on_edit() {
        let defaults = vec![Some("0x0000002A".into()), None, Some("0x0000002B".into())];
        let mut plugs = Value::Null;

        let (displayed, native_defaults) = displayed_plugs(Some(&plugs), &defaults);
        assert!(native_defaults);
        assert_eq!(
            displayed,
            serde_json::json!(["0x0000002A", null, "0x0000002B"])
                .as_array()
                .unwrap()
                .clone()
        );

        let authored = materialize_authored_plugs(&mut plugs, &defaults).unwrap();
        authored[1] = Value::String("0x0000002C".into());
        assert_eq!(
            plugs,
            serde_json::json!(["0x0000002A", "0x0000002C", "0x0000002B"])
        );
    }

    #[test]
    fn character_validation_accepts_sunrise_native_forms() {
        let document = serde_json::json!({
            "state": {
                "characters": [{
                    "soid": 1,
                    "level": 67,
                    "equipment": {
                        "kinetic": {
                            "instance_soid": "0x0000000000000002",
                            "definition_hash": 42,
                            "level": 106,
                            "quantity": 1,
                            "plugs": null
                        },
                        "energy": {
                            "instance_soid": 3,
                            "definition_hash": "0x0000002B",
                            "level": 106,
                            "quantity": 1,
                            "plugs": [null, 44, "0x0000002D"]
                        },
                        "heavy": null
                    }
                }]
            }
        });

        assert_eq!(validate_characters(&document), Ok(()));
    }

    #[test]
    fn character_validation_keeps_sunrise_limits() {
        let mut document = serde_json::json!({
            "state": {
                "characters": [{
                    "soid": "0x1",
                    "level": 256,
                    "equipment": {}
                }]
            }
        });
        assert!(validate_characters(&document).is_err());

        *document.pointer_mut("/state/characters/0/level").unwrap() = Value::from(255);
        document
            .pointer_mut("/state/characters/0/equipment")
            .unwrap()["kinetic"] = serde_json::json!({
            "instance_soid": "0x2",
            "definition_hash": "0x2A",
            "level": 106,
            "quantity": 1,
            "plugs": [null, null, null, null, null, null, null, null, null, null, null, null, null]
        });
        assert!(validate_characters(&document).is_err());
    }

    #[test]
    fn settings_paths_are_derived_from_install() {
        assert_eq!(
            settings_path_for_install(Path::new("game"), SettingsLayout::Root),
            PathBuf::from("game").join(ROOT_SETTINGS_RELATIVE_PATH)
        );
        assert_eq!(
            settings_path_for_install(Path::new("game"), SettingsLayout::BinX64),
            PathBuf::from("game").join(BIN_X64_SETTINGS_RELATIVE_PATH)
        );
    }

    #[test]
    fn settings_resolution_uses_the_only_existing_file_and_never_creates_one() {
        let directory = TestDirectory::new();
        assert!(matches!(
            resolve_settings_path(&directory.0, None),
            SettingsPathResolution::Missing
        ));
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);

        let root = settings_path_for_install(&directory.0, SettingsLayout::Root);
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::write(&root, b"{}\n").unwrap();

        assert!(matches!(
            resolve_settings_path(&directory.0, None),
            SettingsPathResolution::Found(SettingsLayout::Root, path) if path == root
        ));
    }

    #[test]
    fn settings_resolution_requires_a_choice_when_both_files_exist() {
        let directory = TestDirectory::new();
        let root = settings_path_for_install(&directory.0, SettingsLayout::Root);
        let bin_x64 = settings_path_for_install(&directory.0, SettingsLayout::BinX64);
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::create_dir_all(bin_x64.parent().unwrap()).unwrap();
        fs::write(&root, b"{\"layout\":\"root\"}\n").unwrap();
        fs::write(&bin_x64, b"{\"layout\":\"bin\"}\n").unwrap();

        assert!(matches!(
            resolve_settings_path(&directory.0, None),
            SettingsPathResolution::Ambiguous
        ));
        assert!(matches!(
            resolve_settings_path(&directory.0, Some(SettingsLayout::BinX64)),
            SettingsPathResolution::Found(SettingsLayout::BinX64, path) if path == bin_x64
        ));
        assert_eq!(fs::read_to_string(root).unwrap(), "{\"layout\":\"root\"}\n");
        assert_eq!(
            fs::read_to_string(bin_x64).unwrap(),
            "{\"layout\":\"bin\"}\n"
        );
    }

    #[test]
    fn loading_a_missing_selected_settings_file_never_creates_it() {
        let directory = TestDirectory::new();
        let settings = settings_path_for_install(&directory.0, SettingsLayout::BinX64);

        let error = load_json(&settings).unwrap_err();

        assert!(error.contains("No Project Sunrise settings.json was found"));
        assert!(!settings.exists());
    }

    #[test]
    fn stock_classes_use_stock_subclasses_and_movement_defaults() {
        assert_eq!(default_subclass_name(0), "Sunbreaker");
        assert_eq!(default_subclass_name(1), "Nightstalker");
        assert_eq!(default_subclass_name(2), "Dawnblade");

        let abilities = catalog::AbilityOptions {
            movement: vec![
                AbilityChoice {
                    entry: 4,
                    name: "First".into(),
                },
                AbilityChoice {
                    entry: 5,
                    name: "Second".into(),
                },
                AbilityChoice {
                    entry: 6,
                    name: "Third".into(),
                },
            ],
            grenade: vec![AbilityChoice {
                entry: 7,
                name: "Grenade".into(),
            }],
            super_ability: vec![AbilityChoice {
                entry: 10,
                name: "Super".into(),
            }],
            melee: vec![AbilityChoice {
                entry: 11,
                name: "Melee".into(),
            }],
            class_ability: vec![AbilityChoice {
                entry: 2,
                name: "Class".into(),
            }],
            attunements: Vec::new(),
        };
        assert_eq!(default_ability_values(0, &abilities), (5, 7, 10, 11, 2));
        assert_eq!(default_ability_values(1, &abilities), (6, 7, 10, 11, 2));
        assert_eq!(default_ability_values(2, &abilities), (5, 7, 10, 11, 2));
    }

    #[test]
    fn distinctive_super_selection_wins_when_old_attunements_are_mixed() {
        let choice = |entry, name: &str| AbilityChoice {
            entry,
            name: name.into(),
        };
        let abilities = catalog::AbilityOptions {
            attunements: vec![
                catalog::AttunementChoice {
                    name: "Top".into(),
                    super_abilities: vec![choice(10, "Base super")],
                    melee: choice(11, "Top melee"),
                    perks: vec![choice(13, "Former top selector")],
                },
                catalog::AttunementChoice {
                    name: "Bottom".into(),
                    super_abilities: vec![choice(10, "Base super")],
                    melee: choice(15, "Bottom melee"),
                    perks: vec![choice(18, "Former bottom selector")],
                },
                catalog::AttunementChoice {
                    name: "Middle".into(),
                    super_abilities: vec![choice(20, "Middle super")],
                    melee: choice(21, "Middle melee"),
                    perks: Vec::new(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(selected_attunement_index(&abilities, 10, 15), 1);
        assert_eq!(selected_attunement_index(&abilities, 20, 15), 2);
        assert_eq!(selected_attunement_index(&abilities, 18, 21), 1);
    }

    #[test]
    fn settings_encoder_keeps_arrays_compact_and_round_trips() {
        let document = serde_json::json!({
            "outer": {
                "values": [1, 2, 3],
                "records": [{"name": "one"}, {"name": "two"}]
            }
        });
        let encoded = encode_settings(&document).unwrap();
        assert!(encoded.contains("\"values\": [1,2,3]"));
        assert!(encoded.contains("\"records\": [{\"name\":\"one\"},{\"name\":\"two\"}]"));
        assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), document);
    }

    #[test]
    fn settings_saves_are_verified_and_each_keeps_its_own_backup() {
        let directory = TestDirectory::new();
        let settings = directory.0.join("settings.json");
        let backups = directory.0.join("backups");
        fs::write(&settings, b"{\"version\":0}\n").unwrap();

        let first_document = serde_json::json!({"version": 1, "values": [1, 2, 3]});
        let first_backup =
            save_json_with_backup_root(&settings, &first_document, &backups).unwrap();
        let second_document = serde_json::json!({"version": 2, "values": [4, 5, 6]});
        let second_backup =
            save_json_with_backup_root(&settings, &second_document, &backups).unwrap();

        assert_ne!(first_backup, second_backup);
        assert_eq!(load_json(&settings).unwrap(), second_document);
        assert_eq!(
            load_json(&first_backup).unwrap(),
            serde_json::json!({"version": 0})
        );
        assert_eq!(load_json(&second_backup).unwrap(), first_document);
        assert!(fs::read_to_string(&settings).unwrap().ends_with('\n'));
    }

    #[test]
    fn unexpected_settings_get_an_exact_adjacent_backup_without_losing_an_older_one() {
        let directory = TestDirectory::new();
        let settings = directory.0.join("settings.json");
        let original = b"{\"unexpected\":1}\n";
        let newer = b"{\"unexpected\":2}\n";
        fs::write(&settings, original).unwrap();

        let adjacent = create_adjacent_backup(&settings).unwrap();
        assert_eq!(adjacent, directory.0.join("settings.json.bak"));
        assert_eq!(fs::read(&adjacent).unwrap(), original);

        fs::write(&settings, newer).unwrap();
        assert_eq!(create_adjacent_backup(&settings).unwrap(), adjacent);
        assert_eq!(fs::read(&adjacent).unwrap(), newer);
        let archived = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with("settings.json.bak.previous-")
                })
            })
            .unwrap();
        assert_eq!(fs::read(archived).unwrap(), original);
    }

    #[test]
    fn external_settings_changes_are_detected_before_saving() {
        let directory = TestDirectory::new();
        let settings = directory.0.join("settings.json");
        let loaded = serde_json::json!({"state": {"characters": [1, 2, 3]}});
        let newer = serde_json::json!({"state": {"characters": [1, 2, 3], "new": true}});
        fs::write(&settings, serde_json::to_vec(&loaded).unwrap()).unwrap();

        assert_eq!(verify_source_unchanged(&settings, &loaded), Ok(()));
        fs::write(&settings, serde_json::to_vec(&newer).unwrap()).unwrap();

        let error = verify_source_unchanged(&settings, &loaded).unwrap_err();
        assert!(error.contains("changed outside Sundial"));
        assert_eq!(load_json(&settings).unwrap(), newer);
    }

    #[test]
    fn oversized_settings_are_rejected_before_a_backup_or_write() {
        let directory = TestDirectory::new();
        let settings = directory.0.join("settings.json");
        let backups = directory.0.join("backups");
        let original = b"{\"version\":0}\n";
        fs::write(&settings, original).unwrap();
        let document = Value::String("x".repeat(MAX_SETTINGS_BYTES));

        assert!(save_json_with_backup_root(&settings, &document, &backups).is_err());
        assert_eq!(fs::read(&settings).unwrap(), original);
        assert!(!backups.exists());
    }

    #[test]
    fn class_armor_reset_preserves_destination_data() {
        let document = serde_json::json!({
            "state": { "characters": [{
                "class": 1,
                "equipment": {
                    "helmet": { "instance_soid": "template-helmet", "definition_hash": "hunter-helmet", "level": 106 },
                    "gauntlets": { "instance_soid": "template-arms", "definition_hash": "hunter-arms", "level": 106 },
                    "chest": { "instance_soid": "template-chest", "definition_hash": "hunter-chest", "level": 106 },
                    "legs": { "instance_soid": "template-legs", "definition_hash": "hunter-legs", "level": 106 },
                    "class_item": { "instance_soid": "template-class", "definition_hash": "hunter-cloak", "level": 106 }
                }
            }] }
        });
        let defaults = collect_class_armor_defaults(&document);
        let mut destination = serde_json::json!({
            "equipment": {
                "helmet": {
                    "instance_soid": "destination-helmet",
                    "definition_hash": "old",
                    "future_item_data": { "keep": [1, 2, 3] }
                },
                "gauntlets": { "instance_soid": "destination-arms", "definition_hash": "old" },
                "chest": { "instance_soid": "destination-chest", "definition_hash": "old" },
                "legs": { "instance_soid": "destination-legs", "definition_hash": "old" },
                "class_item": { "instance_soid": "destination-class", "definition_hash": "old" }
            }
        });
        let changed = restore_class_armor(
            destination.as_object_mut().unwrap(),
            defaults.get(&1).unwrap(),
        );
        assert!(changed);
        assert_eq!(
            destination.pointer("/equipment/helmet/definition_hash"),
            Some(&Value::String("hunter-helmet".into()))
        );
        assert_eq!(
            destination.pointer("/equipment/helmet/instance_soid"),
            Some(&Value::String("destination-helmet".into()))
        );
        assert_eq!(
            destination.pointer("/equipment/helmet/level"),
            Some(&Value::from(106))
        );
        assert_eq!(
            destination.pointer("/equipment/helmet/future_item_data"),
            Some(&serde_json::json!({ "keep": [1, 2, 3] }))
        );
    }
}
