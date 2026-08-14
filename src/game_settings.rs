use eframe::egui;
use serde_json::{Map, Value};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tab {
    Player,
    Controls,
    Audio,
    Display,
    Interface,
    Social,
    KeyBindings,
}

const BUTTON_LAYOUTS: &[(u64, &str)] = &[
    (0, "Default"),
    (1, "Green Thumb"),
    (2, "Puppeteer"),
    (3, "Mirror"),
    (5, "Jumper"),
    (6, "Cold Shoulder"),
    (9, "Custom"),
];

const STICK_LAYOUTS: &[(u64, &str)] = &[
    (0, "Default"),
    (1, "Southpaw"),
    (2, "Legacy"),
    (3, "Legacy Southpaw"),
];
const DOUBLE_PRESS_DELAYS: &[(u64, &str)] = &[
    (0, "1 — 167 ms (Default)"),
    (1, "2 — 212 ms"),
    (2, "3 — 302 ms"),
    (3, "4 — 347 ms"),
    (4, "5 — 392 ms"),
];
const VOICE_OUTPUT_MODES: &[(u64, &str)] = &[
    (0, "Blended"),
    (1, "Headset Only (Default)"),
    (2, "Speakers Only"),
];
const TEAM_VOICE_MODES: &[(u64, &str)] = &[
    (0, "Manually Opt-in (Default)"),
    (1, "Automatic Opt-in When Solo"),
];
const PROXIMITY_VOICE_OUTPUTS: &[(u64, &str)] = &[(0, "Speakers (Default)"), (1, "Headset Only")];
const HDR_MODES: &[(u64, &str)] = &[(0, "Off (Default)"), (1, "On")];
const SUBTITLE_MODES: &[(u64, &str)] = &[(0, "Language-Based (Default)"), (1, "On"), (2, "Off")];
const COLORBLIND_MODES: &[(u64, &str)] = &[
    (0, "Off (Default)"),
    (1, "Deuteranopia (Red-Green)"),
    (2, "Protanopia (Red-Green)"),
    (3, "Tritanopia (Yellow-Blue)"),
];
const HELMET_MODES: &[(u64, &str)] = &[(0, "Off in Non-Combat Zones"), (1, "Always On")];
const HUD_OPACITY: &[(u64, &str)] = &[(0, "Off"), (1, "Low"), (2, "High"), (3, "Full (Default)")];
const BACKGROUND_OPACITY: &[(u64, &str)] = &[
    (0, "Lowest"),
    (1, "Low"),
    (2, "Medium (Default)"),
    (3, "High"),
    (4, "Highest"),
];
const RETICLE_LOCATIONS: &[(u64, &str)] = &[(0, "PC Default"), (1, "Console Default")];
const TEXT_CHAT_MODES: &[(u64, &str)] = &[
    (0, "Off"),
    (1, "On (No Notifications)"),
    (2, "On (No Audio)"),
    (3, "On (Default)"),
];
const WHISPER_CHAT_MODES: &[(u64, &str)] = &[(0, "On (Default)"), (1, "Off")];
const MANUAL_AUTOMATIC: &[(u64, &str)] = &[(0, "Manual"), (1, "Automatic")];
const AUTO_HIDE_MODES: &[(u64, &str)] = &[(0, "Off"), (1, "On")];

