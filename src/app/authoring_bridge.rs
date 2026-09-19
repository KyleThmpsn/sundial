//! Adapts Sundial's picker widgets, preferences, and account persistence for Parhelion.

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
    let preferences = crate::app::settings::load_preferences().preferences;
    let settings_path = authored_unlock_settings_path(install, &preferences)?;
    crate::account::preview_replacement(&settings_path, hashes, unlocks, socket_changes, slots)
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
        draw_item_header_with_trailing_at_icon_size, draw_plug_icon_picker_with_footer,
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
        crate::ui_help::tooltip_title(ui, name);
        ui.label(super::ui::destiny_text(ui, detail));
    })
}

pub(crate) fn draw_authoring_choice_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: Option<u32>,
    name: &str,
    description: Option<&str>,
    selected: bool,
) -> egui::Response {
    let response = super::item_editor::draw_catalog_picker_row(
        ui,
        catalog,
        super::item_editor::CatalogPickerRow {
            hash: u64::from(hash.unwrap_or_default()),
            primary: name,
            primary_max_rows: 1,
            secondary: description,
            icon_size: if hash.is_some() { 32.0 } else { 0.0 },
            row_height: crate::investment::authoring_choice_row_height(ui),
            selected,
        },
    );
    if let Some(hash) = hash {
        if catalog.display_name(u64::from(hash)) == Some(name) {
            catalog_item_tooltip(response, catalog, u64::from(hash))
        } else {
            response.on_hover_ui(|ui| {
                super::item_editor::draw_item_tooltip(
                    ui,
                    catalog,
                    u64::from(hash),
                    Some(crate::investment::PlugTooltip {
                        name: Some(name),
                        description,
                        classification_hash: None,
                    }),
                );
            })
        }
    } else {
        response.on_hover_ui(|ui| {
            ui.set_max_width(320.0);
            crate::ui_help::tooltip_title(ui, name);
            if let Some(description) = description {
                ui.label(super::ui::destiny_text(ui, description));
            }
        })
    }
}

/// Measured for the active font and padding, shared with the native Reset renderer.
pub fn authoring_socket_reset_width(ui: &egui::Ui) -> f32 {
    super::item_editor::socket_picker_reset_width(ui)
}

pub fn authoring_button_width(ui: &egui::Ui, label: &str) -> f32 {
    super::item_editor::measured_button_width(ui, label, 0.0)
}

/// Renders Sundial's native inline plug-safety selector.
pub fn draw_plug_safety_selector(
    ui: &mut egui::Ui,
    scope: impl Hash,
    mode: &mut PlugSelectionMode,
) -> bool {
    let before = *mode;
    ui.label("Plug Safety");
    ui.push_id(scope, |ui| {
        ui.menu_button(mode.label(), |ui| {
            for candidate in PlugSelectionMode::ALL {
                if ui.radio_value(mode, candidate, candidate.label()).clicked() {
                    ui.close_menu();
                }
            }
        });
    });
    *mode != before
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
    crate::ui_help::info(ui, tooltip)
}

/// Opens a menu from a compact trigger showing an installed item's artwork beside its label.
///
/// The trigger deliberately leaves out the stock watermark and foreground overlay: it stands for
/// the appearance an authored item takes, not for the stock item that lends it.
pub(crate) fn draw_catalog_menu_button<R>(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    icon_hash: Option<u32>,
    cleared_color: Option<[u8; 3]>,
    label: &str,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let icon_size = ui.spacing().interact_size.y - 2.0 * ui.spacing().button_padding.y;
    let button = icon_hash
        .and_then(|hash| catalog.icon_texture_artwork(ui.ctx(), u64::from(hash), cleared_color))
        .map_or_else(
            || egui::Button::new(label),
            |texture| {
                egui::Button::image_and_text(
                    egui::Image::new((texture.id(), egui::vec2(icon_size, icon_size)))
                        .bg_fill(super::ui::package_icon_backdrop(ui)),
                    label,
                )
            },
        );
    egui::menu::menu_custom_button(ui, button.truncate(), contents)
}

pub(crate) enum InvestmentWeaponPickerAction {
    Select(u64),
    Clear,
    Secondary,
}

const DONOR_HEADER_ICON_SIZE: f32 = 52.0;

