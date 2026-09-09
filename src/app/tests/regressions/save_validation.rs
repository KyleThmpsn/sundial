//! Save policy regressions use only disposable documents and files.
use super::*;
use serde_json::{Value, json};

fn defaults() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/sunrise-v16-1120748-defaults.json"
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
fn save_and_exit_closes_after_all_edits_were_reverted() {
    let directory = TestDirectory::new("save-exit-reverted-edits");
    let mut app = app(directory.0.clone());
    load(&mut app, defaults());
    app.dirty = true;
    app.preferences.review_changes_before_saving = true;
    app.request_save(&egui::Context::default(), SaveAction::SaveAndExit);
    assert!(app.exit_confirmed, "{}", app.status);
    assert!(!app.has_unsaved_changes());
    assert!(app.confirmation.is_none());
    assert!(!app.settings_path.exists());
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
    assert_eq!(
        settings::validate_workspace_document(&candidate),
        settings::validate_workspace_document(&app.persisted_document),
        "The former first-error comparison must be unable to distinguish these documents",
    );
    assert!(app.validation_warning_for_write(&candidate).is_err());

    std::fs::write(&app.settings_path, app.raw_json.as_bytes()).unwrap();
    let before = std::fs::read(&app.settings_path).unwrap();
    app.raw_json = candidate.json().to_string();
    assert!(!app.apply_raw_json());
    assert_eq!(app.document, app.persisted_document);
    assert_eq!(std::fs::read(&app.settings_path).unwrap(), before);
    assert!(app.status.contains("Correct invalid known settings"));
}

#[test]
fn repairing_only_the_first_error_does_not_allow_a_second_error() {
    let directory = TestDirectory::new("save-partial-repair");
    let mut app = app(directory.0.clone());
    let mut original = defaults();
    original["core"]["logging"]["debugger_sink"] = json!("invalid");
    original["client"]["external_server"]["host"] = json!("invalid");
    load(&mut app, original);
    let mut candidate = app.document.clone();
    candidate.json_mut()["core"]["logging"]["debugger_sink"] = json!(false);
    let error = app.validation_warning_for_write(&candidate).unwrap_err();
    assert!(error.contains("host"), "{error}");
    candidate.json_mut()["client"]["external_server"]["host"] = json!("127.0.0.1");
    assert_eq!(app.validation_warning_for_write(&candidate), Ok(None));
}

#[test]
fn repaired_json_applies_and_preserves_unknown_fields() {
    let directory = TestDirectory::new("save-repaired-json");
    let mut app = app(directory.0.clone());
    let mut original = defaults();
    original["custom_extension"] = json!({"keep": [1, "opaque", null]});
    original["core"]["logging"]["debugger_sink"] = json!("invalid");
    load(&mut app, original.clone());
    let mut repaired = original.clone();
    repaired["core"]["logging"]["debugger_sink"] = json!(false);
    app.raw_json = repaired.to_string();
    assert!(app.apply_raw_json(), "{}", app.status);
    assert!(app.dirty);
    assert_eq!(app.document.json(), &repaired);
    assert_eq!(app.persisted_document.json(), &original);
    assert_eq!(
        app.document.json()["custom_extension"],
        original["custom_extension"]
    );
}

#[test]
fn unrelated_json_edits_require_repairing_known_invalid_settings() {
    let directory = TestDirectory::new("save-invalid-known-setting");
    let mut app = app(directory.0.clone());
    let mut original = defaults();
    original["core"]["logging"]["debugger_sink"] = json!("invalid");
    load(&mut app, original);
    assert!(matches!(
        app.validation_warning_for_write(&app.document),
        Ok(Some(_))
    ));
    let mut candidate = app.document.clone();
    candidate.json_mut()["custom_extension"] = json!(true);
    assert!(app.validation_warning_for_write(&candidate).is_err());
}

#[cfg(feature = "sqlite-account")]
#[test]
fn unchanged_invalid_json_does_not_block_sqlite_only_edits() {
    let directory = TestDirectory::new("save-sqlite-unchanged-json");
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("data").join("investment.sqlite3"),
        3,
    );
    let mut app = app(directory.0.clone());
    let mut original = defaults();
    original["core"]["logging"]["debugger_sink"] = json!("invalid");
    app.document = WorkspaceDocument::load(original, &app.settings_path);
    assert!(!app.document.uses_json_account());
    app.persisted_document = app.document.clone();
    account_workspace::apply_account_settings(
        &mut app.document,
        vec![sundial_account::AccountSettingsCommand::Set {
            key: sundial_account::AccountSettingKey::known_preference("show_fps").unwrap(),
            value: sundial_account::AccountSettingValue::Boolean(false),
        }],
    )
    .unwrap();
    assert!(app.document.account_changed_from(&app.persisted_document));
    assert!(!app.document.json_changed_from(&app.persisted_document));
    assert!(matches!(
        app.validation_warning_for_write(&app.document),
        Ok(Some(_))
    ));
    let mut candidate = app.document.clone();
    candidate.json_mut()["client"]["external_server"]["host"] = json!("invalid");
    assert!(app.validation_warning_for_write(&candidate).is_err());
}
