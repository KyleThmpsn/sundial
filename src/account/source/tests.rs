use super::*;
use crate::test_support::TestDirectory;
use std::fs;
#[test]
fn settings_resolution_uses_the_only_existing_file_and_never_creates_one() {
    for layout in [SettingsLayout::Root, SettingsLayout::GameRoot] {
        let directory = TestDirectory::new("save");
        assert!(matches!(
            resolve_settings_path(&directory.0, None),
            SettingsPathResolution::Missing
        ));
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
        let settings = settings_path_for_install(&directory.0, layout);
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(&settings, b"{}\n").unwrap();
        assert!(matches!(resolve_settings_path(&directory.0, None),
            SettingsPathResolution::Found(found, path) if found == layout && path == settings));
    }
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
