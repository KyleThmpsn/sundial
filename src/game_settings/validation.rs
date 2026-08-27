//! Sunrise-compatible validation for authored game-settings documents.

use serde_json::{Map, Value};

use super::{
    key_bindings::{ACTIONS, input_code},
    page::valid_orbit_slice_set,
    preferences::GAME_LANGUAGES,
    schema::{
        FIELD_OF_VIEW_KEY, KEY_BINDING_SOURCE_KEY, ORBIT_SLICE_SET_PATH, SettingsSchema,
        VERTICAL_SYNC_INTERVAL_KEY,
    },
};

pub(crate) fn validate(document: &Value) -> Result<(), String> {
    let schema = SettingsSchema::from_document(document)?;
    validate_game_language(document)?;
    validate_orbit_slice_set(document)?;
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
    optional_range(display, VERTICAL_SYNC_INTERVAL_KEY, 0, 4)?;
    optional_range(display, FIELD_OF_VIEW_KEY, 55, 105)?;
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

    optional_string_member(settings, KEY_BINDING_SOURCE_KEY, &["account", "computer"])?;
    validate_key_bindings(settings, schema)
}

pub(crate) fn validate_non_account(document: &Value) -> Result<(), String> {
    SettingsSchema::from_document(document)?;
    validate_game_language(document)?;
    validate_orbit_slice_set(document)
}

pub(super) fn validate_game_language(document: &Value) -> Result<(), String> {
    let Some(value) = document.pointer("/steam/language") else {
        return Ok(());
    };
    if value.as_str().is_some_and(|token| {
        GAME_LANGUAGES
            .iter()
            .any(|(candidate, _)| *candidate == token)
    }) {
        Ok(())
    } else {
        Err("steam.language must be one of Sunrise's supported language tokens".to_owned())
    }
}

pub(super) fn validate_orbit_slice_set(document: &Value) -> Result<(), String> {
    let Some(value) = document.pointer(ORBIT_SLICE_SET_PATH) else {
        return Ok(());
    };
    if value.as_str().is_some_and(valid_orbit_slice_set) {
        Ok(())
    } else {
        Err(
            "client.orbit_slice_set must contain at most 48 ASCII letters, numbers, or underscores"
                .to_owned(),
        )
    }
}

pub(super) fn validate_key_bindings(
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

pub(super) fn group<'a>(
    settings: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a Map<String, Value>, String> {
    settings
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("state.account.settings.{name} must be an object"))
}

pub(super) fn integer(values: &Map<String, Value>, key: &str) -> Result<u64, String> {
    values
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("Game setting {key} must be a non-negative whole number"))
}

pub(super) fn range(
    values: &Map<String, Value>,
    key: &str,
    minimum: u64,
    maximum: u64,
) -> Result<(), String> {
    let value = integer(values, key)?;
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "Game setting {key} must be between {minimum} and {maximum}"
        ))
    }
}

pub(super) fn optional_range(
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

pub(super) fn optional_string_member(
    values: &Map<String, Value>,
    key: &str,
    allowed: &[&str],
) -> Result<(), String> {
    let Some(value) = values.get(key) else {
        return Ok(());
    };
    if value.as_str().is_some_and(|value| allowed.contains(&value)) {
        Ok(())
    } else {
        Err(format!("Game setting {key} has an unsupported value"))
    }
}

pub(super) fn member(
    values: &Map<String, Value>,
    key: &str,
    allowed: &[u64],
) -> Result<(), String> {
    let value = integer(values, key)?;
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("Game setting {key} has an unsupported value"))
    }
}

pub(super) fn exact_integer(
    values: &Map<String, Value>,
    key: &str,
    expected: u64,
) -> Result<(), String> {
    let value = integer(values, key)?;
    if value == expected {
        Ok(())
    } else {
        Err(format!("Game setting {key} must remain {expected}"))
    }
}

pub(super) fn float_range(
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

pub(super) fn exact_float(
    values: &Map<String, Value>,
    key: &str,
    expected: f32,
) -> Result<(), String> {
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
pub(super) fn float32(values: &Map<String, Value>, key: &str) -> Result<f32, String> {
    let value = values
        .get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("Game setting {key} must be a number"))?;
    Ok(value)
}

pub(super) fn bool_fields(
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

// These are the decoded input names accepted by Sunrise schemas 3 through 8. Sunrise's raw table
// contains both its backslash name
// and its JSON-escaped spelling; serde represents the usable value as one
// decoded backslash, leaving 120 logical choices here. Matching is ASCII
// case-insensitive, just like Sunrise.
