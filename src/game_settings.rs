use eframe::egui;
use serde_json::{Map, Value};

#[derive(Default)]
pub(super) struct KeyBindingUiState {
    action_search: String,
    picker: BindingPickerState,
}

impl KeyBindingUiState {
    pub(super) fn clear_pickers(&mut self) {
        self.picker = BindingPickerState::default();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BindingModifier {
    #[default]
    None,
    Shift,
    Control,
    Alt,
}

impl BindingModifier {
    const ALL: [(Self, &'static str); 4] = [
        (Self::None, "None"),
        (Self::Shift, "Shift"),
        (Self::Control, "Ctrl"),
        (Self::Alt, "Alt"),
    ];

    const fn input_name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Shift => Some("shift"),
            Self::Control => Some("control"),
            Self::Alt => Some("alt"),
        }
    }
}

#[derive(Default)]
struct BindingPickerState {
    query: String,
    modifier: BindingModifier,
}

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

// Retained for testing a possible Project Sunrise PR; this may never become a supported feature.
const FIELD_OF_VIEW_EDITING_AVAILABLE: bool = false;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSchema {
    Version2,
    Version3,
}

impl SettingsSchema {
    fn from_document(document: &Value) -> Result<Self, String> {
        match schema_version(document) {
            Some(2) => Ok(Self::Version2),
            Some(3) => Ok(Self::Version3),
            Some(version) => Err(format!(
                "Project Sunrise settings schema version {version} has not been tested with this Sundial release"
            )),
            None => Err("Project Sunrise settings schema version is missing or invalid".into()),
        }
    }

    const fn key_binding_format(self) -> KeyBindingFormat {
        match self {
            Self::Version2 => KeyBindingFormat::Numeric,
            Self::Version3 => KeyBindingFormat::Named,
        }
    }
}

pub(super) fn schema_version(document: &Value) -> Option<u64> {
    document.get("version").and_then(Value::as_u64)
}

pub(super) fn future_schema_version(document: &Value) -> Option<u64> {
    schema_version(document).filter(|version| *version > 3)
}

fn key_bindings_editable(document: &Value) -> bool {
    schema_version(document) == Some(3)
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
    key_bindings: &mut KeyBindingUiState,
) -> bool {
    let bindings_editable = key_bindings_editable(document);
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
                "Edit named key bindings used by Sunrise schema 3."
            } else {
                "Key bindings are shown read-only for this settings schema."
            });
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
                draw_key_bindings(ui, settings, key_bindings, bindings_editable)
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
            ui.visuals().error_fg_color,
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
            ui.visuals().error_fg_color,
            "This settings.json has no steam.user.persona_name field.",
        );
        return false;
    };
    let Some(current) = value.as_str() else {
        ui.colored_label(
            ui.visuals().error_fg_color,
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
        ui.visuals().error_fg_color,
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
            if FIELD_OF_VIEW_EDITING_AVAILABLE && values.contains_key("field_of_view") {
                changed |= integer_slider(ui, values, "field_of_view", "Field of view", 55, 105);
            }
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
    settings: &mut Map<String, Value>,
    state: &mut KeyBindingUiState,
    editable: bool,
) -> bool {
    let Some(bindings) = settings
        .get_mut("key_bindings")
        .and_then(Value::as_object_mut)
    else {
        missing_group(ui, "key bindings");
        return false;
    };
    ui.heading("Key bindings (Experimental)");
    if editable {
        ui.label("Choose a primary and secondary input for each action. Changes apply after Destiny 2 is fully restarted.");
    } else {
        ui.label(
            "Guided editing is available for Sunrise schema 3. These bindings are shown read-only.",
        );
    }
    ui.add_space(6.0);
    ui.add(
        egui::TextEdit::singleline(&mut state.action_search)
            .hint_text("Search actions…")
            .desired_width(320.0),
    );
    ui.add_space(8.0);
    let needle = state.action_search.trim().to_lowercase();
    let mut changed = false;
    egui::Grid::new("game_key_bindings_grid")
        .num_columns(3)
        .spacing([18.0, 8.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Action");
            ui.strong("Primary");
            ui.strong("Secondary");
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
                let Some(binding) = bindings.get_mut(key).and_then(Value::as_object_mut) else {
                    ui.colored_label(ui.visuals().error_fg_color, "Missing");
                    ui.colored_label(ui.visuals().error_fg_color, "Missing");
                    ui.end_row();
                    continue;
                };
                if editable {
                    changed |=
                        binding_picker(ui, state, key, "primary", binding.get_mut("primary"));
                    changed |=
                        binding_picker(ui, state, key, "secondary", binding.get_mut("secondary"));
                } else {
                    binding_label(ui, binding.get("primary"));
                    binding_label(ui, binding.get("secondary"));
                }
                ui.end_row();
            }
            if visible == 0 {
                ui.label(egui::RichText::new("No matching actions").weak());
                ui.end_row();
            }
        });
    changed
}

