use super::*;
use crate::persistence::sqlite_account::{self, ResetPlan};

fn frame(
    app: &mut SundialApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 760.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.draw_preferences_page(ui, ctx));
            app.draw_reset_defaults_confirmation(ctx);
            app.draw_sqlite_reset_confirmation(ctx);
        },
    )
}

fn text_position(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text == label => {
            Some(text.pos + text.galley.rect.center().to_vec2())
        }
        _ => None,
    })
}

#[test]
fn installation_and_recovery_offer_the_active_source_resets_without_writing() {
    for sqlite in [false, true] {
        let directory = TestDirectory::new("reset-preferences");
        let mut app = app(directory.0.clone());
        if sqlite {
            sqlite_account::tests::create_fixture(&directory.0.join("data/investment.sqlite3"), 3);
            app.document =
                WorkspaceDocument::load(serde_json::json!({"version":18}), &app.settings_path);
            app.persisted_document = app.document.clone();
        }
        let original = app.document.clone();
        for tab in [PreferencesTab::Installation, PreferencesTab::SavingRecovery] {
            app.preferences_tab = tab;
            let context = egui::Context::default();
            for _ in 0..2 {
                frame(&mut app, &context, vec![]);
            }
            let output = frame(&mut app, &context, vec![]);
            let reset = text_position(&output, "Reset to Sunrise Defaults…")
                .expect("settings reset is available");
            assert_eq!(
                text_position(&output, "Reset Account Database…").is_some(),
                sqlite
            );
            assert_eq!(text_position(&output, "Restore Backup…").is_some(), sqlite);
            schema_smoke::capture_preferences(
                &context,
                output,
                &format!("recovery-{tab:?}-{sqlite}"),
                960.0,
            );
            for pressed in [true, false] {
                frame(
                    &mut app,
                    &context,
                    vec![
                        egui::Event::PointerMoved(reset),
                        egui::Event::PointerButton {
                            pos: reset,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(app.confirmation == Some(ConfirmationDialog::ResetDefaults));
            app.confirmation = None;
            assert_eq!(app.document, original);
            assert!(!app.settings_path.exists());
        }
    }
}

#[test]
fn cancelling_account_reset_keeps_the_database_and_unsaved_edits() {
    let directory = TestDirectory::new("reset-account-cancel");
    let mut app = app(directory.0.clone());
    let database = directory.0.join("data/investment.sqlite3");
    sqlite_account::tests::create_fixture(&database, 3);
    app.document = WorkspaceDocument::load(serde_json::json!({"version":18}), &app.settings_path);
    app.document.json_mut()["unsaved"] = serde_json::json!("keep");
    let original = app.document.clone();
    let disk = sqlite_account::package::read(&database).unwrap();
    app.pending_sqlite_reset =
        Some(ResetPlan::prepare(&database, &sqlite_account::tests::default_resources()).unwrap());
    app.confirmation = Some(ConfirmationDialog::ResetSqliteDefaults);
    let context = egui::Context::default();
    for _ in 0..3 {
        frame(&mut app, &context, vec![]);
    }
    let output = frame(&mut app, &context, vec![]);
    assert!(text_position(&output, "Reset Account Database?").is_some());
    schema_smoke::capture_preferences(&context, output, "account-reset-confirmation", 960.0);
    frame(
        &mut app,
        &context,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.confirmation.is_none());
    assert!(app.pending_sqlite_reset.is_none());
    assert_eq!(app.document, original);
    assert_eq!(sqlite_account::package::read(&database).unwrap(), disk);
}

#[test]
#[ignore = "Requires SUNDIAL_NATIVE_INSTALL pointing to an installed Sunrise module, read only"]
fn installed_resources_prepare_both_resets_for_the_selected_installation() {
    let install =
        PathBuf::from(std::env::var_os("SUNDIAL_NATIVE_INSTALL").expect("native installation"));
    let settings = settings::load_installed_sunrise_defaults(&install).unwrap();
    assert!(game_settings::schema_version(&settings).unwrap() >= 18);
    let directory = TestDirectory::new("installed-reset-resources");
    let mut app = app(install);
    app.settings_path = directory.0.join("settings.json");
    let database = directory.0.join("data/investment.sqlite3");
    sqlite_account::tests::create_fixture(&database, 3);
    app.document = WorkspaceDocument::load(settings, &app.settings_path);
    let before = sqlite_account::package::read(&database).unwrap();
    app.request_sqlite_defaults_reset();
    assert!(
        app.confirmation == Some(ConfirmationDialog::ResetSqliteDefaults),
        "{}",
        app.status
    );
    assert_eq!(app.pending_sqlite_reset.as_ref().unwrap().path(), database);
    assert_eq!(sqlite_account::package::read(&database).unwrap(), before);
    // Execute only against this disposable fixture, never the installed account.
    let receipt = app.pending_sqlite_reset.take().unwrap().apply().unwrap();
    assert!(matches!(
        sqlite_account::load_document(&database).unwrap(),
        sqlite_account::SqliteAccountDocumentLoad::Loaded(_)
    ));
    assert_eq!(
        sqlite_account::package::read(&receipt.safety_backup).unwrap(),
        before
    );
}
