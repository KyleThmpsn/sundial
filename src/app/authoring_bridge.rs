//! Adapts Sundial's picker widgets, preferences, and account persistence for Parhelion.

mod account_cleanup;
pub(crate) use account_cleanup::{preview_account_cleanup, preview_account_replacement};

use std::{
    cmp::Reverse,
    hash::Hash,
    path::{Path, PathBuf},
};

use eframe::egui;

use crate::{
    catalog::{Catalog, CatalogSearchQuery, ItemDef, UnlockDefinition},
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
        draw_item_header_with_trailing_at_icon_size, draw_plug_icon_picker,
        draw_socket_picker_label, draw_socket_picker_reset, muted_item_header_fill,
        plug_picker_snapshot, socket_picker_label_width,
    },
};

pub(crate) const AUTHORING_SOCKET_RESET_WIDTH: f32 = SOCKET_PICKER_RESET_WIDTH;

pub(crate) fn authoring_socket_reset_width(ui: &egui::Ui) -> f32 {
    super::item_editor::socket_picker_reset_width(ui)
}

pub(crate) fn authoring_button_width(ui: &egui::Ui, label: &str) -> f32 {
    super::item_editor::measured_button_width(ui, label, 0.0)
}

pub(crate) fn draw_plug_safety_selector(
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

pub(crate) fn draw_authoring_plug_safety_warning(ui: &mut egui::Ui, mode: PlugSelectionMode) {
    super::preferences::draw_plug_selection_warning(ui, mode);
}

pub(crate) fn draw_authoring_toolbar<R>(
    ui: &mut egui::Ui,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    super::ui::toolbar(ui, contents)
}

pub(crate) fn authoring_socket_label_width(available_width: f32) -> f32 {
    socket_picker_label_width(available_width)
}

pub(crate) fn draw_authoring_socket_label(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
) -> egui::Response {
    draw_socket_picker_label(ui, label, width)
}

pub(crate) fn draw_authoring_socket_reset(
    ui: &mut egui::Ui,
    enabled: bool,
    tooltip: impl Into<egui::WidgetText>,
) -> egui::Response {
    draw_socket_picker_reset(ui, enabled, tooltip)
}

pub(crate) fn draw_authoring_info_icon(
    ui: &mut egui::Ui,
    tooltip: impl Into<egui::WidgetText>,
) -> egui::Response {
    crate::ui_help::info(ui, tooltip)
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
    let definition = match (selected, hash_text.as_deref()) {
        (Some(item), Some(hash_text)) => super::item_editor::DefinitionSummary::Known {
            name: &item.name,
            hash_display_text: hash_text,
            type_name: &item.type_name,
        },
        (None, Some(hash_text)) => super::item_editor::DefinitionSummary::Unknown {
            hash_display_text: hash_text,
        },
        (_, None) => super::item_editor::DefinitionSummary::Empty,
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
        valid: options.selected_hash.is_none() || selected.is_some(),
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
                ItemFilterScope::Weapon,
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
    if !query.is_empty() {
        matches.sort_by_cached_key(|donor| {
            (
                !donor.collection_backed,
                Reverse(query.name_match_count(&donor.name)),
                donor.name.to_lowercase(),
                donor.hash,
            )
        });
    }
    DefinitionPickerChoices {
        definitions: matches
            .into_iter()
            .map(|donor| DefinitionChoice {
                hash: u64::from(donor.hash),
                name: donor.name.clone(),
                type_name: donor.type_name.clone(),
                group: Some(
                    if donor.collection_backed {
                        "Weapons in Collections"
                    } else {
                        "Weapons without a Collections row"
                    }
                    .to_owned(),
                ),
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
    let settings_path = authored_unlock_settings_path(install, &preferences)?;
    synchronize_authored_collection_unlocks_at(
        &settings_path,
        unlocks,
        |path, document, original| {
            super::settings::save_json(path, document, original, false)
                .map(|receipt| receipt.backup)
                .map_err(Into::into)
        },
    )
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
    match super::settings::resolve_settings_path(install, preferred_layout) {
        super::SettingsPathResolution::Found(_, path) => Ok(path),
        super::SettingsPathResolution::Missing => {
            Err(super::settings::missing_settings_message(install))
        }
        super::SettingsPathResolution::Ambiguous => Err(
            "Multiple settings.json files exist for this installation. Open Sundial and select the active layout before installing authored packages"
                .to_owned(),
        ),
    }
}

fn synchronize_authored_collection_unlocks_at(
    settings_path: &Path,
    unlocks: &[(usize, u8, u16)],
    save: impl FnOnce(&Path, &serde_json::Value, &serde_json::Value) -> Result<PathBuf, String>,
) -> Result<(PathBuf, Option<PathBuf>, usize), String> {
    let original = super::settings::load_workspace_json(settings_path)?;
    let database_path = crate::persistence::investment_path(settings_path);
    if database_path
        .try_exists()
        .map_err(|error| error.to_string())?
        || crate::game_settings::schema_version(&original).is_some_and(|v| v >= 18)
    {
        #[cfg(feature = "sqlite-account")]
        {
            use crate::persistence::sqlite_account::{self, SqliteAccountDocumentLoad};
            let mut document =
                match sqlite_account::load_document(&database_path).map_err(|e| e.to_string())? {
                    SqliteAccountDocumentLoad::Loaded(document) => document,
                    _ => return Err(
                        "A compatible Sunrise investment database is required for authored unlocks"
                            .into(),
                    ),
                };
            let changed = apply_native_authored_unlocks(&mut document, unlocks)?;
            let backup = if changed == 0 {
                None
            } else {
                Some(
                    sqlite_account::save_document(&mut document)
                        .map_err(|e| e.to_string())?
                        .backup,
                )
            };
            return Ok((database_path, backup, changed));
        }
        #[cfg(not(feature = "sqlite-account"))]
        return Err("This build does not include SQLite account support".into());
    }
    synchronize_loaded_authored_collection_unlocks(settings_path, original, unlocks, save)
}

#[cfg(feature = "sqlite-account")]
fn apply_native_authored_unlocks(
    document: &mut crate::persistence::sqlite_account::SqliteAccountDocument,
    unlocks: &[(usize, u8, u16)],
) -> Result<usize, String> {
    let mut changed = 0;
    for unlock in unlocks {
        let count = if matches!(unlock.1, 3 | 6) {
            document.characters().characters().len()
        } else {
            1
        };
        let mut definition_changed = false;
        for index in 0..count {
            let (view, edits) = authored_unlock_changes(
                document.progression_view(index),
                std::slice::from_ref(unlock),
            )?;
            if edits != 0 {
                document
                    .apply_progression_view(index, &view)
                    .map_err(|error| error.to_string())?;
                definition_changed = true;
            }
        }
        changed += usize::from(definition_changed);
    }
    Ok(changed)
}

fn synchronize_loaded_authored_collection_unlocks(
    settings_path: &Path,
    original: serde_json::Value,
    unlocks: &[(usize, u8, u16)],
    save: impl FnOnce(&Path, &serde_json::Value, &serde_json::Value) -> Result<PathBuf, String>,
) -> Result<(PathBuf, Option<PathBuf>, usize), String> {
    let (document, changed) = authored_unlock_changes(original.clone(), unlocks)?;
    let backup = if changed == 0 {
        None
    } else {
        // Refuse to overwrite edits made by Sunrise, Sundial, or the user while the authored
        // package installation was finishing.
        super::settings::verify_workspace_source_unchanged(settings_path, &original, false)?;
        Some(save(settings_path, &document, &original)?)
    };
    Ok((settings_path.to_path_buf(), backup, changed))
}

fn authored_unlock_changes(
    original: serde_json::Value,
    unlocks: &[(usize, u8, u16)],
) -> Result<(serde_json::Value, usize), String> {
    let mut document = original;
    let mut changed = 0usize;
    for &(definition_index, bank, slot) in unlocks {
        if bank == crate::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_BANK
            && usize::from(slot)
                >= crate::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY
        {
            return Err(format!(
                "Authored unlock definition {definition_index} uses account slot {slot}, beyond the extended Shadowkeep account-flag region"
            ));
        }
        let definition = UnlockDefinition {
            hash: 0,
            code: u16::from(bank),
            compact_slot: Some(slot),
            name: None,
            description: None,
            runtime_writers: Vec::new(),
            tested_by: Vec::new(),
        };
        let state = super::progression::collection_state_snapshot(&document)
            .ok_or("The active settings.json does not expose a supported unlock-state layout")?;
        match state.flag_value(definition_index, &definition) {
            Some(true) => continue,
            Some(false) => {}
            None => {
                return Err(format!(
                    "Authored unlock definition {definition_index} uses unsupported bank {bank}"
                ));
            }
        }
        if !super::progression::set_collection_flag(
            &mut document,
            definition_index,
            &definition,
            true,
        ) {
            return Err(format!(
                "Could not set authored unlock definition {definition_index} at bank {bank}, slot {slot}"
            ));
        }
        changed += 1;
    }
    Ok((document, changed))
}

pub(crate) fn show_plug_safety_warnings() -> bool {
    super::settings::load_preferences()
        .preferences
        .show_safety_warnings
}

pub(crate) fn default_plug_selection_mode() -> PlugSelectionMode {
    super::settings::load_preferences()
        .preferences
        .default_plug_selection_mode
}

pub(crate) fn configure_fonts(
    ctx: &egui::Context,
    install: &std::path::Path,
) -> Result<(), String> {
    super::preferences::configure_destiny_symbol_fonts(ctx, install)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_supported_plug_choice_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    socket_type_override: Option<u16>,
    choice_index: usize,
    current_hash: Option<u64>,
    query: &mut String,
    button_text: &str,
    button_icon_hash: Option<u64>,
    button_tooltip: Option<&str>,
    button_width: f32,
    mode: PlugSelectionMode,
) -> Option<(usize, Option<u64>)> {
    let snapshot = plug_picker_snapshot_for_mode(
        catalog,
        item,
        socket_index,
        socket_type_override,
        current_hash,
        Some(button_text),
        mode,
    );
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
        anchor.on_hover_text(tooltip)
    } else {
        button_icon_hash.map_or(anchor.clone(), |hash| {
            catalog_item_tooltip(anchor, catalog, hash)
        })
    };
    match draw_plug_icon_picker(
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
                .is_some_and(|socket| socket.socket_type != *socket_type)
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
    let show_types = matches!(
        mode,
        PlugSelectionMode::GearType | PlugSelectionMode::AnyPlug
    );
    let allowed = match mode {
        PlugSelectionMode::Supported | PlugSelectionMode::SocketAndGearType => catalog
            .socket_and_gear_type_options_for_type(item, socket_type)
            .to_vec(),
        PlugSelectionMode::MatchingSocketType => catalog.socket_type_options(socket_type).to_vec(),
        PlugSelectionMode::GearType => catalog.gear_type_options_for_type(item, socket_type),
        PlugSelectionMode::AnyPlug => catalog.all_plug_options().to_vec(),
    };
    let choices = allowed
        .into_iter()
        .map(|hash| super::item_editor::PlugChoice {
            hash,
            label: catalog.plug_label(hash, true),
            type_name: if show_types {
                catalog
                    .plug_type_name(hash)
                    .unwrap_or("Unknown type")
                    .to_owned()
            } else {
                String::new()
            },
        })
        .collect();
    super::item_editor::PlugPickerSnapshot {
        socket_index,
        socket_label: format!("Socket {} · type {socket_type}", socket_index + 1),
        current_hash,
        current_label,
        native_default,
        native_default_label: None,
        choices,
        show_types,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::{app::SettingsLayout, test_support::TestDirectory};

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn native_authored_unlocks_cover_character_scopes_and_repeat_without_changes() {
        let directory = TestDirectory::new("authored-native-scopes");
        let path = directory.0.join("investment.sqlite3");
        crate::persistence::sqlite_account::tests::create_fixture(&path, 3);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("INSERT INTO characters SELECT 1,soid+1,race,gender,class,level,preview_available,appearance_value,last_orbited_destination,content_bypass,equipped_title,acquired_subclass_mask,next_inventory_serial FROM characters;").unwrap();
        let crate::persistence::sqlite_account::SqliteAccountDocumentLoad::Loaded(mut document) =
            crate::persistence::sqlite_account::load_document(&path).unwrap()
        else {
            panic!()
        };
        let unlocks = [(200, 1, 42), (201, 3, 43), (202, 6, 44)];
        assert_eq!(
            apply_native_authored_unlocks(&mut document, &unlocks).unwrap(),
            3
        );
        assert_eq!(
            apply_native_authored_unlocks(&mut document, &unlocks).unwrap(),
            0
        );
        crate::persistence::sqlite_account::tests::save_fixture_document(
            &mut document,
            &directory.0.join("backup.sqlite3"),
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM unlocks WHERE bank=0 AND slot=42 AND value=2",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM unlocks WHERE value=2 AND ((bank=4 AND slot=43) OR (bank=2 AND slot=44))",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            4
        );
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

    #[cfg(feature = "sqlite-account")]
    #[test]
    fn sqlite_unlock_sync_updates_active_database_and_preserves_json() {
        let directory = TestDirectory::new("authored-unlock-sqlite");
        let settings = directory.0.join("settings.json");
        let database = directory.0.join("data").join("investment.sqlite3");
        fs::write(
            &settings,
            serde_json::to_vec(&unlock_settings_for_test()).unwrap(),
        )
        .unwrap();
        crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
        let json_before = fs::read(&settings).unwrap();

        let (saved_path, backup, changed) = synchronize_authored_collection_unlocks_at(
            &settings,
            &[(200, 1, 42)],
            save_unlock_test_settings(&directory),
        )
        .unwrap();

        assert_eq!(saved_path, database);
        assert_eq!(changed, 1);
        assert!(backup.is_some_and(|path| path.is_file()));
        assert_eq!(fs::read(&settings).unwrap(), json_before);
        let db = rusqlite::Connection::open(&database).unwrap();
        assert_eq!(db.query_row("SELECT value FROM unlocks WHERE character_slot=-1 AND bank=0 AND slot=42 AND lane=0",[],|r|r.get::<_,i32>(0)).unwrap(),2);
        let saved = super::super::settings::load_workspace_json(&settings).unwrap();
        assert_eq!(
            saved.pointer("/state/unlocks/account_flag_runs"),
            Some(&json!([]))
        );
    }

    #[test]
    fn already_acquired_authored_unlocks_do_not_rewrite_or_back_up_settings() {
        let directory = TestDirectory::new("authored-unlock-idempotent");
        let settings = directory.0.join("settings.json");
        let document = json!({
            "version": 8,
            "state": {"unlocks": {"account_flag_runs": [[42, 1]]}}
        });
        let encoded = serde_json::to_vec(&document).unwrap();
        fs::write(&settings, &encoded).unwrap();

        let (_, backup, changed) = synchronize_authored_collection_unlocks_at(
            &settings,
            &[(200, 1, 42)],
            save_unlock_test_settings(&directory),
        )
        .unwrap();

        assert_eq!(changed, 0);
        assert_eq!(backup, None);
        assert_eq!(fs::read(&settings).unwrap(), encoded);
        assert!(!directory.0.join("backups").exists());
    }

    #[test]
    fn authored_unlock_sync_refuses_to_overwrite_a_newer_settings_document() {
        let directory = TestDirectory::new("authored-unlock-conflict");
        let settings = directory.0.join("settings.json");
        let original = unlock_settings_for_test();
        fs::write(&settings, serde_json::to_vec(&original).unwrap()).unwrap();
        let newer = json!({
            "version": 8,
            "state": {
                "unlocks": {"account_flag_runs": []},
                "changed_while_installing": true
            }
        });
        let newer_bytes = serde_json::to_vec(&newer).unwrap();
        fs::write(&settings, &newer_bytes).unwrap();

        let error = synchronize_loaded_authored_collection_unlocks(
            &settings,
            original,
            &[(200, 1, 42)],
            save_unlock_test_settings(&directory),
        )
        .unwrap_err();

        assert!(error.contains("changed outside Sundial"));
        assert_eq!(fs::read(&settings).unwrap(), newer_bytes);
        assert!(!directory.0.join("backups").exists());
    }

    #[test]
    fn authored_unlock_sync_accepts_the_last_extended_account_flag_byte() {
        let directory = TestDirectory::new("authored-unlock-extension-boundary");
        let settings = directory.0.join("settings.json");
        fs::write(
            &settings,
            serde_json::to_vec(&unlock_settings_for_test()).unwrap(),
        )
        .unwrap();
        let slot =
            u16::try_from(crate::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY - 1)
                .unwrap();

        let (_, _, changed) = synchronize_authored_collection_unlocks_at(
            &settings,
            &[(
                200,
                crate::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_BANK,
                slot,
            )],
            save_unlock_test_settings(&directory),
        )
        .unwrap();

        assert_eq!(changed, 1);
        let saved = super::super::settings::load_workspace_json(&settings).unwrap();
        assert_eq!(
            saved.pointer("/state/unlocks/account_flag_runs"),
            Some(&json!([[slot, 1]]))
        );
    }

    #[test]
    fn authored_unlock_sync_rejects_the_first_byte_of_the_next_account_region() {
        let directory = TestDirectory::new("authored-unlock-extension-overflow");
        let settings = directory.0.join("settings.json");
        let original = unlock_settings_for_test();
        let original_bytes = serde_json::to_vec(&original).unwrap();
        fs::write(&settings, &original_bytes).unwrap();
        let slot = u16::try_from(crate::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY)
            .unwrap();

        let error = synchronize_authored_collection_unlocks_at(
            &settings,
            &[(
                200,
                crate::package_authoring::SHADOWKEEP_ACCOUNT_FLAG_BANK,
                slot,
            )],
            save_unlock_test_settings(&directory),
        )
        .unwrap_err();

        assert!(error.contains("beyond the extended Shadowkeep account-flag region"));
        assert_eq!(fs::read(&settings).unwrap(), original_bytes);
        assert!(!directory.0.join("backups").exists());
    }

    fn save_unlock_test_settings(
        directory: &TestDirectory,
    ) -> impl FnOnce(&Path, &serde_json::Value, &serde_json::Value) -> Result<PathBuf, String> + '_
    {
        |path, document, original| {
            super::super::settings::save_test_json_checked(
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

    fn unlock_settings_for_test() -> serde_json::Value {
        json!({
            "version": 8,
            "state": {"unlocks": {"account_flag_runs": []}}
        })
    }

    #[test]
    fn authored_unlock_sync_preserves_v8_and_v13_documents_and_is_idempotent() {
        for version in [8, 13] {
            let directory = TestDirectory::new(&format!("authored-unlock-v{version}"));
            let settings = directory.0.join("settings.json");
            let original = json!({
                "version": version,
                "custom_setting": {"preserve": true},
                "state": {"unlocks": {
                    "account_flag_runs": [[42, 1]],
                    "character_flag_runs": [[61, 1]]
                }}
            });
            fs::write(&settings, serde_json::to_vec(&original).unwrap()).unwrap();
            let (_, backup, changed) = synchronize_authored_collection_unlocks_at(
                &settings,
                &[(21613, 1, 11923), (21614, 1, 11924)],
                save_unlock_test_settings(&directory),
            )
            .unwrap();
            assert_eq!(changed, 2);
            assert!(backup.unwrap().is_file());
            let saved = super::super::settings::load_workspace_json(&settings).unwrap();
            let mut expected = original;
            expected["state"]["unlocks"]["account_flag_runs"] = json!([[42, 1], [11923, 2]]);
            assert_eq!(saved, expected);
            let (_, backup, changed) = synchronize_authored_collection_unlocks_at(
                &settings,
                &[(21613, 1, 11923), (21614, 1, 11924)],
                save_unlock_test_settings(&directory),
            )
            .unwrap();
            assert_eq!(changed, 0);
            assert!(backup.is_none());
        }
    }
}