pub(crate) fn draw_weapon_donor_header_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    candidates: &[&WeaponDonorSummary],
    options: WeaponDonorPickerOptions<'_>,
) -> Option<InvestmentWeaponPickerAction> {
    let scope = ui.make_persistent_id(scope);
    let selected = options
        .selected_hash
        .and_then(|hash| catalog.item(u64::from(hash)));
    let hash_text = options
        .selected_hash
        .map(|hash| format_hash_hex(u64::from(hash)));
    // Ornaments and other plugs are named installed items without a socketed item definition.
    // Describing them from the catalog's name and type maps keeps a selected appearance source
    // readable instead of reporting it as missing.
    let plug = options
        .selected_hash
        .filter(|_| selected.is_none())
        .and_then(|hash| {
            let hash = u64::from(hash);
            let name = catalog.display_name(hash)?;
            Some((name, catalog.plug_type_name(hash).unwrap_or_default()))
        });
    let definition =
        match (selected, hash_text.as_deref()) {
            (Some(item), Some(hash_text)) => super::item_editor::DefinitionSummary::Known {
                name: &item.name,
                hash_display_text: hash_text,
                type_name: &item.type_name,
            },
            (None, Some(hash_text)) => {
                super::item_editor::DefinitionSummary::from_name_and_type(hash_text, plug)
            }
            // Nothing selected: name the choice that leads here rather than the generic Empty, so a
            // header like Unique Weapon Behavior does not read as two ways of saying nothing.
            (_, None) => options.clear.as_ref().map_or(
                super::item_editor::DefinitionSummary::Empty,
                |choice| super::item_editor::DefinitionSummary::Known {
                    name: choice.label,
                    hash_display_text: "",
                    type_name: "",
                },
            ),
        };
    let header = ItemHeader {
        label: options.header_label,
        soid: None,
        definition,
        icon: options.selected_icon_override.cloned().or_else(|| {
            options
                .selected_hash
                .and_then(|hash| catalog.icon_texture(ui.ctx(), u64::from(hash)))
        }),
        fill: muted_item_header_fill(ui),
        valid: options.selected_hash.is_none() || selected.is_some() || plug.is_some(),
        invalid_message: "Not in the loaded catalog",
    };
    let mut action_button = None;
    let mut secondary_button = None;
    let action_width = authoring_button_width(ui, options.action_label)
        + options.secondary_action_label.map_or(0.0, |label| {
            authoring_button_width(ui, label) + ui.spacing().item_spacing.x
        });
    let trigger = draw_item_header_with_trailing_at_icon_size(
        ui,
        header,
        DONOR_HEADER_ICON_SIZE,
        action_width,
        |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    action_button = Some(ui.button(options.action_label));
                    if let Some(label) = options.secondary_action_label {
                        secondary_button = Some(ui.button(label));
                    }
                });
            });
        },
    );
    if let Some(hash) = options.selected_hash {
        drop(catalog_item_tooltip(trigger, catalog, u64::from(hash)));
    }
    let action_button = action_button.expect("a donor header always draws its action button");
    let action = draw_definition_picker_with_open_request_and_item_filter(
        ui,
        catalog,
        scope.with("picker"),
        query,
        PickerHeight {
            min: 220.0,
            max: 480.0,
        },
        (Some(&action_button), false),
        |ui, query_text, filter| {
            let items = candidates
                .iter()
                .filter_map(|donor| catalog.item(u64::from(donor.hash)))
                .collect::<Vec<_>>();
            let interacted = draw_item_filter_bar(
                ui,
                scope.with("filters"),
                ItemFilterScope::WeaponDonor,
                &items,
                filter,
            );
            let filtered = filtered_weapon_donors(catalog, candidates, filter);
            (
                weapon_donor_choices(catalog, query_text, &filtered, options),
                interacted,
            )
        },
    );
    if secondary_button.is_some_and(|button| button.clicked()) {
        return Some(InvestmentWeaponPickerAction::Secondary);
    }
    match action {
        Some(ItemEditorAction::SetDefinition { hash }) => {
            Some(InvestmentWeaponPickerAction::Select(hash))
        }
        Some(ItemEditorAction::ClearDefinition) => Some(InvestmentWeaponPickerAction::Clear),
        Some(_) | None => None,
    }
}

fn filtered_weapon_donors<'a>(
    catalog: &Catalog,
    candidates: &[&'a WeaponDonorSummary],
    filter: &ItemFilter,
) -> Vec<&'a WeaponDonorSummary> {
    candidates
        .iter()
        .copied()
        .filter(|donor| {
            if !filter.include_dummy_weapons && crate::dummy_items::contains(u64::from(donor.hash))
            {
                return false;
            }
            catalog
                .item(u64::from(donor.hash))
                .is_some_and(|item| filter.matches(catalog, item))
        })
        .collect()
}

