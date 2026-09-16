use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    package_authoring::{parhelion_data_directory, parhelion_recipe_library_directory},
    package_runtime::sunrise_module_path,
    paths,
};

use super::{
    SettingsLayout,
    account_workspace::{AccountSourceInfo, AccountSourceKind},
    persistence_compatibility::{PersistenceCompatibility, WARNING_MESSAGE},
    settings::settings_path_for_install,
};

const MAX_LOG_BYTES: usize = 5 * 1024 * 1024;
const LOG_DIRECTORY_NAME: &str = "logs";
const LOG_FILE_NAME: &str = "sundial-troubleshooting.log";

mod activity;
mod runtime;

use runtime::append_sunrise_runtime_files;
#[cfg(test)]
use runtime::scan_runtime_folder;

pub(super) struct CatalogSummary<'a> {
    pub cache_path: &'a Path,
    pub loaded_from_cache: bool,
    pub items: usize,
    pub plugs: usize,
    pub icons: usize,
    pub descriptions: usize,
    pub unlock_flags: usize,
    pub unlock_values: usize,
    pub progressions: usize,
    pub objectives: usize,
    pub expressions: usize,
    pub progression_error: Option<&'a str>,
}

pub(super) struct ReportContext<'a> {
    pub install_path: &'a Path,
    pub settings_path: &'a Path,
    pub settings_layout: &'a str,
    pub sunrise_version: &'a str,
    pub settings_schema: Option<u64>,
    pub account_source: &'a AccountSourceInfo,
    pub catalog: CatalogSummary<'a>,
    pub recent_activity: &'a str,
    pub current_status: &'a str,
    pub source_warning: Option<&'a str>,
    pub has_unsaved_changes: bool,
    pub destiny_process_status: &'a str,
}

pub(super) fn log_path() -> Option<PathBuf> {
    paths::data_dir().map(|path| path.join(LOG_DIRECTORY_NAME).join(LOG_FILE_NAME))
}

pub(super) fn build_report(context: &ReportContext<'_>) -> String {
    let mut report = report_header();

    append_build_information(&mut report);
    append_path_section(&mut report, context);
    append_workspace_section(&mut report, context);
    append_persistence_compatibility(&mut report, context.install_path);
    append_settings_candidates(&mut report, context.install_path);
    append_sunrise_runtime_files(&mut report, context.install_path);
    append_package_summary(&mut report, context.install_path);
    report.push_str(
        "Recent Sundial Activity (Newest First)\n-------------------------------------\n",
    );
    report.push_str(context.recent_activity);
    report.push_str("\n\n");

    activity::append_parhelion_log(&mut report);

    report
}

pub(super) fn build_startup_failure_report(install_path: Option<&Path>, error: &str) -> String {
    let mut report = report_header();
    append_build_information(&mut report);
    report.push_str("Startup Failure\n---------------\n");
    writeln!(report, "error = {}", error.replace(['\r', '\n'], " "))
        .expect("writing to a String cannot fail");
    writeln!(report).expect("writing to a String cannot fail");
    append_optional_known_path(&mut report, "sundial_config_directory", paths::config_dir());
    append_optional_known_path(&mut report, "sundial_data_directory", paths::data_dir());
    append_optional_known_path(&mut report, "sundial_cache_directory", paths::cache_dir());
    append_optional_known_path(&mut report, "troubleshooting_log", log_path());
    if let Some(install) = install_path {
        append_path(&mut report, "selected_install", install);
        append_path(
            &mut report,
            "destiny_executable",
            &install.join("destiny2.exe"),
        );
        append_path(&mut report, "sunrise_module", &sunrise_module_path(install));
        append_path(&mut report, "packages_directory", &install.join("packages"));
        writeln!(report).expect("writing to a String cannot fail");
        append_persistence_compatibility(&mut report, install);
        append_settings_candidates(&mut report, install);
        append_sunrise_runtime_files(&mut report, install);
        append_package_summary(&mut report, install);
    } else {
        report.push_str("selected_install = unavailable\n\n");
    }
    activity::append_parhelion_log(&mut report);
    report
}

