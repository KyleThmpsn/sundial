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

use super::persistence_compatibility::{PersistenceCompatibility, WARNING_MESSAGE};

const MAX_LOG_BYTES: usize = 5 * 1024 * 1024;
const MAX_RUNTIME_FILES: usize = 10_000;
const LOG_DIRECTORY_NAME: &str = "logs";
const LOG_FILE_NAME: &str = "sundial-troubleshooting.log";

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
    pub account_source: &'a str,
    pub account_contract: &'a str,
    pub account_detail: &'a str,
    #[cfg(feature = "sqlite-account")]
    pub account_database_path: &'a Path,
    pub catalog: CatalogSummary<'a>,
    pub recent_activity: &'a str,
    pub current_status: &'a str,
    pub source_warning: Option<&'a str>,
    pub has_unsaved_changes: bool,
    pub destiny_process_status: &'a str,
}

#[derive(Clone)]
struct RuntimeFile {
    path: PathBuf,
    relative_path: PathBuf,
    kind: &'static str,
    bytes: Option<u64>,
    modified_unix_seconds: Option<u64>,
    readonly: Option<bool>,
}

#[derive(Default)]
struct RuntimeScan {
    files: Vec<RuntimeFile>,
    errors: Vec<String>,
    truncated: bool,
}

pub(super) fn log_path() -> Option<PathBuf> {
    paths::data_dir().map(|path| path.join(LOG_DIRECTORY_NAME).join(LOG_FILE_NAME))
}

pub(super) fn build_report(context: &ReportContext<'_>) -> String {
    let mut report = String::new();
    writeln!(report, "Sundial troubleshooting log").expect("writing to a String cannot fail");
    writeln!(report, "format_version = 2").expect("writing to a String cannot fail");
    writeln!(
        report,
        "generated_unix_seconds = {}",
        unix_seconds(SystemTime::now())
            .map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
    )
    .expect("writing to a String cannot fail");
    writeln!(report).expect("writing to a String cannot fail");

    append_build_information(&mut report);
    append_path_section(&mut report, context);
    append_workspace_section(&mut report, context);
    append_persistence_compatibility(&mut report, context.install_path);
    append_settings_candidates(&mut report, context.install_path);
    let runtime_scans = append_sunrise_runtime_files(&mut report, context.install_path);
    append_bin_files(&mut report, context.settings_path, &runtime_scans);
    append_package_summary(&mut report, context.install_path);
    report.push_str(
        "Recent Sundial Activity (Newest First)\n-------------------------------------\n",
    );
    report.push_str(context.recent_activity);
    report.push_str("\n\n");

    report.push_str(
        "Privacy\n-------\nSettings and account files are not copied into this report. Full local paths, file metadata, errors, and Sundial status messages are included. Messages can contain item names or other contextual details. Review before sharing. Parhelion activity is in its separate log.\n",
    );
    report
}

pub(super) fn build_startup_failure_report(install_path: Option<&Path>, error: &str) -> String {
    let mut report = String::new();
    writeln!(report, "Sundial troubleshooting log").expect("writing to a String cannot fail");
    writeln!(report, "format_version = 2").expect("writing to a String cannot fail");
    writeln!(
        report,
        "generated_unix_seconds = {}",
        unix_seconds(SystemTime::now())
            .map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
    )
    .expect("writing to a String cannot fail");
    writeln!(report).expect("writing to a String cannot fail");
    append_build_information(&mut report);
    report.push_str("Startup failure\n---------------\n");
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
        let runtime_scans = append_sunrise_runtime_files(&mut report, install);
        append_bin_files(&mut report, &install.join("settings.json"), &runtime_scans);
        append_package_summary(&mut report, install);
    } else {
        report.push_str("selected_install = unavailable\n\n");
    }
    report.push_str(
        "Privacy\n-------\nSettings and account files are not copied into this report. Full local paths, file metadata, errors, and Sundial status messages are included. Messages can contain item names or other contextual details. Review before sharing. Parhelion activity is in its separate log.\n",
    );
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
        "\n\n===== Refreshed environment snapshot =====\n{report}"
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
    report.push_str("Build and process\n-----------------\n");
    writeln!(report, "sundial_version = {}", env!("CARGO_PKG_VERSION"))
        .expect("writing to a String cannot fail");
    writeln!(report, "operating_system = {}", std::env::consts::OS)
        .expect("writing to a String cannot fail");
    writeln!(report, "architecture = {}", std::env::consts::ARCH)
        .expect("writing to a String cannot fail");
    writeln!(report, "debug_build = {}", cfg!(debug_assertions))
        .expect("writing to a String cannot fail");
    #[cfg(feature = "sqlite-account")]
    writeln!(
        report,
        "experimental_pr88_sqlite_account_support = {}",
        cfg!(feature = "sqlite-account")
    )
    .expect("writing to a String cannot fail");
    writeln!(report, "process_id = {}", std::process::id())
        .expect("writing to a String cannot fail");
    append_optional_path(report, "current_executable", std::env::current_exe());
    append_optional_path(report, "working_directory", std::env::current_dir());
    writeln!(report).expect("writing to a String cannot fail");
}