fn weapon_donor_choices(
    catalog: &Catalog,
    query_text: &str,
    candidates: &[&WeaponDonorSummary],
    options: WeaponDonorPickerOptions<'_>,
) -> DefinitionPickerChoices {
    let query = CatalogSearchQuery::new(query_text);
    let mut matches = candidates
        .iter()
        .copied()
        .filter(|donor| {
            query.matches(
                catalog,
                u64::from(donor.hash),
                &[donor.name.as_str(), donor.type_name.as_str()],
            )
        })
        .collect::<Vec<_>>();
    // One list. Whether a weapon has a Collections row changes how it is built, not how it is
    // browsed, so ordering follows the search, then the weapon type and name. An empty query
    // scores every candidate zero and falls through to that ordering.
    matches.sort_by_cached_key(|donor| {
        (
            Reverse(query.name_match_count(&donor.name)),
            donor.type_name.to_lowercase(),
            donor.name.to_lowercase(),
            donor.hash,
        )
    });
    DefinitionPickerChoices {
        definitions: matches
            .into_iter()
            .map(|donor| DefinitionChoice {
                hash: u64::from(donor.hash),
                name: donor.name.clone(),
                type_name: donor.type_name.clone(),
                group: None,
            })
            .collect(),
        existing_inventory: Vec::new(),
        clear: options.clear.map(|choice| ClearDefinitionChoice {
            label: choice.label.to_owned(),
            tooltip: choice.tooltip.to_owned(),
            selected: choice.selected,
        }),
        random_item_builder_hash: None,
        empty_message: "No compatible installed weapons found".to_owned(),
    }
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
    let (target, durable) = authored_unlock_target(install, preferences)?;
    if durable {
        // The settings path guards its own writes this way. A durable account is the same
        // hazard: the runtime owns the database while it is up, and its uncheckpointed journal
        // would be written over.
        super::settings::require_game_closed(super::platform::destiny_is_running())?;
        let receipt =
            crate::persistence::dawn_account::apply_authored_unlocks(&target, &dawn_rows(unlocks)?)
                .map_err(|error| error.to_string())?;
        return Ok((target, receipt.backup, receipt.changed));
    }
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
fn authored_unlock_target(
    install: &Path,
    preferences: &super::Preferences,
) -> Result<(PathBuf, bool), String> {
    let inspection = crate::package_runtime::installation::RuntimeInspection::inspect(install);
    if let Some(runtime) = inspection.launch_copy().filter(|runtime| runtime.dawn) {
        // Dawn owns the folder named after it, so its settings and the account beside them are
        // taken from the runtime itself. An installation that has also run Sunrise still holds
        // that runtime's folder, and guessing between the two would write the unlock to whichever
        // one happened to be preferred rather than to the one the game reads.
        let settings_path = runtime.settings_path.clone();
        crate::account::source::validate_runtime_document(install, &settings_path)?;
        return Ok((crate::persistence::dawn_path(&settings_path), true));
    }
    Ok((authored_unlock_settings_path(install, preferences)?, false))
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
    authored_unlock_settings_path(install, &preferences)
}

fn authored_unlock_settings_path(
    install: &Path,
    preferences: &super::Preferences,
) -> Result<PathBuf, String> {
    let preferred_layout = preferences
        .install_selection()
        .and_then(|selection| {
            let selected = crate::paths::resolve_path_for_comparison(&selection.install_path)
                .unwrap_or_else(|_| selection.install_path.clone());
            let current = crate::paths::resolve_path_for_comparison(install)
                .unwrap_or_else(|_| install.to_path_buf());
            crate::paths::paths_equal(&selected, &current).then_some(selection.preferred_layout)
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

pub(crate) fn draw_supported_plug_choice_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &ItemDef,
    query: &mut String,
    options: crate::investment::PlugChoicePickerOptions<'_>,
    footer: impl FnOnce(&mut egui::Ui) -> bool,
) -> Option<(usize, Option<u64>)> {
    let crate::investment::PlugChoicePickerOptions {
        socket_index,
        socket_type_override,
        choice_index,
        current_hash,
        button,
        mode,
        ..
    } = options;
    let current_hash = current_hash.map(u64::from);
    let button_text = button.text;
    let button_icon_hash = button.icon_hash.map(u64::from);
    let button_tooltip = button.tooltip;
    let button_width = f32::from(button.width);
    let mut snapshot = plug_picker_snapshot_for_mode(
        catalog,
        item,
        socket_index,
        socket_type_override,
        current_hash,
        Some(button_text),
        mode,
    );
    snapshot.custom_current = current_hash.is_none() && button_tooltip.is_some();
    let row_height = ui
        .spacing()
        .interact_size
        .y
        .max(16.0 + 2.0 * ui.spacing().button_padding.y);
    let button = button_icon_hash.map_or_else(
        || egui::Button::new(button_text),
        |hash| {
            catalog_button(
                ui,
                catalog,
                hash,
                button_text,
                row_height - 2.0 * ui.spacing().button_padding.y,
            )
        },
    );
    let button = button
        .truncate()
        .min_size(egui::vec2(button_width.max(0.0), row_height));
    let left_aligned =
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min);
    let anchor = if button_width > 0.0 {
        ui.allocate_ui_with_layout(egui::vec2(button_width, row_height), left_aligned, |ui| {
            ui.add_sized([button_width, row_height], button)
        })
        .inner
    } else {
        ui.with_layout(left_aligned, |ui| ui.add(button)).inner
    };
    let anchor = if let Some(tooltip) = button_tooltip {
        anchor.on_hover_ui(|ui| {
            super::item_editor::draw_item_tooltip(
                ui,
                catalog,
                button_icon_hash.unwrap_or_default(),
                Some(tooltip),
            );
        })
    } else {
        button_icon_hash.map_or(anchor.clone(), |hash| {
            catalog_item_tooltip(anchor, catalog, hash)
        })
    };
    match draw_plug_icon_picker_with_footer(
        ui,
        catalog,
        (
            "investment-authoring-choice",
            item.hash,
            socket_index,
            choice_index,
        ),
        query,
        &snapshot,
        PickerHeight {
            min: 180.0,
            max: 420.0,
        },
        &anchor,
        footer,
    ) {
        Some(ItemEditorAction::SetPlug { socket_index, hash }) => Some((socket_index, hash)),
        Some(_) | None => None,
    }
}

fn plug_picker_snapshot_for_mode(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    socket_type_override: Option<u16>,
    current_hash: Option<u64>,
    empty_label: Option<&str>,
    mode: PlugSelectionMode,
) -> super::item_editor::PlugPickerSnapshot {
    let native_default = socket_type_override
        .filter(|socket_type| {
            item.sockets
                .get(socket_index)
                .is_none_or(|socket| socket.socket_type != *socket_type)
        })
        .map_or_else(
            || match item.default_plugs.get(socket_index) {
                Some(Some(hash)) => parse_hash_hex(hash).map(NativePlugDefault::Plug),
                Some(None) => Some(NativePlugDefault::Empty),
                None => None,
            },
            |_| None,
        );
    let current_label = current_hash.map_or_else(
        || empty_label.unwrap_or("None").to_owned(),
        |hash| catalog.plug_label(hash, true),
    );
    let Some(socket_type) = socket_type_override else {
        return plug_picker_snapshot(
            catalog,
            item,
            socket_index,
            current_hash,
            current_label,
            native_default,
            mode,
        );
    };
    let (choices, show_types) = super::item_editor::plug_choices_for_socket_type(
        catalog,
        item,
        socket_index,
        Some(socket_type),
        mode,
    );
    super::item_editor::PlugPickerSnapshot {
        socket_index,
        socket_label: format!("Socket {} · type {socket_type}", socket_index + 1),
        current_hash,
        current_label,
        custom_current: false,
        native_default,
        native_default_label: native_default.and_then(|default| match default {
            super::item_editor::NativePlugDefault::Plug(hash) => {
                Some(catalog.plug_label(hash, true))
            }
            super::item_editor::NativePlugDefault::Empty => None,
        }),
        choices,
        show_types,
    }
}

/// A compact native icon-and-name catalog row.
pub(crate) fn draw_perk_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u32,
    name: &str,
    selected: bool,
    tooltip: crate::investment::PlugTooltip<'_>,
) -> egui::Response {
    let height =
        ui.spacing().interact_size.y.max(
            ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y,
        );
    super::item_editor::draw_catalog_picker_row(
        ui,
        catalog,
        super::item_editor::CatalogPickerRow {
            hash: u64::from(hash),
            primary: name,
            primary_max_rows: 1,
            secondary: None,
            icon_size: height - 8.0,
            row_height: height,
            selected,
        },
    )
    .on_hover_ui(|ui| {
        super::item_editor::draw_item_tooltip(ui, catalog, u64::from(hash), Some(tooltip));
    })
}

/// Native icon with the same backdrop as the catalog controls.
pub(crate) fn draw_perk_icon(ui: &mut egui::Ui, catalog: &Catalog, hash: u32, size: f32) {
    if let Some(icon) = catalog.icon_texture(ui.ctx(), u64::from(hash)) {
        ui.add(
            egui::Image::new(&icon)
                .fit_to_exact_size(egui::vec2(size, size))
                .bg_fill(super::ui::package_icon_backdrop(ui)),
        );
    } else {
        ui.allocate_space(egui::vec2(size, size));
    }
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

#[cfg(test)]
mod tests {
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