fn binding_picker(
    ui: &mut egui::Ui,
    state: &mut KeyBindingUiState,
    action: &str,
    half: &str,
    value: Option<&mut Value>,
) -> bool {
    let Some(value) = value else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
        return false;
    };

    let (label, valid) = binding_value_label(value);
    let label = if valid {
        egui::RichText::new(label)
    } else {
        egui::RichText::new(label).color(ui.visuals().error_fg_color)
    };
    let popup_id = ui.make_persistent_id(("key-binding-picker", action, half));
    let button = ui.add_sized(
        [220.0, ui.spacing().interact_size.y],
        egui::Button::new(label),
    );
    if button.clicked() {
        state.picker = BindingPickerState {
            query: String::new(),
            modifier: value
                .as_str()
                .and_then(modified_input)
                .map_or(BindingModifier::None, |(modifier, _)| {
                    binding_modifier(modifier)
                }),
        };
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }

    let picker = &mut state.picker;
    let mut selection = None::<Option<String>>;
    egui::popup::popup_below_widget(
        ui,
        popup_id,
        &button,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(400.0);
            ui.label(egui::RichText::new("Modifier").strong());
            ui.horizontal_wrapped(|ui| {
                for (modifier, label) in BindingModifier::ALL {
                    ui.selectable_value(&mut picker.modifier, modifier, label);
                }
            });
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut picker.query)
                    .hint_text("Search key names…")
                    .desired_width(380.0),
            );
            ui.separator();

            let current = value.as_str().map(trim_input_name);
            let needle = picker.query.trim().to_lowercase();
            egui::ScrollArea::vertical()
                .min_scrolled_height(300.0)
                .max_height(400.0)
                .show(ui, |ui| {
                    if ui.selectable_label(value.is_null(), "Unassigned").clicked() {
                        selection = Some(None);
                    }
                    ui.separator();

                    let mut visible = 0usize;
                    for &key in NAMED_INPUTS {
                        let display = display_input_name(key);
                        if !needle.is_empty()
                            && !key.to_lowercase().contains(&needle)
                            && !display.to_lowercase().contains(&needle)
                        {
                            continue;
                        }
                        visible += 1;
                        let input = picker
                            .modifier
                            .input_name()
                            .map_or_else(|| key.to_owned(), |modifier| format!("{modifier}+{key}"));
                        debug_assert!(valid_named_input(&input));
                        if ui
                            .selectable_label(
                                current.is_some_and(|current| current.eq_ignore_ascii_case(&input)),
                                display,
                            )
                            .clicked()
                        {
                            selection = Some(Some(input));
                        }
                    }
                    if visible == 0 {
                        ui.label(egui::RichText::new("No matching keys found").weak());
                    }
                });
        },
    );

    let Some(selection) = selection else {
        return false;
    };
    let Ok(changed) = set_named_binding_value(value, selection.as_deref()) else {
        return false;
    };
    ui.memory_mut(egui::Memory::close_popup);
    changed
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
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
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
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
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
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
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
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
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
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
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
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
    }
    ui.end_row();
}

fn binding_label(ui: &mut egui::Ui, value: Option<&Value>) {
    let Some(value) = value else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
        return;
    };
    if value.is_null() {
        ui.label(egui::RichText::new("Unassigned").weak());
    } else if let Some(code) = value.as_u64() {
        ui.add_enabled(false, egui::Label::new(code.to_string()));
    } else if let Some(name) = value.as_str() {
        ui.add_enabled(false, egui::Label::new(name));
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
    }
}

