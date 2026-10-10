//! Adapts Sundial's picker widgets, preferences, and account persistence for Parhelion.
mod appearance_picker;
mod donor_picker;
mod grants;
pub use donor_picker::WeaponChoiceFilter;
pub(crate) use donor_picker::draw_weapon_choice_filters;
pub(crate) use donor_picker::draw_weapon_donor_dropdown_picker;
pub(crate) use donor_picker::draw_weapon_donor_header_picker;
pub(crate) use donor_picker::draw_weapon_donor_picker_from;
pub(crate) use donor_picker::weapon_choice_passes;
use donor_picker::*;
mod plug_picker;
pub(crate) use grants::grant_authored_items;
pub(crate) use plug_picker::draw_perk_card_row;
pub(crate) use plug_picker::draw_perk_icon;
pub(crate) use plug_picker::draw_perk_row;
pub(crate) use plug_picker::draw_supported_plug_choice_picker;

use crate::account::{
    AuthoredAccountCleanup, AuthoredCollectionUnlock, AuthoredSlotReplacement, AuthoredSocketChange,
};
use std::collections::BTreeSet;
pub(crate) fn preview_account_cleanup(
    install: &Path,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
) -> Result<AuthoredAccountCleanup, String> {
    preview_account_replacement(install, hashes, unlocks, &[], None)
}

pub(crate) fn preview_account_replacement(
    install: &Path,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    socket_changes: &[AuthoredSocketChange],
    slots: Option<&AuthoredSlotReplacement>,
) -> Result<AuthoredAccountCleanup, String> {
    let runtime = crate::package_runtime::installed_runtime(install)?;
    preview_account_replacement_with_runtime(
        install,
        &runtime,
        hashes,
        unlocks,
        socket_changes,
        slots,
    )
}

pub(crate) fn preview_account_replacement_with_runtime(
    install: &Path,
    runtime: &crate::package_runtime::RuntimeSnapshot,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    socket_changes: &[AuthoredSocketChange],
    slots: Option<&AuthoredSlotReplacement>,
) -> Result<AuthoredAccountCleanup, String> {
    let preferences = crate::app::settings::load_preferences().preferences;
    let (target, durable) = authored_unlock_target_with_runtime(install, &preferences, runtime)?;
    if durable {
        crate::persistence::dawn_account::preview_replacement(
            &target,
            hashes,
            unlocks,
            socket_changes,
            slots,
        )
    } else {
        crate::account::preview_replacement(&target, hashes, unlocks, socket_changes, slots)
    }
}

use std::{
    cmp::Reverse,
    hash::Hash,
    path::{Path, PathBuf},
};

use eframe::egui;

use crate::{
    catalog::{Catalog, CatalogSearchQuery, ItemDef},
    hash::{format_hash_hex, parse_hash_hex},
    investment::{WeaponDonorPickerOptions, WeaponDonorSummary},
};

use super::{
    PlugSelectionMode,
    item_editor::{
        ClearDefinitionChoice, DefinitionChoice, DefinitionPickerChoices, ItemEditorAction,
        ItemFilter, ItemFilterScope, ItemHeader, NativePlugDefault, PickerHeight,
        SOCKET_PICKER_RESET_WIDTH, catalog_button, catalog_item_tooltip,
        draw_definition_picker_with_open_request_and_item_filter, draw_item_filter_bar,
        draw_item_header_with_trailing_at_icon_size, draw_plug_icon_picker_with_action,
        draw_socket_picker_label, draw_socket_picker_reset, muted_item_header_fill,
        plug_picker_snapshot, socket_picker_label_width,
    },
};

/// Width of the native Sundial Reset control used by an authoring socket row.
pub const AUTHORING_SOCKET_RESET_WIDTH: f32 = SOCKET_PICKER_RESET_WIDTH;

/// Native asset choices use the same row layout as investment choices.
pub fn draw_asset_choice_row(
    ui: &mut egui::Ui,
    name: &str,
    detail: &str,
    selected: bool,
) -> egui::Response {
    super::item_editor::draw_picker_row(
        ui,
        None,
        super::item_editor::CatalogPickerRow {
            hash: 0,
            primary: name,
            primary_max_rows: 1,
            secondary: Some(detail),
            icon_size: 0.0,
            row_height: crate::investment::authoring_choice_row_height(ui),
            selected,
        },
    )
    .on_hover_ui(|ui| {
        ui.set_max_width(320.0);
        crate::ui::help::tooltip_title(ui, name);
        ui.label(super::ui::destiny_text(ui, detail));
    })
}

