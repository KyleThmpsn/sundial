use super::*;

#[test]
fn account_resource_sets_select_v2_or_v3_and_require_starter_defaults() {
    for version in [2, 3] {
        let base = crate::persistence::sqlite_account::tests::default_resources();
        let v3 = crate::persistence::sqlite_account::tests::v3_resources();
        let selected = if version == 3 { &v3 } else { &base };
        let resources = std::collections::BTreeMap::from([
            (106, selected.schema.as_str()),
            (107, selected.rows.as_str()),
            (109, base.settings_schema.as_str()),
            (110, base.settings_rows.as_str()),
            (111, v3.starter.as_deref().expect("v3 starter defaults")),
        ]);
        let mut requested = Vec::new();
        let defaults = decode_account_defaults(&mut |id, _| {
            requested.push(id);
            Ok(resources[&id].to_owned())
        })
        .unwrap();
        assert_eq!(requested.contains(&111), version == 3);
        let dir = crate::test_support::TestDirectory::new("resource-defaults");
        let path = dir.0.join("investment.sqlite3");
        crate::persistence::sqlite_account::tests::versions::fixture(
            &path,
            if version == 3 { "v3-fresh" } else { "v2" },
        );
        crate::persistence::sqlite_account::ResetPlan::prepare(&path, &defaults)
            .unwrap()
            .apply()
            .unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            version
        );
        if version == 3 {
            assert_eq!(
                db.query_row("SELECT count(*) FROM starter_items", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                3
            );
            assert!(
                decode_account_defaults(&mut |id, _| if id == 111 {
                    Err("missing starter resource".into())
                } else {
                    Ok(resources[&id].into())
                })
                .is_err()
            );
        }
    }
}

#[test]
fn logging_off_and_trace_survive_loading_and_unrelated_edits() {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/sunrise-v18-169fd29-defaults.json"
    ))
    .unwrap();
    value["core"]["logging"]["levels"]["core"] = Value::from("trace");
    value["core"]["logging"]["levels"]["client"] = Value::from("off");
    let mut loaded = decode_settings_defaults(&value.to_string()).unwrap();
    loaded["unknown"] = Value::from("preserved");
    let saved = decode_settings_defaults(&loaded.to_string()).unwrap();
    assert_eq!(saved["core"]["logging"], value["core"]["logging"]);
    assert_eq!(saved["unknown"], "preserved");
}

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
