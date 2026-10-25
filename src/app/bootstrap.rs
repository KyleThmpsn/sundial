use std::env;

use super::{InstallSelection, preferences::store::LoadedPreferences, settings::load_preferences};

pub(super) fn parse_args() -> (Option<InstallSelection>, bool, LoadedPreferences) {
    let preferences = load_preferences();
    let mut install = preferences.preferences.install_selection();
    let mut check_only = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--install" => {
                if let Some(value) = args.next() {
                    let install_path = std::path::PathBuf::from(value);
                    // An update restart names the installation it left, so the layout saved
                    // for that installation still applies. Another installation has none yet.
                    let preferred_layout = install
                        .as_ref()
                        .filter(|saved| {
                            crate::system::paths::paths_equal(&saved.install_path, &install_path)
                        })
                        .and_then(|saved| saved.preferred_layout);
                    install = Some(InstallSelection {
                        install_path,
                        preferred_layout,
                    });
                }
            }
            "--check" => check_only = true,
            _ => {}
        }
    }
    (install, check_only, preferences)
}
