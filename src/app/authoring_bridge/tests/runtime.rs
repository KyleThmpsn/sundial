use super::*;
use crate::package_runtime::{installation::RuntimeLocation, tests::fixture};

fn preferences(root: &Path, layout: &str) -> crate::app::Preferences {
    crate::app::Preferences {
        install: Some(root.to_owned()),
        settings_layout: Some(layout.into()),
        ..Default::default()
    }
}

#[test]
fn authored_accounts_reject_inactive_settings_even_without_a_second_dll() {
    for brand in ["Sunrise", "Dawn"] {
        for location in RuntimeLocation::ALL {
            let directory = fixture::install();
            let root = directory.path();
            let paths = [
                ("root", root.join("Sunrise/settings.json")),
                ("bin_x64", root.join("bin/x64/Sunrise/settings.json")),
            ];
            for (_, path) in &paths {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, br#"{"version":6,"keep":"unchanged"}"#).unwrap();
            }
            let active = location.directory(root);
            fs::write(
                active.join("steam_api64.dll"),
                fixture::module(brand, false, None),
            )
            .unwrap();
            for (layout, path) in &paths {
                let before = fs::read(path).unwrap();
                let result = authored_unlock_settings_path(root, &preferences(root, layout));
                if *path == active.join("Sunrise/settings.json") {
                    assert_eq!(result.unwrap(), *path);
                } else {
                    assert!(result.unwrap_err().contains("takes precedence"));
                }
                assert_eq!(fs::read(path).unwrap(), before);
            }
        }
    }
}

#[test]
fn authored_accounts_require_the_active_runtime_format_and_keep_both_sources_unchanged() {
    for (brand, runtime_schema) in [("Dawn", 6), ("Sunrise", 18)] {
        let directory = fixture::install();
        let root = directory.path();
        fs::create_dir(root.join("Sunrise")).unwrap();
        fs::write(
            root.join("steam_api64.dll"),
            fixture::module_with_schema(brand, runtime_schema),
        )
        .unwrap();
        let path = root.join("Sunrise/settings.json");
        let database = crate::persistence::investment_path(&path);
        crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
        let database_before = fs::read(&database).unwrap();
        for schema in [6, 18] {
            let original = serde_json::to_vec(&json!({"version":schema})).unwrap();
            fs::write(&path, &original).unwrap();
            let result = authored_unlock_settings_path(root, &preferences(root, "root"));
            if schema == runtime_schema {
                assert_eq!(result.unwrap(), path);
            } else {
                assert!(result.unwrap_err().contains("requires settings"));
            }
            assert_eq!(fs::read(&path).unwrap(), original);
            assert_eq!(fs::read(&database).unwrap(), database_before);
        }
    }
}
