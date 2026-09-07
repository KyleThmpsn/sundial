use crate::app::settings::load_json;
use crate::app::settings::load_workspace_json;
use crate::app::settings::normalize_sunrise_version;
use crate::app::settings::resolve_settings_path;
use crate::app::settings::save_json_with_backup_root;
use crate::app::settings::settings_path_for_install;
use crate::app::*;
use crate::test_support::TestDirectory;
use std::fs;

#[test]
fn sunrise_versions_are_normalized_for_display() {
    assert_eq!(normalize_sunrise_version("0.3.2.0"), Some("0.3.2".into()));
    assert_eq!(normalize_sunrise_version("0.3.1.0"), Some("0.3.1".into()));
    assert_eq!(normalize_sunrise_version("0.2.1.0"), Some("0.2.1".into()));
    assert_eq!(normalize_sunrise_version("0.2.0.0"), Some("0.2".into()));
    assert_eq!(normalize_sunrise_version("0.1.0.0"), Some("0.1".into()));
    assert_eq!(normalize_sunrise_version(" 1.4.2 "), Some("1.4.2".into()));
    assert_eq!(normalize_sunrise_version("0"), None);
    assert_eq!(normalize_sunrise_version("not-a-version"), None);
}

#[test]
fn settings_resolution_uses_the_only_existing_file_and_never_creates_one() {
    let directory = TestDirectory::new("save");
    assert!(matches!(
        resolve_settings_path(&directory.0, None),
        SettingsPathResolution::Missing
    ));
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);

    let root = settings_path_for_install(&directory.0, SettingsLayout::Root);
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    fs::write(&root, b"{}\n").unwrap();

    assert!(matches!(
        resolve_settings_path(&directory.0, None),
        SettingsPathResolution::Found(SettingsLayout::Root, path) if path == root
    ));
}

#[test]
fn settings_resolution_accepts_a_game_root_settings_file() {
    let directory = TestDirectory::new("game-root-settings");
    let settings = settings_path_for_install(&directory.0, SettingsLayout::GameRoot);
    fs::write(&settings, b"{}\n").unwrap();

    assert!(matches!(
        resolve_settings_path(&directory.0, None),
        SettingsPathResolution::Found(SettingsLayout::GameRoot, path) if path == settings
    ));
}

#[test]
fn settings_resolution_requires_a_choice_when_both_files_exist() {
    let directory = TestDirectory::new("save");
    let root = settings_path_for_install(&directory.0, SettingsLayout::Root);
    let bin_x64 = settings_path_for_install(&directory.0, SettingsLayout::BinX64);
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    fs::create_dir_all(bin_x64.parent().unwrap()).unwrap();
    fs::write(&root, b"{\"layout\":\"root\"}\n").unwrap();
    fs::write(&bin_x64, b"{\"layout\":\"bin\"}\n").unwrap();

    assert!(matches!(
        resolve_settings_path(&directory.0, None),
        SettingsPathResolution::Ambiguous
    ));
    assert!(matches!(
        resolve_settings_path(&directory.0, Some(SettingsLayout::BinX64)),
        SettingsPathResolution::Found(SettingsLayout::BinX64, path) if path == bin_x64
    ));
    assert_eq!(fs::read_to_string(root).unwrap(), "{\"layout\":\"root\"}\n");
    assert_eq!(
        fs::read_to_string(bin_x64).unwrap(),
        "{\"layout\":\"bin\"}\n"
    );
}

#[test]
fn generated_settings_paths_match_each_layout() {
    let install = std::path::Path::new("install");
    let game_root = settings_path_for_install(install, SettingsLayout::GameRoot);
    let root = settings_path_for_install(install, SettingsLayout::Root);
    let bin_x64 = settings_path_for_install(install, SettingsLayout::BinX64);

    assert_eq!(game_root, install.join("settings.json"));
    assert_eq!(root, install.join("Sunrise").join("settings.json"));
    assert_eq!(
        bin_x64,
        install
            .join("bin")
            .join("x64")
            .join("Sunrise")
            .join("settings.json")
    );
}

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
