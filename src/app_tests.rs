use std::sync::atomic::{AtomicU64, Ordering};

use crate::catalog;

use super::*;

static NEXT_TEST_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "sundial-save-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_TEST_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn check_mode_rejects_documents_that_the_gui_loads_with_a_warning() {
    for document in [
        serde_json::json!({"version": 3}),
        serde_json::json!({"version": 4}),
    ] {
        let error = validate_for_check(&document).unwrap_err();
        assert!(error.starts_with("Invalid settings: "));
    }
}

#[test]
fn hashes_are_strict_hex_and_normalized() {
    assert_eq!(parse_hash("0xE516CF40"), Some(0xE516_CF40));
    assert_eq!(parse_hash("0Xe516cf40"), Some(0xE516_CF40));
    assert_eq!(format_hash(0x123), "0x00000123");
    assert_eq!(parse_hash("E516CF40"), None);
    assert_eq!(parse_hash("0xnope"), None);
    assert_eq!(parse_unsigned_value(&Value::from(42)), Some(42));
    assert_eq!(
        parse_unsigned_value(&Value::String("0x0000002A".into())),
        Some(42)
    );
}

#[test]
fn sunrise_native_plugs_are_displayed_and_materialized_on_edit() {
    let defaults = vec![Some("0x0000002A".into()), None, Some("0x0000002B".into())];
    let mut plugs = Value::Null;

    let (displayed, native_defaults) = displayed_plugs(Some(&plugs), &defaults);
    assert!(native_defaults);
    assert_eq!(
        displayed,
        serde_json::json!(["0x0000002A", null, "0x0000002B"])
            .as_array()
            .unwrap()
            .clone()
    );

    let authored = materialize_authored_plugs(&mut plugs, &defaults).unwrap();
    authored[1] = Value::String("0x0000002C".into());
    assert_eq!(
        plugs,
        serde_json::json!(["0x0000002A", "0x0000002C", "0x0000002B"])
    );
}

#[test]
fn weapon_slots_can_be_emptied_and_equipped_again() {
    let mut document = serde_json::json!({
        "state": {
            "characters": [{
                "soid": 1,
                "equipment": {
                    "kinetic": {
                        "instance_soid": "0x4000000000000001",
                        "definition_hash": "0x0000002A",
                        "level": 67,
                        "quantity": 1,
                        "plugs": [],
                        "preserved_until_emptied": true
                    },
                    "energy": {
                        "instance_soid": "0x4000000000000002",
                        "definition_hash": "0x0000002B",
                        "level": 67,
                        "quantity": 1,
                        "plugs": []
                    }
                },
                "untouched": "kept"
            }]
        }
    });

    set_weapon_slot_empty(&mut document, 0, "kinetic").unwrap();
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic"),
        Some(&Value::Null)
    );
    assert_eq!(
        document.pointer("/state/characters/0/untouched"),
        Some(&Value::String("kept".into()))
    );

    let defaults = vec![Some("0x00000030".into()), None];
    equip_definition(&mut document, 0, "kinetic", 0x2C, &defaults).unwrap();
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic"),
        Some(&serde_json::json!({
            "instance_soid": "0x4000000000000001",
            "definition_hash": "0x0000002C",
            "level": 67,
            "quantity": 1,
            "plugs": ["0x00000030", null]
        }))
    );
    assert_eq!(validate_characters(&document), Ok(()));
}

#[test]
fn empty_weapon_action_never_overwrites_unexpected_slot_data() {
    let mut document = serde_json::json!({
        "state": { "characters": [{ "equipment": { "kinetic": "unexpected" } }] }
    });

    assert!(set_weapon_slot_empty(&mut document, 0, "kinetic").is_err());
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic"),
        Some(&Value::String("unexpected".into()))
    );
    assert!(set_weapon_slot_empty(&mut document, 0, "helmet").is_err());
}

#[test]
fn character_validation_accepts_sunrise_native_forms() {
    let document = serde_json::json!({
        "state": {
            "characters": [{
                "soid": 1,
                "level": 67,
                "equipment": {
                    "kinetic": {
                        "instance_soid": "0x0000000000000002",
                        "definition_hash": 42,
                        "level": 106,
                        "quantity": 1,
                        "plugs": null
                    },
                    "energy": {
                        "instance_soid": 3,
                        "definition_hash": "0x0000002B",
                        "level": 106,
                        "quantity": 1,
                        "plugs": [null, 44, "0x0000002D"]
                    },
                    "heavy": null
                }
            }]
        }
    });

    assert_eq!(validate_characters(&document), Ok(()));
}

