//! Save policy checks use only disposable documents and files.
use crate::app::tests::driver::*;
use crate::app::*;
use serde_json::{Value, json};

fn defaults() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/sunrise-v16-1120748-defaults.json"
    ))
    .unwrap()
}

fn load(app: &mut SundialApp, json: Value) {
    app.document = WorkspaceDocument::json_only(json);
    app.persisted_document = app.document.clone();
    app.sync_raw_json();
}

#[test]
fn invalid_runtime_setting_is_rejected_before_save_review() {
    let directory = TestDirectory::new("save-review-invalid-runtime");
    let mut app = app(directory.0.clone());
    load(&mut app, defaults());
    std::fs::write(&app.settings_path, &app.raw_json).unwrap();
    let before = std::fs::read(&app.settings_path).unwrap();
    app.preferences.review_changes_before_saving = true;
    app.document.json_mut()["server"]["gameplay"]["bind_address"] = json!("not-an-ip");
    app.dirty = true;
    app.request_save(&egui::Context::default(), SaveAction::Save);
    assert!(app.confirmation.is_none());
    assert!(app.pending_save_action.is_none());
    assert!(app.status_is_error);
    assert!(app.status.contains("bind_address"), "{}", app.status);
    assert_eq!(std::fs::read(&app.settings_path).unwrap(), before);
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[test]
fn unchanged_error_cannot_hide_a_new_invalid_setting() {
    let directory = TestDirectory::new("save-hidden-validation-error");
    let mut app = app(directory.0.clone());
    let mut original = defaults();
    original["core"]["logging"]["debugger_sink"] = json!("invalid");
    load(&mut app, original);
    let mut candidate = app.document.clone();
    candidate.json_mut()["client"]["external_server"]["host"] = json!("not an IP address");
    assert!(app.validation_warning_for_write(&candidate).is_err());

    std::fs::write(&app.settings_path, app.raw_json.as_bytes()).unwrap();
    let before = std::fs::read(&app.settings_path).unwrap();
    app.raw_json = candidate.json().to_string();
    assert!(!app.apply_raw_json());
    assert_eq!(app.document, app.persisted_document);
    assert_eq!(std::fs::read(&app.settings_path).unwrap(), before);
    assert!(app.status.contains("Correct invalid known settings"));
}
