use super::*;

#[test]
fn bundled_defaults_follow_the_json_and_sqlite_account_contracts() {
    let legacy = decode_settings_defaults(include_str!(
        "../../../../tests/fixtures/sunrise-v8-d0fe8886-defaults.json"
    ))
    .unwrap();
    assert!(
        legacy
            .pointer("/state/account/settings")
            .unwrap()
            .is_object()
    );
    let native = include_str!("../../../../tests/fixtures/sunrise-v18-169fd29-defaults.json");
    let mut parsed = decode_settings_defaults(native).unwrap();
    assert_eq!(parsed, serde_json::from_str::<Value>(native).unwrap());
    assert!(parsed.pointer("/state/account/settings").is_none());

    // Sunrise 0.5 kept schema 18 but dropped four controls, so the same branch has to accept a
    // document that simply omits them.
    let retired = include_str!("../../../../tests/fixtures/sunrise-v18-5e4cbc7-defaults.json");
    let shortened = decode_settings_defaults(retired).unwrap();
    assert_eq!(shortened, serde_json::from_str::<Value>(retired).unwrap());
    parsed["steam"]["language"] = Value::from("not-a-language");
    assert!(decode_settings_defaults(&parsed.to_string()).is_err());
}