const ACTIONS: &[(&str, &str)] = &[
    ("fire", "Fire"),
    ("toggle_zoom", "Toggle zoom"),
    ("hold_zoom", "Hold zoom"),
    ("melee", "Melee"),
    ("grenade", "Grenade"),
    ("super", "Super"),
    ("reload", "Reload"),
    ("light_attack", "Light attack"),
    ("heavy_attack", "Heavy attack"),
    ("block", "Block"),
    ("switch_weapons", "Switch weapons"),
    ("next_weapon", "Next weapon"),
    ("previous_weapon", "Previous weapon"),
    ("primary_weapon", "Primary weapon"),
    ("special_weapon", "Special weapon"),
    ("heavy_weapon", "Heavy weapon"),
    ("move_forward", "Move forward"),
    ("move_backward", "Move backward"),
    ("move_left", "Move left"),
    ("move_right", "Move right"),
    ("jump", "Jump"),
    ("toggle_crouch", "Toggle crouch"),
    ("hold_crouch", "Hold crouch"),
    ("toggle_sprint", "Toggle sprint"),
    ("hold_sprint", "Hold sprint"),
    ("vehicle_boost", "Vehicle boost"),
    ("vehicle_brake", "Vehicle brake"),
    ("vehicle_zoom", "Vehicle zoom"),
    ("vehicle_fire_primary", "Vehicle primary fire"),
    ("vehicle_fire_secondary", "Vehicle secondary fire"),
    ("vehicle_exit", "Exit vehicle"),
    ("interact", "Interact"),
    ("highlight_player", "Highlight player"),
    ("emote_1", "Emote 1"),
    ("emote_2", "Emote 2"),
    ("emote_3", "Emote 3"),
    ("emote_4", "Emote 4"),
    ("air_move", "Air move"),
    ("class_ability", "Class ability"),
    ("death_cam_zoom_in", "Death camera zoom in"),
    ("death_cam_zoom_out", "Death camera zoom out"),
    ("push_to_talk", "Push to talk"),
    ("ui_gamepad_button_back", "Gamepad back"),
    ("ui_open_director", "Open Director"),
    ("ui_open_director_store_tab", "Director: Store"),
    ("ui_open_director_pursuits_tab", "Director: Pursuits"),
    ("ui_open_director_map_tab", "Director: Map"),
    (
        "ui_open_director_destinations_tab",
        "Director: Destinations",
    ),
    ("ui_open_director_roster_tab", "Director: Roster"),
    ("ui_open_director_seasons_tab", "Director: Seasons"),
    ("ui_open_start_menu_alternative", "Open character menu"),
    ("ui_open_start_menu_records_tab", "Character menu: Records"),
    (
        "ui_open_start_menu_collections_tab",
        "Character menu: Collections",
    ),
    ("ui_open_start_menu_clan_tab", "Character menu: Clan"),
    (
        "ui_open_start_menu_inventory_tab",
        "Character menu: Inventory",
    ),
    (
        "ui_open_start_menu_settings_tab",
        "Character menu: Settings",
    ),
    ("ui_open_exit_dialog_confirm", "Confirm exit dialog"),
    ("ui_abort_activity", "Abort activity"),
    ("ui_text_chat_toggle_state", "Toggle text chat"),
    ("screenshot", "Screenshot"),
];

pub(super) fn draw_page(
    ui: &mut egui::Ui,
    document: &mut Value,
    tab: &mut Tab,
    binding_search: &mut String,
) -> bool {
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
        ui.add_enabled_ui(false, |ui| {
            ui.selectable_value(tab, Tab::KeyBindings, "Key bindings")
        })
        .inner
        .on_hover_text("Key-name mapping is still in progress.");
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .show(ui, |ui| match *tab {
            Tab::Player => draw_player(ui, document),
            Tab::Controls => draw_account_settings(ui, document, draw_controls),
            Tab::Audio => draw_account_settings(ui, document, draw_audio),
            Tab::Display => draw_account_settings(ui, document, draw_display),
            Tab::Interface => draw_account_settings(ui, document, draw_interface),
            Tab::Social => draw_account_settings(ui, document, draw_social),
            Tab::KeyBindings => draw_account_settings(ui, document, |ui, settings| {
                draw_key_bindings(ui, settings, binding_search)
            }),
        })
        .inner
}

fn draw_account_settings(
    ui: &mut egui::Ui,
    document: &mut Value,
    draw: impl FnOnce(&mut egui::Ui, &mut Map<String, Value>) -> bool,
) -> bool {
    let Some(settings) = document
        .pointer_mut("/state/account/settings")
        .and_then(Value::as_object_mut)
    else {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            "This settings.json has no state.account.settings object.",
        );
        return false;
    };
    draw(ui, settings)
}

