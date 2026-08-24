use std::env;

use super::{InstallSelection, Preferences, settings::load_preferences};

pub(super) fn parse_args() -> (Option<InstallSelection>, bool, Preferences) {
    let preferences = load_preferences();
    let mut install = preferences.install_selection();
    let mut check_only = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--install" => {
                if let Some(value) = args.next() {
                    install = Some(InstallSelection {
                        install_path: value.into(),
                        preferred_layout: None,
                    });
                }
            }
            "--check" => check_only = true,
            _ => {}
        }
    }
    (install, check_only, preferences)
}
