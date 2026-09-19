use std::fs;

use crate::test_support::TestDirectory;

use super::*;
use crate::app::account_workspace::WorkspaceDocument;
use serde_json::json;

#[test]
fn report_lists_runtime_files_databases_bins_and_relevant_packages_without_contents() {
    let directory = TestDirectory::new("troubleshooting-report");
    let install = &directory.0;
    let sunrise = install.join("bin").join("x64").join("Sunrise");
    let cache = sunrise.join("cache");
    let packages = install.join("packages");
    fs::create_dir_all(&cache).unwrap();
    fs::create_dir_all(&packages).unwrap();
    let settings = sunrise.join("settings.json");
    let database = crate::persistence::investment_path(&settings);
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    fs::write(&settings, br#"{"secret":"not-in-report"}"#).unwrap();
    fs::write(&database, b"private account bytes").unwrap();
    fs::write(cache.join("build_data.bin"), b"private cache bytes").unwrap();
    let mut runtime_state_header = Vec::from(0x5352_5354u32.to_le_bytes());
    runtime_state_header.extend_from_slice(&1u32.to_le_bytes());
    fs::write(sunrise.join("runtime-state.bin"), runtime_state_header).unwrap();
    fs::write(sunrise.join("runtime.toml"), b"private runtime settings").unwrap();
    fs::write(
        packages.join("w64_investment_globals_client_058c_4.pkg"),
        b"package",
    )
    .unwrap();
    fs::write(packages.join("w64_other_0001_0.pkg"), b"stock").unwrap();

    let report = build_report(&ReportContext {
        install_path: install,
        settings_path: &settings,
        settings_layout: "bin_x64",
        sunrise_version: "test",
        settings_schema: Some(8),
        account_source: &WorkspaceDocument::load(json!({"version": 8}), &settings, false)
            .source_info(),
        catalog: CatalogSummary {
            cache_path: &directory.0.join("catalog.json"),
            loaded_from_cache: true,
            items: 1,
            plugs: 2,
            icons: 3,
            descriptions: 4,
            unlock_flags: 5,
            unlock_values: 6,
            progressions: 7,
            objectives: 8,
            expressions: 9,
            progression_error: Some("Missing table\nread failed"),
        },
        recent_activity: "[Error] Earlier failure",
        current_status: "Ready",
        source_warning: None,
        has_unsaved_changes: false,
        destiny_process_status: "not_running",
    });

    assert!(report.contains("runtime.toml"));
    assert!(report.contains(
        "progression_counts = flags:5 values:6 progressions:7 objectives:8 expressions:9"
    ));
    assert!(report.contains("progression_package_error = Missing table read failed"));
    assert!(report.contains("[Error] Earlier failure"));
    assert_report_paths(&report);
    assert_account_source_details(&report);
    assert!(!report.contains("Sunrise .bin files"));
    assert!(report.contains("alternate_runtime_persistence_detected = true"));
    assert!(report.contains("detection_evidence = runtime_state_header"));
    assert!(report.contains("runtime_state_header = recognized | format_version=1"));
    assert!(report.contains("w64_investment_globals_client_058c_4.pkg"));
    assert!(!report.contains("w64_other_0001_0.pkg"));
    assert!(!report.contains("not-in-report"));
    assert!(!report.contains("private account bytes"));
    assert!(!report.contains("private cache bytes"));
}

fn assert_report_paths(report: &str) {
    assert!(report.contains("cache\\build_data.bin") || report.contains("cache/build_data.bin"));
    if let Some(data_directory) = crate::package_authoring::parhelion_data_directory() {
        assert!(report.contains(&format!(
            "parhelion_package_backups_directory = {}",
            data_directory.join("backups").join("packages").display()
        )));
    }
}

fn assert_account_source_details(report: &str) {
    assert!(!report.contains("state_db"));
    assert!(!report.contains("state_sqlite3"));
    assert!(report.contains("account_source = settings.json"));
    assert!(report.contains("active_account_json = "));
    assert!(!report.contains("active_account_database = "));
    assert!(report.contains("investment_database = "));
}

#[test]
fn account_candidates_use_data_directory_in_every_supported_layout() {
    let directory = TestDirectory::new("diagnostic-account-candidates");
    for layout in SettingsLayout::ALL {
        let settings = settings_path_for_install(&directory.0, layout);
        let database = crate::persistence::investment_path(&settings);
        fs::create_dir_all(database.parent().unwrap()).unwrap();
        fs::write(settings, b"private settings").unwrap();
        fs::write(database, b"private account").unwrap();
    }
    let mut report = String::new();
    append_settings_candidates(&mut report, &directory.0);
    for relative in [
        "data/investment.sqlite3",
        "Sunrise/data/investment.sqlite3",
        "bin/x64/Sunrise/data/investment.sqlite3",
    ] {
        let path = directory.0.join(relative.split('/').collect::<PathBuf>());
        assert!(
            report.contains(&format!(
                "investment_database = {} | kind=file",
                path.display()
            )),
            "{report}"
        );
    }
    assert!(!report.contains("state.db"));
    assert!(!report.contains("state_sqlite3"));
    assert!(!report.contains("private"));
    assert!(!report.contains("missing"));
}

#[test]
fn account_paths_follow_loaded_json_sqlite_and_blocked_sources() {
    for layout in SettingsLayout::ALL {
        let directory = TestDirectory::new("diagnostic-active-account");
        let settings = settings_path_for_install(&directory.0, layout);
        let database = crate::persistence::investment_path(&settings);
        fs::create_dir_all(database.parent().unwrap()).unwrap();
        crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
        for (version, kind, key) in [
            (8, AccountSourceKind::Json, "active_account_json"),
            (18, AccountSourceKind::Sqlite, "active_account_database"),
        ] {
            fs::write(
                &settings,
                serde_json::to_vec(&json!({"version": version})).unwrap(),
            )
            .unwrap();
            let workspace = WorkspaceDocument::load(json!({"version": version}), &settings, false);
            let source = workspace.source_info();
            assert_eq!(source.kind, kind);
            let mut report = String::new();
            append_path_section(&mut report, &context(&directory.0, &settings, &source));
            append_workspace_section(&mut report, &context(&directory.0, &settings, &source));
            let account_path = if version == 8 { &settings } else { &database };
            assert!(
                report.contains(&format!("{key} = {} | kind=file", account_path.display())),
                "{report}"
            );
            assert!(!report.contains("required_account_database = "));
            assert_eq!(report.contains("active_account_json = "), version == 8);
            assert_eq!(report.contains("active_account_database = "), version == 18);
        }
        let missing_settings = settings_path_for_install(&directory.0.join("missing"), layout);
        let blocked =
            WorkspaceDocument::load(json!({"version": 18}), &missing_settings, false).source_info();
        assert_eq!(blocked.kind, AccountSourceKind::Blocked);
        let mut report = String::new();
        append_path_section(
            &mut report,
            &context(&directory.0, &missing_settings, &blocked),
        );
        assert!(report.contains(&format!(
            "required_account_database = {} | missing",
            blocked.database_path.display()
        )));
        assert!(!report.contains("active_account_json = "));
        assert!(!report.contains("active_account_database = "));
    }
}

fn context<'a>(
    install: &'a Path,
    settings: &'a Path,
    source: &'a AccountSourceInfo,
) -> ReportContext<'a> {
    ReportContext {
        install_path: install,
        settings_path: settings,
        settings_layout: "test",
        sunrise_version: "test",
        settings_schema: Some(if source.kind == AccountSourceKind::Json {
            8
        } else {
            18
        }),
        account_source: source,
        catalog: CatalogSummary {
            cache_path: install,
            loaded_from_cache: false,
            items: 0,
            plugs: 0,
            icons: 0,
            descriptions: 0,
            unlock_flags: 0,
            unlock_values: 0,
            progressions: 0,
            objectives: 0,
            expressions: 0,
            progression_error: None,
        },
        recent_activity: "",
        current_status: "Ready",
        source_warning: None,
        has_unsaved_changes: false,
        destiny_process_status: "not_running",
    }
}

