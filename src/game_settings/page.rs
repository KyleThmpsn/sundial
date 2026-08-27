//! Game-settings page routing and player identity controls.

use std::path::Path;

use eframe::egui;
use serde_json::{Map, Value};

use super::{
    key_bindings::{KeyBindingUiState, draw_key_bindings},
    preferences::{
        GAME_LANGUAGES, draw_audio, draw_controls, draw_display, draw_interface, draw_social,
    },
    schema::ORBIT_SLICE_SET_PATH,
    widgets::{CommandBatch, json_string_choice},
};

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum Tab {
    Player,
    Controls,
    Audio,
    Display,
    Interface,
    Social,
    KeyBindings,
}

pub(crate) struct PageContext<'a> {
    pub json_document: &'a mut Value,
    pub account_settings: Result<&'a Map<String, Value>, &'a str>,
    pub bindings_editable: bool,
    pub orbit_backdrops: &'a [String],
    pub player_tools: PlayerTools,
    pub tab: &'a mut Tab,
    pub key_bindings: &'a mut KeyBindingUiState,
}

pub(crate) fn draw_page(ui: &mut egui::Ui, context: PageContext<'_>) -> PageEdits {
    let PageContext {
        json_document,
        account_settings,
        bindings_editable,
        orbit_backdrops,
        player_tools,
        tab,
        key_bindings,
    } = context;
    ui.heading("Game settings");
    ui.label("Edit the settings replicated to Destiny 2 by Project Sunrise.");
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(tab, Tab::Player, "Player");
        ui.selectable_value(tab, Tab::Controls, "Controls");
        ui.selectable_value(tab, Tab::Audio, "Audio");
        ui.selectable_value(tab, Tab::Display, "Display");
        ui.selectable_value(tab, Tab::Interface, "Interface");
        ui.selectable_value(tab, Tab::Social, "Social");
        ui.selectable_value(tab, Tab::KeyBindings, "Key bindings")
            .on_hover_text(if bindings_editable {
                "Edit named key bindings used by supported Sunrise schemas."
            } else {
                "Key bindings are shown read-only for the active account source or settings schema."
            });
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt(("game_settings_scroll", *tab))
        .show(ui, |ui| match *tab {
            Tab::Player => PageEdits {
                json_changed: draw_player(ui, json_document, orbit_backdrops, &player_tools),
                account_commands: Vec::new(),
            },
            Tab::Controls => draw_account_settings(ui, account_settings, draw_controls),
            Tab::Audio => draw_account_settings(ui, account_settings, draw_audio),
            Tab::Display => draw_account_settings(ui, account_settings, draw_display),
            Tab::Interface => draw_account_settings(ui, account_settings, draw_interface),
            Tab::Social => draw_account_settings(ui, account_settings, draw_social),
            Tab::KeyBindings => draw_account_settings(ui, account_settings, |ui, settings| {
                draw_key_bindings(ui, settings, key_bindings, bindings_editable)
            }),
        })
        .inner
}

#[derive(Default)]
pub(crate) struct PageEdits {
    pub(crate) json_changed: bool,
    pub(crate) account_commands: Vec<sundial_account::AccountSettingsCommand>,
}

pub(crate) struct PlayerTools {
    pub orbit_backdrops_enabled: bool,
}

pub(super) fn draw_account_settings(
    ui: &mut egui::Ui,
    settings: Result<&Map<String, Value>, &str>,
    draw: impl FnOnce(&mut egui::Ui, &Map<String, Value>) -> CommandBatch,
) -> PageEdits {
    let Ok(settings) = settings else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            settings.expect_err("the account settings result was checked"),
        );
        return PageEdits::default();
    };
    PageEdits {
        json_changed: false,
        account_commands: draw(ui, settings).into_vec(),
    }
}

