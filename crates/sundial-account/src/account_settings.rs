//! Storage-neutral account preferences and key-binding commands.

use std::collections::BTreeMap;

use crate::{AccountError, AccountResult};

/// Lowest field-of-view value accepted by current Project Sunrise account settings.
pub const FIELD_OF_VIEW_MINIMUM: u64 = 55;
/// Highest field-of-view value accepted by current Project Sunrise account settings.
pub const FIELD_OF_VIEW_MAXIMUM: u64 = 155;

/// Adapter-derived write capabilities for account settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountSettingsCapabilities {
    pub writable: bool,
    pub named_key_bindings_writable: bool,
    pub numeric_key_bindings_writable: bool,
    pub extended_field_of_view: bool,
}

/// A logical Project Sunrise account-settings group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AccountSettingGroup {
    Root,
    Controls,
    Audio,
    Display,
    Interface,
    Social,
}

/// One half of an account key binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyBindingSlot {
    Primary,
    Secondary,
}

/// A logical account setting. Persistence adapters map this key to their own layout.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AccountSettingKey {
    Preference {
        group: AccountSettingGroup,
        name: Box<str>,
    },
    KeyBinding {
        action: Box<str>,
        slot: KeyBindingSlot,
    },
}

impl AccountSettingKey {
    #[must_use]
    pub fn preference(group: AccountSettingGroup, name: impl Into<String>) -> Self {
        Self::Preference {
            group,
            name: name.into().into_boxed_str(),
        }
    }

    #[must_use]
    pub fn key_binding(action: impl Into<String>, slot: KeyBindingSlot) -> Self {
        Self::KeyBinding {
            action: action.into().into_boxed_str(),
            slot,
        }
    }

    #[must_use]
    pub fn known_preference(name: &str) -> Option<Self> {
        use AccountSettingGroup::{Audio, Controls, Display, Interface, Root, Social};

        let group = match name {
            "key_binding_source" => Root,
            "button_layout"
            | "movement_mode"
            | "controller_look_sensitivity"
            | "controller_invert_vertical"
            | "controller_auto_look_centering"
            | "controller_vibration"
            | "controller_swap_shoulders"
            | "controller_invert_horizontal"
            | "mouse_look_sensitivity"
            | "mouse_invert_vertical"
            | "mouse_invert_horizontal"
            | "unidentified_toggle"
            | "mouse_aim_smoothing"
            | "ads_sensitivity_modifier"
            | "double_press_delay" => Controls,
            "voice_output_mode"
            | "team_voice_channel"
            | "reserved_mode"
            | "chat_volume"
            | "mute_when_unfocused"
            | "sound_effects_volume"
            | "dialogue_volume"
            | "music_volume" => Audio,
            "brightness" | "show_fps" | "hdr_mode" | "vertical_sync_interval" | "field_of_view" => {
                Display
            }
            "subtitles_mode"
            | "colorblind_mode"
            | "helmet_mode"
            | "hud_opacity"
            | "display_hints"
            | "background_opacity"
            | "reticle_location"
            | "reticle_color"
            | "text_size"
            | "text_color"
            | "text_background_style"
            | "text_background_opacity" => Interface,
            "prefer_good_connection"
            | "text_chat_mode"
            | "show_real_names"
            | "clan_invite_notifications"
            | "profanity_filter"
            | "voice_chat_enabled"
            | "whisper_chat_mode"
            | "team_chat_join_mode"
            | "local_chat_join_mode"
            | "clan_chat_join_mode"
            | "chat_auto_hide_mode" => Social,
            _ => return None,
        };
        Some(Self::preference(group, name))
    }
}

/// A finite floating-point setting value with equality based on its exact representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FiniteF64(u64);

impl FiniteF64 {
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then(|| Self(value.to_bits()))
    }

    #[must_use]
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// A storage-neutral scalar used by editable account settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountSettingValue {
    Boolean(bool),
    Unsigned(u64),
    Decimal(FiniteF64),
    Text(Box<str>),
    /// Numeric input code stored by Sunrise, with at most one modifier flag.
    InputCode(u16),
    Unassigned,
}

impl AccountSettingValue {
    #[must_use]
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into().into_boxed_str())
    }
}

/// An atomic account-settings mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountSettingsCommand {
    Set {
        key: AccountSettingKey,
        value: AccountSettingValue,
    },
}

/// The settings selected by an adapter for one operation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountSettingsState {
    values: BTreeMap<AccountSettingKey, AccountSettingValue>,
}