#[test]
fn runtime_report_includes_game_root_database_sidecars_and_cache_once() {
    let directory = TestDirectory::new("diagnostic-game-root-data");
    for relative in [
        "data/investment.sqlite3",
        "data/investment.sqlite3-wal",
        "data/investment.sqlite3-shm",
        "cache/build_data.bin",
    ] {
        let path = directory.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"private runtime bytes").unwrap();
    }
    let mut report = String::new();
    append_sunrise_runtime_files(&mut report, &directory.0);
    for file in [
        "investment.sqlite3",
        "investment.sqlite3-wal",
        "investment.sqlite3-shm",
        "build_data.bin",
    ] {
        assert_eq!(
            report.matches(&format!("{file} | kind=file")).count(),
            1,
            "{report}"
        );
    }
    assert!(!report.contains("private runtime bytes"));
    assert!(!report.contains("Sunrise .bin files"));
}

#[test]
fn runtime_scan_is_recursive_and_sorted() {
    let directory = TestDirectory::new("troubleshooting-runtime-scan");
    fs::create_dir_all(directory.0.join("nested")).unwrap();
    fs::write(directory.0.join("z.bin"), b"z").unwrap();
    fs::write(directory.0.join("nested").join("a.json"), b"a").unwrap();

    let scan = scan_runtime_folder(&directory.0);
    let names = scan
        .files
        .iter()
        .map(|file| file.relative_path.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["nested/a.json", "z.bin"]);
    assert!(scan.errors.is_empty());
    assert!(!scan.truncated);
}

#[test]
fn startup_failure_report_keeps_paths_and_error_without_reading_files() {
    let directory = TestDirectory::new("troubleshooting-startup-failure");
    fs::create_dir_all(directory.0.join("Sunrise")).unwrap();
    fs::write(
        directory.0.join("Sunrise").join("settings.json"),
        b"private settings",
    )
    .unwrap();

    let report = build_startup_failure_report(
        Some(&directory.0),
        "Could not parse settings\nprivate detail",
    );
    assert!(report.contains("Startup Failure"));
    assert!(report.contains("Could not parse settings private detail"));
    assert!(!report.contains("alternate_runtime_persistence_detected"));
    assert!(report.contains("Sunrise"));
    assert!(report.contains("settings.json"));
    assert!(!report.contains("private settings"));
}

#[test]
fn log_initialization_preserves_old_sessions_and_events_append() {
    let directory = TestDirectory::new("troubleshooting-log-write");
    let log = directory.0.join("nested").join("troubleshooting.log");

    initialize_log_at(&log, "first session").unwrap();
    append_log_text_at(&log, "\nstatus event").unwrap();
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .ends_with("first session\nstatus event")
    );

    initialize_log_at(&log, "second session").unwrap();
    let text = fs::read_to_string(log).unwrap();
    assert!(text.contains("first session\nstatus event"));
    assert!(text.ends_with("second session"));
}