fn append_path_section(report: &mut String, context: &ReportContext<'_>) {
    report.push_str("Resolved paths\n--------------\n");
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
    #[cfg(feature = "sqlite-account")]
    append_path(
        report,
        "active_account_database",
        context.account_database_path,
    );
    append_optional_known_path(report, "sundial_config_directory", paths::config_dir());
    append_optional_known_path(report, "sundial_data_directory", paths::data_dir());
    append_optional_known_path(report, "sundial_cache_directory", paths::cache_dir());
    append_optional_known_path(
        report,
        "sundial_preferences",
        paths::config_dir().map(|path| path.join("preferences.json")),
    );
    append_optional_known_path(
        report,
        "sundial_backups_directory",
        paths::data_dir().map(|path| path.join("backups")),
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
    report.push_str("Active workspace\n----------------\n");
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
    writeln!(report, "account_source = {}", context.account_source)
        .expect("writing to a String cannot fail");
    writeln!(report, "account_contract = {}", context.account_contract)
        .expect("writing to a String cannot fail");
    writeln!(
        report,
        "account_source_detail = {}",
        single_line(context.account_detail)
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
    report.push_str("Sunrise persistence compatibility\n---------------------------------\n");
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
    writeln!(
        report,
        "compatibility_warning = {}",
        if inspection.detected() {
            WARNING_MESSAGE
        } else {
            "none"
        }
    )
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
    report.push_str("Settings and account candidates\n-------------------------------\n");
    let candidates = [
        ("game_root", install.join("settings.json")),
        ("root", install.join("Sunrise").join("settings.json")),
        (
            "bin_x64",
            install
                .join("bin")
                .join("x64")
                .join("Sunrise")
                .join("settings.json"),
        ),
    ];
    for (layout, settings) in candidates {
        writeln!(report, "[{layout}]").expect("writing to a String cannot fail");
        append_path(report, "settings_json", &settings);
        let directory = settings.parent().unwrap_or(install);
        let state_db = directory.join("state.db");
        #[cfg(feature = "sqlite-account")]
        {
            let sqlite = directory.join("state.sqlite3");
            append_path(report, "state_sqlite3", &sqlite);
            writeln!(report, "state_sqlite3_exists = {}", sqlite.is_file())
                .expect("writing to a String cannot fail");
        }
        append_path(report, "state_db", &state_db);
        writeln!(report, "state_db_exists = {}", state_db.is_file())
            .expect("writing to a String cannot fail");
    }
    writeln!(report).expect("writing to a String cannot fail");
}

fn append_sunrise_runtime_files(report: &mut String, install: &Path) -> Vec<RuntimeScan> {
    report.push_str("Sunrise runtime folders\n-----------------------\n");
    let roots = [
        ("root", install.join("Sunrise")),
        ("bin_x64", install.join("bin").join("x64").join("Sunrise")),
    ];
    let mut scans = Vec::with_capacity(roots.len());
    for (label, root) in roots {
        writeln!(report, "[{label}] {}", root.display()).expect("writing to a String cannot fail");
        append_path(report, "runtime_directory", &root);
        let scan = scan_runtime_folder(&root);
        if !root.exists() {
            report.push_str("status = missing\n");
        } else if !root.is_dir() {
            report.push_str("status = not_a_directory\n");
        } else {
            writeln!(report, "file_count = {}", scan.files.len())
                .expect("writing to a String cannot fail");
            for file in &scan.files {
                writeln!(
                    report,
                    "{} | kind={} | bytes={} | modified_unix_seconds={} | readonly={}",
                    file.relative_path.display(),
                    file.kind,
                    optional_number(file.bytes),
                    optional_number(file.modified_unix_seconds),
                    file.readonly
                        .map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
                )
                .expect("writing to a String cannot fail");
            }
        }
        for error in &scan.errors {
            writeln!(report, "scan_error = {}", single_line(error))
                .expect("writing to a String cannot fail");
        }
        if scan.truncated {
            writeln!(report, "scan_truncated_after = {MAX_RUNTIME_FILES}")
                .expect("writing to a String cannot fail");
        }
        scans.push(scan);
    }
    writeln!(report).expect("writing to a String cannot fail");
    scans
}

fn append_bin_files(report: &mut String, settings_path: &Path, runtime_scans: &[RuntimeScan]) {
    report.push_str("Sunrise .bin files\n------------------\n");
    let mut files = runtime_scans
        .iter()
        .flat_map(|scan| scan.files.iter())
        .filter(|file| extension_eq(&file.path, "bin"))
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    if let Some(directory) = settings_path.parent()
        && let Ok(entries) = fs::read_dir(directory)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if extension_eq(&path, "bin") {
                files.push(path);
            }
        }
    }
    sort_and_deduplicate_paths(&mut files);
    writeln!(report, "bin_file_count = {}", files.len()).expect("writing to a String cannot fail");
    for path in files {
        append_path(report, "bin_file", &path);
    }
    writeln!(report).expect("writing to a String cannot fail");
}

