//! Named and numeric key-binding models, pickers, labels, and validation primitives.

mod codes;

use eframe::egui;
use serde_json::{Map, Value};
use sundial_account::{
    AccountSettingKey, AccountSettingValue, AccountSettingsCommand, KeyBindingSlot,
};

use super::{
    page::missing_group,
    preferences::KEY_BINDING_SOURCES,
    schema::{KEY_BINDING_SOURCE_KEY, KeyBindingFormat, show_presence_gated_preference},
    widgets::{CommandBatch, account_string_choice},
};

#[derive(Default)]
pub(crate) struct KeyBindingUiState {
    action_search: String,
    picker: BindingPickerState,
}

impl KeyBindingUiState {
    pub(crate) fn clear_pickers(&mut self) {
        self.picker = BindingPickerState::default();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum BindingModifier {
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
pub(super) struct BindingPickerState {
    query: String,
    modifier: BindingModifier,
}

pub(super) const ACTIONS: &[(&str, &str)] = &[
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
        "Character Menu: Collections",
    ),
    ("ui_open_start_menu_clan_tab", "Character menu: Clan"),
    (
        "ui_open_start_menu_inventory_tab",
        "Character Menu: Inventory",
    ),
    (
        "ui_open_start_menu_settings_tab",
        "Character Menu: Settings",
    ),
    ("ui_open_exit_dialog_confirm", "Confirm exit dialog"),
    ("ui_abort_activity", "Abort activity"),
    ("ui_text_chat_toggle_state", "Toggle text chat"),
    ("screenshot", "Screenshot"),
];

fn binding_help(has_source_choice: bool) -> &'static str {
    if has_source_choice {
        "Choose a primary and secondary input for each action. With Binding Source set to Account, changes apply after Destiny 2 is fully restarted."
    } else {
        "Choose a primary and secondary input for each action. Save and fully restart Destiny 2 to apply changes."
    }
}

pub(super) fn draw_key_bindings(
    ui: &mut egui::Ui,
    settings: &Map<String, Value>,
    state: &mut KeyBindingUiState,
    editable: bool,
    numeric: bool,
) -> CommandBatch {
    let mut changed = CommandBatch::default();
    ui.horizontal(|ui| {
        ui.heading("Key Bindings");
        crate::ui_help::info(
            ui,
            if numeric || editable {
                binding_help(show_presence_gated_preference(
                    settings,
                    KEY_BINDING_SOURCE_KEY,
                ))
            } else {
                "These bindings are read-only for this settings schema."
            },
        );
    });
    ui.add_space(8.0);
    if show_presence_gated_preference(settings, KEY_BINDING_SOURCE_KEY) {
        egui::Grid::new("game_key_binding_source_grid")
            .num_columns(2)
            .spacing([18.0, 9.0])
            .striped(true)
            .show(ui, |ui| {
                changed |= account_string_choice(
                    ui,
                    settings,
                    KEY_BINDING_SOURCE_KEY,
                    "Binding Source",
                    KEY_BINDING_SOURCES,
                );
            });
        ui.label(
            "Account uses the bindings in the active account source. Computer leaves bindings under Destiny's control in cvars.xml.",
        );
        if settings.get(KEY_BINDING_SOURCE_KEY).and_then(Value::as_str) == Some("computer") {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Bindings edited here will not apply while the source is Computer. Switch Binding Source to Account to have Sunrise use them.",
            );
        }
        ui.add_space(8.0);
    }
    let Some(bindings) = settings.get("key_bindings").and_then(Value::as_object) else {
        missing_group(ui, "key bindings");
        return changed;
    };
    ui.add_space(6.0);
    ui.add(
        egui::TextEdit::singleline(&mut state.action_search)
            .hint_text("Search actions…")
            .desired_width(320.0),
    );
    ui.add_space(8.0);
    let needle = state.action_search.trim().to_lowercase();
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
                let Some(binding) = bindings.get(key).and_then(Value::as_object) else {
                    ui.colored_label(ui.visuals().error_fg_color, "Missing");
                    ui.colored_label(ui.visuals().error_fg_color, "Missing");
                    ui.end_row();
                    continue;
                };
                if numeric || editable {
                    changed |=
                        binding_picker(ui, state, key, "primary", binding.get("primary"), numeric);
                    changed |= binding_picker(
                        ui,
                        state,
                        key,
                        "secondary",
                        binding.get("secondary"),
                        numeric,
                    );
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

pub(super) fn binding_picker(
    ui: &mut egui::Ui,
    state: &mut KeyBindingUiState,
    action: &str,
    half: &str,
    value: Option<&Value>,
    numeric: bool,
) -> Option<AccountSettingsCommand> {
    let Some(original) = value else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
        return None;
    };

    let decoded = if numeric {
        original
            .as_u64()
            .and_then(codes::input_name)
            .map(Value::String)
    } else {
        None
    };
    let value = decoded.as_ref().unwrap_or(original);
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
                    for &key in if numeric {
                        &NAMED_INPUTS[..codes::INPUT_COUNT]
                    } else {
                        NAMED_INPUTS
                    } {
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

    let selection = selection?;
    let replacement = match selection.as_deref() {
        None => AccountSettingValue::Unassigned,
        Some(input) if numeric => AccountSettingValue::InputCode(codes::input_code(input)?),
        Some(input) => AccountSettingValue::text(input),
    };
    let unchanged = match &replacement {
        AccountSettingValue::Unassigned => original.is_null(),
        AccountSettingValue::Text(input) => original.as_str() == Some(input),
        AccountSettingValue::InputCode(code) => original.as_u64() == Some(u64::from(*code)),
        _ => false,
    };
    ui.memory_mut(egui::Memory::close_popup);
    (!unchanged).then(|| AccountSettingsCommand::Set {
        key: AccountSettingKey::key_binding(
            action,
            match half {
                "primary" => KeyBindingSlot::Primary,
                "secondary" => KeyBindingSlot::Secondary,
                _ => unreachable!("guided bindings only expose primary and secondary slots"),
            },
        ),
        value: replacement,
    })
}

pub(super) fn binding_label(ui: &mut egui::Ui, value: Option<&Value>) {
    let Some(value) = value else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
        return;
    };
    if value.is_null() {
        ui.label(egui::RichText::new("Unassigned").weak());
    } else if let Some(code) = value.as_u64() {
        ui.add_enabled(
            false,
            egui::Label::new(codes::input_name(code).map_or_else(
                || format!("Unknown Input ({code})"),
                |name| display_input_name(&name),
            )),
        );
    } else if let Some(name) = value.as_str() {
        ui.add_enabled(false, egui::Label::new(display_input_name(name)));
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
    }
}

pub(super) const NAMED_INPUTS: &[&str; 120] = &[
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

pub(super) const MODIFIER_INPUTS: &[&str; 12] = &[
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

pub(super) fn trim_input_name(name: &str) -> &str {
    name.trim_matches([' ', '\t'])
}

pub(super) fn matches_input_name(candidate: &str, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| candidate.eq_ignore_ascii_case(name))
}

pub(super) fn modified_input(name: &str) -> Option<(&str, &str)> {
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

pub(super) fn valid_named_input(name: &str) -> bool {
    sundial_account::is_valid_named_binding_input(name)
}

pub(super) fn binding_modifier(name: &str) -> BindingModifier {
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

pub(super) fn display_input_part(name: &str) -> String {
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

pub(super) fn display_input_name(name: &str) -> String {
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

pub(super) fn binding_value_label(value: &Value) -> (String, bool) {
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

#[cfg(test)]
pub(super) fn set_named_binding_value(
    value: &mut Value,
    input: Option<&str>,
) -> Result<bool, String> {
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

pub(super) fn input_code(
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
            "Key binding {label} {half} must be unassigned, a recognized key name, or one modifier plus a key for Sunrise's named-binding format"
        )),
    }
}
