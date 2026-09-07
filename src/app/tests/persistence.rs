use crate::app::settings::create_adjacent_backup;
use crate::app::settings::encode_settings;
use crate::app::settings::load_json;
use crate::app::settings::load_workspace_json;
use crate::app::settings::save_json_with_backup_root;
use crate::app::settings::settings_size_limit_for_schema;
use crate::app::settings::verify_source_unchanged;
use crate::app::settings::verify_workspace_source_unchanged;
use crate::app::*;
use crate::test_support::TestDirectory;
use std::fs;

#[test]
fn settings_encoder_matches_sunrise_array_formatting() {
    let document = serde_json::json!({
        "server": {
            "entitlements": [
                {"name": "1085660", "owned": "handle"},
                {"name": "STEAM_PAID_TIER", "owned": "application"}
            ]
        },
        "state": {
            "investment": {"pairs": [[1, 2], [3, 4]]},
            "unlocks": {"flags": [1, 2, 3]},
            "account": {
                "profile_items": [{"definition_hash": "0x1", "quantity": 1}],
                "settings": {"key_bindings": {
                    "fire": {"primary": "left mouse button", "secondary": null}
                }}
            },
            "characters": [{
                "equipment": {
                    "ghost": {"plugs": ["one", null, "three", "four"]},
                    "helmet": {"plugs": [
                        "0x11111111", "0x22222222", "0x33333333", "0x44444444",
                        "0x55555555", "0x66666666"
                    ]}
                }
            }]
        }
    });
    let encoded = encode_settings(&document).unwrap();
    assert!(encoded.contains("\"pairs\": [[1,2], [3,4]]"));
    assert!(encoded.contains("\"flags\": [1,2,3]"));
    assert!(encoded.contains("\"plugs\": [\"one\", null, \"three\", \"four\"]"));
    assert!(encoded.contains(
        "\"helmet\": {\r\n            \"plugs\": [\r\n              \"0x11111111\",\r\n              \"0x22222222\","
    ));
    assert!(encoded.contains(
        "\"entitlements\": [\r\n      { \"name\": \"1085660\", \"owned\": \"handle\" },"
    ));
    assert!(
        encoded.contains("\"fire\": { \"primary\": \"left mouse button\", \"secondary\": null }")
    );
    assert!(encoded.contains(
        "\"profile_items\": [\r\n      {\r\n      \"definition_hash\": \"0x1\",\r\n      \"quantity\": 1\r\n      }\r\n      ]"
    ));
    assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), document);
}

#[test]
fn settings_encoder_uses_standard_profile_item_indentation_from_schema_four() {
    let document = serde_json::json!({
        "version": 6,
        "state": {
            "account": {
                "profile_items": [{"definition_hash": "0x1", "quantity": 1}]
            }
        }
    });

    let encoded = encode_settings(&document).unwrap();
    assert!(encoded.contains(
        "\"profile_items\": [\r\n        {\r\n          \"definition_hash\": \"0x1\",\r\n          \"quantity\": 1\r\n        }\r\n      ]"
    ));
    assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), document);
}

#[test]
fn settings_saves_are_verified_and_each_keeps_its_own_backup() {
    let directory = TestDirectory::new("save");
    let settings = directory.0.join("settings.json");
    let backups = directory.0.join("backups");
    fs::write(&settings, b"{\"version\":0}\n").unwrap();

    let first_document = serde_json::json!({"version": 1, "values": [1, 2, 3]});
    let first_result = save_json_with_backup_root(&settings, &first_document, &backups).unwrap();
    let second_document = serde_json::json!({"version": 2, "values": [4, 5, 6]});
    let second_result = save_json_with_backup_root(&settings, &second_document, &backups).unwrap();

    assert_ne!(first_result.backup, second_result.backup);
    let source_directory = crate::backups::source_directory(&backups, &settings).unwrap();
    assert_eq!(
        first_result.backup.parent(),
        Some(source_directory.as_path())
    );
    assert_eq!(
        second_result.backup.parent(),
        Some(source_directory.as_path())
    );
    assert!(
        first_result
            .backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("settings-v0-")
    );
    assert!(
        second_result
            .backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("settings-v1-")
    );
    assert!(!first_result.compacted);
    assert_eq!(load_json(&settings).unwrap(), second_document);
    assert_eq!(
        load_json(&first_result.backup).unwrap(),
        serde_json::json!({"version": 0})
    );
    assert_eq!(load_json(&second_result.backup).unwrap(), first_document);
    assert!(fs::read_to_string(&settings).unwrap().ends_with('\n'));
}

