use super::*;

#[test]
fn cancelling_first_enable_leaves_parhelion_disabled_and_prompts_again() {
    let directory = TestDirectory::new("parhelion-first-enable-cancel");
    let mut app = app(directory.0.clone());
    assert!(!app.request_parhelion_enabled(true));
    assert!(app.confirmation == Some(ConfirmationDialog::EnableParhelion));
    assert!(!app.preferences.experimental_package_authoring);
    assert!(!app.preferences.parhelion_warning_acknowledged);
    app.finish_parhelion_confirmation(false);
    assert!(app.confirmation.is_none());
    assert!(!app.preferences.experimental_package_authoring);
    assert!(!app.preferences.parhelion_warning_acknowledged);
    assert!(!app.request_parhelion_enabled(true));
    assert!(app.confirmation == Some(ConfirmationDialog::EnableParhelion));
}

#[test]
fn accepted_introduction_survives_restart_and_later_toggles() {
    let directory = TestDirectory::new("parhelion-first-enable-accept");
    let mut app = app(directory.0.clone());
    // Older preference files have no acknowledgment and must still be readable.
    app.preferences = serde_json::from_str("{}").unwrap();
    assert!(!app.preferences.parhelion_warning_acknowledged);
    assert!(!app.request_parhelion_enabled(true));
    app.finish_parhelion_confirmation(true);
    let path = directory.0.join("preferences.json");
    preferences::store::save_preferences(&path, &app.preferences).unwrap();
    app.preferences = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(app.preferences.experimental_package_authoring);
    assert!(app.preferences.parhelion_warning_acknowledged);
    assert!(app.request_parhelion_enabled(false));
    assert!(app.request_parhelion_enabled(true));
    assert!(app.confirmation.is_none());
}