impl AccountSettingsState {
    pub fn try_new(
        _capabilities: AccountSettingsCapabilities,
        values: BTreeMap<AccountSettingKey, AccountSettingValue>,
    ) -> AccountResult<Self> {
        for (key, value) in &values {
            validate_loaded_setting(key, value)?;
        }
        Ok(Self { values })
    }

    #[must_use]
    pub const fn values(&self) -> &BTreeMap<AccountSettingKey, AccountSettingValue> {
        &self.values
    }

    pub fn apply_all(
        &mut self,
        capabilities: AccountSettingsCapabilities,
        commands: impl IntoIterator<Item = AccountSettingsCommand>,
    ) -> AccountResult<()> {
        if !capabilities.writable {
            return Err(AccountError::AccountSettingsReadOnly);
        }
        let mut candidate = self.clone();
        for command in commands {
            candidate.apply_inner(capabilities, command)?;
        }
        *self = candidate;
        Ok(())
    }

    fn apply_inner(
        &mut self,
        capabilities: AccountSettingsCapabilities,
        command: AccountSettingsCommand,
    ) -> AccountResult<()> {
        match command {
            AccountSettingsCommand::Set { key, value } => {
                if !self.values.contains_key(&key) {
                    return Err(AccountError::AccountSettingNotLoaded);
                }
                if matches!(key, AccountSettingKey::KeyBinding { .. })
                    && !capabilities.named_key_bindings_writable
                    && !capabilities.numeric_key_bindings_writable
                {
                    return Err(AccountError::KeyBindingsReadOnly);
                }
                if !capabilities.extended_field_of_view
                    && matches!((&key, &value), (AccountSettingKey::Preference { group: AccountSettingGroup::Display, name }, AccountSettingValue::Unsigned(value)) if name.as_ref() == "field_of_view" && *value > 105)
                {
                    return Err(AccountError::InvalidAccountSettingValue);
                }
                if matches!(key, AccountSettingKey::KeyBinding { .. })
                    && capabilities.numeric_key_bindings_writable
                {
                    if !matches!(
                        value,
                        AccountSettingValue::InputCode(_) | AccountSettingValue::Unassigned
                    ) {
                        return Err(AccountError::InvalidAccountSettingValue);
                    }
                    validate_loaded_setting(&key, &value)?;
                } else {
                    validate_setting(&key, &value)?;
                }
                self.values.insert(key, value);
            }
        }
        Ok(())
    }
}

fn validate_setting(key: &AccountSettingKey, value: &AccountSettingValue) -> AccountResult<()> {
    match key {
        AccountSettingKey::Preference { group, name } => validate_preference(*group, name, value),
        AccountSettingKey::KeyBinding { action, .. } => {
            if !is_supported_key_binding_action(action) {
                return Err(AccountError::UnknownAccountSetting);
            }
            match value {
                AccountSettingValue::Unassigned => Ok(()),
                AccountSettingValue::Text(input) if is_valid_named_binding_input(input) => Ok(()),
                _ => Err(AccountError::InvalidAccountSettingValue),
            }
        }
    }
}

fn validate_loaded_setting(
    key: &AccountSettingKey,
    value: &AccountSettingValue,
) -> AccountResult<()> {
    match (key, value) {
        (AccountSettingKey::KeyBinding { action, .. }, AccountSettingValue::InputCode(code)) => {
            if !is_supported_key_binding_action(action) {
                return Err(AccountError::UnknownAccountSetting);
            }
            // Sunrise key_bindings.h: inputs 0..=0x73 with at most one native modifier.
            if code & 0x00ff <= 0x73 && matches!(code & 0xff00, 0 | 0x100 | 0x200 | 0x400) {
                Ok(())
            } else {
                Err(AccountError::InvalidAccountSettingValue)
            }
        }
        _ => validate_setting(key, value),
    }
}

