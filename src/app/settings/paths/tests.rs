use crate::app::settings::load_json;
use crate::app::settings::load_workspace_json;
use crate::app::settings::save_json_with_backup_root;
use crate::app::settings::settings_path_for_install;
use crate::app::*;
use crate::test_support::TestDirectory;
use std::fs;

#[test]
fn loading_a_missing_selected_settings_file_never_creates_it() {
    let directory = TestDirectory::new("save");
    let settings = settings_path_for_install(&directory.0, SettingsLayout::BinX64);

    let error = load_json(&settings).unwrap_err();

    assert!(error.contains("No Project Sunrise settings.json was found"));
    assert!(!settings.exists());
}

#[test]
fn workspace_json_accepts_the_utf8_bom_that_sunrise_accepts() {
    let directory = TestDirectory::new("settings-bom");
    let settings = directory.0.join("settings.json");
    let source = b"\xEF\xBB\xBF{\"version\":8}\r\n";
    fs::write(&settings, source).unwrap();

    let document = load_workspace_json(&settings).unwrap();
    assert_eq!(document, serde_json::json!({"version": 8}));

    let result =
        save_json_with_backup_root(&settings, &document, &directory.0.join("backups")).unwrap();
    assert!(
        result
            .backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("settings-v8-")
    );
    assert_eq!(fs::read(result.backup).unwrap(), source);
}