#[test]
fn upstream_accounts_survive_edit_save_reload_and_backup_restore() {
    for fixture in [
        include_str!("../../../tests/fixtures/sunrise-v6-0.3.2-defaults.json"),
        include_str!("../../../tests/fixtures/sunrise-v13-a57dc9a9-defaults.json"),
    ] {
        let directory = TestDirectory::new("upstream-save-recovery");
        let settings = directory.0.join("settings.json");
        let backups = directory.0.join("backups");
        let mut original: Value = serde_json::from_str(fixture).unwrap();
        original["future_extension"] = serde_json::json!({
            "enabled": false,
            "nullable": null,
            "nested": [1, {"value": "retain exactly"}]
        });
        // Include the BOM accepted by Sunrise and retain the exact recovery bytes.
        let original_bytes = format!(
            "\u{feff}{}\n",
            serde_json::to_string_pretty(&original).unwrap()
        );
        fs::write(&settings, &original_bytes).unwrap();
        let mut edited = load_workspace_json(&settings).unwrap();
        edited["steam"]["user"]["persona_name"] = Value::String("Release check".into());
        verify_workspace_source_unchanged(&settings, &original, false).unwrap();

        let receipt = save_json_with_backup_root(&settings, &edited, &backups).unwrap();
        assert_eq!(
            fs::read(&receipt.backup).unwrap(),
            original_bytes.as_bytes()
        );
        assert_eq!(load_workspace_json(&settings).unwrap(), edited);
        assert_eq!(edited["version"], original["version"]);
        assert_eq!(edited["future_extension"], original["future_extension"]);
        assert!(verify_workspace_source_unchanged(&settings, &original, false).is_err());

        let restored = load_workspace_json(&receipt.backup).unwrap();
        let restore_receipt = save_json_with_backup_root(&settings, &restored, &backups).unwrap();
        assert_eq!(load_workspace_json(&settings).unwrap(), original);
        assert_eq!(
            load_workspace_json(&restore_receipt.backup).unwrap(),
            edited
        );
        assert_eq!(
            fs::read(&receipt.backup).unwrap(),
            original_bytes.as_bytes()
        );
    }
}

#[test]
fn backup_failure_leaves_settings_untouched() {
    let directory = TestDirectory::new("blocked-backup");
    let settings = directory.0.join("settings.json");
    let backups = directory.0.join("backups");
    let original = b"{\"version\":6,\"keep\":true}\n";
    fs::write(&settings, original).unwrap();
    fs::write(&backups, b"not a directory").unwrap();

    let result = save_json_with_backup_root(
        &settings,
        &serde_json::json!({"version": 6, "keep": false}),
        &backups,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&settings).unwrap(), original);
    assert_eq!(fs::read(&backups).unwrap(), b"not a directory");
}

#[test]
fn timestamped_backup_names_describe_the_source_schema() {
    let directory = TestDirectory::new("save");
    let settings = directory.0.join("settings.json");
    let backups = directory.0.join("backups");

    for (source, expected_prefix) in [
        (serde_json::json!({"version": 2}), "settings-v2-"),
        (serde_json::json!({"version": 3}), "settings-v3-"),
        (serde_json::json!({"version": 6}), "settings-v6-"),
        (serde_json::json!({"value": true}), "settings-v0-"),
    ] {
        fs::write(&settings, serde_json::to_vec(&source).unwrap()).unwrap();
        let result = save_json_with_backup_root(&settings, &source, &backups).unwrap();
        let file_name = result.backup.file_name().unwrap().to_string_lossy();
        let timestamp = file_name
            .strip_prefix(expected_prefix)
            .and_then(|name| name.strip_suffix(".json"))
            .unwrap_or_else(|| {
                panic!(
                    "{} did not match {expected_prefix}<timestamp>.json",
                    result.backup.display()
                )
            });
        assert!(
            !timestamp.is_empty() && timestamp.bytes().all(|byte| byte.is_ascii_digit()),
            "{} did not contain a numeric timestamp",
            result.backup.display()
        );
        assert_eq!(load_json(&result.backup).unwrap(), source);
    }
}

#[test]
fn unexpected_settings_get_an_exact_adjacent_backup_without_losing_an_older_one() {
    let directory = TestDirectory::new("save");
    let settings = directory.0.join("settings.json");
    let original = b"{\"unexpected\":1}\n";
    let newer = b"{\"unexpected\":2}\n";
    fs::write(&settings, original).unwrap();

    let adjacent = create_adjacent_backup(&settings).unwrap();
    assert_eq!(adjacent, directory.0.join("settings.json.bak"));
    assert_eq!(fs::read(&adjacent).unwrap(), original);

    fs::write(&settings, newer).unwrap();
    assert_eq!(create_adjacent_backup(&settings).unwrap(), adjacent);
    assert_eq!(fs::read(&adjacent).unwrap(), newer);
    let archived = fs::read_dir(&directory.0)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("settings.json.bak.previous-")
            })
        })
        .unwrap();
    assert_eq!(fs::read(archived).unwrap(), original);
}