fn draw_player(ui: &mut egui::Ui, document: &mut Value) -> bool {
    ui.heading("Player");
    ui.label("Change the player name shown by Project Sunrise in Destiny 2.");
    ui.add_space(8.0);

    let Some(value) = document.pointer("/steam/user/persona_name") else {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            "This settings.json has no steam.user.persona_name field.",
        );
        return false;
    };
    let Some(current) = value.as_str() else {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            "steam.user.persona_name must be text.",
        );
        return false;
    };

    let mut edited = current.to_owned();
    let response = ui.add(
        egui::TextEdit::singleline(&mut edited)
            .desired_width(360.0)
            .char_limit(63),
    );
    ui.label(egui::RichText::new(format!("{}/63", edited.len())).weak());
    ui.label("Use 1–63 printable ASCII characters. Changes take effect after fully restarting Destiny 2.");

    if !response.changed() {
        return false;
    }

    set_player_name(document, &edited)
}

fn valid_player_name(name: &str) -> Option<&str> {
    (!name.is_empty() && name.len() <= 63 && name.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
        .then_some(name)
}

fn set_player_name(document: &mut Value, name: &str) -> bool {
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

fn group_mut<'a>(
    settings: &'a mut Map<String, Value>,
    name: &str,
) -> Option<&'a mut Map<String, Value>> {
    settings.get_mut(name)?.as_object_mut()
}

fn missing_group(ui: &mut egui::Ui, name: &str) {
    ui.colored_label(
        egui::Color32::LIGHT_RED,
        format!("The {name} settings group is missing or malformed."),
    );
}

fn draw_controls(ui: &mut egui::Ui, settings: &mut Map<String, Value>) -> bool {
    let Some(values) = group_mut(settings, "controls") else {
        missing_group(ui, "controls");
        return false;
    };
    ui.heading("Controls");
    ui.label("Controller and mouse behavior.");
    ui.add_space(8.0);
    egui::Grid::new("game_controls_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = false;
            changed |= choice(ui, values, "button_layout", "Button layout", BUTTON_LAYOUTS);
            changed |= choice(ui, values, "movement_mode", "Stick layout", STICK_LAYOUTS);
            changed |= offset_slider(
                ui,
                values,
                "controller_look_sensitivity",
                "Controller look sensitivity",
                0,
                9,
                1,
            );
            changed |= boolean(
                ui,
                values,
                "controller_invert_vertical",
                "Invert controller vertical look",
            );
            changed |= boolean(
                ui,
                values,
                "controller_auto_look_centering",
                "Controller auto-look centering",
            );
            changed |= boolean(ui, values, "controller_vibration", "Controller vibration");
            changed |= boolean(
                ui,
                values,
                "controller_swap_shoulders",
                "Swap controller shoulder buttons",
            );
            changed |= boolean(
                ui,
                values,
                "controller_invert_horizontal",
                "Invert controller horizontal look",
            );
            changed |= integer_slider(
                ui,
                values,
                "mouse_look_sensitivity",
                "Mouse look sensitivity",
                1,
                100,
            );
            changed |= boolean(
                ui,
                values,
                "mouse_invert_vertical",
                "Invert mouse vertical look",
            );
            changed |= boolean(
                ui,
                values,
                "mouse_invert_horizontal",
                "Invert mouse horizontal look",
            );
            changed |= boolean(
                ui,
                values,
                "unidentified_toggle",
                "Unidentified control toggle",
            );
            changed |= boolean(ui, values, "mouse_aim_smoothing", "Mouse aim smoothing");
            changed |= float_slider(
                ui,
                values,
                "ads_sensitivity_modifier",
                "ADS sensitivity modifier",
                0.5,
                1.5,
                0.1,
            );
            changed |= choice(
                ui,
                values,
                "double_press_delay",
                "Double-press delay",
                DOUBLE_PRESS_DELAYS,
            );
            changed
        })
        .inner
}

