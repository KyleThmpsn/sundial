//! Bounded metadata-only inventory of runtime folders.

use super::{append_path, optional_number, path_sort_key, single_line, unix_seconds};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

const MAX_RUNTIME_ENTRIES: usize = 10_000;

pub(super) struct RuntimeFile {
    pub(super) relative_path: PathBuf,
    kind: &'static str,
    bytes: Option<u64>,
    modified_unix_seconds: Option<u64>,
    readonly: bool,
}

#[derive(Default)]
pub(super) struct RuntimeScan {
    pub(super) files: Vec<RuntimeFile>,
    pub(super) errors: Vec<String>,
    entries_scanned: usize,
    pub(super) truncated: bool,
}

pub(super) fn append_runtime_files(report: &mut String, install: &Path) {
    report.push_str("Runtime Folders\n---------------\n");
    let roots = [
        ("game_root_data", install.join("data")),
        ("game_root_cache", install.join("cache")),
        ("sunrise_root", install.join("Sunrise")),
        (
            "sunrise_bin_x64",
            install.join("bin").join("x64").join("Sunrise"),
        ),
        ("dawn_root", install.join("Dawn")),
        ("dawn_bin_x64", install.join("bin").join("x64").join("Dawn")),
    ];
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
            writeln!(report, "entries_scanned = {}", scan.entries_scanned)
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
                )
                .expect("writing to a String cannot fail");
            }
        }
        for error in &scan.errors {
            writeln!(report, "scan_error = {}", single_line(error))
                .expect("writing to a String cannot fail");
        }
        if scan.truncated {
            writeln!(
                report,
                "scan_truncated_after_entries = {}",
                scan.entries_scanned
            )
            .expect("writing to a String cannot fail");
        }
    }
    writeln!(report).expect("writing to a String cannot fail");
}

pub(super) fn scan_runtime_folder(root: &Path) -> RuntimeScan {
    scan_runtime_folder_with_limit(root, MAX_RUNTIME_ENTRIES)
}

fn scan_runtime_folder_with_limit(root: &Path, limit: usize) -> RuntimeScan {
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

    let mut pending = vec![root.to_path_buf()];
    'scan: while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                scan.errors
                    .push(format!("Could not read {}: {error}", directory.display()));
                continue;
            }
        };
        for entry in entries {
            if scan.entries_scanned == limit {
                scan.truncated = true;
                break 'scan;
            }
            // Directories and failed entries consume the same budget as files.
            scan.entries_scanned += 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    scan.errors.push(format!(
                        "Could not enumerate an entry in {}: {error}",
                        directory.display()
                    ));
                    continue;
                }
            };
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
                pending.push(path);
                continue;
            }
            scan.files.push(RuntimeFile {
                relative_path: path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                kind: if file_type.is_file() {
                    "file"
                } else if file_type.is_symlink() {
                    "symlink"
                } else {
                    "other"
                },
                bytes: file_type.is_file().then_some(metadata.len()),
                modified_unix_seconds: metadata.modified().ok().and_then(unix_seconds),
                readonly: metadata.permissions().readonly(),
            });
        }
    }
    scan.files
        .sort_by_key(|file| path_sort_key(&file.relative_path));
    scan
}

#[cfg(test)]
mod tests;