#[test]
fn all_shadowkeep_ability_combinations_validate() {
    let subclasses = [
        (0xB055_4739_u64, 20),
        (0xB920_CE9A, 20),
        (0xC99B_33E9, 10),
        (0xD8B8_D1FC, 20),
        (0x4F91_DC97, 10),
        (0xC048_3D8B, 20),
        (0xCF88_FEA5, 20),
        (0x686A_154A, 20),
        (0xE7BC_88B0, 20),
    ];

    for (subclass_hash, middle_super) in subclasses {
        for movement in 4..=6 {
            for grenade in 7..=9 {
                for class_ability in 2..=3 {
                    for (super_ability, melee_ability) in [(10, 11), (10, 15), (middle_super, 21)] {
                        let document = character_with_abilities(
                            subclass_hash,
                            movement,
                            grenade,
                            super_ability,
                            melee_ability,
                            class_ability,
                        );
                        assert_eq!(validate_characters(&document), Ok(()));
                    }
                }
            }
        }
    }
}

#[test]
fn guard_subclasses_reject_entry_twenty_as_the_super() {
    for subclass_hash in [0x4F91_DC97, 0xC99B_33E9] {
        let document = character_with_abilities(subclass_hash, 6, 8, 20, 21, 3);
        assert!(validate_characters(&document).is_err());
        let warning = document
            .pointer("/state/characters/0")
            .and_then(Value::as_object)
            .and_then(character_ability_issue)
            .unwrap();
        assert!(warning.contains("unsupported super and melee combination (20/21)"));
        assert!(warning.contains("expected 10/11, 10/15, or 10/21"));
    }
}

#[test]
fn every_known_super_and_melee_pair_is_valid_after_save_repair() {
    let subclasses = [
        (0xB055_4739_u64, 20),
        (0xB920_CE9A, 20),
        (0xC99B_33E9, 10),
        (0xD8B8_D1FC, 20),
        (0x4F91_DC97, 10),
        (0xC048_3D8B, 20),
        (0xCF88_FEA5, 20),
        (0x686A_154A, 20),
        (0xE7BC_88B0, 20),
    ];

    for (subclass_hash, middle_super) in subclasses {
        let supported = [(10, 11), (10, 15), (middle_super, 21)];
        for super_ability in 0..=63 {
            for melee_ability in 0..=63 {
                let mut document =
                    character_with_abilities(subclass_hash, 6, 8, super_ability, melee_ability, 3);
                document["future_data"] = serde_json::json!({"keep": true});

                let repaired = repair_known_ability_pairs(&mut document);
                let was_supported = supported.contains(&(super_ability, melee_ability));
                assert_eq!(repaired, usize::from(!was_supported));
                assert_eq!(validate_characters(&document), Ok(()));
                assert_eq!(
                    document.pointer("/future_data/keep"),
                    Some(&Value::Bool(true))
                );
            }
        }
    }
}

#[test]
fn unknown_subclasses_keep_loose_ability_validation() {
    let mut document = character_with_abilities(0x1234_5678, 12, 13, 14, 15, 16);
    assert_eq!(validate_characters(&document), Ok(()));
    let original = document.clone();
    assert_eq!(repair_known_ability_pairs(&mut document), 0);
    assert_eq!(document, original);
}

fn character_with_abilities(
    subclass_hash: u64,
    movement: u64,
    grenade: u64,
    super_ability: u64,
    melee: u64,
    class_ability: u64,
) -> Value {
    serde_json::json!({
        "state": {
            "characters": [{
                "soid": "0x1",
                "movement_ability": movement,
                "grenade_ability": grenade,
                "super_ability": super_ability,
                "melee_ability": melee,
                "class_ability": class_ability,
                "equipment": {
                    "subclass": {
                        "instance_soid": "0x2",
                        "definition_hash": format_hash(subclass_hash),
                        "level": 0,
                        "quantity": 1,
                        "plugs": []
                    }
                }
            }]
        }
    })
}

#[test]
fn character_validation_keeps_sunrise_limits() {
    let mut document = serde_json::json!({
        "state": {
            "characters": [{
                "soid": "0x1",
                "level": 256,
                "equipment": {}
            }]
        }
    });
    assert!(validate_characters(&document).is_err());

    *document.pointer_mut("/state/characters/0/level").unwrap() = Value::from(255);
    document
        .pointer_mut("/state/characters/0/equipment")
        .unwrap()["kinetic"] = serde_json::json!({
        "instance_soid": "0x2",
        "definition_hash": "0x2A",
        "level": 106,
        "quantity": 1,
        "plugs": [null, null, null, null, null, null, null, null, null, null, null, null, null]
    });
    assert!(validate_characters(&document).is_err());
}