/// The same row without the hover copy of its own text, for lists whose rows say
/// everything already.
pub fn draw_asset_choice_row_plain(
    ui: &mut egui::Ui,
    name: &str,
    detail: &str,
    selected: bool,
) -> egui::Response {
    super::item_editor::draw_picker_row(
        ui,
        None,
        super::item_editor::CatalogPickerRow {
            hash: 0,
            primary: name,
            primary_max_rows: 1,
            secondary: Some(detail),
            icon_size: 0.0,
            row_height: crate::investment::authoring_choice_row_height(ui),
            selected,
        },
    )
}

pub(crate) fn draw_authoring_choice_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: Option<u32>,
    name: &str,
    description: Option<&str>,
    selected: bool,
    icon: Option<crate::investment::IconOverride>,
) -> egui::Response {
    let response = super::item_editor::draw_picker_row_with_icon(
        ui,
        Some(catalog),
        super::item_editor::CatalogPickerRow {
            hash: u64::from(hash.unwrap_or_default()),
            primary: name,
            primary_max_rows: 1,
            secondary: description,
            icon_size: if hash.is_some() { 32.0 } else { 0.0 },
            row_height: crate::investment::authoring_choice_row_height(ui),
            selected,
        },
        icon,
    );
    if let Some(hash) = hash {
        if icon.is_none() && catalog.display_name(u64::from(hash)) == Some(name) {
            catalog_item_tooltip(response, catalog, u64::from(hash))
        } else {
            response.on_hover_ui(|ui| {
                super::item_editor::draw_item_tooltip_with_icon(
                    ui,
                    catalog,
                    u64::from(hash),
                    Some(crate::investment::PlugTooltip {
                        name: Some(name),
                        description,
                        classification_hash: None,
                    }),
                    icon,
                );
            })
        }
    } else {
        response.on_hover_ui(|ui| {
            ui.set_max_width(320.0);
            crate::ui::help::tooltip_title(ui, name);
            if let Some(description) = description {
                ui.label(super::ui::destiny_text(ui, description));
            }
        })
    }
}

/// The item tooltip's layout for something that is not an item.
pub(crate) fn draw_display_tooltip(
    ui: &mut egui::Ui,
    tooltip: crate::investment::DisplayTooltip<'_>,
) {
    super::item_editor::draw_display_tooltip(ui, tooltip);
}

/// Measured for the active font and padding, shared with the native Reset renderer.
pub fn authoring_socket_reset_width(ui: &egui::Ui) -> f32 {
    super::item_editor::socket_picker_reset_width(ui)
}

pub fn authoring_button_width(ui: &egui::Ui, label: &str) -> f32 {
    super::item_editor::measured_button_width(ui, label, 0.0)
}

/// Renders Sundial's matching risk warning for a selected plug-safety mode.
pub fn draw_authoring_plug_safety_warning(ui: &mut egui::Ui, mode: PlugSelectionMode) {
    super::preferences::draw_plug_selection_warning(ui, mode);
}

/// Draws Sundial's standard compact action toolbar for companion authoring utilities.
pub fn draw_authoring_toolbar<R>(
    ui: &mut egui::Ui,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    super::ui::toolbar(ui, contents)
}

/// Returns the responsive right-aligned label width used by Sundial's plug rows.
#[must_use]
pub fn authoring_socket_label_width(available_width: f32) -> f32 {
    socket_picker_label_width(available_width)
}

/// Renders the compact, right-aligned socket label used by Sundial's plug rows.
pub fn draw_authoring_socket_label(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    draw_socket_picker_label(ui, label, width)
}

/// Renders the native Sundial Reset control used by an authoring socket row.
pub fn draw_authoring_socket_reset(
    ui: &mut egui::Ui,
    enabled: bool,
    tooltip: impl Into<egui::WidgetText>,
) -> egui::Response {
    draw_socket_picker_reset(ui, enabled, tooltip)
}

