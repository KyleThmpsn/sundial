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

/// Resolve an installed Dawn runtime, synchronize its authored unlock, and independently reload
/// the durable account. Unsupported layouts must leave the account and its settings untouched.
#[test]
#[expect(
    clippy::cognitive_complexity,
    reason = "The account lifecycle keeps accepted writes, refused layouts and independent readback together"
)]
fn dawn_collection_sync_preserves_accounts_and_refuses_unsupported_layouts() {
    use crate::persistence::native_account::snapshot;
    use rusqlite::{Connection, OpenFlags};

    let read = |path: &Path| {
        let db = snapshot::open(path, false).unwrap();
        snapshot::snapshot(&db).unwrap()
    };

    let cases = [
        ("supported", "", None),
        ("older", "PRAGMA user_version=4", Some("schema version 4")),
        ("newer", "PRAGMA user_version=6", Some("schema version 6")),
        (
            "unknown",
            "PRAGMA user_version=99",
            Some("schema version 99"),
        ),
        (
            "extended-account",
            "ALTER TABLE account ADD COLUMN future_value TEXT",
            Some("account has unrecognized columns"),
        ),
        (
            "trigger",
            "CREATE TRIGGER future_unlock AFTER INSERT ON durable_flags BEGIN DELETE FROM editor_notes; END",
            Some("unrecognized trigger future_unlock"),
        ),
    ];
    let mut evidence = Vec::new();
    for (name, change, refused) in cases {
        let directory = fixture::install();
        let root = directory.path();
        fs::create_dir(root.join("Dawn")).unwrap();
        fs::write(
            root.join("steam_api64.dll"),
            fixture::module_with_schema("Dawn", 6),
        )
        .unwrap();
        let settings = root.join("Dawn/settings.json");
        let original = serde_json::to_vec(&json!({"version":6,"keep":"unchanged"})).unwrap();
        fs::write(&settings, &original).unwrap();
        let player_state = crate::persistence::dawn_path(&settings);
        crate::persistence::dawn_account::tests::create_fixture(&player_state);
        let (target, durable) =
            authored_unlock_target(root, &preferences(root, "dawn_root")).unwrap();
        assert!(durable);
        assert_eq!(target, player_state);
        {
            let db = Connection::open(&target).unwrap();
            db.execute_batch("CREATE TABLE editor_notes(note TEXT); INSERT INTO editor_notes VALUES('keep this row')")
                .unwrap();
            db.execute_batch(change).unwrap();
        }
        let before = read(&target);
        let bytes = fs::read(&target).unwrap();
        let backup_directory =
            crate::backups::source_directory(&crate::backups::root().unwrap(), &target).unwrap();
        let rows = dawn_rows(&[(
            1,
            crate::account::contract::SHADOWKEEP_ACCOUNT_FLAG_BANK,
            11_930,
        )])
        .unwrap();
        let result = crate::persistence::dawn_account::apply_authored_unlocks(&target, &rows);
        if let Some(expected) = refused {
            let error = result.unwrap_err().to_string();
            assert!(error.contains(expected), "{name}: {error}");
            assert_eq!(read(&target), before, "{name}");
            assert_eq!(fs::read(&target).unwrap(), bytes, "{name}");
            assert!(!backup_directory.exists(), "{name}");
            evidence.push(json!({"case":name,"error":error,"before":before,"after":read(&target)}));
        } else {
            let receipt = result.unwrap();
            assert_eq!(receipt.changed, 1);
            assert_eq!(read(receipt.backup.as_ref().unwrap()), before);
            let after = read(&target);
            for (table, rows) in &before.tables {
                if table != "metadata" && table != "durable_flags" {
                    assert_eq!(after.tables.get(table), Some(rows), "{table}");
                }
            }
            let db =
                Connection::open_with_flags(&target, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let (owner, stored): (String, i64) = db
                .query_row(
                    "SELECT owner_soid,value FROM durable_flags WHERE scope=0 AND slot=11930",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(owner, "9EAA300100100100");
            assert_eq!(stored, i64::from(sundial_account::FLAG_SET));
            drop(db);
            let repeated =
                crate::persistence::dawn_account::apply_authored_unlocks(&target, &rows).unwrap();
            assert_eq!(repeated.changed, 0);
            assert!(repeated.backup.is_none());
            assert_eq!(read(&target), after);
            // Already-set flags must still refuse a database that has changed its schema.
            Connection::open(&target)
                .unwrap()
                .pragma_update(None, "user_version", 6)
                .unwrap();
            let unsupported = read(&target);
            let error = crate::persistence::dawn_account::apply_authored_unlocks(&target, &rows)
                .unwrap_err()
                .to_string();
            assert!(error.contains("schema version 6"), "{error}");
            assert_eq!(read(&target), unsupported);
            evidence.push(json!({"case":name,"before":before,"after":after,"repeated_changed":repeated.changed,"unsupported_noop_error":error}));
        }
        assert_eq!(fs::read(&settings).unwrap(), original, "{name}");
    }
    crate::test_support::artifact(
        "dawn-collection-schema-readback.json",
        &json!({"cases":evidence}),
    );
}
