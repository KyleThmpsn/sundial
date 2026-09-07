use crate::app::preferences::ColorTheme;
use crate::app::preferences::ItemCardWidth;
use crate::app::preferences::normalized_automatic_backup_limit;
use crate::app::*;

#[test]
fn legacy_preferences_default_to_socket_and_gear_type_plugs_with_warnings() {
    let decoded: Preferences = serde_json::from_value(serde_json::json!({
        "install": null,
        "settings_layout": null,
        "really_unsafe_warning_acknowledged": false
    }))
    .unwrap();

    assert_legacy_safety_defaults(&decoded);
    assert_legacy_display_defaults(&decoded);
}

fn assert_legacy_safety_defaults(preferences: &Preferences) {
    assert_eq!(
        preferences.default_plug_selection_mode,
        PlugSelectionMode::SocketAndGearType
    );
    assert!(preferences.show_safety_warnings);
    assert!(!preferences.review_changes_before_saving);
    assert!(!preferences.limit_automatic_backups);
    assert_eq!(preferences.automatic_backup_limit, 20);
    assert!(!preferences.experimental_progression);
    assert!(!preferences.experimental_power_above_cap);
    assert!(!preferences.experimental_extended_fov);
    assert!(!preferences.experimental_cross_class_subclasses);
    assert!(!preferences.troubleshooting_logging);
    assert!(!preferences.experimental_package_authoring);
    assert!(!preferences.show_parhelion_experimental_options);
}

fn assert_legacy_display_defaults(preferences: &Preferences) {
    assert_eq!(preferences.color_theme, ColorTheme::Dark);
    assert!(!preferences.always_open_json_editor_in_second_window);
    assert!(!preferences.show_plug_hashes);
    assert_eq!(preferences.item_card_width, ItemCardWidth::Standard);
    assert_eq!(
        preferences.character_inventory_layout,
        CharacterInventoryLayout::Cards
    );
}

#[test]
fn automatic_backup_limit_is_clamped_to_the_supported_range() {
    assert_eq!(normalized_automatic_backup_limit(0), 5);
    assert_eq!(normalized_automatic_backup_limit(50), 50);
    assert_eq!(normalized_automatic_backup_limit(u16::MAX), 100);
}

#[test]
fn runtime_preferences_are_normalized_once_when_loaded() {
    let mut preferences = Preferences {
        default_plug_selection_mode: PlugSelectionMode::AnyPlug,
        automatic_backup_limit: u16::MAX,
        ..Preferences::default()
    };

    preferences.normalize_for_runtime();

    assert_eq!(
        preferences.default_plug_selection_mode,
        PlugSelectionMode::SocketAndGearType
    );
    assert_eq!(preferences.automatic_backup_limit, 100);
}

#[test]
fn resetting_preferences_preserves_the_active_install_selection() {
    let mut preferences = Preferences {
        install: Some(PathBuf::from("install")),
        settings_layout: Some("bin_x64".to_owned()),
        show_plug_hashes: true,
        experimental_progression: true,
        ..Preferences::default()
    };

    preferences.reset_editable_settings();

    assert_eq!(preferences.install, Some(PathBuf::from("install")));
    assert_eq!(preferences.settings_layout.as_deref(), Some("bin_x64"));
    assert!(!preferences.show_plug_hashes);
    assert!(!preferences.experimental_progression);
}

#[test]
fn retired_orbit_preference_is_ignored_when_loading() {
    let decoded: Preferences = serde_json::from_value(serde_json::json!({
        "experimental_orbit_backdrops": true,
        "experimental_progression": true
    }))
    .unwrap();
    assert!(decoded.experimental_progression);
    assert!(
        serde_json::to_value(decoded)
            .unwrap()
            .get("experimental_orbit_backdrops")
            .is_none()
    );
}