#[test]
fn external_settings_changes_are_detected_before_saving() {
    let directory = TestDirectory::new("save");
    let settings = directory.0.join("settings.json");
    let loaded = serde_json::json!({"state": {"characters": [1, 2, 3]}});
    let newer = serde_json::json!({"state": {"characters": [1, 2, 3], "new": true}});
    fs::write(&settings, serde_json::to_vec(&loaded).unwrap()).unwrap();

    assert_eq!(verify_source_unchanged(&settings, &loaded), Ok(()));
    fs::write(&settings, serde_json::to_vec(&newer).unwrap()).unwrap();

    let error = verify_source_unchanged(&settings, &loaded).unwrap_err();
    assert!(error.contains("changed outside Sundial"));
    assert_eq!(load_json(&settings).unwrap(), newer);
}

#[test]
fn workspace_source_checks_normalize_only_for_json_account_mode() {
    let directory = TestDirectory::new("workspace-source-check");
    let settings = directory.0.join("settings.json");
    let raw = serde_json::json!({
        "version": 8,
        "state": {"account": {"settings": {"display": {}}}}
    });
    fs::write(&settings, serde_json::to_vec(&raw).unwrap()).unwrap();

    assert_eq!(load_workspace_json(&settings).unwrap(), raw);
    assert_eq!(
        verify_workspace_source_unchanged(&settings, &raw, false),
        Ok(())
    );
    assert!(verify_workspace_source_unchanged(&settings, &raw, true).is_err());
}

#[test]
fn readable_settings_over_the_limit_fall_back_to_compact_json() {
    let directory = TestDirectory::new("save");
    let settings = directory.0.join("settings.json");
    let backups = directory.0.join("backups");
    let original = b"{\"version\":0}\n";
    fs::write(&settings, original).unwrap();
    let document = serde_json::json!({"values": vec![0; 12_000]});

    let result = save_json_with_backup_root(&settings, &document, &backups).unwrap();
    let size_limit = settings_size_limit_for_schema(None);
    assert!(result.compacted);
    assert_eq!(result.size_limit_bytes, size_limit);
    assert!(result.encoded_bytes < size_limit);
    assert_eq!(load_json(&settings).unwrap(), document);
    assert_eq!(fs::read(&result.backup).unwrap(), original);
    assert_eq!(fs::read_to_string(&settings).unwrap().lines().count(), 1);
}

#[test]
fn compact_settings_over_the_limit_are_rejected_without_changing_the_source() {
    let directory = TestDirectory::new("save");
    let settings = directory.0.join("settings.json");
    let backups = directory.0.join("backups");
    let original = b"{\"version\":0}\n";
    fs::write(&settings, original).unwrap();
    let size_limit = settings_size_limit_for_schema(None);
    let document = Value::String("x".repeat(size_limit));

    let error = save_json_with_backup_root(&settings, &document, &backups)
        .err()
        .expect("oversize settings must be rejected");
    assert!(error.contains("after compaction"));
    assert!(error.contains(&format!("supported {size_limit}-byte Sunrise limit")));
    assert_eq!(fs::read(&settings).unwrap(), original);
    assert!(!backups.exists());
}

#[test]
fn settings_at_exactly_64_kib_do_not_trigger_compaction() {
    let directory = TestDirectory::new("save");
    let settings = directory.0.join("settings.json");
    let backups = directory.0.join("backups");
    fs::write(&settings, b"{}\n").unwrap();
    let size_limit = settings_size_limit_for_schema(None);
    // Two JSON quotes plus the trailing CRLF account for the four non-payload bytes.
    let document = Value::String("x".repeat(size_limit - 4));

    let result = save_json_with_backup_root(&settings, &document, &backups).unwrap();
    assert_eq!(result.encoded_bytes, size_limit);
    assert!(!result.compacted);
}

#[test]
fn settings_size_limits_follow_sunrise_schema_history() {
    const KIB: usize = 1024;
    assert_eq!(settings_size_limit_for_schema(None), 64 * KIB);
    for schema in 0..=3 {
        assert_eq!(settings_size_limit_for_schema(Some(schema)), 64 * KIB);
    }
    for schema in 4..=5 {
        assert_eq!(settings_size_limit_for_schema(Some(schema)), 128 * KIB);
    }
    for schema in 6..=crate::game_settings::MAX_SUPPORTED_SCHEMA {
        assert_eq!(settings_size_limit_for_schema(Some(schema)), 1024 * KIB);
    }
}

#[test]
fn schema_six_keeps_readable_json_above_the_legacy_limit() {
    let document = serde_json::json!({"version": 6, "values": vec![0; 12_000]});
    let prepared = prepare_settings(&document).unwrap();

    assert!(prepared.encoded_bytes > 64 * 1024);
    assert_eq!(prepared.size_limit_bytes, 1024 * 1024);
    assert!(!prepared.compacted);
}
