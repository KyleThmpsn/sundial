use crate::app::settings::load_json;
use crate::app::settings::settings_path_for_install;
use crate::app::*;
use crate::test_support::TestDirectory;

#[test]
fn loading_a_missing_selected_settings_file_never_creates_it() {
    let directory = TestDirectory::new("save");
    let settings = settings_path_for_install(&directory.0, SettingsLayout::BinX64);

    let error = load_json(&settings).unwrap_err();

    assert!(error.contains("No Project Sunrise settings.json was found"));
    assert!(!settings.exists());
}