fn draw_audio(ui: &mut egui::Ui, settings: &mut Map<String, Value>) -> bool {
    let Some(values) = group_mut(settings, "audio") else {
        missing_group(ui, "audio");
        return false;
    };
    ui.heading("Audio");
    ui.label("Voice, volume, and focus behavior.");
    ui.add_space(8.0);
    egui::Grid::new("game_audio_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = false;
            changed |= choice(
                ui,
                values,
                "voice_output_mode",
                "Voice output mode",
                VOICE_OUTPUT_MODES,
            );
            changed |= choice(
                ui,
                values,
                "team_voice_channel",
                "Team voice channel",
                TEAM_VOICE_MODES,
            );
            changed |= choice(
                ui,
                values,
                "reserved_mode",
                "Proximity voice output",
                PROXIMITY_VOICE_OUTPUTS,
            );
            fixed(ui, values, "migration_version", "Audio migration version");
            changed |= integer_slider(ui, values, "chat_volume", "Voice chat volume", 0, 8);
            changed |= boolean(ui, values, "mute_when_unfocused", "Mute when unfocused");
            changed |= integer_slider(
                ui,
                values,
                "sound_effects_volume",
                "Sound effects volume",
                0,
                10,
            );
            changed |= integer_slider(ui, values, "dialogue_volume", "Dialogue volume", 0, 10);
            changed |= integer_slider(ui, values, "music_volume", "Music volume", 0, 10);
            changed
        })
        .inner
}

fn draw_display(ui: &mut egui::Ui, settings: &mut Map<String, Value>) -> bool {
    let Some(values) = group_mut(settings, "display") else {
        missing_group(ui, "display");
        return false;
    };
    ui.heading("Display");
    ui.label("Brightness and display overlays. Renderer calibration is shown but kept at Sunrise's required values.");
    ui.add_space(8.0);
    egui::Grid::new("game_display_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = false;
            changed |= integer_slider(ui, values, "brightness", "Brightness", 0, 6);
            changed |= boolean(ui, values, "show_fps", "Show FPS");
            changed |= choice(ui, values, "hdr_mode", "HDR mode", HDR_MODES);
            fixed(ui, values, "calibration_primary", "Renderer calibration");
            fixed(
                ui,
                values,
                "calibration_alpha",
                "Renderer calibration alpha",
            );
            changed
        })
        .inner
}

fn draw_interface(ui: &mut egui::Ui, settings: &mut Map<String, Value>) -> bool {
    let Some(values) = group_mut(settings, "interface") else {
        missing_group(ui, "interface");
        return false;
    };
    ui.heading("Interface");
    ui.label("HUD, subtitle, reticle, and text presentation.");
    ui.add_space(8.0);
    egui::Grid::new("game_interface_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = false;
            changed |= choice(
                ui,
                values,
                "subtitles_mode",
                "Subtitles mode",
                SUBTITLE_MODES,
            );
            changed |= choice(
                ui,
                values,
                "colorblind_mode",
                "Colorblind mode",
                COLORBLIND_MODES,
            );
            changed |= choice(ui, values, "helmet_mode", "Helmet mode", HELMET_MODES);
            changed |= choice(ui, values, "hud_opacity", "HUD opacity", HUD_OPACITY);
            changed |= boolean(ui, values, "display_hints", "Display hints");
            changed |= choice(
                ui,
                values,
                "background_opacity",
                "Background opacity",
                BACKGROUND_OPACITY,
            );
            changed |= choice(
                ui,
                values,
                "reticle_location",
                "Reticle location",
                RETICLE_LOCATIONS,
            );
            changed |= integer_slider(ui, values, "reticle_color", "Reticle color", 0, 6);
            changed |= integer_slider(ui, values, "text_size", "Text size", 0, 4);
            changed |= integer_slider(ui, values, "text_color", "Text color", 0, 3);
            changed |= integer_slider(
                ui,
                values,
                "text_background_style",
                "Text background style",
                0,
                3,
            );
            changed |= integer_slider(
                ui,
                values,
                "text_background_opacity",
                "Text background opacity",
                0,
                4,
            );
            fixed(ui, values, "reserved_text_mode", "Reserved text mode");
            fixed(
                ui,
                values,
                "subtitle_options_entry",
                "Subtitle options entry",
            );
            changed
        })
        .inner
}

