use crate::app::tests::driver::*;
use crate::app::*;
use crate::persistence::sqlite_account;

#[test]
#[ignore = "Requires SUNDIAL_INSTALL pointing to an installed Sunrise module, read only"]
fn installed_resources_prepare_both_resets_for_the_selected_installation() {
    let install = crate::test_support::install();
    let settings = settings::load_installed_sunrise_defaults(&install).unwrap();
    assert!(game_settings::schema_version(&settings).unwrap() >= 18);
    let directory = TestDirectory::new("installed-reset-resources");
    let mut app = app(install);
    app.settings_path = directory.0.join("settings.json");
    let database = directory.0.join("data/investment.sqlite3");
    sqlite_account::tests::create_fixture(&database, 3);
    app.document = WorkspaceDocument::load(settings, &app.settings_path, false);
    let before = sqlite_account::snapshot::read(&database).unwrap();
    app.request_sqlite_defaults_reset();
    assert!(
        app.confirmation == Some(ConfirmationDialog::ResetSqliteDefaults),
        "{}",
        app.status
    );
    assert_eq!(app.pending_sqlite_reset.as_ref().unwrap().path(), database);
    assert_eq!(sqlite_account::snapshot::read(&database).unwrap(), before);
    // Execute only against this disposable fixture, never the installed account.
    let receipt = app.pending_sqlite_reset.take().unwrap().apply().unwrap();
    assert!(matches!(
        sqlite_account::load_document(&database).unwrap(),
        sqlite_account::SqliteAccountDocumentLoad::Loaded(_)
    ));
    assert_eq!(
        sqlite_account::snapshot::read(&receipt.safety_backup).unwrap(),
        before
    );
}