pub(super) fn draw_player(
    ui: &mut egui::Ui,
    document: &mut Value,
    orbit_backdrops: &[String],
    player_tools: &PlayerTools,
) -> bool {
    ui.heading("Player");
    ui.label("Change the player identity and language Project Sunrise reports to Destiny 2.");
    ui.add_space(8.0);
    ui.strong("Player name");

    let mut changed = false;
    match document.pointer("/steam/user/persona_name") {
        Some(value) => {
            if let Some(current) = value.as_str() {
                let mut edited = current.to_owned();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut edited)
                        .desired_width(360.0)
                        .char_limit(63),
                );
                ui.label(egui::RichText::new(format!("{}/63", edited.len())).weak());
                ui.label("Use 1–63 printable ASCII characters. Changes take effect after fully restarting Destiny 2.");
                if response.changed() {
                    changed |= set_player_name(document, &edited);
                }
            } else {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "steam.user.persona_name must be text.",
                );
            }
        }
        None => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "This settings.json has no steam.user.persona_name field.",
            );
        }
    }

    if document.pointer("/steam/language").is_some() {
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        ui.strong("Game language");
        ui.label("Controls the language Sunrise reports to Destiny 2 through Steam. Changes take effect after fully restarting Destiny 2.");
        ui.add_space(4.0);
        if let Some(steam) = document
            .pointer_mut("/steam")
            .and_then(Value::as_object_mut)
        {
            changed |= egui::Grid::new("game_language_grid")
                .num_columns(2)
                .spacing([18.0, 9.0])
                .show(ui, |ui| {
                    json_string_choice(ui, steam, "language", "Language", GAME_LANGUAGES)
                })
                .inner;
        }
    }

    let orbit_value = document.pointer(ORBIT_SLICE_SET_PATH).cloned();
    if player_tools.orbit_backdrops_enabled
        && let Some(orbit_value) = orbit_value
    {
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        ui.strong("Orbit backdrop");
        if let Some(current) = orbit_value.as_str() {
            let mut selected = current.to_owned();
            let selected_text = if selected.is_empty() {
                "Sunrise default"
            } else {
                selected.as_str()
            };
            egui::ComboBox::from_id_salt("orbit_slice_set")
                .selected_text(selected_text)
                .width(280.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut selected, String::new(), "Sunrise default");
                    for name in orbit_backdrops {
                        ui.selectable_value(&mut selected, name.clone(), name);
                    }
                });
            if selected != current {
                changed |= set_existing_orbit_slice_set(document, &selected);
            }
            if orbit_backdrops.is_empty() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "No supported Orbit backdrops were found in the installed game packages.",
                );
            }
            let orbit_map_path = Path::new("Sunrise").join("orbit_map.txt");
            ui.label(format!(
                "Uses the selected internal Orbit slice set. Saving also rebuilds {} from the installed game packages; changes take effect after fully restarting Destiny 2.",
                orbit_map_path.display()
            ));
        } else {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "client.orbit_slice_set must be text.",
            );
        }
    }

    changed
}

pub(super) fn valid_player_name(name: &str) -> Option<&str> {
    (!name.is_empty() && name.len() <= 63 && name.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
        .then_some(name)
}

pub(super) fn set_player_name(document: &mut Value, name: &str) -> bool {
    let Some(name) = valid_player_name(name) else {
        return false;
    };
    let Some(value) = document.pointer_mut("/steam/user/persona_name") else {
        return false;
    };
    if !value.is_string() || value.as_str() == Some(name) {
        return false;
    }
    *value = Value::String(name.to_owned());
    true
}

pub(super) fn valid_orbit_slice_set(name: &str) -> bool {
    name.len() <= 48
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

pub(super) fn set_existing_orbit_slice_set(document: &mut Value, name: &str) -> bool {
    if !valid_orbit_slice_set(name) {
        return false;
    }
    let Some(value) = document.pointer_mut(ORBIT_SLICE_SET_PATH) else {
        return false;
    };
    if !value.is_string() || value.as_str() == Some(name) {
        return false;
    }
    *value = Value::String(name.to_owned());
    true
}

pub(super) fn group<'a>(
    settings: &'a Map<String, Value>,
    name: &str,
) -> Option<&'a Map<String, Value>> {
    settings.get(name)?.as_object()
}

pub(super) fn missing_group(ui: &mut egui::Ui, name: &str) {
    ui.colored_label(
        ui.visuals().error_fg_color,
        format!("The {name} settings group is missing or malformed."),
    );
}