fn validate_preference(
    group: AccountSettingGroup,
    name: &str,
    value: &AccountSettingValue,
) -> AccountResult<()> {
    use AccountSettingGroup::{Audio, Controls, Display, Interface, Root, Social};

    let valid = match (group, name, value) {
        (Root, "key_binding_source", AccountSettingValue::Text(value)) => {
            matches!(value.as_ref(), "account" | "computer")
        }

        (Controls, "button_layout", AccountSettingValue::Unsigned(value)) => {
            [0, 1, 2, 3, 5, 6, 9].contains(value)
        }
        (Controls, "movement_mode", AccountSettingValue::Unsigned(value)) => *value <= 3,
        (Controls, "controller_look_sensitivity", AccountSettingValue::Unsigned(value)) => {
            *value <= 9
        }
        (Controls, "mouse_look_sensitivity", AccountSettingValue::Unsigned(value)) => {
            (1..=100).contains(value)
        }
        (Controls, "double_press_delay", AccountSettingValue::Unsigned(value)) => *value <= 4,
        (Controls, "ads_sensitivity_modifier", AccountSettingValue::Decimal(value)) => {
            (0.5..=1.5).contains(&value.get())
        }
        (
            Controls,
            "controller_invert_vertical"
            | "controller_auto_look_centering"
            | "controller_vibration"
            | "controller_swap_shoulders"
            | "controller_invert_horizontal"
            | "mouse_invert_vertical"
            | "mouse_invert_horizontal"
            | "unidentified_toggle"
            | "mouse_aim_smoothing",
            AccountSettingValue::Boolean(_),
        ) => true,

        (Audio, "voice_output_mode", AccountSettingValue::Unsigned(value)) => *value <= 2,
        (Audio, "team_voice_channel" | "reserved_mode", AccountSettingValue::Unsigned(value)) => {
            *value <= 1
        }
        (Audio, "chat_volume", AccountSettingValue::Unsigned(value)) => *value <= 8,
        (
            Audio,
            "sound_effects_volume" | "dialogue_volume" | "music_volume",
            AccountSettingValue::Unsigned(value),
        ) => *value <= 10,
        (Audio, "mute_when_unfocused", AccountSettingValue::Boolean(_)) => true,

        (Display, "brightness", AccountSettingValue::Unsigned(value)) => *value <= 6,
        (Display, "show_fps", AccountSettingValue::Boolean(_)) => true,
        (Display, "hdr_mode", AccountSettingValue::Unsigned(value)) => *value <= 1,
        (Display, "vertical_sync_interval", AccountSettingValue::Unsigned(value)) => *value <= 4,
        (Display, "field_of_view", AccountSettingValue::Unsigned(value)) => {
            (FIELD_OF_VIEW_MINIMUM..=FIELD_OF_VIEW_MAXIMUM).contains(value)
        }

        (
            Interface,
            "subtitles_mode" | "colorblind_mode" | "hud_opacity" | "text_color",
            AccountSettingValue::Unsigned(value),
        ) => match name {
            "subtitles_mode" => *value <= 2,
            "colorblind_mode" | "hud_opacity" | "text_color" => *value <= 3,
            _ => false,
        },
        (Interface, "helmet_mode" | "reticle_location", AccountSettingValue::Unsigned(value)) => {
            *value <= 1
        }
        (
            Interface,
            "background_opacity" | "text_size" | "text_background_opacity",
            AccountSettingValue::Unsigned(value),
        ) => *value <= 4,
        (Interface, "reticle_color", AccountSettingValue::Unsigned(value)) => *value <= 6,
        (Interface, "text_background_style", AccountSettingValue::Unsigned(value)) => *value <= 3,
        (Interface, "display_hints", AccountSettingValue::Boolean(_)) => true,

        (
            Social,
            "prefer_good_connection"
            | "show_real_names"
            | "clan_invite_notifications"
            | "profanity_filter"
            | "voice_chat_enabled",
            AccountSettingValue::Boolean(_),
        ) => true,
        (Social, "text_chat_mode", AccountSettingValue::Unsigned(value)) => *value <= 3,
        (
            Social,
            "whisper_chat_mode"
            | "team_chat_join_mode"
            | "local_chat_join_mode"
            | "clan_chat_join_mode"
            | "chat_auto_hide_mode",
            AccountSettingValue::Unsigned(value),
        ) => *value <= 1,

        _ => return Err(AccountError::UnknownAccountSetting),
    };
    if valid {
        Ok(())
    } else {
        Err(AccountError::InvalidAccountSettingValue)
    }
}