fn draw_social(ui: &mut egui::Ui, settings: &mut Map<String, Value>) -> bool {
    let Some(values) = group_mut(settings, "social") else {
        missing_group(ui, "social");
        return false;
    };
    ui.heading("Social");
    ui.label("Chat, voice, names, and notifications.");
    ui.add_space(8.0);
    egui::Grid::new("game_social_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = false;
            changed |= boolean(
                ui,
                values,
                "prefer_good_connection",
                "Prefer good connection",
            );
            changed |= choice(
                ui,
                values,
                "text_chat_mode",
                "Text chat mode",
                TEXT_CHAT_MODES,
            );
            changed |= boolean(ui, values, "show_real_names", "Show real names");
            changed |= boolean(
                ui,
                values,
                "clan_invite_notifications",
                "Clan invite notifications",
            );
            changed |= boolean(ui, values, "profanity_filter", "Profanity filter");
            changed |= boolean(ui, values, "voice_chat_enabled", "Voice chat enabled");
            changed |= choice(
                ui,
                values,
                "whisper_chat_mode",
                "Whisper chat mode",
                WHISPER_CHAT_MODES,
            );
            changed |= choice(
                ui,
                values,
                "team_chat_join_mode",
                "Team chat join mode",
                MANUAL_AUTOMATIC,
            );
            changed |= choice(
                ui,
                values,
                "local_chat_join_mode",
                "Local chat join mode",
                MANUAL_AUTOMATIC,
            );
            changed |= choice(
                ui,
                values,
                "clan_chat_join_mode",
                "Clan chat join mode",
                MANUAL_AUTOMATIC,
            );
            changed |= choice(
                ui,
                values,
                "chat_auto_hide_mode",
                "Chat auto-hide mode",
                AUTO_HIDE_MODES,
            );
            changed
        })
        .inner
}

fn draw_key_bindings(
    ui: &mut egui::Ui,
    settings: &Map<String, Value>,
    search: &mut String,
) -> bool {
    let Some(bindings) = settings.get("key_bindings").and_then(Value::as_object) else {
        missing_group(ui, "key bindings");
        return false;
    };
    ui.heading("Key bindings");
    ui.label("Key binding editing is not available yet while Sundial's key-name mapping is being completed.");
    ui.add_space(6.0);
    ui.add(
        egui::TextEdit::singleline(search)
            .hint_text("Search actions…")
            .desired_width(320.0),
    );
    ui.add_space(8.0);
    let needle = search.trim().to_lowercase();
    egui::Grid::new("game_key_bindings_grid")
        .num_columns(3)
        .spacing([18.0, 8.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Action");
            ui.strong("Primary input ID");
            ui.strong("Secondary input ID");
            ui.end_row();
            let mut visible = 0usize;
            for &(key, label) in ACTIONS {
                if !needle.is_empty()
                    && !label.to_lowercase().contains(&needle)
                    && !key.contains(&needle)
                {
                    continue;
                }
                visible += 1;
                ui.label(label);
                let Some(binding) = bindings.get(key).and_then(Value::as_object) else {
                    ui.colored_label(egui::Color32::LIGHT_RED, "Missing");
                    ui.colored_label(egui::Color32::LIGHT_RED, "Missing");
                    ui.end_row();
                    continue;
                };
                binding_label(ui, binding.get("primary"));
                binding_label(ui, binding.get("secondary"));
                ui.end_row();
            }
            if visible == 0 {
                ui.label(egui::RichText::new("No matching actions").weak());
                ui.end_row();
            }
            false
        })
        .inner
}

fn boolean(ui: &mut egui::Ui, values: &mut Map<String, Value>, key: &str, label: &str) -> bool {
    ui.label(label);
    let mut changed = false;
    if let Some(value) = values.get_mut(key) {
        if let Some(mut checked) = value.as_bool() {
            if ui.checkbox(&mut checked, "").changed() {
                *value = Value::Bool(checked);
                changed = true;
            }
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Invalid value");
        }
    } else {
        ui.colored_label(egui::Color32::LIGHT_RED, "Missing");
    }
    ui.end_row();
    changed
}

