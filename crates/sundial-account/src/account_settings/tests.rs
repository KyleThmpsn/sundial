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