pub(super) fn validate(document: &Value) -> Result<(), String> {
    let schema = SettingsSchema::from_document(document)?;
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
    if FIELD_OF_VIEW_EDITING_AVAILABLE {
        optional_range(display, "field_of_view", 55, 105)?;
    }
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

    validate_key_bindings(settings, schema)
}

fn validate_key_bindings(
    settings: &Map<String, Value>,
    schema: SettingsSchema,
) -> Result<(), String> {
    let bindings = group(settings, "key_bindings")?;
    let binding_format = schema.key_binding_format();
    for &(key, label) in ACTIONS {
        let binding = bindings
            .get(key)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("Key binding {label} must be an object"))?;
        input_code(binding, label, "primary", binding_format)?;
        input_code(binding, label, "secondary", binding_format)?;
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

fn optional_range(
    values: &Map<String, Value>,
    key: &str,
    minimum: u64,
    maximum: u64,
) -> Result<(), String> {
    if values.contains_key(key) {
        range(values, key, minimum, maximum)
    } else {
        Ok(())
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

// These are the decoded input names accepted by Sunrise schemas 3 (Project
// Sunrise 0.2 and 0.2.1). Sunrise's raw table contains both its backslash name
// and its JSON-escaped spelling; serde represents the usable value as one
// decoded backslash, leaving 120 logical choices here. Matching is ASCII
// case-insensitive, just like Sunrise.
const NAMED_INPUTS: &[&str; 120] = &[
    "escape",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "print screen",
    "scroll lock",
    "pause",
    "`",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "0",
    "-",
    "=",
    "backspace",
    "tab",
    "q",
    "w",
    "e",
    "r",
    "t",
    "y",
    "u",
    "i",
    "o",
    "p",
    "[",
    "]",
    r"\",
    "caps lock",
    "a",
    "s",
    "d",
    "f",
    "g",
    "h",
    "j",
    "k",
    "l",
    ";",
    "'",
    "return",
    "left shift",
    "z",
    "x",
    "c",
    "v",
    "b",
    "n",
    "m",
    ",",
    ".",
    "/",
    "right shift",
    "left control",
    "left windows",
    "left alt",
    "space",
    "right alt",
    "right windows",
    "menu",
    "right control",
    "up",
    "down",
    "left",
    "right",
    "insert",
    "home",
    "page up",
    "delete",
    "end",
    "page down",
    "num lock",
    "keypad /",
    "keypad *",
    "keypad 0",
    "keypad 1",
    "keypad 2",
    "keypad 3",
    "keypad 4",
    "keypad 5",
    "keypad 6",
    "keypad 7",
    "keypad 8",
    "keypad 9",
    "keypad -",
    "keypad +",
    "keypad enter",
    "keypad .",
    "<",
    "shift",
    "control",
    "key_windows",
    "alt",
    "left mouse button",
    "middle mouse button",
    "right mouse button",
    "extra mouse button 1",
    "extra mouse button 2",
    "mouse wheel up",
    "mouse wheel down",
    "unused",
    "ctrl",
    "left ctrl",
    "right ctrl",
];

const MODIFIER_INPUTS: &[&str; 12] = &[
    "left shift",
    "right shift",
    "shift",
    "left control",
    "right control",
    "control",
    "ctrl",
    "left ctrl",
    "right ctrl",
    "left alt",
    "right alt",
    "alt",
];

fn trim_input_name(name: &str) -> &str {
    name.trim_matches([' ', '\t'])
}

fn matches_input_name(candidate: &str, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| candidate.eq_ignore_ascii_case(name))
}

fn modified_input(name: &str) -> Option<(&str, &str)> {
    let name = trim_input_name(name);
    if name.is_empty() || matches_input_name(name, NAMED_INPUTS) {
        return None;
    }
    let separator = name.find(['+', '-'])?;
    let modifier = trim_input_name(&name[..separator]);
    let key = trim_input_name(&name[separator + 1..]);
    (matches_input_name(modifier, MODIFIER_INPUTS) && matches_input_name(key, NAMED_INPUTS))
        .then_some((modifier, key))
}

fn valid_named_input(name: &str) -> bool {
    let name = trim_input_name(name);
    !name.is_empty() && (matches_input_name(name, NAMED_INPUTS) || modified_input(name).is_some())
}

fn binding_modifier(name: &str) -> BindingModifier {
    if matches_input_name(name, &["left shift", "right shift", "shift"]) {
        BindingModifier::Shift
    } else if matches_input_name(
        name,
        &[
            "left control",
            "right control",
            "control",
            "ctrl",
            "left ctrl",
            "right ctrl",
        ],
    ) {
        BindingModifier::Control
    } else if matches_input_name(name, &["left alt", "right alt", "alt"]) {
        BindingModifier::Alt
    } else {
        BindingModifier::None
    }
}

fn display_input_part(name: &str) -> String {
    name.replace('_', " ")
        .split(' ')
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_input_name(name: &str) -> String {
    modified_input(name).map_or_else(
        || display_input_part(trim_input_name(name)),
        |(modifier, key)| {
            format!(
                "{} + {}",
                display_input_part(modifier),
                display_input_part(key)
            )
        },
    )
}

fn binding_value_label(value: &Value) -> (String, bool) {
    if value.is_null() {
        ("Unassigned".into(), true)
    } else if let Some(name) = value.as_str() {
        if valid_named_input(name) {
            (display_input_name(name), true)
        } else {
            (format!("Invalid: {name}"), false)
        }
    } else {
        ("Invalid value".into(), false)
    }
}

fn set_named_binding_value(value: &mut Value, input: Option<&str>) -> Result<bool, String> {
    if let Some(input) = input
        && !valid_named_input(input)
    {
        return Err(format!("Unsupported Sunrise key name: {input}"));
    }
    let replacement = input.map_or(Value::Null, |input| Value::String(input.into()));
    if *value == replacement {
        return Ok(false);
    }
    *value = replacement;
    Ok(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyBindingFormat {
    Numeric,
    Named,
}

fn input_code(
    binding: &Map<String, Value>,
    label: &str,
    half: &str,
    format: KeyBindingFormat,
) -> Result<(), String> {
    let Some(value) = binding.get(half) else {
        return Err(format!("Key binding {label} is missing its {half} value"));
    };
    if value.is_null() {
        return Ok(());
    }
    match format {
        KeyBindingFormat::Numeric
            if value
                .as_u64()
                .is_some_and(|code| u16::try_from(code).is_ok()) =>
        {
            Ok(())
        }
        KeyBindingFormat::Named if value.as_str().is_some_and(valid_named_input) => Ok(()),
        KeyBindingFormat::Numeric => Err(format!(
            "Key binding {label} {half} must be unassigned or between 0 and {} for Sunrise 0.1",
            u16::MAX
        )),
        KeyBindingFormat::Named => Err(format!(
            "Key binding {label} {half} must be unassigned, a recognized key name, or one modifier plus a key for Sunrise 0.2"
        )),
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

    #[test]
    fn key_binding_forms_follow_sunrise_schema_versions() {
        assert_eq!(
            SettingsSchema::Version2.key_binding_format(),
            KeyBindingFormat::Numeric
        );
        assert_eq!(
            SettingsSchema::Version3.key_binding_format(),
            KeyBindingFormat::Named
        );

        let numeric = serde_json::json!({"primary": 109, "secondary": null});
        let numeric = numeric.as_object().unwrap();
        assert_eq!(
            input_code(numeric, "Fire", "primary", KeyBindingFormat::Numeric),
            Ok(())
        );
        assert!(input_code(numeric, "Fire", "primary", KeyBindingFormat::Named).is_err());

        let named = serde_json::json!({"primary": "left mouse button", "secondary": null});
        let named = named.as_object().unwrap();
        assert_eq!(
            input_code(named, "Fire", "primary", KeyBindingFormat::Named),
            Ok(())
        );
        assert!(input_code(named, "Fire", "primary", KeyBindingFormat::Numeric).is_err());
        assert_eq!(
            input_code(named, "Fire", "secondary", KeyBindingFormat::Named),
            Ok(())
        );

        let numeric_max = serde_json::json!({"primary": 65535, "secondary": null});
        let numeric_max = numeric_max.as_object().unwrap();
        assert_eq!(
            input_code(numeric_max, "Fire", "primary", KeyBindingFormat::Numeric),
            Ok(())
        );
        let numeric_too_large = serde_json::json!({"primary": 65536, "secondary": null});
        assert!(
            input_code(
                numeric_too_large.as_object().unwrap(),
                "Fire",
                "primary",
                KeyBindingFormat::Numeric
            )
            .is_err()
        );
    }

    #[test]
    fn named_key_binding_validation_matches_sunrise() {
        for valid in [
            "left mouse button",
            "A",
            "\tCTRL + keypad -\t",
            "right alt-page down",
            r"\",
        ] {
            assert!(valid_named_input(valid), "expected {valid:?} to be valid");
        }

        for invalid in [
            "not-a-key",
            "left windows+a",
            "shift+ctrl+a",
            "shift+",
            "a+b",
            "\nA\n",
            r"\\",
        ] {
            assert!(
                !valid_named_input(invalid),
                "expected {invalid:?} to be invalid"
            );
        }

        let invalid = serde_json::json!({"primary": "not-a-key", "secondary": null});
        assert!(
            input_code(
                invalid.as_object().unwrap(),
                "Fire",
                "primary",
                KeyBindingFormat::Named
            )
            .is_err()
        );
    }

    #[test]
    fn key_binding_editing_is_schema_3_only() {
        assert!(!key_bindings_editable(&serde_json::json!({"version": 2})));
        assert!(key_bindings_editable(&serde_json::json!({"version": 3})));
        assert!(!key_bindings_editable(&serde_json::json!({"version": 4})));
        assert!(!key_bindings_editable(&serde_json::json!({})));
    }

    #[test]
    fn every_picker_choice_is_accepted_by_sunrise() {
        for &key in NAMED_INPUTS {
            assert!(valid_named_input(key), "direct key {key:?}");
            for modifier in ["shift", "control", "alt"] {
                let input = format!("{modifier}+{key}");
                assert!(valid_named_input(&input), "modified key {input:?}");
            }
        }
    }

    #[test]
    fn named_binding_edits_only_replace_the_selected_value() {
        let mut binding = serde_json::json!({
            "primary": "not-a-key",
            "secondary": null,
            "future_binding_data": { "keep": [1, 2, 3] }
        });

        let untouched = binding.clone();
        assert!(
            set_named_binding_value(binding.pointer_mut("/primary").unwrap(), Some("not-a-key"))
                .is_err()
        );
        assert_eq!(binding, untouched);

        assert_eq!(
            set_named_binding_value(binding.pointer_mut("/primary").unwrap(), Some("control+a")),
            Ok(true)
        );
        assert_eq!(
            binding,
            serde_json::json!({
                "primary": "control+a",
                "secondary": null,
                "future_binding_data": { "keep": [1, 2, 3] }
            })
        );
        assert_eq!(
            set_named_binding_value(binding.pointer_mut("/primary").unwrap(), Some("control+a")),
            Ok(false)
        );
        assert_eq!(
            set_named_binding_value(binding.pointer_mut("/primary").unwrap(), None),
            Ok(true)
        );
        assert!(binding.pointer("/primary").unwrap().is_null());
        assert_eq!(
            binding.pointer("/future_binding_data/keep"),
            Some(&serde_json::json!([1, 2, 3]))
        );
    }

    #[test]
    fn only_newer_schema_versions_require_a_confirmation() {
        assert_eq!(schema_version(&serde_json::json!({"version": 3})), Some(3));
        assert_eq!(schema_version(&serde_json::json!({"version": "3"})), None);
        assert_eq!(
            future_schema_version(&serde_json::json!({"version": 2})),
            None
        );
        assert_eq!(
            future_schema_version(&serde_json::json!({"version": 3})),
            None
        );
        assert_eq!(
            future_schema_version(&serde_json::json!({"version": 4})),
            Some(4)
        );
    }

    #[test]
    fn future_field_of_view_validation_accepts_stock_and_supported_values() {
        let stock = Map::new();
        assert_eq!(optional_range(&stock, "field_of_view", 55, 105), Ok(()));

        let patched = serde_json::json!({"field_of_view": 85});
        let patched = patched.as_object().unwrap();
        assert_eq!(optional_range(patched, "field_of_view", 55, 105), Ok(()));

        let invalid = serde_json::json!({"field_of_view": 106});
        let invalid = invalid.as_object().unwrap();
        assert!(optional_range(invalid, "field_of_view", 55, 105).is_err());
    }
}