fn choice(
    ui: &mut egui::Ui,
    values: &mut Map<String, Value>,
    key: &str,
    label: &str,
    choices: &[(u64, &str)],
) -> bool {
    ui.label(label);
    let mut changed = false;
    if let Some(value) = values.get_mut(key) {
        if let Some(mut current) = value.as_u64() {
            let selected = choices
                .iter()
                .find(|(candidate, _)| *candidate == current)
                .map_or("Invalid value", |(_, name)| *name);
            egui::ComboBox::from_id_salt(("game_setting", key))
                .selected_text(selected)
                .width(210.0)
                .show_ui(ui, |ui| {
                    for &(candidate, name) in choices {
                        if ui.selectable_value(&mut current, candidate, name).changed() {
                            changed = true;
                        }
                    }
                });
            if changed {
                *value = Value::from(current);
            }
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Invalid value");
        }
    } else {
        ui.colored_label(egui::Color32::LIGHT_RED, "Missing");
    }
    ui.end_row();
    changed
}

fn integer_slider(
    ui: &mut egui::Ui,
    values: &mut Map<String, Value>,
    key: &str,
    label: &str,
    minimum: u64,
    maximum: u64,
) -> bool {
    ui.label(label);
    let mut changed = false;
    if let Some(value) = values.get_mut(key) {
        if let Some(mut current) = value.as_u64() {
            if ui
                .add(egui::Slider::new(&mut current, minimum..=maximum))
                .changed()
            {
                *value = Value::from(current);
                changed = true;
            }
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Invalid value");
        }
    } else {
        ui.colored_label(egui::Color32::LIGHT_RED, "Missing");
    }
    ui.end_row();
    changed
}

fn offset_slider(
    ui: &mut egui::Ui,
    values: &mut Map<String, Value>,
    key: &str,
    label: &str,
    minimum: u64,
    maximum: u64,
    display_offset: u64,
) -> bool {
    ui.label(label);
    let mut changed = false;
    if let Some(value) = values.get_mut(key) {
        if let Some(current) = value.as_u64() {
            let mut displayed = current.saturating_add(display_offset);
            if ui
                .add(egui::Slider::new(
                    &mut displayed,
                    minimum + display_offset..=maximum + display_offset,
                ))
                .changed()
            {
                *value = Value::from(displayed - display_offset);
                changed = true;
            }
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Invalid value");
        }
    } else {
        ui.colored_label(egui::Color32::LIGHT_RED, "Missing");
    }
    ui.end_row();
    changed
}

fn float_slider(
    ui: &mut egui::Ui,
    values: &mut Map<String, Value>,
    key: &str,
    label: &str,
    minimum: f64,
    maximum: f64,
    step: f64,
) -> bool {
    ui.label(label);
    let mut changed = false;
    if let Some(value) = values.get_mut(key) {
        if let Some(mut current) = value.as_f64() {
            if ui
                .add(
                    egui::Slider::new(&mut current, minimum..=maximum)
                        .step_by(step)
                        .fixed_decimals(1),
                )
                .changed()
            {
                if let Some(number) = serde_json::Number::from_f64(current) {
                    *value = Value::Number(number);
                    changed = true;
                }
            }
        } else {
            ui.colored_label(egui::Color32::LIGHT_RED, "Invalid value");
        }
    } else {
        ui.colored_label(egui::Color32::LIGHT_RED, "Missing");
    }
    ui.end_row();
    changed
}

fn fixed(ui: &mut egui::Ui, values: &Map<String, Value>, key: &str, label: &str) {
    ui.label(label);
    if let Some(value) = values.get(key) {
        ui.add_enabled(false, egui::Label::new(value.to_string()))
            .on_hover_text("Project Sunrise requires this exact value.");
    } else {
        ui.colored_label(egui::Color32::LIGHT_RED, "Missing");
    }
    ui.end_row();
}

fn binding_label(ui: &mut egui::Ui, value: Option<&Value>) {
    let Some(value) = value else {
        ui.colored_label(egui::Color32::LIGHT_RED, "Missing");
        return;
    };
    if value.is_null() {
        ui.label(egui::RichText::new("Unassigned").weak());
    } else if let Some(code) = value.as_u64() {
        ui.add_enabled(false, egui::Label::new(code.to_string()));
    } else {
        ui.colored_label(egui::Color32::LIGHT_RED, "Invalid value");
    }
}

