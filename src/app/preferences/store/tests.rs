use super::*;
use crate::test_support::TestDirectory;

#[test]
fn saved_preferences_preserve_opt_ins_and_layout_choices() {
    use crate::app::preferences::{CharacterInventoryLayout, PlugSelectionMode};

    let directory = TestDirectory::new("saved-preference-options");
    let path = directory.0.join("config/preferences.json");
    for (mode, mode_name) in [
        (PlugSelectionMode::GearType, "gear_type"),
        (PlugSelectionMode::SocketAndGearType, "socket_and_gear_type"),
    ] {
        let preferences = Preferences {
            experimental_progression: true,
            experimental_activity_state: true,
            experimental_power_above_cap: true,
            experimental_extended_fov: true,
            experimental_cross_class_subclasses: true,
            experimental_package_authoring: true,
            show_parhelion_experimental_options: true,
            troubleshooting_logging: true,
            review_changes_before_saving: true,
            limit_automatic_backups: true,
            automatic_backup_limit: 35,
            character_inventory_layout: CharacterInventoryLayout::Panoptes,
            default_plug_selection_mode: mode,
            ..Preferences::default()
        };
        let expected = serde_json::json!({
            "experimental_progression": true,
            "experimental_activity_state": true,
            "experimental_power_above_cap": true,
            "experimental_extended_fov": true,
            "experimental_cross_class_subclasses": true,
            "experimental_package_authoring": true,
            "show_parhelion_experimental_options": true,
            "troubleshooting_logging": true,
            "review_changes_before_saving": true,
            "limit_automatic_backups": true,
            "automatic_backup_limit": 35,
            "character_inventory_layout": "panoptes",
            "default_plug_selection_mode": mode_name
        });

        save_preferences(&path, &preferences).unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let loaded = load_from_paths(Some(&path), None);
        assert!(loaded.warning.is_none());
        let decoded = serde_json::to_value(loaded.preferences).unwrap();
        for (key, value) in expected.as_object().unwrap() {
            assert_eq!(&stored[key], value, "saved {key}");
            assert_eq!(&decoded[key], value, "loaded {key}");
        }
    }
}

#[test]
fn missing_preferences_use_defaults_without_a_warning() {
    let directory = TestDirectory::new("missing-preferences");
    let loaded = load_from_paths(Some(&directory.0.join("preferences.json")), None);
    assert!(loaded.warning.is_none());
    assert!(loaded.preferences.install.is_none());
    assert!(!loaded.preferences.experimental_activity_state);
}

#[test]
fn legacy_preferences_are_only_used_when_current_is_missing() {
    let directory = TestDirectory::new("legacy-preferences");
    let current = directory.0.join("preferences.json");
    let legacy = directory.0.join("paths.json");
    fs::write(&legacy, br#"{"install":"legacy"}"#).unwrap();
    let loaded = load_from_paths(Some(&current), Some(&legacy));
    assert!(loaded.warning.is_none());
    assert_eq!(loaded.preferences.install, Some(PathBuf::from("legacy")));
    fs::write(&current, br#"{"install":"current"}"#).unwrap();
    assert_eq!(
        load_from_paths(Some(&current), Some(&legacy))
            .preferences
            .install,
        Some(PathBuf::from("current"))
    );
}

#[test]
fn malformed_current_preferences_warn_without_using_stale_legacy_values() {
    let directory = TestDirectory::new("invalid-preferences");
    let current = directory.0.join("preferences.json");
    let legacy = directory.0.join("paths.json");
    fs::write(&current, b"{broken").unwrap();
    fs::write(&legacy, br#"{"install":"stale"}"#).unwrap();
    let loaded = load_from_paths(Some(&current), Some(&legacy));
    assert!(
        loaded
            .warning
            .unwrap()
            .contains(&current.display().to_string())
    );
    assert!(loaded.preferences.install.is_none());
    assert_eq!(fs::read(&current).unwrap(), b"{broken");
}

#[test]
fn invalid_legacy_preferences_also_warn() {
    let directory = TestDirectory::new("invalid-legacy-preferences");
    let legacy = directory.0.join("paths.json");
    fs::write(&legacy, b"invalid").unwrap();
    assert!(load_from_paths(None, Some(&legacy)).warning.is_some());
}

#[test]
fn save_preserves_invalid_bytes_and_writes_valid_preferences() {
    let directory = TestDirectory::new("preserved-preferences");
    let current = directory.0.join("preferences.json");
    let invalid = b"\xff{broken";
    fs::write(&current, invalid).unwrap();
    save_preferences(&current, &Preferences::default()).unwrap();
    assert!(read_preferences(&current).unwrap().is_some());
    let backups = fs::read_dir(&directory.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path != &current)
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(&backups[0]).unwrap(), invalid);
    save_preferences(&current, &Preferences::default()).unwrap();
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
}

#[test]
fn unreadable_preferences_warn_and_are_not_replaced() {
    let directory = TestDirectory::new("unreadable-preferences");
    let current = directory.0.join("preferences.json");
    fs::create_dir(&current).unwrap();
    assert!(load_from_paths(Some(&current), None).warning.is_some());
    assert!(save_preferences(&current, &Preferences::default()).is_err());
    assert!(current.is_dir());
}
