//! Application service for storage-neutral account-setting commands.

use serde_json::Value;
use sundial_account::AccountSettingsCommand;

use crate::persistence::json_account::JsonAccountSettingsAdapter;

pub(super) fn apply_commands(
    document: &mut Value,
    commands: Vec<AccountSettingsCommand>,
) -> Result<bool, String> {
    if commands.is_empty() {
        return Ok(false);
    }
    let adapter = JsonAccountSettingsAdapter::load_for_commands(document, &commands)
        .map_err(|error| error.to_string())?;
    let (_, candidate, changed) = adapter
        .apply(document, commands)
        .map_err(|error| error.to_string())?;
    if changed {
        *document = candidate;
    }
    Ok(changed)
}

#[cfg(test)]
mod legacy;

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};
    use sundial_account::{
        AccountSettingGroup, AccountSettingKey, AccountSettingValue, AccountSettingsCommand,
        FiniteF64, KeyBindingSlot,
    };

    use super::{apply_commands, legacy};
    use crate::game_settings::MAX_SUPPORTED_SCHEMA;

    fn set(key: AccountSettingKey, value: AccountSettingValue) -> AccountSettingsCommand {
        AccountSettingsCommand::Set { key, value }
    }

    fn document_for(key: &AccountSettingKey, version: u64) -> Value {
        let mut settings = Map::from_iter([("future_group".into(), json!({"keep": true}))]);
        legacy::insert_target(&mut settings, key, Value::Null);
        json!({
            "version": version,
            "state": {"account": {"settings": settings, "future_account": {"keep": true}}},
            "future_root": {"keep": true}
        })
    }

    fn preference(name: &str, value: AccountSettingValue) -> AccountSettingsCommand {
        set(
            AccountSettingKey::known_preference(name)
                .expect("test cases contain registered preferences"),
            value,
        )
    }

    fn preference_cases() -> Vec<AccountSettingsCommand> {
        use AccountSettingValue::{Boolean, Decimal, Unsigned};

        vec![
            preference("key_binding_source", AccountSettingValue::text("account")),
            preference("button_layout", Unsigned(9)),
            preference("movement_mode", Unsigned(3)),
            preference("controller_look_sensitivity", Unsigned(9)),
            preference("controller_invert_vertical", Boolean(true)),
            preference("controller_auto_look_centering", Boolean(true)),
            preference("controller_vibration", Boolean(true)),
            preference("controller_swap_shoulders", Boolean(true)),
            preference("controller_invert_horizontal", Boolean(true)),
            preference("mouse_look_sensitivity", Unsigned(100)),
            preference("mouse_invert_vertical", Boolean(true)),
            preference("mouse_invert_horizontal", Boolean(true)),
            preference("unidentified_toggle", Boolean(true)),
            preference("mouse_aim_smoothing", Boolean(true)),
            preference(
                "ads_sensitivity_modifier",
                Decimal(FiniteF64::new(1.5).unwrap()),
            ),
            preference("double_press_delay", Unsigned(4)),
            preference("voice_output_mode", Unsigned(2)),
            preference("team_voice_channel", Unsigned(1)),
            preference("reserved_mode", Unsigned(1)),
            preference("chat_volume", Unsigned(8)),
            preference("mute_when_unfocused", Boolean(true)),
            preference("sound_effects_volume", Unsigned(10)),
            preference("dialogue_volume", Unsigned(10)),
            preference("music_volume", Unsigned(10)),
            preference("brightness", Unsigned(6)),
            preference("show_fps", Boolean(true)),
            preference("hdr_mode", Unsigned(1)),
            preference("vertical_sync_interval", Unsigned(4)),
            preference("field_of_view", Unsigned(105)),
            preference("subtitles_mode", Unsigned(2)),
            preference("colorblind_mode", Unsigned(3)),
            preference("helmet_mode", Unsigned(1)),
            preference("hud_opacity", Unsigned(3)),
            preference("display_hints", Boolean(true)),
            preference("background_opacity", Unsigned(4)),
            preference("reticle_location", Unsigned(1)),
            preference("reticle_color", Unsigned(6)),
            preference("text_size", Unsigned(4)),
            preference("text_color", Unsigned(3)),
            preference("text_background_style", Unsigned(3)),
            preference("text_background_opacity", Unsigned(4)),
            preference("prefer_good_connection", Boolean(true)),
            preference("text_chat_mode", Unsigned(3)),
            preference("show_real_names", Boolean(true)),
            preference("clan_invite_notifications", Boolean(true)),
            preference("profanity_filter", Boolean(true)),
            preference("voice_chat_enabled", Boolean(true)),
            preference("whisper_chat_mode", Unsigned(1)),
            preference("team_chat_join_mode", Unsigned(1)),
            preference("local_chat_join_mode", Unsigned(1)),
            preference("clan_chat_join_mode", Unsigned(1)),
            preference("chat_auto_hide_mode", Unsigned(1)),
        ]
    }

    #[test]
    fn production_preference_writes_match_the_frozen_json_behavior() {
        for command in preference_cases() {
            let AccountSettingsCommand::Set { key, .. } = &command;
            let key = key.clone();
            let mut production = document_for(&key, 8);
            let mut expected = production.clone();

            assert!(legacy::apply_commands(&mut expected, vec![command.clone()]).unwrap());
            assert!(apply_commands(&mut production, vec![command]).unwrap());
            assert_eq!(production, expected, "parity failed for {key:?}");
            assert_eq!(
                production.pointer("/state/account/future_account/keep"),
                Some(&Value::Bool(true))
            );
            assert_eq!(
                production.pointer("/future_root/keep"),
                Some(&Value::Bool(true))
            );
        }
    }

    #[test]
    fn production_named_binding_writes_match_legacy_and_preserve_siblings() {
        for (slot, value) in [
            (
                KeyBindingSlot::Primary,
                AccountSettingValue::text("control+f"),
            ),
            (
                KeyBindingSlot::Secondary,
                AccountSettingValue::text("space"),
            ),
        ] {
            for version in [MAX_SUPPORTED_SCHEMA, MAX_SUPPORTED_SCHEMA + 1] {
                let key = AccountSettingKey::key_binding("fire", slot);
                let command = set(key.clone(), value.clone());
                let mut production = document_for(&key, version);
                production
                    .pointer_mut("/state/account/settings/key_bindings/fire")
                    .and_then(Value::as_object_mut)
                    .unwrap()
                    .insert("future".into(), json!({"keep": true}));
                let mut expected = production.clone();

                legacy::apply_commands(&mut expected, vec![command.clone()]).unwrap();
                apply_commands(&mut production, vec![command]).unwrap();

                assert_eq!(production, expected, "schema {version}");
                assert_eq!(
                    production.pointer("/state/account/settings/key_bindings/fire/future/keep"),
                    Some(&Value::Bool(true)),
                    "schema {version}"
                );
            }
        }
    }

    #[test]
    fn invalid_batches_and_missing_targets_leave_the_document_untouched() {
        let brightness = AccountSettingKey::preference(AccountSettingGroup::Display, "brightness");
        let mut document = document_for(&brightness, 8);
        let before = document.clone();
        let error = apply_commands(
            &mut document,
            vec![
                set(brightness.clone(), AccountSettingValue::Unsigned(2)),
                set(brightness.clone(), AccountSettingValue::Unsigned(7)),
            ],
        )
        .unwrap_err();
        assert!(error.contains("invalid"));
        assert_eq!(document, before);

        let mut missing = json!({
            "version": 8,
            "state": {"account": {"settings": {"display": {}}}}
        });
        let before = missing.clone();
        let error = apply_commands(
            &mut missing,
            vec![set(brightness, AccountSettingValue::Unsigned(2))],
        )
        .unwrap_err();
        assert!(error.contains("brightness"));
        assert_eq!(missing, before);
    }

    #[test]
    fn json_field_of_view_respects_legacy_and_current_schema_limits() {
        let key = AccountSettingKey::known_preference("field_of_view").unwrap();
        for version in [6, 8, 13, MAX_SUPPORTED_SCHEMA] {
            let maximum = if version >= 16 { 155 } else { 105 };
            let mut document = document_for(&key, version);

            assert!(
                apply_commands(
                    &mut document,
                    vec![set(key.clone(), AccountSettingValue::Unsigned(maximum))],
                )
                .unwrap(),
                "schema {version}"
            );
            assert_eq!(
                document.pointer("/state/account/settings/display/field_of_view"),
                Some(&Value::from(maximum)),
                "schema {version}"
            );

            let before = document.clone();
            assert!(
                apply_commands(
                    &mut document,
                    vec![set(key.clone(), AccountSettingValue::Unsigned(maximum + 1))],
                )
                .is_err(),
                "schema {version}"
            );
            assert_eq!(document, before, "schema {version}");
        }
    }

    #[test]
    fn schema_two_named_bindings_remain_read_only() {
        let key = AccountSettingKey::key_binding("fire", KeyBindingSlot::Primary);
        let mut document = document_for(&key, 2);
        let before = document.clone();

        let error = apply_commands(
            &mut document,
            vec![set(key, AccountSettingValue::text("space"))],
        )
        .unwrap_err();

        assert_eq!(error, "key bindings are read-only");
        assert_eq!(document, before);
    }

    #[test]
    fn real_release_preferences_match_legacy_preserve_unknown_data_and_roundtrip() {
        for fixture in [
            include_str!("../../tests/fixtures/sunrise-v6-4aebb148-defaults.json"),
            include_str!("../../tests/fixtures/sunrise-v13-a57dc9a9-defaults.json"),
            include_str!("../../tests/fixtures/sunrise-v16-1120748-defaults.json"),
        ] {
            let mut original: Value = serde_json::from_str(fixture).unwrap();
            original["state"]["account"]["settings"]["future_settings"] =
                json!({"keep":[null,false,"玩家"]});
            let mut accepted = 0;
            for command in preference_cases() {
                let mut actual = original.clone();
                let mut expected = original.clone();
                let result = apply_commands(&mut actual, vec![command.clone()]);
                let legacy_result = legacy::apply_commands(&mut expected, vec![command.clone()]);
                assert_eq!(result.is_ok(), legacy_result.is_ok(), "{command:?}");
                assert_eq!(
                    actual, expected,
                    "schema {}: {command:?}",
                    original["version"]
                );
                if let Err(error) = result {
                    assert!(error.contains("missing"), "{error}: {command:?}");
                    assert_eq!(actual, original);
                    continue;
                }
                accepted += 1;
                crate::app::settings::validate_document(&actual).unwrap();
                let encoded = crate::app::settings::encode_settings(&actual).unwrap();
                assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), actual);
                actual["state"]["account"]["settings"] =
                    original["state"]["account"]["settings"].clone();
                assert_eq!(actual, original);
            }
            assert!(
                accepted > 30,
                "schema {} only exercised {accepted} controls",
                original["version"]
            );
        }
    }

    #[test]
    fn real_release_key_bindings_and_failed_batches_preserve_the_other_slot() {
        for fixture in [
            include_str!("../../tests/fixtures/sunrise-v6-4aebb148-defaults.json"),
            include_str!("../../tests/fixtures/sunrise-v16-1120748-defaults.json"),
        ] {
            let mut original: Value = serde_json::from_str(fixture).unwrap();
            original["state"]["account"]["settings"]["key_bindings"]["fire"]["future"] =
                json!({"keep":true});
            for slot in [KeyBindingSlot::Primary, KeyBindingSlot::Secondary] {
                for value in [
                    AccountSettingValue::text("control+f"),
                    AccountSettingValue::text("space"),
                    AccountSettingValue::Unassigned,
                ] {
                    let mut actual = original.clone();
                    let key = AccountSettingKey::key_binding("fire", slot);
                    let command = set(key.clone(), value);
                    apply_commands(&mut actual, vec![command.clone()]).unwrap();
                    let mut expected = original.clone();
                    legacy::apply_commands(&mut expected, vec![command.clone()]).unwrap();
                    assert_eq!(actual, expected);
                    crate::app::settings::validate_document(&actual).unwrap();
                    let before = actual.clone();
                    assert!(
                        apply_commands(
                            &mut actual,
                            vec![
                                command,
                                set(key, AccountSettingValue::text("definitely not a valid key"))
                            ]
                        )
                        .is_err()
                    );
                    assert_eq!(actual, before);
                }
            }
        }
    }
}
