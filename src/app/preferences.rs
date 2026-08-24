use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use eframe::egui;
use serde::{Deserialize, Serialize};

const DESTINY_SYMBOL_FONTS: &[(&str, &str)] = &[
    ("Destiny Symbols 360", "Destiny_Symbols_360.ttf"),
    ("Destiny Symbols PC", "Destiny_Symbols_PC.otf"),
];
const MATCHING_SOCKET_WARNING: &str = "Use caution: these plugs match the socket type but are not known to be supported by this item. Incompatible choices may prevent the item or loadout from working correctly.";
const GEAR_TYPE_WARNING: &str = "High risk: this exposes plugs used anywhere on the same broad gear type, not just this socket. Incompatible choices may prevent the item or loadout from working correctly.";
const ANY_PLUG_WARNING: &str = "High risk: this exposes every discovered plug for every socket. Incompatible choices may prevent Sunrise/Destiny 2 from loading or cause instability.";

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum PlugSelectionMode {
    #[default]
    Supported,
    MatchingSocketType,
    GearType,
    AnyPlug,
}

impl PlugSelectionMode {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Supported => "Compatible",
            Self::MatchingSocketType => "Socket type",
            Self::GearType => "Gear type",
            Self::AnyPlug => "All",
        }
    }
}

pub(super) fn draw_plug_selection_warning(ui: &mut egui::Ui, mode: PlugSelectionMode) {
    match mode {
        PlugSelectionMode::Supported => {}
        PlugSelectionMode::MatchingSocketType => {
            ui.colored_label(ui.visuals().warn_fg_color, MATCHING_SOCKET_WARNING);
        }
        PlugSelectionMode::GearType => {
            ui.colored_label(ui.visuals().error_fg_color, GEAR_TYPE_WARNING);
        }
        PlugSelectionMode::AnyPlug => {
            ui.colored_label(ui.visuals().error_fg_color, ANY_PLUG_WARNING);
        }
    }
}

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
    Root,
    BinX64,
}

impl SettingsLayout {
    pub(super) const ALL: [Self; 2] = [Self::Root, Self::BinX64];

    pub(super) fn relative_path(self) -> PathBuf {
        match self {
            Self::Root => PathBuf::from("Sunrise").join("settings.json"),
            Self::BinX64 => PathBuf::from("bin")
                .join("x64")
                .join("Sunrise")
                .join("settings.json"),
        }
    }

    pub(super) const fn preference_value(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::BinX64 => "bin_x64",
        }
    }

    pub(super) fn from_preference(value: &str) -> Option<Self> {
        match value {
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
    pub(super) experimental_orbit_backdrops: bool,
    #[serde(default)]
    pub(super) experimental_progression: bool,
    #[serde(default)]
    pub(super) experimental_power_above_cap: bool,
}

const fn default_show_safety_warnings() -> bool {
    true
}

pub(super) fn configure_destiny_symbol_fonts(
    ctx: &egui::Context,
    install: &Path,
) -> Result<(), String> {
    let mut fonts = egui::FontDefinitions::default();
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
            default_plug_selection_mode: PlugSelectionMode::Supported,
            show_safety_warnings: true,
            color_theme: ColorTheme::Dark,
            always_open_json_editor_in_second_window: false,
            show_plug_hashes: false,
            item_card_width: ItemCardWidth::Standard,
            character_inventory_layout: CharacterInventoryLayout::Cards,
            experimental_orbit_backdrops: false,
            experimental_progression: false,
            experimental_power_above_cap: false,
        }
    }
}

impl Preferences {
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
