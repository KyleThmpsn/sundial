use super::*;

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
