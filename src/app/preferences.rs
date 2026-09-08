pub use crate::investment::PlugSelectionMode;
pub(super) use crate::investment::plug_selection::draw_plug_selection_warning;

pub(super) mod store;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use eframe::egui;
use serde::{Deserialize, Serialize};

use super::ui::destiny_text_font_family;

const DESTINY_SYMBOL_FONTS: &[(&str, &str)] = &[
    ("Destiny Symbols PC", "Destiny_Symbols_PC.otf"),
    ("Destiny Symbols 360", "Destiny_Symbols_360.ttf"),
];
pub(super) const MIN_AUTOMATIC_BACKUP_LIMIT: u16 = 5;
pub(super) const MAX_AUTOMATIC_BACKUP_LIMIT: u16 = 100;
const DEFAULT_AUTOMATIC_BACKUP_LIMIT: u16 = 20;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ColorTheme {
    #[default]
    Dark,
    Light,
}

impl ColorTheme {
    pub(super) const fn egui_theme(self) -> egui::Theme {
        match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ItemCardWidth {
    Compact,
    #[default]
    Standard,
    Wide,
}

impl ItemCardWidth {
    pub(super) const fn dimensions(self) -> (f32, f32) {
        match self {
            Self::Compact => (285.0, 315.0),
            Self::Standard => (335.0, 390.0),
            Self::Wide => (430.0, 520.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CharacterInventoryLayout {
    #[default]
    Cards,
    Panoptes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsLayout {
    GameRoot,
    Root,
    BinX64,
}

impl SettingsLayout {
    pub(super) const ALL: [Self; 3] = [Self::GameRoot, Self::Root, Self::BinX64];

    pub(super) fn relative_path(self) -> PathBuf {
        match self {
            Self::GameRoot => PathBuf::from("settings.json"),
            Self::Root => PathBuf::from("Sunrise").join("settings.json"),
            Self::BinX64 => PathBuf::from("bin")
                .join("x64")
                .join("Sunrise")
                .join("settings.json"),
        }
    }

    pub(super) const fn preference_value(self) -> &'static str {
        match self {
            Self::GameRoot => "game_root",
            Self::Root => "root",
            Self::BinX64 => "bin_x64",
        }
    }

    pub(super) fn from_preference(value: &str) -> Option<Self> {
        match value {
            "game_root" => Some(Self::GameRoot),
            "root" => Some(Self::Root),
            "bin_x64" => Some(Self::BinX64),
            _ => None,
        }
    }
}

pub(super) enum SettingsPathResolution {
    Found(SettingsLayout, PathBuf),
    Missing,
    Ambiguous,
}

#[derive(Clone)]
pub(super) struct InstallSelection {
    pub(super) install_path: PathBuf,
    pub(super) preferred_layout: Option<SettingsLayout>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Preferences {
    #[serde(default)]
    pub(super) install: Option<PathBuf>,
    #[serde(default)]
    pub(super) settings_layout: Option<String>,
    #[serde(default)]
    pub(super) really_unsafe_warning_acknowledged: bool,
    #[serde(default)]
    pub(super) default_plug_selection_mode: PlugSelectionMode,
    #[serde(default = "default_show_safety_warnings")]
    pub(super) show_safety_warnings: bool,
    #[serde(default)]
    pub(super) review_changes_before_saving: bool,
    #[serde(default)]
    pub(super) limit_automatic_backups: bool,
    #[serde(default = "default_automatic_backup_limit")]
    pub(super) automatic_backup_limit: u16,
    #[serde(default)]
    pub(super) color_theme: ColorTheme,
    #[serde(default)]
    pub(super) always_open_json_editor_in_second_window: bool,
    #[serde(default)]
    pub(super) show_plug_hashes: bool,
    #[serde(default)]
    pub(super) item_card_width: ItemCardWidth,
    #[serde(default)]
    pub(super) character_inventory_layout: CharacterInventoryLayout,
    #[serde(default)]
    pub(super) experimental_progression: bool,
    #[serde(default)]
    pub(super) experimental_activity_state: bool,
    #[serde(default)]
    pub(super) experimental_power_above_cap: bool,
    #[serde(default)]
    pub(super) experimental_extended_fov: bool,
    #[serde(default)]
    pub(super) experimental_cross_class_subclasses: bool,
    #[serde(default)]
    pub(super) experimental_package_authoring: bool,
    #[serde(default)]
    pub(super) parhelion_warning_acknowledged: bool,
    #[serde(default)]
    pub(super) show_parhelion_experimental_options: bool,
    #[serde(default)]
    pub(super) troubleshooting_logging: bool,
}

const fn default_show_safety_warnings() -> bool {
    true
}

const fn default_automatic_backup_limit() -> u16 {
    DEFAULT_AUTOMATIC_BACKUP_LIMIT
}

pub(super) fn normalized_automatic_backup_limit(limit: u16) -> u16 {
    limit.clamp(MIN_AUTOMATIC_BACKUP_LIMIT, MAX_AUTOMATIC_BACKUP_LIMIT)
}

pub(super) fn configure_destiny_symbol_fonts(
    ctx: &egui::Context,
    install: &Path,
) -> Result<(), String> {
    let mut fonts = egui::FontDefinitions::default();
    let proportional_fallbacks = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    for &(name, file_name) in DESTINY_SYMBOL_FONTS {
        let path = install.join("fonts").join(file_name);
        match fs::read(&path) {
            Ok(bytes) => {
                fonts
                    .font_data
                    .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
                loaded.push(name.to_owned());
            }
            Err(error) => errors.push(format!("Could not read {}: {error}", path.display())),
        }
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .extend(loaded.clone());
    }
    let mut destiny_text_fonts = loaded;
    destiny_text_fonts.extend(proportional_fallbacks);
    fonts
        .families
        .insert(destiny_text_font_family(), destiny_text_fonts);
    ctx.set_fonts(fonts);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            install: None,
            settings_layout: None,
            really_unsafe_warning_acknowledged: false,
            default_plug_selection_mode: PlugSelectionMode::SocketAndGearType,
            show_safety_warnings: true,
            review_changes_before_saving: false,
            limit_automatic_backups: false,
            automatic_backup_limit: DEFAULT_AUTOMATIC_BACKUP_LIMIT,
            color_theme: ColorTheme::Dark,
            always_open_json_editor_in_second_window: false,
            show_plug_hashes: false,
            item_card_width: ItemCardWidth::Standard,
            character_inventory_layout: CharacterInventoryLayout::Cards,
            experimental_progression: false,
            experimental_activity_state: false,
            experimental_power_above_cap: false,
            experimental_extended_fov: false,
            experimental_cross_class_subclasses: false,
            experimental_package_authoring: false,
            parhelion_warning_acknowledged: false,
            show_parhelion_experimental_options: false,
            troubleshooting_logging: false,
        }
    }
}

impl Preferences {
    pub(super) fn normalize_for_runtime(&mut self) {
        self.automatic_backup_limit =
            normalized_automatic_backup_limit(self.automatic_backup_limit);
        if self.default_plug_selection_mode == PlugSelectionMode::AnyPlug
            && !self.really_unsafe_warning_acknowledged
        {
            self.default_plug_selection_mode = PlugSelectionMode::SocketAndGearType;
        }
    }

    pub(super) fn reset_editable_settings(&mut self) {
        let install = self.install.take();
        let settings_layout = self.settings_layout.take();
        *self = Self {
            install,
            settings_layout,
            ..Self::default()
        };
    }

    pub(super) fn install_selection(&self) -> Option<InstallSelection> {
        Some(InstallSelection {
            install_path: self.install.clone()?,
            preferred_layout: self
                .settings_layout
                .as_deref()
                .and_then(SettingsLayout::from_preference),
        })
    }
}