/// Renders Sundial's compact information glyph with hover help.
pub fn draw_authoring_info_icon(
    ui: &mut egui::Ui,
    tooltip: impl Into<egui::WidgetText>,
) -> egui::Response {
    crate::ui::help::info(ui, tooltip)
}

/// Renders Sundial's compact warning glyph with hover help.
pub fn draw_authoring_warning_icon(
    ui: &mut egui::Ui,
    tooltip: impl Into<egui::WidgetText>,
) -> egui::Response {
    crate::ui::help::warning(ui, tooltip)
}

pub(crate) enum InvestmentWeaponPickerAction {
    Select(u64),
    Clear,
    Secondary,
}

const DONOR_HEADER_ICON_SIZE: f32 = 52.0;
type AppearancePreview<'a> = dyn FnMut(&mut egui::Ui, Option<u32>) + 'a;

/// An item identified by its source rather than the installed catalog.
pub struct AuthoringItemHeader<'a> {
    pub name: &'a str,
    pub type_name: &'a str,
    pub hash: Option<u32>,
    pub icon: Option<&'a egui::TextureHandle>,
}

/// Draw the usual authoring item card from supplied identity and artwork.
/// Returns the card's action button, so the caller owns what the action does.
pub fn draw_authoring_item_header(
    ui: &mut egui::Ui,
    item: AuthoringItemHeader<'_>,
    action_label: &str,
) -> egui::Response {
    let hash_text = item
        .hash
        .map(|hash| format_hash_hex(u64::from(hash)))
        .unwrap_or_default();
    let header = ItemHeader {
        label: None,
        soid: None,
        definition: super::item_editor::DefinitionSummary::Known {
            name: item.name,
            hash_display_text: &hash_text,
            type_name: item.type_name,
        },
        icon: item.icon.cloned(),
        fill: muted_item_header_fill(ui),
        valid: true,
        invalid_message: "",
    };
    let width = authoring_button_width(ui, action_label);
    let mut action = None;
    let card = draw_item_header_with_trailing_at_icon_size(
        ui,
        header,
        DONOR_HEADER_ICON_SIZE,
        width,
        |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    action = Some(ui.button(action_label));
                });
            });
        },
    );
    let action = action.expect("an authoring item header always draws its action button");
    if let Some(hash) = item.hash {
        copy_hash_menu(ui, &card, action.rect.left(), hash);
    }
    action
}

/// Copy Hash on a secondary click on a card's face. The face ends before the card's buttons at
/// `buttons_left`. egui gives a click to the later of two
/// widgets when it covers the other, and the card is added after its buttons, so a click region
/// over the whole card would take the buttons' clicks.
fn copy_hash_menu(ui: &egui::Ui, card: &egui::Response, buttons_left: f32, hash: u32) {
    let face = card
        .rect
        .with_max_x(buttons_left - ui.spacing().item_spacing.x);
    if !face.is_positive() {
        return;
    }
    let face = ui.interact(face, card.id.with("copy-hash"), egui::Sense::click());
    face.context_menu(|ui| {
        if ui.button("Copy Hash").clicked() {
            ui.ctx().copy_text(format_hash_hex(u64::from(hash)));
            ui.close();
        }
    });
}

/// A tile drawn like the donor cards, for something that is not a catalog item: an image, a
/// name and a second line, and `actions` on the right, the first rightmost. Each action is a
/// label and whether it is enabled. Returns the index of the action clicked.
pub fn draw_authoring_tile(
    ui: &mut egui::Ui,
    icon: Option<&egui::TextureHandle>,
    name: &str,
    detail: &str,
    actions: &[(&str, bool)],
) -> Option<usize> {
    let header = ItemHeader {
        label: None,
        soid: None,
        definition: super::item_editor::DefinitionSummary::Known {
            name,
            hash_display_text: "",
            type_name: detail,
        },
        icon: icon.cloned(),
        fill: muted_item_header_fill(ui),
        valid: true,
        invalid_message: "",
    };
    let spacing = ui.spacing().item_spacing.x;
    let width = actions
        .iter()
        .map(|(label, _)| authoring_button_width(ui, label) + spacing)
        .sum::<f32>()
        - spacing;
    let mut clicked = None;
    let _ = draw_item_header_with_trailing_at_icon_size(
        ui,
        header,
        DONOR_HEADER_ICON_SIZE,
        width.max(0.0),
        |ui| {
            // A donor card's hash line sits above its actions, a row at least a control tall in
            // a slightly larger monospace font. The same height here puts the actions where a
            // donor card's are.
            let hash_line = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
            ui.add_space(ui.spacing().interact_size.y.max(hash_line) + 2.0);
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for (index, (label, enabled)) in actions.iter().enumerate() {
                        if ui
                            .add_enabled(*enabled, egui::Button::new(*label))
                            .clicked()
                        {
                            clicked = Some(index);
                        }
                    }
                });
            });
        },
    );
    clicked
}

