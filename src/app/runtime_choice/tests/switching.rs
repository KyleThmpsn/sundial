use super::*;
use crate::package_runtime::installation::RuntimeLocation;
use std::fs;

fn sunrise_v18() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/sunrise-v18-169fd29-defaults.json"
    ))
    .unwrap()
}

fn set_runtime(inspection: &mut RuntimeInspection, dawn: bool, schema: u64) {
    let copy = &mut inspection.copies[0];
    copy.dawn = dawn;
    copy.dawn_runtime = dawn.then(|| Runtime::inspect(&copy.dll_path));
    copy.bundled_schema = Some(schema);
}

#[test]
fn runtime_swap_blocks_raw_apply_and_save_review_for_each_settings_layout() {
    for layout in SettingsLayout::ALL {
        let (directory, mut app, _) = setup();
        app.settings_path = directory.0.join(layout.relative_path());
        app.settings_layout = layout;
        fs::create_dir_all(app.settings_path.parent().unwrap()).unwrap();
        fs::write(&app.settings_path, &app.raw_json).unwrap();
        let mut dll = directory.0.join("steam_api64.dll");
        // Both bin/x64 layouts put the runtime there, so the DLL has to move for either or the
        // swap is not a swap in place.
        if matches!(layout, SettingsLayout::BinX64 | SettingsLayout::DawnBinX64) {
            let bin = directory.0.join("bin/x64/steam_api64.dll");
            fs::rename(&dll, &bin).unwrap();
            dll = bin;
        }
        app.runtime_choice.inspection = RuntimeInspection::inspect(&directory.0);
        fs::write(dll, b"swapped DLL").unwrap();
        let before = app.document.clone();
        let saved = fs::read(&app.settings_path).unwrap();
        let mut draft = app.document.json().clone();
        draft["future_setting"] = json!("keep this draft");
        app.raw_json = draft.to_string();
        assert!(!app.apply_raw_json());
        assert!(app.status.contains("runtime DLL changed"), "{}", app.status);
        assert_eq!(app.document, before);
        assert_eq!(app.raw_json, draft.to_string());

        app.sync_raw_json();
        app.document.json_mut()["future_setting"] = json!(true);
        app.preferences.review_changes_before_saving = true;
        app.request_save(&egui::Context::default(), SaveAction::Save);
        assert!(app.confirmation.is_none());
        assert!(app.status.contains("runtime DLL changed"), "{}", app.status);
        assert_eq!(fs::read(&app.settings_path).unwrap(), saved);
    }
}

#[test]
fn a_new_game_folder_dll_cannot_hide_behind_the_selected_bin_copy() {
    let directory = TestDirectory::new("dawn-runtime-precedence");
    let bin = directory.0.join("bin/x64");
    fs::create_dir_all(bin.join("Sunrise")).unwrap();
    fs::write(bin.join("steam_api64.dll"), b"bin runtime").unwrap();
    let mut app = app(directory.0.clone());
    app.settings_path = bin.join("Sunrise/settings.json");
    fs::write(directory.0.join("steam_api64.dll"), b"new root runtime").unwrap();
    let error = app.validation_warning_for_write(&app.document).unwrap_err();
    assert!(error.contains("takes precedence"), "{error}");
    assert!(error.contains("matching settings"), "{error}");

    let mut inspection = RuntimeInspection::inspect(&directory.0);
    // The old bin copy still matches v18, but Dawn at the root takes precedence.
    inspection.copies[1].bundled_schema = Some(18);
    set_runtime(&mut inspection, true, 6);
    let error = inspection
        .workspace_problem(&app.settings_path, &sunrise_v18())
        .unwrap();
    assert!(error.contains("takes precedence"), "{error}");
    inspection.copies.reverse();
    assert_eq!(
        inspection.launch_copy().unwrap().location,
        RuntimeLocation::Root
    );
}