fn report_header() -> String {
    let mut report = String::new();
    writeln!(report, "Sundial Troubleshooting Log").expect("writing to a String cannot fail");
    writeln!(report, "format_version = 4").expect("writing to a String cannot fail");
    writeln!(
        report,
        "generated_unix_seconds = {}",
        unix_seconds(SystemTime::now())
            .map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
    )
    .expect("writing to a String cannot fail");
    writeln!(report).expect("writing to a String cannot fail");
    report
}

pub(super) fn initialize_log(report: &str) -> Result<PathBuf, String> {
    let path = log_path().ok_or("Could not locate Sundial's local data folder")?;
    initialize_log_at(&path, report)?;
    Ok(path)
}

fn initialize_log_at(path: &Path, report: &str) -> Result<(), String> {
    append_log_text_at(
        path,
        &format!("\n===== Session Environment Snapshot =====\n{report}"),
    )
}

pub(super) fn append_snapshot(report: &str) -> Result<PathBuf, String> {
    append_log_text(&format!(
        "\n\n===== Refreshed Environment Snapshot =====\n{report}"
    ))
}

pub(super) fn append_status(message: &str, is_error: bool) -> Result<PathBuf, String> {
    let timestamp = unix_seconds(SystemTime::now())
        .map_or_else(|| "unavailable".to_owned(), |value| value.to_string());
    let level = if is_error { "ERROR" } else { "INFO" };
    let message = message.replace(['\r', '\n'], " ");
    append_log_text(&format!("\n[{timestamp}] {level}: {message}"))
}

fn append_log_text(text: &str) -> Result<PathBuf, String> {
    let path = log_path().ok_or("Could not locate Sundial's local data folder")?;
    append_log_text_at(&path, text)?;
    Ok(path)
}

fn append_log_text_at(path: &Path, text: &str) -> Result<(), String> {
    crate::activity_log::append_text_at(path, text, MAX_LOG_BYTES)
        .map_err(|error| format!("Could not update {}: {error}", path.display()))
}

fn append_build_information(report: &mut String) {
    report.push_str("Build and Process\n-----------------\n");
    writeln!(report, "sundial_version = {}", env!("CARGO_PKG_VERSION"))
        .expect("writing to a String cannot fail");
    writeln!(report, "operating_system = {}", std::env::consts::OS)
        .expect("writing to a String cannot fail");
    writeln!(report, "architecture = {}", std::env::consts::ARCH)
        .expect("writing to a String cannot fail");
    writeln!(report, "debug_build = {}", cfg!(debug_assertions))
        .expect("writing to a String cannot fail");
    writeln!(report, "process_id = {}", std::process::id())
        .expect("writing to a String cannot fail");
    append_optional_path(report, "current_executable", std::env::current_exe());
    append_optional_path(report, "working_directory", std::env::current_dir());
    writeln!(report).expect("writing to a String cannot fail");
}

fn append_path_section(report: &mut String, context: &ReportContext<'_>) {
    report.push_str("Resolved Paths\n--------------\n");
    append_path(report, "selected_install", context.install_path);
    append_path(
        report,
        "destiny_executable",
        &context.install_path.join("destiny2.exe"),
    );
    append_path(
        report,
        "sunrise_module",
        &sunrise_module_path(context.install_path),
    );
    append_path(
        report,
        "packages_directory",
        &context.install_path.join("packages"),
    );
    append_path(report, "active_settings", context.settings_path);
    if let Some(parent) = context.settings_path.parent() {
        append_path(report, "active_settings_directory", parent);
    }
    match context.account_source.kind {
        AccountSourceKind::Json => {
            append_path(report, "active_account_json", context.settings_path);
        }
        AccountSourceKind::Sqlite => append_path(
            report,
            "active_account_database",
            &context.account_source.database_path,
        ),
        AccountSourceKind::Blocked => append_path(
            report,
            "required_account_database",
            &context.account_source.database_path,
        ),
    }
    append_optional_known_path(report, "sundial_config_directory", paths::config_dir());
    append_optional_known_path(report, "sundial_data_directory", paths::data_dir());
    append_optional_known_path(report, "sundial_cache_directory", paths::cache_dir());
    append_optional_known_path(
        report,
        "sundial_preferences",
        super::settings::preferences_path(),
    );
    append_optional_known_path(
        report,
        "sundial_backups_directory",
        super::settings::backups_path(),
    );
    append_optional_known_path(
        report,
        "parhelion_data_directory",
        parhelion_data_directory(),
    );
    append_optional_known_path(
        report,
        "parhelion_recipe_library",
        parhelion_recipe_library_directory(),
    );
    append_optional_known_path(
        report,
        "parhelion_staging_directory",
        parhelion_data_directory().map(|path| path.join("staging")),
    );
    append_optional_known_path(
        report,
        "parhelion_package_backups_directory",
        parhelion_data_directory().map(|path| path.join("backups").join("packages")),
    );
    append_optional_known_path(report, "troubleshooting_log", log_path());
    append_path(report, "catalog_cache", context.catalog.cache_path);
    writeln!(report).expect("writing to a String cannot fail");
}