pub(super) fn validate(document: &Value) -> Result<(), String> {
    let settings = document
        .pointer("/state/account/settings")
        .and_then(Value::as_object)
        .ok_or("state.account.settings must be an object")?;

    let controls = group(settings, "controls")?;
    member(controls, "button_layout", &[0, 1, 2, 3, 5, 6, 9])?;
    range(controls, "movement_mode", 0, 3)?;
    range(controls, "controller_look_sensitivity", 0, 9)?;
    bool_fields(
        controls,
        "controls",
        &[
            "controller_invert_vertical",
            "controller_auto_look_centering",
            "controller_vibration",
            "controller_swap_shoulders",
            "controller_invert_horizontal",
            "mouse_invert_vertical",
            "mouse_invert_horizontal",
            "unidentified_toggle",
            "mouse_aim_smoothing",
        ],
    )?;
    range(controls, "mouse_look_sensitivity", 1, 100)?;
    float_range(controls, "ads_sensitivity_modifier", 0.5, 1.5)?;
    range(controls, "double_press_delay", 0, 4)?;

    let audio = group(settings, "audio")?;
    range(audio, "voice_output_mode", 0, 2)?;
    range(audio, "team_voice_channel", 0, 1)?;
    range(audio, "reserved_mode", 0, 1)?;
    exact_integer(audio, "migration_version", 8)?;
    range(audio, "chat_volume", 0, 8)?;
    bool_fields(audio, "audio", &["mute_when_unfocused"])?;
    range(audio, "sound_effects_volume", 0, 10)?;
    range(audio, "dialogue_volume", 0, 10)?;
    range(audio, "music_volume", 0, 10)?;

    let display = group(settings, "display")?;
    range(display, "brightness", 0, 6)?;
    bool_fields(display, "display", &["show_fps"])?;
    range(display, "hdr_mode", 0, 1)?;
    exact_float(display, "calibration_primary", 10_000.0)?;
    exact_float(display, "calibration_alpha", 0.0)?;

    let interface = group(settings, "interface")?;
    range(interface, "subtitles_mode", 0, 2)?;
    range(interface, "colorblind_mode", 0, 3)?;
    range(interface, "helmet_mode", 0, 1)?;
    range(interface, "hud_opacity", 0, 3)?;
    bool_fields(interface, "interface", &["display_hints"])?;
    range(interface, "background_opacity", 0, 4)?;
    range(interface, "reticle_location", 0, 1)?;
    range(interface, "reticle_color", 0, 6)?;
    range(interface, "text_size", 0, 4)?;
    range(interface, "text_color", 0, 3)?;
    range(interface, "text_background_style", 0, 3)?;
    range(interface, "text_background_opacity", 0, 4)?;
    exact_integer(interface, "reserved_text_mode", 0)?;
    exact_integer(interface, "subtitle_options_entry", 0)?;

    let social = group(settings, "social")?;
    bool_fields(
        social,
        "social",
        &[
            "prefer_good_connection",
            "show_real_names",
            "clan_invite_notifications",
            "profanity_filter",
            "voice_chat_enabled",
        ],
    )?;
    range(social, "text_chat_mode", 0, 3)?;
    range(social, "whisper_chat_mode", 0, 1)?;
    range(social, "team_chat_join_mode", 0, 1)?;
    range(social, "local_chat_join_mode", 0, 1)?;
    range(social, "clan_chat_join_mode", 0, 1)?;
    range(social, "chat_auto_hide_mode", 0, 1)?;

    let bindings = group(settings, "key_bindings")?;
    for &(key, label) in ACTIONS {
        let binding = bindings
            .get(key)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("Key binding {label} must be an object"))?;
        input_code(binding, label, "primary")?;
        input_code(binding, label, "secondary")?;
    }
    Ok(())
}

fn group<'a>(
    settings: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a Map<String, Value>, String> {
    settings
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("state.account.settings.{name} must be an object"))
}