/// Actions in native action-ID order, shared by validation and persistence.
pub const KEY_BINDING_ACTIONS: &[&str] = &[
    "fire",
    "toggle_zoom",
    "hold_zoom",
    "melee",
    "grenade",
    "super",
    "reload",
    "light_attack",
    "heavy_attack",
    "block",
    "switch_weapons",
    "next_weapon",
    "previous_weapon",
    "primary_weapon",
    "special_weapon",
    "heavy_weapon",
    "move_forward",
    "move_backward",
    "move_left",
    "move_right",
    "jump",
    "toggle_crouch",
    "hold_crouch",
    "toggle_sprint",
    "hold_sprint",
    "vehicle_boost",
    "vehicle_brake",
    "vehicle_zoom",
    "vehicle_fire_primary",
    "vehicle_fire_secondary",
    "vehicle_exit",
    "interact",
    "highlight_player",
    "emote_1",
    "emote_2",
    "emote_3",
    "emote_4",
    "air_move",
    "class_ability",
    "death_cam_zoom_in",
    "death_cam_zoom_out",
    "push_to_talk",
    "ui_gamepad_button_back",
    "ui_open_director",
    "ui_open_director_store_tab",
    "ui_open_director_pursuits_tab",
    "ui_open_director_map_tab",
    "ui_open_director_destinations_tab",
    "ui_open_director_roster_tab",
    "ui_open_director_seasons_tab",
    "ui_open_start_menu_alternative",
    "ui_open_start_menu_records_tab",
    "ui_open_start_menu_collections_tab",
    "ui_open_start_menu_clan_tab",
    "ui_open_start_menu_inventory_tab",
    "ui_open_start_menu_settings_tab",
    "ui_open_exit_dialog_confirm",
    "ui_abort_activity",
    "ui_text_chat_toggle_state",
    "screenshot",
];

#[must_use]
pub fn is_supported_key_binding_action(action: &str) -> bool {
    KEY_BINDING_ACTIONS.contains(&action)
}

