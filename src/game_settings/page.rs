//! Game-settings page routing and player identity controls.

use eframe::egui;
use serde_json::{Map, Value};

use super::{
    key_bindings::{KeyBindingUiState, draw_key_bindings},
    preferences::{
        GAME_LANGUAGES, draw_audio, draw_controls, draw_display, draw_interface, draw_social,
    },
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
    Sunrise,
}

pub(crate) struct PageContext<'a> {
    pub json_document: &'a mut Value,
    pub account_settings: Result<&'a Map<String, Value>, &'a str>,
    pub bindings_editable: bool,
    pub json_account: bool,
    pub extended_fov: bool,
    pub tab: &'a mut Tab,
    pub key_bindings: &'a mut KeyBindingUiState,
}

pub(crate) fn draw_page(ui: &mut egui::Ui, context: PageContext<'_>) -> PageEdits {
    let PageContext {
        json_document,
        account_settings,
        bindings_editable,
        json_account,
        extended_fov,
        tab,
        key_bindings,
    } = context;
    let runtime_available = super::runtime::available(json_document);
    if *tab == Tab::Sunrise && !runtime_available {
        *tab = Tab::Player;
    }
    ui.horizontal(|ui| {
        ui.heading("Game Settings");
        crate::ui_help::info(
            ui,
            "Edit the settings Project Sunrise applies to Destiny 2.",
        );
    });
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(tab, Tab::Player, "Player");
        ui.selectable_value(tab, Tab::Controls, "Controls");
        ui.selectable_value(tab, Tab::Audio, "Audio");
        ui.selectable_value(tab, Tab::Display, "Display");
        ui.selectable_value(tab, Tab::Interface, "Interface");
        ui.selectable_value(tab, Tab::Social, "Social");
        ui.selectable_value(tab, Tab::KeyBindings, "Key Bindings")
            .on_hover_text(if !json_account {
                "Edit the numeric input codes stored in the active account database."
            } else if bindings_editable {
                "Edit named key bindings used by supported Sunrise schemas."
            } else {
                "Key bindings are shown read-only for the active account source or settings schema."
            });
        if runtime_available {
            ui.selectable_value(tab, Tab::Sunrise, "Sunrise");
        }
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt(("game_settings_scroll", *tab))
        .show(ui, |ui| match *tab {
            Tab::Sunrise => PageEdits {
                json_changed: super::runtime::draw(
                    ui,
                    json_document,
                    json_account || account_settings.is_ok(),
                ),
                account_commands: Vec::new(),
            },
            Tab::Player => PageEdits {
                json_changed: draw_player(ui, json_document),
                account_commands: Vec::new(),
            },
            Tab::Controls => draw_account_settings(ui, account_settings, draw_controls),
            Tab::Audio => draw_account_settings(ui, account_settings, draw_audio),
            Tab::Display => draw_account_settings(ui, account_settings, |ui, settings| {
                draw_display(
                    ui,
                    settings,
                    extended_fov && (runtime_available || !json_account),
                )
            }),
            Tab::Interface => draw_account_settings(ui, account_settings, draw_interface),
            Tab::Social => draw_account_settings(ui, account_settings, draw_social),
            Tab::KeyBindings => draw_account_settings(ui, account_settings, |ui, settings| {
                draw_key_bindings(ui, settings, key_bindings, bindings_editable, !json_account)
            }),
        })
        .inner
}

#[derive(Default)]
pub(crate) struct PageEdits {
    pub(crate) json_changed: bool,
    pub(crate) account_commands: Vec<sundial_account::AccountSettingsCommand>,
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

pub(super) fn draw_player(ui: &mut egui::Ui, document: &mut Value) -> bool {
    ui.horizontal(|ui| {
        ui.heading("Player");
        crate::ui_help::info(
            ui,
            "Change the player identity and language Project Sunrise reports to Destiny 2.",
        );
    });
    ui.add_space(8.0);
    let mut changed = false;
    ui.vertical(|ui| {
        ui.set_width(ui.available_width().min(360.0));
        ui.horizontal(|ui| {
            ui.strong("Player Name");
            crate::ui_help::info(ui, "Use 1–63 printable ASCII characters. Changes take effect after fully restarting Destiny 2.");
            if let Some(current) = document.pointer("/steam/user/persona_name").and_then(Value::as_str) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("{}/63", current.len())).weak());
                });
            }
        });
        match document.pointer("/steam/user/persona_name") {
        Some(value) => {
            if let Some(current) = value.as_str() {
                let mut edited = current.to_owned();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut edited)
                        .desired_width(f32::INFINITY)
                        .char_limit(63),
                );
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
    });

    if document.pointer("/steam/language").is_some() {
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.strong("Game Language");
            crate::ui_help::info(
                ui,
                "Controls the language Sunrise reports to Destiny 2 through Steam.",
            );
        });
        ui.label("Fully restart Destiny 2 to apply language changes.");
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
