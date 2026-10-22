use super::*;

fn capabilities() -> AccountSettingsCapabilities {
    AccountSettingsCapabilities {
        writable: true,
        named_key_bindings_writable: true,
        numeric_key_bindings_writable: false,
        extended_field_of_view: true,
    }
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