pub(crate) fn synchronize_authored_collection_unlocks(
    install: &Path,
    unlocks: &[(usize, u8, u16)],
) -> Result<(PathBuf, Option<PathBuf>, usize), String> {
    let preferences = super::settings::load_preferences().preferences;
    synchronize_authored_collection_unlocks_with(install, &preferences, unlocks)
}

fn synchronize_authored_collection_unlocks_with(
    install: &Path,
    preferences: &super::Preferences,
    unlocks: &[(usize, u8, u16)],
) -> Result<(PathBuf, Option<PathBuf>, usize), String> {
    let runtime = crate::package_runtime::installed_runtime(install)?;
    let (target, durable) = authored_unlock_target_with_runtime(install, preferences, &runtime)?;
    if durable {
        // The settings path guards its own writes this way. A durable account is the same
        // hazard: the runtime owns the database while it is up, and its uncheckpointed journal
        // would be written over.
        super::settings::require_game_closed(super::platform::destiny_is_running())?;
        crate::package_runtime::verify_installed_runtime(install, &runtime)?;
        let receipt =
            crate::persistence::dawn_account::apply_authored_unlocks(&target, &dawn_rows(unlocks)?)
                .map_err(|error| error.to_string())?;
        return Ok((target, receipt.backup, receipt.changed));
    }
    crate::package_runtime::verify_installed_runtime(install, &runtime)?;
    crate::account::unlocks::synchronize_authored_collection_unlocks_at(
        &target,
        unlocks,
        |path, document, original| {
            super::settings::save_json(path, document, original, false)
                .map(|receipt| receipt.backup)
                .map_err(Into::into)
        },
    )
}

/// Where an authored unlock is written for this installation, and whether that is a durable
/// account database rather than the settings document.
///
/// Which runtime is installed decides where the account lives, and only the DLL says so: a Dawn
/// install keeps its unlocks in player-state.db beside its settings, while Sunrise keeps them in
/// the settings document or the investment database next to it.
fn authored_unlock_target_with_runtime(
    install: &Path,
    preferences: &super::Preferences,
    runtime: &crate::package_runtime::RuntimeSnapshot,
) -> Result<(PathBuf, bool), String> {
    let settings_path = authored_runtime_settings_path(install, preferences, runtime)?;
    if runtime.brand() == crate::package_runtime::RuntimeBrand::Dawn {
        return Ok((crate::persistence::dawn_path(&settings_path), true));
    }
    Ok((settings_path, false))
}

#[cfg(test)]
fn authored_unlock_target(
    install: &Path,
    preferences: &super::Preferences,
) -> Result<(PathBuf, bool), String> {
    let runtime = crate::package_runtime::installed_runtime(install)?;
    authored_unlock_target_with_runtime(install, preferences, &runtime)
}

/// The authored unlocks as the storage-neutral rows every account adapter shares.
fn dawn_rows(unlocks: &[(usize, u8, u16)]) -> Result<Vec<sundial_account::AuthoredUnlock>, String> {
    unlocks
        .iter()
        .map(|(definition_index, bank, slot)| {
            u16::try_from(*definition_index)
                .map(|definition_index| sundial_account::AuthoredUnlock {
                    definition_index,
                    bank: *bank,
                    slot: *slot,
                })
                .map_err(|_| {
                    format!("Authored unlock definition {definition_index} is outside the range an unlock map addresses")
                })
        })
        .collect()
}

pub(crate) fn authored_client_settings_path(install: &Path) -> Result<PathBuf, String> {
    let preferences = super::settings::load_preferences().preferences;
    let runtime = crate::package_runtime::installed_runtime(install)?;
    authored_runtime_settings_path(install, &preferences, &runtime)
}