#[test]
fn settings_paths_are_derived_from_install() {
    assert_eq!(
        settings_path_for_install(Path::new("game"), SettingsLayout::Root),
        PathBuf::from("game").join(ROOT_SETTINGS_RELATIVE_PATH)
    );
    assert_eq!(
        settings_path_for_install(Path::new("game"), SettingsLayout::BinX64),
        PathBuf::from("game").join(BIN_X64_SETTINGS_RELATIVE_PATH)
    );
}

#[test]
fn settings_resolution_uses_the_only_existing_file_and_never_creates_one() {
    let directory = TestDirectory::new();
    assert!(matches!(
        resolve_settings_path(&directory.0, None),
        SettingsPathResolution::Missing
    ));
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);

    let root = settings_path_for_install(&directory.0, SettingsLayout::Root);
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    fs::write(&root, b"{}\n").unwrap();

    assert!(matches!(
        resolve_settings_path(&directory.0, None),
        SettingsPathResolution::Found(SettingsLayout::Root, path) if path == root
    ));
}

#[test]
fn settings_resolution_requires_a_choice_when_both_files_exist() {
    let directory = TestDirectory::new();
    let root = settings_path_for_install(&directory.0, SettingsLayout::Root);
    let bin_x64 = settings_path_for_install(&directory.0, SettingsLayout::BinX64);
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    fs::create_dir_all(bin_x64.parent().unwrap()).unwrap();
    fs::write(&root, b"{\"layout\":\"root\"}\n").unwrap();
    fs::write(&bin_x64, b"{\"layout\":\"bin\"}\n").unwrap();

    assert!(matches!(
        resolve_settings_path(&directory.0, None),
        SettingsPathResolution::Ambiguous
    ));
    assert!(matches!(
        resolve_settings_path(&directory.0, Some(SettingsLayout::BinX64)),
        SettingsPathResolution::Found(SettingsLayout::BinX64, path) if path == bin_x64
    ));
    assert_eq!(fs::read_to_string(root).unwrap(), "{\"layout\":\"root\"}\n");
    assert_eq!(
        fs::read_to_string(bin_x64).unwrap(),
        "{\"layout\":\"bin\"}\n"
    );
}

#[test]
fn loading_a_missing_selected_settings_file_never_creates_it() {
    let directory = TestDirectory::new();
    let settings = settings_path_for_install(&directory.0, SettingsLayout::BinX64);

    let error = load_json(&settings).unwrap_err();

    assert!(error.contains("No Project Sunrise settings.json was found"));
    assert!(!settings.exists());
}

#[test]
fn stock_classes_use_stock_subclasses_and_movement_defaults() {
    assert_eq!(default_subclass_name(0), "Sunbreaker");
    assert_eq!(default_subclass_name(1), "Nightstalker");
    assert_eq!(default_subclass_name(2), "Dawnblade");

    let abilities = catalog::AbilityOptions {
        movement: vec![
            AbilityChoice {
                entry: 4,
                name: "First".into(),
            },
            AbilityChoice {
                entry: 5,
                name: "Second".into(),
            },
            AbilityChoice {
                entry: 6,
                name: "Third".into(),
            },
        ],
        grenade: vec![AbilityChoice {
            entry: 7,
            name: "Grenade".into(),
        }],
        super_ability: vec![AbilityChoice {
            entry: 10,
            name: "Super".into(),
        }],
        melee: vec![AbilityChoice {
            entry: 11,
            name: "Melee".into(),
        }],
        class_ability: vec![AbilityChoice {
            entry: 2,
            name: "Class".into(),
        }],
        attunements: Vec::new(),
    };
    assert_eq!(
        default_ability_values(0, &abilities, Some(2)),
        (5, 7, 10, 11, 2)
    );
    assert_eq!(
        default_ability_values(0, &abilities, Some(3)),
        (6, 7, 10, 11, 2)
    );
    assert_eq!(
        default_ability_values(1, &abilities, Some(2)),
        (6, 7, 10, 11, 2)
    );
    assert_eq!(
        default_ability_values(2, &abilities, Some(3)),
        (5, 7, 10, 11, 2)
    );
}

#[test]
fn distinctive_super_selection_wins_when_old_attunements_are_mixed() {
    let choice = |entry, name: &str| AbilityChoice {
        entry,
        name: name.into(),
    };
    let abilities = catalog::AbilityOptions {
        attunements: vec![
            catalog::AttunementChoice {
                name: "Top".into(),
                super_abilities: vec![choice(10, "Base super")],
                melee: choice(11, "Top melee"),
                perks: vec![choice(13, "Former top selector")],
            },
            catalog::AttunementChoice {
                name: "Bottom".into(),
                super_abilities: vec![choice(10, "Base super")],
                melee: choice(15, "Bottom melee"),
                perks: vec![choice(18, "Former bottom selector")],
            },
            catalog::AttunementChoice {
                name: "Middle".into(),
                super_abilities: vec![choice(20, "Middle super")],
                melee: choice(21, "Middle melee"),
                perks: Vec::new(),
            },
        ],
        ..Default::default()
    };
    assert_eq!(selected_attunement_index(&abilities, 10, 15), 1);
    assert_eq!(selected_attunement_index(&abilities, 20, 15), 2);
    assert_eq!(selected_attunement_index(&abilities, 18, 21), 1);
}

