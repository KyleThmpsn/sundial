use super::*;
use crate::{game_settings::dawn::Runtime, package_runtime::installation::RuntimeInspection};
use serde_json::{Value, json};

fn setup() -> (TestDirectory, SundialApp, RuntimeInspection) {
    let directory = TestDirectory::new("dawn-validation");
    std::fs::create_dir_all(directory.0.join("Sunrise")).unwrap();
    std::fs::write(directory.0.join("steam_api64.dll"), b"unknown runtime").unwrap();
    let mut app = app(directory.0.clone());
    app.settings_path = directory.0.join("Sunrise/settings.json");
    let document: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/sunrise-v6-4aebb148-defaults.json"
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
fn dawn_rules_do_not_apply_to_unknown_dll_or_other_settings_copy() {
    let (directory, mut app, inspection) = setup();
    let mut candidate = app.document.clone();
    candidate.json_mut()["experiments"] = json!({"omega": {"coo_executor": "opaque extension"}});
    assert_eq!(app.validation_warning_for_write(&candidate), Ok(None));
    app.raw_json = candidate.json().to_string();
    assert!(app.apply_raw_json(), "{}", app.status);
    app.settings_path = directory.0.join("bin/x64/Sunrise/settings.json");
    assert_eq!(
        app.validation_warning_for_runtime(&candidate, &inspection),
        Ok(None)
    );
}

#[test]
fn dawn_runtime_change_blocks_raw_apply_before_mutating_the_document() {
    let (directory, mut app, inspection) = setup();
    app.runtime_choice.inspection = inspection;
    std::fs::write(directory.0.join("steam_api64.dll"), b"replacement runtime").unwrap();
    let before = app.document.clone();
    let saved_bytes = std::fs::read(&app.settings_path).unwrap();
    app.raw_json = {
        let mut candidate = app.document.json().clone();
        candidate["experiments"] = json!({"omega": {"coo_executor": true}});
        candidate.to_string()
    };
    assert!(!app.apply_raw_json());
    assert!(app.status.contains("runtime DLL changed"), "{}", app.status);
    assert_eq!(app.document, before);
    assert_eq!(std::fs::read(&app.settings_path).unwrap(), saved_bytes);
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