pub(crate) fn authored_client_settings_path_with_runtime(
    install: &Path,
    runtime: &crate::package_runtime::RuntimeSnapshot,
) -> Result<PathBuf, String> {
    let preferences = super::settings::load_preferences().preferences;
    authored_runtime_settings_path(install, &preferences, runtime)
}

fn authored_runtime_settings_path(
    install: &Path,
    preferences: &super::Preferences,
    runtime: &crate::package_runtime::RuntimeSnapshot,
) -> Result<PathBuf, String> {
    let selected_module = crate::package_runtime::sunrise_module_path(install);
    let path = match runtime.brand() {
        crate::package_runtime::RuntimeBrand::Dawn => selected_module
            .parent()
            .ok_or_else(|| {
                format!(
                    "The selected Dawn runtime DLL has no parent directory: {}",
                    selected_module.display()
                )
            })?
            .join(runtime.brand().folder())
            .join("settings.json"),
        crate::package_runtime::RuntimeBrand::Sunrise => {
            authored_unlock_settings_path(install, preferences)?
        }
    };
    crate::account::source::validate_runtime_document(install, &path)?;
    crate::package_runtime::verify_installed_runtime(install, runtime)?;
    Ok(path)
}

fn authored_unlock_settings_path(
    install: &Path,
    preferences: &super::Preferences,
) -> Result<PathBuf, String> {
    let preferred_layout = preferences
        .install_selection()
        .and_then(|selection| {
            let selected =
                crate::system::paths::resolve_path_for_comparison(&selection.install_path)
                    .unwrap_or_else(|_| selection.install_path.clone());
            let current = crate::system::paths::resolve_path_for_comparison(install)
                .unwrap_or_else(|_| install.to_path_buf());
            crate::system::paths::paths_equal(&selected, &current)
                .then_some(selection.preferred_layout)
        })
        .flatten();
    use crate::account::source::{self, SettingsPathResolution};
    match source::resolve_settings_path(install, preferred_layout) {
        SettingsPathResolution::Found(_, path) => {
            source::validate_runtime_document(install, &path)?;
            Ok(path)
        }
        SettingsPathResolution::Missing => Err(source::missing_settings_message(install)),
        SettingsPathResolution::Ambiguous => Err("Multiple settings.json files exist for this installation. Open Sundial and select the active layout before installing authored packages".to_owned()),
    }
}

/// Returns whether Sundial's saved preferences allow plug-selection safety warnings.
#[must_use]
pub fn show_plug_safety_warnings() -> bool {
    super::settings::load_preferences()
        .preferences
        .show_safety_warnings
}

/// Returns Sundial's saved default scope for plug selection.
#[must_use]
pub fn default_plug_selection_mode() -> PlugSelectionMode {
    super::settings::load_preferences()
        .preferences
        .default_plug_selection_mode
}

/// Installs Sundial's text/symbol font families for an external investment-authoring window.
/// The fallback family is registered even when optional game font files cannot be read.
pub fn configure_fonts(ctx: &egui::Context, install: &std::path::Path) -> Result<(), String> {
    super::preferences::configure_destiny_symbol_fonts(ctx, install)
}

#[cfg(test)]
pub(crate) fn save_unlock_test_settings(
    directory: &crate::test_support::TestDirectory,
) -> impl FnOnce(&Path, &serde_json::Value, &serde_json::Value) -> Result<PathBuf, String> + '_ {
    |path, document, original| {
        crate::app::settings::save_test_json_checked(
            path,
            document,
            original,
            false,
            &directory.0.join("backups"),
        )
        .map(|receipt| receipt.backup)
        .map_err(Into::into)
    }
}

pub(crate) fn resolve_preview(
    catalog: &Catalog,
    loadout: &crate::ui::model_preview::Loadout,
) -> crate::ui::model_preview::Appearance {
    super::item_editor::appearance::resolve(catalog, loadout)
}

pub(crate) fn preview_loadout(
    catalog: &Catalog,
    hash: u32,
) -> Option<crate::ui::model_preview::Loadout> {
    super::item_editor::appearance::loadout(catalog, u64::from(hash))
}