const NAMED_INPUTS: &[&str] = &[
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

const MODIFIER_INPUTS: &[&str] = &[
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

fn matches_input_name(candidate: &str, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| candidate.eq_ignore_ascii_case(name))
}

#[must_use]
pub fn is_valid_named_binding_input(name: &str) -> bool {
    let name = name.trim_matches([' ', '\t']);
    if name.is_empty() || matches_input_name(name, NAMED_INPUTS) {
        return !name.is_empty();
    }
    let Some(separator) = name.find(['+', '-']) else {
        return false;
    };
    let modifier = name[..separator].trim_matches([' ', '\t']);
    let key = name[separator + 1..].trim_matches([' ', '\t']);
    matches_input_name(modifier, MODIFIER_INPUTS) && matches_input_name(key, NAMED_INPUTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> AccountSettingsCapabilities {
        AccountSettingsCapabilities {
            writable: true,
            named_key_bindings_writable: true,
            numeric_key_bindings_writable: false,
            extended_field_of_view: true,
        }
    }

    fn preference(group: AccountSettingGroup, name: &str) -> AccountSettingKey {
        AccountSettingKey::preference(group, name)
    }

    #[test]
    fn batches_are_validated_atomically() {
        let brightness = preference(AccountSettingGroup::Display, "brightness");
        let show_fps = preference(AccountSettingGroup::Display, "show_fps");
        let mut state = AccountSettingsState::try_new(
            capabilities(),
            BTreeMap::from([
                (brightness.clone(), AccountSettingValue::Unsigned(3)),
                (show_fps.clone(), AccountSettingValue::Boolean(false)),
            ]),
        )
        .unwrap();
        let before = state.clone();

        let error = state
            .apply_all(
                capabilities(),
                [
                    AccountSettingsCommand::Set {
                        key: show_fps,
                        value: AccountSettingValue::Boolean(true),
                    },
                    AccountSettingsCommand::Set {
                        key: brightness,
                        value: AccountSettingValue::Unsigned(7),
                    },
                ],
            )
            .unwrap_err();

        assert_eq!(error, AccountError::InvalidAccountSettingValue);
        assert_eq!(state, before);
    }

    #[test]
    fn unknown_and_unloaded_settings_are_distinct() {
        let known = preference(AccountSettingGroup::Display, "brightness");
        let unknown = preference(AccountSettingGroup::Display, "future_setting");
        assert_eq!(
            AccountSettingsState::try_new(
                capabilities(),
                BTreeMap::from([(unknown, AccountSettingValue::Unsigned(1))]),
            ),
            Err(AccountError::UnknownAccountSetting)
        );

        let mut state = AccountSettingsState::default();
        assert_eq!(
            state.apply_all(
                capabilities(),
                [AccountSettingsCommand::Set {
                    key: known,
                    value: AccountSettingValue::Unsigned(1),
                }],
            ),
            Err(AccountError::AccountSettingNotLoaded)
        );
    }

    #[test]
    fn named_bindings_validate_actions_inputs_and_capabilities() {
        let key = AccountSettingKey::key_binding("fire", KeyBindingSlot::Primary);
        let mut state = AccountSettingsState::try_new(
            capabilities(),
            BTreeMap::from([(key.clone(), AccountSettingValue::Unassigned)]),
        )
        .unwrap();

        state
            .apply_all(
                capabilities(),
                [AccountSettingsCommand::Set {
                    key: key.clone(),
                    value: AccountSettingValue::text("control+f"),
                }],
            )
            .unwrap();
        assert_eq!(
            state.apply_all(
                capabilities(),
                [AccountSettingsCommand::Set {
                    key: key.clone(),
                    value: AccountSettingValue::text("not-a-real-key"),
                }],
            ),
            Err(AccountError::InvalidAccountSettingValue)
        );

        let read_only = AccountSettingsCapabilities {
            named_key_bindings_writable: false,
            numeric_key_bindings_writable: false,
            ..capabilities()
        };
        assert_eq!(
            state.apply_all(
                read_only,
                [AccountSettingsCommand::Set {
                    key,
                    value: AccountSettingValue::Unassigned,
                }],
            ),
            Err(AccountError::KeyBindingsReadOnly)
        );
    }

    #[test]
    fn field_of_view_accepts_the_active_sunrise_upper_boundary() {
        let key = preference(AccountSettingGroup::Display, "field_of_view");
        let mut state = AccountSettingsState::try_new(
            capabilities(),
            BTreeMap::from([(
                key.clone(),
                AccountSettingValue::Unsigned(FIELD_OF_VIEW_MAXIMUM),
            )]),
        )
        .unwrap();

        assert_eq!(
            state.apply_all(
                capabilities(),
                [AccountSettingsCommand::Set {
                    key,
                    value: AccountSettingValue::Unsigned(FIELD_OF_VIEW_MAXIMUM + 1),
                }],
            ),
            Err(AccountError::InvalidAccountSettingValue)
        );
    }

    #[test]
    fn named_binding_capability_rejects_numeric_writes_and_unknown_actions() {
        let key = AccountSettingKey::key_binding("fire", KeyBindingSlot::Primary);
        let mut state = AccountSettingsState::try_new(
            capabilities(),
            BTreeMap::from([(key.clone(), AccountSettingValue::InputCode(42))]),
        )
        .unwrap();
        let before = state.clone();

        assert_eq!(
            state.apply_all(
                capabilities(),
                [AccountSettingsCommand::Set {
                    key,
                    value: AccountSettingValue::InputCode(43),
                }],
            ),
            Err(AccountError::InvalidAccountSettingValue)
        );
        assert_eq!(state, before);

        let unknown = AccountSettingKey::key_binding("future_action", KeyBindingSlot::Primary);
        assert_eq!(
            AccountSettingsState::try_new(
                capabilities(),
                BTreeMap::from([(unknown, AccountSettingValue::InputCode(42))]),
            ),
            Err(AccountError::UnknownAccountSetting)
        );
    }

    #[test]
    fn numeric_bindings_reject_the_sentinel_and_combined_modifiers_atomically() {
        let capabilities = AccountSettingsCapabilities {
            named_key_bindings_writable: false,
            numeric_key_bindings_writable: true,
            ..capabilities()
        };
        let key = AccountSettingKey::key_binding("fire", KeyBindingSlot::Primary);
        let mut state = AccountSettingsState::try_new(
            capabilities,
            BTreeMap::from([(key.clone(), AccountSettingValue::Unassigned)]),
        )
        .unwrap();
        for code in [0, 0x73, 0x100, 0x173, 0x200, 0x273, 0x400, 0x473] {
            state
                .apply_all(
                    capabilities,
                    [AccountSettingsCommand::Set {
                        key: key.clone(),
                        value: AccountSettingValue::InputCode(code),
                    }],
                )
                .unwrap();
        }
        for code in [0x74, 0x174, 0x274, 0x474, 0x300, 0x500, 0x600, 0xffff] {
            let before = state.clone();
            assert_eq!(
                state.apply_all(
                    capabilities,
                    [
                        AccountSettingsCommand::Set {
                            key: key.clone(),
                            value: AccountSettingValue::Unassigned
                        },
                        AccountSettingsCommand::Set {
                            key: key.clone(),
                            value: AccountSettingValue::InputCode(code)
                        },
                    ]
                ),
                Err(AccountError::InvalidAccountSettingValue)
            );
            assert_eq!(state, before);
        }
    }
}