fn integer(values: &Map<String, Value>, key: &str) -> Result<u64, String> {
    values
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("Game setting {key} must be a non-negative whole number"))
}

fn range(values: &Map<String, Value>, key: &str, minimum: u64, maximum: u64) -> Result<(), String> {
    let value = integer(values, key)?;
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "Game setting {key} must be between {minimum} and {maximum}"
        ))
    }
}

fn member(values: &Map<String, Value>, key: &str, allowed: &[u64]) -> Result<(), String> {
    let value = integer(values, key)?;
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("Game setting {key} has an unsupported value"))
    }
}

fn exact_integer(values: &Map<String, Value>, key: &str, expected: u64) -> Result<(), String> {
    let value = integer(values, key)?;
    if value == expected {
        Ok(())
    } else {
        Err(format!("Game setting {key} must remain {expected}"))
    }
}

fn float_range(
    values: &Map<String, Value>,
    key: &str,
    minimum: f32,
    maximum: f32,
) -> Result<(), String> {
    let value = float32(values, key)?;
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "Game setting {key} must be between {minimum} and {maximum}"
        ))
    }
}

fn exact_float(values: &Map<String, Value>, key: &str, expected: f32) -> Result<(), String> {
    let value = float32(values, key)?;
    if value.to_bits() == expected.to_bits() {
        Ok(())
    } else {
        Err(format!("Game setting {key} must remain {expected}"))
    }
}

// Sunrise stores these values as float, so validation intentionally uses the
// same f64-to-f32 conversion after serde_json parses the JSON number.
#[allow(clippy::cast_possible_truncation)]
fn float32(values: &Map<String, Value>, key: &str) -> Result<f32, String> {
    let value = values
        .get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("Game setting {key} must be a number"))?;
    Ok(value)
}

fn bool_fields(
    values: &Map<String, Value>,
    group_name: &str,
    fields: &[&str],
) -> Result<(), String> {
    for &key in fields {
        if values.get(key).and_then(Value::as_bool).is_none() {
            return Err(format!(
                "Game setting {group_name}.{key} must be true or false"
            ));
        }
    }
    Ok(())
}

fn input_code(binding: &Map<String, Value>, label: &str, half: &str) -> Result<(), String> {
    let Some(value) = binding.get(half) else {
        return Err(format!("Key binding {label} is missing its {half} value"));
    };
    if value.is_null()
        || value
            .as_u64()
            .is_some_and(|code| u16::try_from(code).is_ok())
    {
        Ok(())
    } else {
        Err(format!(
            "Key binding {label} {half} must be unassigned or between 0 and {}",
            u16::MAX
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_validation_matches_sunrise_float_storage() {
        let values = serde_json::json!({
            "calibration": 10000.0001,
            "ads": 1.500_000_01
        });
        let values = values.as_object().unwrap();

        assert_eq!(exact_float(values, "calibration", 10_000.0), Ok(()));
        assert_eq!(float_range(values, "ads", 0.5, 1.5), Ok(()));
    }

    #[test]
    fn player_name_matches_sunrise_persona_format() {
        assert_eq!(valid_player_name("Player"), Some("Player"));
        assert!(valid_player_name(&"x".repeat(63)).is_some());
        assert_eq!(valid_player_name(""), None);
        assert_eq!(valid_player_name(&"x".repeat(64)), None);
        assert_eq!(valid_player_name("Guardian\n"), None);
        assert_eq!(valid_player_name("Guardián"), None);
    }

    #[test]
    fn player_name_edit_preserves_every_other_json_value() {
        let mut document = serde_json::json!({
            "steam": {
                "user": {
                    "persona_name": "Player",
                    "future_user_setting": { "keep": [1, 2, 3] }
                },
                "future_steam_setting": true
            },
            "unknown_top_level_data": { "also_keep": "untouched" }
        });
        let mut expected = document.clone();
        *expected.pointer_mut("/steam/user/persona_name").unwrap() =
            Value::String("Guardian".into());

        assert!(set_player_name(&mut document, "Guardian"));

        assert_eq!(document, expected);
    }
}
