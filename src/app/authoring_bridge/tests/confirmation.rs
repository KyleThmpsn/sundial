use crate::app::tests::driver::*;
use crate::app::*;

#[test]
fn accepted_introduction_survives_restart_and_later_opens() {
    let directory = TestDirectory::new("parhelion-first-open-accept");
    let mut app = app(directory.0.clone());
    let ctx = egui::Context::default();
    // Older preference files have no acknowledgment and must still be readable.
    app.preferences = serde_json::from_str("{}").unwrap();
    assert!(!app.preferences.parhelion_warning_acknowledged);
    app.request_open_parhelion(&ctx);
    assert!(app.confirmation == Some(ConfirmationDialog::OpenParhelion));
    app.finish_parhelion_confirmation(true);
    let path = directory.0.join("preferences.json");
    preferences::store::save_preferences(&path, &app.preferences).unwrap();
    app.preferences = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(app.preferences.parhelion_warning_acknowledged);
    app.request_open_parhelion(&ctx);
    assert!(app.confirmation.is_none());
}
