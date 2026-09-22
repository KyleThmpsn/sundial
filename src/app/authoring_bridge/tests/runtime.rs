use super::*;
use crate::package_runtime::{installation::RuntimeLocation, tests::fixture};
use std::collections::BTreeMap;

fn preferences(root: &Path, layout: &str) -> crate::app::Preferences {
    crate::app::Preferences {
        install: Some(root.to_owned()),
        settings_layout: Some(layout.into()),
        ..Default::default()
    }
}

#[test]
fn authored_accounts_follow_the_installed_dll_rather_than_a_saved_layout() {
    for brand in ["Sunrise", "Dawn"] {
        for location in RuntimeLocation::ALL {
            let directory = fixture::install();
            let root = directory.path();
            // A runtime owns the folder named after it, so the layouts under test are that
            // runtime's own two locations.
            let folder = if brand == "Dawn" { "Dawn" } else { "Sunrise" };
            let prefix = if brand == "Dawn" { "dawn_" } else { "" };
            let paths = [
                (
                    format!("{prefix}root"),
                    root.join(folder).join("settings.json"),
                ),
                (
                    format!("{prefix}bin_x64"),
                    root.join("bin/x64").join(folder).join("settings.json"),
                ),
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
            // Swapping the DLL is how a player changes runtime, so whichever copy is installed
            // decides the settings, and a layout saved against the other one does not drag the
            // account back to a file the game no longer reads.
            let live = active.join(folder).join("settings.json");
            for (layout, path) in &paths {
                let before = fs::read(path).unwrap();
                let resolved = authored_unlock_settings_path(root, &preferences(root, layout));
                assert_eq!(resolved.unwrap(), live, "{brand} at {}", active.display());
                assert_eq!(fs::read(path).unwrap(), before);
            }
        }
    }
}

#[test]
fn authored_accounts_require_the_active_runtime_format_and_keep_both_sources_unchanged() {
    // Sunrise moved its accounts from settings.json into data/investment.sqlite3 at v18, so a
    // copy of either vintage must refuse settings of the other. Dawn keeps its account in
    // player-state.db at every schema and is checked by its own runtime validation instead.
    for (brand, runtime_schema) in [("Sunrise", 6), ("Sunrise", 18)] {
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

/// An authored unlock has to reach whichever account the installed runtime actually reads. Only
/// the DLL says which that is, so the target is chosen from the runtime rather than from the
/// settings document, which both runtimes keep in the same place.
#[test]
fn authored_unlocks_target_the_account_the_installed_runtime_reads() {
    for brand in ["Dawn", "Sunrise"] {
        let directory = fixture::install();
        let root = directory.path();
        let folder = if brand == "Dawn" { "Dawn" } else { "Sunrise" };
        fs::create_dir(root.join(folder)).unwrap();
        fs::write(
            root.join("steam_api64.dll"),
            fixture::module_with_schema(brand, 6),
        )
        .unwrap();
        let path = root.join(folder).join("settings.json");
        fs::write(&path, serde_json::to_vec(&json!({"version":6})).unwrap()).unwrap();

        let layout = if brand == "Dawn" { "dawn_root" } else { "root" };
        let (target, durable) = authored_unlock_target(root, &preferences(root, layout)).unwrap();

        if brand == "Dawn" {
            assert!(durable, "Dawn keeps its unlocks in player-state.db");
            assert_eq!(target, crate::persistence::dawn_path(&path));
        } else {
            assert!(!durable, "Sunrise keeps its unlocks with its settings");
            assert_eq!(target, path);
        }
    }
}

#[test]
fn replacement_cleanup_follows_the_runtime_and_does_not_fall_back_from_missing_dawn() {
    for (brand, schema) in [("Sunrise", 8), ("Sunrise", 18), ("Dawn", 6)] {
        let directory = fixture::install();
        let root = directory.path();
        fs::write(
            root.join("steam_api64.dll"),
            fixture::module_with_schema(brand, schema),
        )
        .unwrap();
        let folder = if brand == "Dawn" { "Dawn" } else { "Sunrise" };
        let settings = root.join(folder).join("settings.json");
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        let original = serde_json::to_vec(&json!({"version":schema,"state": {
            "account": {"primary_soid": "0x0000000000000001"},
            "characters": [{"soid": "0x0000000000000002", "class": 0,
                "equipment": {}, "inventory": [{"instance_soid": "0x0000000000000003",
                    "definition_hash": 300, "level": 106, "quantity": 1, "plugs": null}]}]
        }}))
        .unwrap();
        fs::write(&settings, &original).unwrap();
        let dawn = crate::persistence::dawn_path(&settings);
        let sunrise = crate::persistence::investment_path(&settings);
        crate::persistence::dawn_account::tests::create_fixture(&dawn);
        crate::persistence::sqlite_account::tests::create_fixture(&sunrise, 3);
        let dawn_before = crate::account::read_authored_account_source(&dawn).unwrap();
        let sunrise_before = crate::persistence::sqlite_account::snapshot::read(&sunrise).unwrap();
        let review =
            preview_account_cleanup(root, &BTreeSet::from([2715114534, 300]), &[]).unwrap();
        let expected = match (brand, schema) {
            ("Dawn", _) => &dawn,
            (_, 18) => &sunrise,
            _ => &settings,
        };
        assert_eq!(&review.settings_path, expected);
        let removed = match (brand, schema) {
            ("Dawn", _) => BTreeMap::from([(2715114534, 1)]),
            _ => BTreeMap::from([(300, 1)]),
        };
        assert_eq!(review.removed_items, removed);
        assert_eq!(fs::read(&settings).unwrap(), original);
        assert_eq!(
            crate::account::read_authored_account_source(&dawn).unwrap(),
            dawn_before
        );
        assert_eq!(
            crate::persistence::sqlite_account::snapshot::read(&sunrise).unwrap(),
            sunrise_before
        );
        if brand == "Dawn" {
            fs::remove_file(&dawn).unwrap();
            assert!(preview_account_cleanup(root, &BTreeSet::from([300]), &[]).is_err());
            assert!(!dawn.exists());
        }
    }
}

#[test]
fn authored_account_operations_fail_closed_without_a_recognized_runtime() {
    for module in [Some(b"unrecognized runtime".as_slice()), None] {
        let directory = fixture::install();
        let root = directory.path();
        if let Some(module) = module {
            fs::write(root.join("steam_api64.dll"), module).unwrap();
        }
        for folder in ["Dawn", "Sunrise"] {
            let settings = root.join(folder).join("settings.json");
            fs::create_dir_all(settings.parent().unwrap()).unwrap();
            fs::write(&settings, br#"{"version":6,"state":{"characters":[]}}"#).unwrap();
        }
        let dawn = crate::persistence::dawn_path(&root.join("Dawn/settings.json"));
        let sunrise = root.join("Sunrise/settings.json");
        crate::persistence::dawn_account::tests::create_fixture(&dawn);
        let dawn_before = crate::account::read_authored_account_source(&dawn).unwrap();
        let sunrise_before = fs::read(&sunrise).unwrap();

        assert!(preview_account_cleanup(root, &BTreeSet::from([300]), &[]).is_err());
        assert!(authored_client_settings_path(root).is_err());
        assert_eq!(
            crate::account::read_authored_account_source(&dawn).unwrap(),
            dawn_before
        );
        assert_eq!(fs::read(sunrise).unwrap(), sunrise_before);
    }
}

/// The durable account is written through the same storage-neutral rows the settings path uses,
/// and a Dawn install gains the flag without its settings document being touched at all.
#[test]
fn an_authored_unlock_reaches_a_dawn_durable_account() {
    let directory = fixture::install();
    let root = directory.path();
    fs::create_dir(root.join("Sunrise")).unwrap();
    let path = root.join("Sunrise/settings.json");
    let original = serde_json::to_vec(&json!({"version":6})).unwrap();
    fs::write(&path, &original).unwrap();
    let player_state = crate::persistence::dawn_path(&path);
    crate::persistence::dawn_account::tests::create_fixture(&player_state);

    let rows = dawn_rows(&[(
        1,
        crate::account_contract::SHADOWKEEP_ACCOUNT_FLAG_BANK,
        11_930,
    )])
    .unwrap();
    let receipt =
        crate::persistence::dawn_account::apply_authored_unlocks(&player_state, &rows).unwrap();

    assert_eq!(receipt.changed, 1);
    let stored: i64 = rusqlite::Connection::open(&player_state)
        .unwrap()
        .query_row(
            "SELECT value FROM durable_flags WHERE scope=0 AND slot=11930",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, i64::from(sundial_account::FLAG_SET));
    assert_eq!(
        fs::read(&path).unwrap(),
        original,
        "Dawn keeps no unlocks in its settings document"
    );
}
