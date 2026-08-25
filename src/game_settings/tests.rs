//! Cross-domain contract tests for game-settings editing and validation.

use super::*;
use super::{key_bindings::*, page::*, preferences::*, schema::*, validation::*, widgets::*};
use serde_json::{Map, Value};

fn valid_game_settings_document(version: u64) -> Value {
    let key_bindings = ACTIONS
        .iter()
        .map(|(key, _)| {
            (
                (*key).to_owned(),
                serde_json::json!({"primary": null, "secondary": null}),
            )
        })
        .collect::<Map<_, _>>();
    let mut document = serde_json::json!({
        "version": version,
        "state": {"account": {"settings": {
            "controls": {
                "button_layout": 0,
                "movement_mode": 0,
                "controller_look_sensitivity": 2,
                "controller_invert_vertical": false,
                "controller_auto_look_centering": false,
                "controller_vibration": true,
                "controller_swap_shoulders": false,
                "controller_invert_horizontal": false,
                "mouse_look_sensitivity": 15,
                "mouse_invert_vertical": false,
                "mouse_invert_horizontal": false,
                "unidentified_toggle": false,
                "mouse_aim_smoothing": false,
                "ads_sensitivity_modifier": 1.0,
                "double_press_delay": 0
            },
            "audio": {
                "voice_output_mode": 1,
                "team_voice_channel": 0,
                "reserved_mode": 0,
                "migration_version": 8,
                "chat_volume": 2,
                "mute_when_unfocused": false,
                "sound_effects_volume": 3,
                "dialogue_volume": 2,
                "music_volume": 2
            },
            "display": {
                "brightness": 3,
                "show_fps": false,
                "hdr_mode": 0,
                "calibration_primary": 10000.0,
                "calibration_alpha": 0.0
            },
            "interface": {
                "subtitles_mode": 0,
                "colorblind_mode": 0,
                "helmet_mode": 1,
                "hud_opacity": 3,
                "display_hints": true,
                "background_opacity": 2,
                "reticle_location": 0,
                "reticle_color": 0,
                "text_size": 0,
                "text_color": 0,
                "text_background_style": 0,
                "text_background_opacity": 0,
                "reserved_text_mode": 0,
                "subtitle_options_entry": 0
            },
            "social": {
                "prefer_good_connection": false,
                "text_chat_mode": 3,
                "show_real_names": true,
                "clan_invite_notifications": true,
                "profanity_filter": false,
                "voice_chat_enabled": true,
                "whisper_chat_mode": 0,
                "team_chat_join_mode": 0,
                "local_chat_join_mode": 0,
                "clan_chat_join_mode": 1,
                "chat_auto_hide_mode": 1
            },
            "key_bindings": {}
        }}}
    });
    *document
        .pointer_mut("/state/account/settings/key_bindings")
        .unwrap() = Value::Object(key_bindings);
    document
}

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
fn game_language_validation_matches_sunrise_tokens() {
    for &(token, _) in GAME_LANGUAGES {
        let document = serde_json::json!({"steam": {"language": token}});
        assert_eq!(validate_game_language(&document), Ok(()), "{token}");
    }

    assert_eq!(
        validate_game_language(&serde_json::json!({"steam": {}})),
        Ok(())
    );
    assert!(
        validate_game_language(&serde_json::json!({
            "steam": {"language": "unsupported"}
        }))
        .is_err()
    );
    assert!(validate_game_language(&serde_json::json!({"steam": {"language": 1}})).is_err());
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
    *expected.pointer_mut("/steam/user/persona_name").unwrap() = Value::String("Guardian".into());

    assert!(set_player_name(&mut document, "Guardian"));

    assert_eq!(document, expected);
}

#[test]
fn orbit_slice_set_is_only_edited_when_the_field_exists() {
    let mut unsupported = serde_json::json!({"client": {"future": true}});
    assert!(!set_existing_orbit_slice_set(
        &mut unsupported,
        "orbit_hiveship_d2"
    ));
    assert!(unsupported.pointer(ORBIT_SLICE_SET_PATH).is_none());

    let mut supported = serde_json::json!({
        "client": {
            "orbit_slice_set": "",
            "future": true
        }
    });
    assert!(set_existing_orbit_slice_set(
        &mut supported,
        "orbit_hiveship_d2"
    ));
    assert_eq!(
        supported.pointer(ORBIT_SLICE_SET_PATH),
        Some(&Value::String("orbit_hiveship_d2".into()))
    );
    assert_eq!(
        supported.pointer("/client/future"),
        Some(&Value::Bool(true))
    );
    assert!(!set_existing_orbit_slice_set(
        &mut supported,
        "orbit_hiveship_d2"
    ));
    assert!(!set_existing_orbit_slice_set(
        &mut supported,
        "orbit-hiveship-d2"
    ));
}