#[test]
fn settings_encoder_keeps_arrays_compact_and_round_trips() {
    let document = serde_json::json!({
        "outer": {
            "values": [1, 2, 3],
            "records": [{"name": "one"}, {"name": "two"}]
        }
    });
    let encoded = encode_settings(&document).unwrap();
    assert!(encoded.contains("\"values\": [1,2,3]"));
    assert!(encoded.contains("\"records\": [{\"name\":\"one\"},{\"name\":\"two\"}]"));
    assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), document);
}

#[test]
fn settings_saves_are_verified_and_each_keeps_its_own_backup() {
    let directory = TestDirectory::new();
    let settings = directory.0.join("settings.json");
    let backups = directory.0.join("backups");
    fs::write(&settings, b"{\"version\":0}\n").unwrap();

    let first_document = serde_json::json!({"version": 1, "values": [1, 2, 3]});
    let first_backup = save_json_with_backup_root(&settings, &first_document, &backups).unwrap();
    let second_document = serde_json::json!({"version": 2, "values": [4, 5, 6]});
    let second_backup = save_json_with_backup_root(&settings, &second_document, &backups).unwrap();

    assert_ne!(first_backup, second_backup);
    assert_eq!(load_json(&settings).unwrap(), second_document);
    assert_eq!(
        load_json(&first_backup).unwrap(),
        serde_json::json!({"version": 0})
    );
    assert_eq!(load_json(&second_backup).unwrap(), first_document);
    assert!(fs::read_to_string(&settings).unwrap().ends_with('\n'));
}

#[test]
fn unexpected_settings_get_an_exact_adjacent_backup_without_losing_an_older_one() {
    let directory = TestDirectory::new();
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
    let directory = TestDirectory::new();
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
fn oversized_settings_are_rejected_before_a_backup_or_write() {
    let directory = TestDirectory::new();
    let settings = directory.0.join("settings.json");
    let backups = directory.0.join("backups");
    let original = b"{\"version\":0}\n";
    fs::write(&settings, original).unwrap();
    let document = Value::String("x".repeat(MAX_SETTINGS_BYTES));

    assert!(save_json_with_backup_root(&settings, &document, &backups).is_err());
    assert_eq!(fs::read(&settings).unwrap(), original);
    assert!(!backups.exists());
}

#[test]
fn class_armor_reset_preserves_destination_data() {
    let document = serde_json::json!({
        "state": { "characters": [{
            "class": 1,
            "equipment": {
                "helmet": { "instance_soid": "template-helmet", "definition_hash": "hunter-helmet", "level": 106 },
                "gauntlets": { "instance_soid": "template-arms", "definition_hash": "hunter-arms", "level": 106 },
                "chest": { "instance_soid": "template-chest", "definition_hash": "hunter-chest", "level": 106 },
                "legs": { "instance_soid": "template-legs", "definition_hash": "hunter-legs", "level": 106 },
                "class_item": { "instance_soid": "template-class", "definition_hash": "hunter-cloak", "level": 106 }
            }
        }] }
    });
    let defaults = collect_class_armor_defaults(&document);
    let mut destination = serde_json::json!({
        "equipment": {
            "helmet": {
                "instance_soid": "destination-helmet",
                "definition_hash": "old",
                "future_item_data": { "keep": [1, 2, 3] }
            },
            "gauntlets": { "instance_soid": "destination-arms", "definition_hash": "old" },
            "chest": { "instance_soid": "destination-chest", "definition_hash": "old" },
            "legs": { "instance_soid": "destination-legs", "definition_hash": "old" },
            "class_item": { "instance_soid": "destination-class", "definition_hash": "old" }
        }
    });
    let changed = restore_class_armor(
        destination.as_object_mut().unwrap(),
        defaults.get(&1).unwrap(),
    );
    assert!(changed);
    assert_eq!(
        destination.pointer("/equipment/helmet/definition_hash"),
        Some(&Value::String("hunter-helmet".into()))
    );
    assert_eq!(
        destination.pointer("/equipment/helmet/instance_soid"),
        Some(&Value::String("destination-helmet".into()))
    );
    assert_eq!(
        destination.pointer("/equipment/helmet/level"),
        Some(&Value::from(106))
    );
    assert_eq!(
        destination.pointer("/equipment/helmet/future_item_data"),
        Some(&serde_json::json!({ "keep": [1, 2, 3] }))
    );
}
