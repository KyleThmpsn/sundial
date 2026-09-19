use super::*;
use crate::{game_settings::dawn::Runtime, package_runtime::installation::RuntimeInspection};
use serde_json::{Value, json};

mod switching;
mod ui;

fn setup() -> (TestDirectory, SundialApp, RuntimeInspection) {
    let directory = TestDirectory::new("dawn-validation");
    std::fs::create_dir_all(directory.0.join("Sunrise")).unwrap();
    std::fs::write(directory.0.join("steam_api64.dll"), b"unknown runtime").unwrap();
    let mut app = app(directory.0.clone());
    app.settings_path = directory.0.join("Sunrise/settings.json");
    let document: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/dawn-v6-42fc41e-defaults.json"
    ))
    .unwrap();
    app.document = WorkspaceDocument::json_only(document);
    app.persisted_document = app.document.clone();
    app.sync_raw_json();
    std::fs::write(&app.settings_path, &app.raw_json).unwrap();
    let mut inspection = RuntimeInspection::inspect(&directory.0);
    // Isolate save policy from binary recognition, which has separate signature tests.
    let copy = &mut inspection.copies[0];
    copy.dawn = true;
    copy.bundled_schema = Some(6);
    copy.dawn_runtime = Some(Runtime::inspect(&copy.dll_path));
    (directory, app, inspection)
}

#[test]
fn dawn_save_validation_rejects_invalid_flags_and_enabled_missing_script() {
    let (_directory, app, inspection) = setup();
    for flag in [json!("true"), json!(true)] {
        let mut candidate = app.document.clone();
        candidate.json_mut()["experiments"] = json!({"omega": {"coo_executor": flag}});
        assert!(
            app.validation_warning_for_runtime(&candidate, &inspection)
                .is_err()
        );
        candidate.json_mut()["experiments"]["omega"]["coo_executor"] = json!(false);
        assert_eq!(
            app.validation_warning_for_runtime(&candidate, &inspection),
            Ok(None)
        );
    }
}

#[test]
fn dawn_rules_require_positive_detection_and_the_matching_settings_copy() {
    let (directory, mut app, inspection) = setup();
    let mut candidate = app.document.clone();
    candidate.json_mut()["experiments"] = json!({"omega": {"coo_executor": "opaque extension"}});
    assert_eq!(app.validation_warning_for_write(&candidate), Ok(None));
    app.raw_json = candidate.json().to_string();
    assert!(app.apply_raw_json(), "{}", app.status);
    app.settings_path = directory.0.join("bin/x64/Sunrise/settings.json");
    let error = app
        .validation_warning_for_runtime(&candidate, &inspection)
        .unwrap_err();
    assert!(error.contains("matching settings"), "{error}");
}

#[test]
fn dawn_switch_to_normal_sunrise_rechecks_account_format_without_migrating() {
    let (_directory, app, mut inspection) = setup();
    inspection.copies[0].dawn = false;
    inspection.copies[0].dawn_runtime = None;
    inspection.copies[0].bundled_schema = Some(6);
    let mut candidate = app.document.clone();
    candidate.json_mut()["experiments"] = json!({"omega": {"coo_executor": true, "future": [1]}});
    assert_eq!(
        app.validation_warning_for_runtime(&candidate, &inspection),
        Ok(None)
    );
    let before = candidate.clone();
    inspection.copies[0].bundled_schema = Some(18);
    assert!(
        app.validation_warning_for_runtime(&candidate, &inspection)
            .unwrap_err()
            .contains("SQLite account storage")
    );
    assert_eq!(candidate, before);
    candidate.json_mut()["version"] = json!(18);
    inspection.copies[0].bundled_schema = Some(6);
    assert!(
        app.validation_warning_for_runtime(&candidate, &inspection)
            .unwrap_err()
            .contains("Restore matching JSON")
    );
}