#[test]
fn key_binding_forms_follow_sunrise_schema_versions() {
    assert_eq!(
        SettingsSchema(2).key_binding_format(),
        KeyBindingFormat::Numeric
    );
    assert_eq!(
        SettingsSchema(3).key_binding_format(),
        KeyBindingFormat::Named
    );
    assert_eq!(
        SettingsSchema(6).key_binding_format(),
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
fn named_key_binding_editing_uses_the_last_known_format_for_future_schemas() {
    assert!(!key_bindings_editable(&serde_json::json!({"version": 2})));
    for version in 3..=6 {
        assert!(key_bindings_editable(
            &serde_json::json!({"version": version})
        ));
    }
    assert!(key_bindings_editable(&serde_json::json!({
        "version": MAX_SUPPORTED_SCHEMA + 1
    })));
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
fn future_schema_binding_edit_round_trip_preserves_unknown_actions_and_members() {
    const FIRE_PRIMARY_PATH: &str = "/state/account/settings/key_bindings/Fire/primary";
    const FIRE_FUTURE_DATA_PATH: &str =
        "/state/account/settings/key_bindings/Fire/future_binding_data/keep";
    const FUTURE_ACTION_DATA_PATH: &str =
        "/state/account/settings/key_bindings/FutureAction/opaque/keep";
    let future_version = MAX_SUPPORTED_SCHEMA + 117;
    let mut document = serde_json::json!({
        "version": future_version,
        "future_root_data": {"keep": true},
        "state": {
            "account": {
                "settings": {
                    "key_bindings": {
                        "Fire": {
                            "primary": "a",
                            "secondary": null,
                            "future_binding_data": {"keep": [1, 2, 3]}
                        },
                        "FutureAction": {
                            "opaque": {"keep": "unchanged"}
                        }
                    }
                }
            }
        }
    });

    assert!(SettingsSchema::from_document(&document).is_err());
    assert!(key_bindings_editable(&document));
    assert_eq!(
        set_named_binding_value(
            document.pointer_mut(FIRE_PRIMARY_PATH).unwrap(),
            Some("control+a"),
        ),
        Ok(true)
    );

    let encoded = serde_json::to_string(&document).unwrap();
    let reparsed: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        reparsed.pointer(FIRE_FUTURE_DATA_PATH),
        Some(&serde_json::json!([1, 2, 3]))
    );
    assert_eq!(
        reparsed.pointer(FUTURE_ACTION_DATA_PATH),
        Some(&Value::String("unchanged".into()))
    );
    assert_eq!(
        reparsed.pointer("/future_root_data/keep"),
        Some(&Value::Bool(true))
    );
    assert_eq!(reparsed, document);
}

#[test]
fn only_newer_schema_versions_require_a_confirmation() {
    let future_version = MAX_SUPPORTED_SCHEMA + 1;

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
        future_schema_version(&serde_json::json!({"version": 6})),
        None
    );
    assert_eq!(
        future_schema_version(&serde_json::json!({"version": future_version})),
        Some(future_version)
    );
}

#[test]
fn presence_gated_preferences_are_optional_but_validated_when_present() {
    let stock = Map::new();
    assert_eq!(optional_range(&stock, "field_of_view", 55, 105), Ok(()));
    assert_eq!(
        optional_range(&stock, "vertical_sync_interval", 0, 4),
        Ok(())
    );
    assert_eq!(
        optional_string_member(&stock, "key_binding_source", &["account", "computer"]),
        Ok(())
    );

    let patched = serde_json::json!({
        "field_of_view": 85,
        "vertical_sync_interval": 1,
        "key_binding_source": "computer"
    });
    let patched = patched.as_object().unwrap();
    assert_eq!(optional_range(patched, "field_of_view", 55, 105), Ok(()));
    assert_eq!(
        optional_range(patched, "vertical_sync_interval", 0, 4),
        Ok(())
    );
    assert_eq!(
        optional_string_member(patched, "key_binding_source", &["account", "computer"]),
        Ok(())
    );

    let invalid = serde_json::json!({
        "field_of_view": 106,
        "vertical_sync_interval": 5,
        "key_binding_source": "cloud"
    });
    let invalid = invalid.as_object().unwrap();
    assert!(optional_range(invalid, "field_of_view", 55, 105).is_err());
    assert!(optional_range(invalid, "vertical_sync_interval", 0, 4).is_err());
    assert!(
        optional_string_member(invalid, "key_binding_source", &["account", "computer"]).is_err()
    );
}

#[test]
fn schema_v8_materializes_missing_preferences_without_overwriting_values() {
    let mut document = valid_game_settings_document(8);
    assert!(ensure_schema_v8_preferences(&mut document));
    assert_eq!(
        document.pointer("/state/account/settings/display/field_of_view"),
        Some(&Value::from(85))
    );
    assert_eq!(
        document.pointer("/state/account/settings/display/vertical_sync_interval"),
        Some(&Value::from(0))
    );
    assert_eq!(
        document.pointer("/state/account/settings/key_binding_source"),
        Some(&Value::String("computer".into()))
    );

    *document
        .pointer_mut("/state/account/settings/display/field_of_view")
        .unwrap() = Value::from(100);
    *document
        .pointer_mut("/state/account/settings/display/vertical_sync_interval")
        .unwrap() = Value::from(2);
    *document
        .pointer_mut("/state/account/settings/key_binding_source")
        .unwrap() = Value::String("account".into());

    assert!(!ensure_schema_v8_preferences(&mut document));
    assert_eq!(
        document.pointer("/state/account/settings/display/field_of_view"),
        Some(&Value::from(100))
    );
    assert_eq!(
        document.pointer("/state/account/settings/display/vertical_sync_interval"),
        Some(&Value::from(2))
    );
    assert_eq!(
        document.pointer("/state/account/settings/key_binding_source"),
        Some(&Value::String("account".into()))
    );
}

#[test]
fn schema_v8_preferences_are_not_added_to_other_schemas() {
    for version in [6, 7, 9] {
        let mut document = valid_game_settings_document(version);
        assert!(!ensure_schema_v8_preferences(&mut document));
        assert!(
            document
                .pointer("/state/account/settings/display/field_of_view")
                .is_none()
        );
        assert!(
            document
                .pointer("/state/account/settings/display/vertical_sync_interval")
                .is_none()
        );
        assert!(
            document
                .pointer("/state/account/settings/key_binding_source")
                .is_none()
        );
    }
}

#[test]
fn vertical_sync_intervals_show_the_effective_frame_rate() {
    assert_eq!(nominal_refresh_rate_hz(119), 120);
    assert_eq!(
        vertical_sync_intervals(Some(120)),
        vec![
            (0, "Off (Default)".to_owned()),
            (1, "Every refresh (120 FPS)".to_owned()),
            (2, "Every 2 refreshes (60 FPS)".to_owned()),
            (3, "Every 3 refreshes (40 FPS)".to_owned()),
            (4, "Every 4 refreshes (30 FPS)".to_owned()),
        ]
    );
}

#[test]
fn schemas_two_through_eight_share_one_validated_policy() {
    for version in MIN_SUPPORTED_SCHEMA..=MAX_SUPPORTED_SCHEMA {
        assert_eq!(validate(&valid_game_settings_document(version)), Ok(()));
    }
    assert!(validate(&valid_game_settings_document(1)).is_err());
    assert!(validate(&valid_game_settings_document(MAX_SUPPORTED_SCHEMA + 1)).is_err());
}

#[test]
fn schema_six_accepts_presence_gated_preference_fields() {
    let mut document = valid_game_settings_document(6);
    let settings = document
        .pointer_mut("/state/account/settings")
        .and_then(Value::as_object_mut)
        .unwrap();
    settings.insert(
        "key_binding_source".into(),
        Value::String("computer".into()),
    );
    let display = settings
        .get_mut("display")
        .and_then(Value::as_object_mut)
        .unwrap();
    display.insert("vertical_sync_interval".into(), Value::from(1));
    display.insert("field_of_view".into(), Value::from(105));

    assert_eq!(validate(&document), Ok(()));
}