pub(crate) fn shader_preview(
    catalog: &Catalog,
    hash: u32,
    shader: &[Vec<(i8, u16)>; 3],
) -> Option<crate::ui::model_preview::Appearance> {
    super::item_editor::appearance::with_shader(catalog, u64::from(hash), shader)
}

#[cfg(test)]
mod tests {
    mod confirmation;
    mod resync;
    mod runtime;
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::{app::SettingsLayout, test_support::TestDirectory};

    #[test]
    fn weapon_picker_hides_known_dummies_without_hiding_same_name_or_custom_weapons() {
        let hashes = [
            0xCB6F_6266,
            0x3B4F_02D5,
            0xC92D_6B37,
            0x3452_F1F3,
            0xBB5D_B0A6,
            0x9954_40F4,
            0x3EBA_E846,
            0x3735_6D87,
            0x032B_2570,
            1,
        ];
        let donors: Vec<_> = hashes
            .into_iter()
            .map(|hash| WeaponDonorSummary {
                hash,
                name: "Polaris Lance".into(),
                type_name: "Scout Rifle".into(),
                bucket_hash: 1_498_876_634,
                collection_backed: hash == 0xCB6F_6266,
                power_cap: None,
                damage_type: None,
                inventory_slot: None,
                ammo_type: None,
                weapon_pattern_index: None,
                weapon_translation_group: None,
                stat_group_index: None,
                damage_profile: crate::investment::WeaponDamageProfile::Unknown,
                rarity: crate::investment::WeaponRarity::Legendary,
            })
            .collect();
        let catalog = Catalog::for_test(
            donors
                .iter()
                .map(|donor| ItemDef {
                    hash: u64::from(donor.hash),
                    name: donor.name.clone(),
                    type_name: donor.type_name.clone(),
                    bucket_hash: donor.bucket_hash,
                    class_type: 3,
                    default_plugs: Vec::new(),
                    sockets: Vec::new(),
                    abilities: Default::default(),
                })
                .collect(),
            Default::default(),
        );
        let candidates: Vec<_> = donors.iter().collect();
        let mut filter = ItemFilter::default();
        let visible = filtered_weapon_donors(&catalog, &candidates, &filter);
        assert_eq!(
            visible.iter().map(|donor| donor.hash).collect::<Vec<_>>(),
            [0xCB6F_6266, 0x032B_2570, 1]
        );
        filter.include_dummy_weapons = true;
        assert_eq!(
            filtered_weapon_donors(&catalog, &candidates, &filter).len(),
            10
        );
        filter.weapon_type = Some("Auto Rifle".into());
        assert!(filtered_weapon_donors(&catalog, &candidates, &filter).is_empty());
    }

    #[test]
    fn authored_unlock_path_honors_each_saved_layout() {
        let directory = TestDirectory::new("authored-unlock-layout");
        let game_root = super::super::settings::settings_path_for_install(
            &directory.0,
            SettingsLayout::GameRoot,
        );
        let root =
            super::super::settings::settings_path_for_install(&directory.0, SettingsLayout::Root);
        let bin =
            super::super::settings::settings_path_for_install(&directory.0, SettingsLayout::BinX64);
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::create_dir_all(bin.parent().unwrap()).unwrap();
        fs::write(&game_root, b"{}\n").unwrap();
        fs::write(&root, b"{}\n").unwrap();
        fs::write(&bin, b"{}\n").unwrap();

        for (layout, expected) in [("game_root", game_root), ("root", root), ("bin_x64", bin)] {
            let preferences = super::super::Preferences {
                install: Some(directory.0.clone()),
                settings_layout: Some(layout.to_owned()),
                ..Default::default()
            };
            assert_eq!(
                authored_unlock_settings_path(&directory.0, &preferences).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn authored_unlock_path_keeps_ambiguous_layouts_blocked_without_a_matching_preference() {
        let directory = TestDirectory::new("authored-unlock-ambiguous");
        for layout in SettingsLayout::ALL {
            let path = super::super::settings::settings_path_for_install(&directory.0, layout);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"{}\n").unwrap();
        }
        let preferences = super::super::Preferences {
            install: Some(directory.0.join("different-install")),
            settings_layout: Some("root".to_owned()),
            ..Default::default()
        };

        let error = authored_unlock_settings_path(&directory.0, &preferences).unwrap_err();
        assert!(error.contains("Multiple settings.json files exist"));
    }
}
