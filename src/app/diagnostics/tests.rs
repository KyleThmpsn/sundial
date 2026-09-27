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
        runtime_name: "Sunrise",
        runtime_version: "0.5",
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
    assert!(report.contains("format_version = 5"));
    assert!(report.contains("detected_runtime_name = Sunrise"));
    assert!(report.contains("detected_runtime_version = 0.5"));
    assert!(report.contains("settings_json_schema = 8"));
    assert!(report.contains(
        "progression_counts = flags:5 values:6 progressions:7 objectives:8 expressions:9"
    ));
    assert!(report.contains("progression_package_error = Missing table read failed"));
    assert!(report.contains("[Error] Earlier failure"));
    assert_report_paths(&report);
    assert_account_source_details(&report);
    assert!(report.contains("alternate_runtime_persistence_detected = true"));
    assert!(report.contains("detection_evidence = runtime_state_header"));
    assert!(report.contains("runtime_state_header = recognized | format_version=1"));
    assert_report_privacy(&report);
}

fn assert_report_privacy(report: &str) {
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
    assert!(report.contains("account_source = settings.json"));
    assert!(report.contains("active_account_json = "));
    assert!(!report.contains("active_account_database = "));
    assert!(report.contains("investment_database = "));
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