fn append_package_summary(report: &mut String, install: &Path) {
    report.push_str("Package summary\n---------------\n");
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

fn scan_runtime_folder(root: &Path) -> RuntimeScan {
    let mut scan = RuntimeScan::default();
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return scan,
        Err(error) => {
            scan.errors
                .push(format!("Could not inspect {}: {error}", root.display()));
            return scan;
        }
    };
    if metadata.file_type().is_symlink() {
        scan.errors.push(format!(
            "Runtime root {} is a symlink and was not traversed",
            root.display()
        ));
        return scan;
    }
    if !metadata.is_dir() {
        return scan;
    }
    scan_runtime_directory(root, root, &mut scan);
    scan.files
        .sort_by_key(|file| path_sort_key(&file.relative_path));
    scan
}

fn scan_runtime_directory(root: &Path, directory: &Path, scan: &mut RuntimeScan) {
    if scan.files.len() >= MAX_RUNTIME_FILES {
        scan.truncated = true;
        return;
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            scan.errors
                .push(format!("Could not read {}: {error}", directory.display()));
            return;
        }
    };
    let mut sorted_entries = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => sorted_entries.push(entry),
            Err(error) => scan.errors.push(format!(
                "Could not enumerate an entry in {}: {error}",
                directory.display()
            )),
        }
    }
    let mut entries = sorted_entries;
    entries.sort_by_key(|entry| path_sort_key(&entry.path()));
    for entry in entries {
        if scan.files.len() >= MAX_RUNTIME_FILES {
            scan.truncated = true;
            return;
        }
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                scan.errors
                    .push(format!("Could not inspect {}: {error}", path.display()));
                continue;
            }
        };
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            scan_runtime_directory(root, &path, scan);
            continue;
        }
        scan.files.push(RuntimeFile {
            relative_path: path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
            path,
            kind: if file_type.is_file() {
                "file"
            } else if file_type.is_symlink() {
                "symlink"
            } else {
                "other"
            },
            bytes: file_type.is_file().then_some(metadata.len()),
            modified_unix_seconds: metadata.modified().ok().and_then(unix_seconds),
            readonly: Some(metadata.permissions().readonly()),
        });
    }
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
    let canonical = fs::canonicalize(path)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|error| format!("unavailable ({error})"));
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

fn sort_and_deduplicate_paths(paths: &mut Vec<PathBuf>) {
    paths.sort_by_key(|path| path_sort_key(path));
    paths.dedup_by(|right, left| path_sort_key(right) == path_sort_key(left));
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
mod tests {
    use std::fs;

    use crate::test_support::TestDirectory;

    use super::{
        CatalogSummary, ReportContext, append_log_text_at, build_report,
        build_startup_failure_report, initialize_log_at, scan_runtime_folder,
    };

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
        let database = sunrise.join("state.sqlite3");
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
            account_source: "settings.json",
            account_contract: "test contract",
            account_detail: "test detail",
            #[cfg(feature = "sqlite-account")]
            account_database_path: &database,
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
        assert_account_feature_details(&report);
        assert!(report.contains("state_db_exists = false"));
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
        assert!(
            report.contains("cache\\build_data.bin") || report.contains("cache/build_data.bin")
        );
        if let Some(data_directory) = crate::package_authoring::parhelion_data_directory() {
            assert!(report.contains(&format!(
                "parhelion_package_backups_directory = {}",
                data_directory.join("backups").join("packages").display()
            )));
        }
    }

    fn assert_account_feature_details(report: &str) {
        #[cfg(feature = "sqlite-account")]
        assert!(report.contains("state_sqlite3_exists = true"));
        #[cfg(not(feature = "sqlite-account"))]
        {
            assert!(!report.contains("state_sqlite3_exists"));
            assert!(!report.contains("active_account_database"));
            assert!(!report.contains("sqlite_account_support"));
        }
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
        assert!(report.contains("Startup failure"));
        assert!(report.contains("Could not parse settings private detail"));
        assert!(report.contains("alternate_runtime_persistence_detected = false"));
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
}
