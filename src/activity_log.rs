//! Shared activity entries and bounded, best-effort desktop log files.
//!
//! Construction is memory-only. Desktop startup explicitly enables persistence,
//! so previews and headless UI tests never write to the user's log directory.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
const MAX_ENTRY_BYTES: usize = 16 * 1024;

/// An activity event with a UTC timestamp captured when it happened.
pub struct Entry {
    pub text: String,
    pub error: bool,
    timestamp: String,
}

impl Entry {
    pub fn info(text: impl Into<String>) -> Self {
        Self::new(text.into(), false)
    }
    pub fn error(text: impl Into<String>) -> Self {
        Self::new(text.into(), true)
    }

    pub fn new(text: String, error: bool) -> Self {
        Self {
            text,
            error,
            timestamp: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_else(|_| "Timestamp unavailable".to_owned()),
        }
    }

    pub fn formatted(&self) -> String {
        format!(
            "{} [{}] {}",
            self.timestamp,
            if self.error { "Error" } else { "Info" },
            self.text
        )
    }
}

/// Fixed product names keep all file operations within the log directory.
pub enum Product {
    Sundial,
    Parhelion,
}

impl Product {
    pub fn log_path(&self) -> Option<PathBuf> {
        crate::paths::data_dir().map(|directory| {
            directory.join("logs").join(match self {
                Self::Sundial => "sundial.log",
                Self::Parhelion => "parhelion.log",
            })
        })
    }
}

/// Appends events across sessions, retaining the current file and two archives.
/// Errors remain visible to the UI but never escape into editing/install flows.
#[derive(Default)]
pub struct FileLog {
    path: Option<PathBuf>,
    error: Option<String>,
}

impl FileLog {
    pub fn enable(&mut self, product: Product) {
        self.path = product.log_path();
        if self.path.is_none() {
            self.error = Some("Could not locate the log folder".to_owned());
        }
    }

    pub fn directory(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn append(&mut self, entry: &Entry) {
        let Some(path) = &self.path else {
            return;
        };
        if let Err(error) = append_at(path, entry, MAX_FILE_BYTES) {
            self.error = Some(format!("Could not save activity log: {error}"));
        } else {
            self.error = None;
        }
    }

    pub fn open_folder(&mut self) {
        let result = self
            .directory()
            .ok_or_else(|| "Log folder is unavailable".to_owned())
            .and_then(|path| {
                fs::create_dir_all(path)
                    .map_err(|error| error.to_string())
                    .map(|()| path)
            })
            .and_then(crate::package_authoring::open_directory);
        if let Err(error) = result {
            self.error = Some(error);
        }
    }
}

fn append_at(path: &Path, entry: &Entry, limit: usize) -> io::Result<()> {
    let record = bounded_record(entry, MAX_ENTRY_BYTES.min(limit));
    append_text_at(path, &record, limit)
}

/// Shared bounded append and rotation for activity and diagnostic records.
pub(crate) fn append_text_at(path: &Path, text: &str, limit: usize) -> io::Result<()> {
    if limit < 32 {
        return Err(io::Error::other("Log size limit is too small"));
    }
    let record = if text.len() > limit {
        let suffix = "\n[truncated]\n";
        let mut end = limit - suffix.len();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}{suffix}", &text[..end])
    } else {
        text.to_owned()
    };
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::other("Missing log directory"))?,
    )?;
    // A separate, stable lock survives rotation. Never wait for another process.
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("log.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    let size = match fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    if size > (limit - record.len()) as u64 {
        rotate(path)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(record.as_bytes())?;
    file.flush()
    // Closing the lock releases it, including on any error above.
}

fn rotate(path: &Path) -> io::Result<()> {
    let newest = path.with_extension("log.1");
    let oldest = path.with_extension("log.2");
    match fs::remove_file(&oldest) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    rename_if_present(&newest, &oldest)?;
    rename_if_present(path, &newest)
}

fn rename_if_present(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn bounded_record(entry: &Entry, limit: usize) -> String {
    let mut record = entry.formatted();
    if record.len() >= limit {
        let suffix = " [truncated]\n";
        let mut end = limit.saturating_sub(suffix.len());
        while !record.is_char_boundary(end) {
            end -= 1;
        }
        record.truncate(end);
        record.push_str(suffix);
    } else {
        record.push('\n');
    }
    record
}

#[cfg(test)]
mod tests;