fn append_workspace_section(report: &mut String, context: &ReportContext<'_>) {
    report.push_str("Active Workspace\n----------------\n");
    writeln!(report, "settings_layout = {}", context.settings_layout)
        .expect("writing to a String cannot fail");
    writeln!(
        report,
        "settings_schema = {}",
        context.settings_schema.map_or_else(
            || "missing_or_invalid".to_owned(),
            |value| value.to_string()
        )
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "detected_sunrise_version = {}",
        context.sunrise_version
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "destiny_process = {}",
        context.destiny_process_status
    )
    .expect("writing to a String cannot fail");
    writeln!(report, "account_source = {}", context.account_source.label)
        .expect("writing to a String cannot fail");
    writeln!(
        report,
        "account_contract = {}",
        context.account_source.contract
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "account_source_detail = {}",
        single_line(&context.account_source.detail)
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "has_unsaved_changes = {}",
        context.has_unsaved_changes
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "current_status = {}",
        single_line(context.current_status)
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "source_warning = {}",
        context
            .source_warning
            .map_or_else(|| "none".to_owned(), single_line)
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "catalog_source = {}",
        if context.catalog.loaded_from_cache {
            "local_cache"
        } else {
            "game_packages"
        }
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "catalog_counts = items:{} plugs:{} icons:{} descriptions:{}",
        context.catalog.items,
        context.catalog.plugs,
        context.catalog.icons,
        context.catalog.descriptions
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "progression_counts = flags:{} values:{} progressions:{} objectives:{} expressions:{}",
        context.catalog.unlock_flags,
        context.catalog.unlock_values,
        context.catalog.progressions,
        context.catalog.objectives,
        context.catalog.expressions
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "progression_package_error = {}",
        context
            .catalog
            .progression_error
            .map_or_else(|| "none".to_owned(), single_line)
    )
    .expect("writing to a String cannot fail");
    writeln!(report).expect("writing to a String cannot fail");
}

fn append_persistence_compatibility(report: &mut String, install: &Path) {
    let inspection = PersistenceCompatibility::inspect(install);
    if !inspection.detected() {
        return;
    }
    report.push_str("Alternate Runtime Persistence\n-----------------------------\n");
    writeln!(
        report,
        "alternate_runtime_persistence_detected = {}",
        inspection.detected()
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "detection_evidence = {}",
        inspection.detection_evidence()
    )
    .expect("writing to a String cannot fail");
    writeln!(report, "compatibility_warning = {WARNING_MESSAGE}")
        .expect("writing to a String cannot fail");
    append_path(
        report,
        "runtime_state_file",
        inspection.runtime_state_path(),
    );
    writeln!(
        report,
        "runtime_state_header = {}",
        inspection.runtime_state_status()
    )
    .expect("writing to a String cannot fail");
    append_path(report, "sunrise_module", inspection.module_path());
    writeln!(
        report,
        "sunrise_module_marker = {}",
        inspection.module_status()
    )
    .expect("writing to a String cannot fail");
    writeln!(report).expect("writing to a String cannot fail");
}

fn append_settings_candidates(report: &mut String, install: &Path) {
    report.push_str("Settings and Account Candidates\n-------------------------------\n");
    for layout in SettingsLayout::ALL {
        let settings = settings_path_for_install(install, layout);
        writeln!(report, "[{}]", layout.preference_value())
            .expect("writing to a String cannot fail");
        append_path(report, "settings_json", &settings);
        append_path(
            report,
            "investment_database",
            &crate::persistence::investment_path(&settings),
        );
    }
    writeln!(report).expect("writing to a String cannot fail");
}

fn append_package_summary(report: &mut String, install: &Path) {
    report.push_str("Package Summary\n---------------\n");
    let directory = install.join("packages");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => {
            writeln!(
                report,
                "package_directory_error = Could not read {}: {error}",
                directory.display()
            )
            .expect("writing to a String cannot fail");
            writeln!(report).expect("writing to a String cannot fail");
            return;
        }
    };
    let mut package_count = 0usize;
    let mut package_bytes = 0u64;
    let mut relevant = Vec::new();
    let mut read_errors = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                read_errors.push(error.to_string());
                continue;
            }
        };
        let path = entry.path();
        if !extension_eq(&path, "pkg") {
            continue;
        }
        package_count += 1;
        if let Ok(metadata) = entry.metadata() {
            package_bytes = package_bytes.saturating_add(metadata.len());
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.starts_with("w64_investment_globals_client_") || name.starts_with("w64_parhelion_")
        {
            relevant.push(path);
        }
    }
    relevant.sort_by_key(|path| path_sort_key(path));
    writeln!(report, "pkg_file_count = {package_count}").expect("writing to a String cannot fail");
    writeln!(report, "pkg_total_bytes = {package_bytes}").expect("writing to a String cannot fail");
    writeln!(
        report,
        "investment_or_parhelion_pkg_count = {}",
        relevant.len()
    )
    .expect("writing to a String cannot fail");
    for path in relevant {
        append_path(report, "investment_or_parhelion_pkg", &path);
    }
    for error in read_errors {
        writeln!(report, "package_scan_error = {}", single_line(&error))
            .expect("writing to a String cannot fail");
    }
    writeln!(report).expect("writing to a String cannot fail");
}

fn append_optional_path(report: &mut String, label: &str, result: Result<PathBuf, std::io::Error>) {
    match result {
        Ok(path) => append_path(report, label, &path),
        Err(error) => writeln!(
            report,
            "{label} = unavailable | error={}",
            single_line(&error.to_string())
        )
        .expect("writing to a String cannot fail"),
    }
}

fn append_optional_known_path(report: &mut String, label: &str, path: Option<PathBuf>) {
    match path {
        Some(path) => append_path(report, label, &path),
        None => writeln!(report, "{label} = unavailable").expect("writing to a String cannot fail"),
    }
}

fn append_path(report: &mut String, label: &str, path: &Path) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            writeln!(report, "{label} = {} | missing", path.display())
                .expect("writing to a String cannot fail");
            return;
        }
        Err(error) => {
            writeln!(
                report,
                "{label} = {} | inaccessible | error={}",
                path.display(),
                single_line(&error.to_string())
            )
            .expect("writing to a String cannot fail");
            return;
        }
    };
    let file_type = metadata.file_type();
    let kind = if file_type.is_file() {
        "file"
    } else if file_type.is_dir() {
        "directory"
    } else if file_type.is_symlink() {
        "symlink"
    } else {
        "other"
    };
    let canonical = fs::canonicalize(path).map_or_else(
        |error| format!("unavailable ({error})"),
        |path| path.display().to_string(),
    );
    writeln!(
        report,
        "{label} = {} | kind={kind} | bytes={} | modified_unix_seconds={} | readonly={} | canonical={canonical}",
        path.display(),
        file_type.is_file().then_some(metadata.len()).map_or_else(|| "not_applicable".to_owned(), |value| value.to_string()),
        metadata.modified().ok().and_then(unix_seconds).map_or_else(|| "unavailable".to_owned(), |value| value.to_string()),
        metadata.permissions().readonly(),
    )
    .expect("writing to a String cannot fail");
}

fn extension_eq(path: &Path, expected: &str) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn path_sort_key(path: &Path) -> String {
    let value = path.to_string_lossy();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value.into_owned()
    }
}

fn optional_number(value: Option<u64>) -> String {
    value.map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
}

fn unix_seconds(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|value| value.as_secs())
}

fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests;
