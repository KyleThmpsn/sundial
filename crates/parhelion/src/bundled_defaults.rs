//! Bundled defaults are copied into a library once and refreshed when a release changes them.
//!
//! Each library records the digest of the bundled bytes it last applied. When a newer
//! build ships different bytes for a default the library already holds, the on-disk copy
//! is backed up and replaced. A default the reader edited stays untouched until the
//! bundled version changes, and a missing record counts as a change.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Record {
    schema: u32,
    applied: BTreeMap<String, String>,
}

/// The digests of the bundled defaults a library last applied, keyed per default.
pub(crate) struct AppliedVersions {
    path: PathBuf,
    record: Record,
    dirty: bool,
}

impl AppliedVersions {
    pub(crate) fn load(path: PathBuf) -> Result<Self, String> {
        let record = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("Could not read {}: {error}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Record::default(),
            Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
        };
        Ok(Self {
            path,
            record,
            dirty: false,
        })
    }

    pub(crate) fn is_current(&self, key: &str, digest: &str) -> bool {
        self.record
            .applied
            .get(key)
            .is_some_and(|applied| applied == digest)
    }

    pub(crate) fn record(&mut self, key: &str, digest: String) {
        if self.record.applied.get(key) != Some(&digest) {
            self.record.applied.insert(key.to_owned(), digest);
            self.dirty = true;
        }
    }

    pub(crate) fn save(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        self.record.schema = SCHEMA;
        let mut encoded = serde_json::to_vec_pretty(&self.record)
            .map_err(|error| format!("Could not encode {}: {error}", self.path.display()))?;
        encoded.push(b'\n');
        sundial::package_authoring::replace_authoring_file(&self.path, &encoded)
            .map_err(|error| format!("Could not write {}: {error}", self.path.display()))?;
        self.dirty = false;
        Ok(())
    }
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Defaults replaced while opening a library, with the folder holding the previous copies.
/// An edited default is duplicated into the library first, so the edits stay editable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DefaultsRefresh {
    pub(crate) names: Vec<String>,
    pub(crate) copies: Vec<String>,
    pub(crate) backup: PathBuf,
}

impl DefaultsRefresh {
    pub(crate) fn summary(&self, kind: &str) -> String {
        let mut summary = format!(
            "Updated {} default {kind}: {}.",
            self.names.len(),
            self.names.join(", ")
        );
        if !self.copies.is_empty() {
            summary.push_str(&format!(
                " Your edits were kept as: {}.",
                self.copies.join(", ")
            ));
        }
        summary.push_str(&format!(" Previous copies: {}", self.backup.display()));
        summary
    }
}

/// Copies the previous bytes of each stale default into `backup` before it is replaced.
pub(crate) fn back_up(backup: &Path, file_name: &str, bytes: &[u8]) -> Result<(), String> {
    sundial::storage::create_file(&backup.join(file_name), bytes)
        .map_err(|error| format!("Backup failed at {}: {error}", backup.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_round_trip_and_only_write_when_changed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("versions.json");
        let mut versions = AppliedVersions::load(path.clone()).unwrap();
        assert!(!versions.is_current("a", "1"));
        versions.save().unwrap();
        assert!(!path.exists());

        versions.record("a", "1".into());
        versions.save().unwrap();
        let written = fs::read(&path).unwrap();

        let mut reloaded = AppliedVersions::load(path.clone()).unwrap();
        assert!(reloaded.is_current("a", "1"));
        assert!(!reloaded.is_current("a", "2"));
        reloaded.record("a", "1".into());
        reloaded.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), written);
    }

    #[test]
    fn unreadable_versions_are_an_error_not_an_empty_record() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("versions.json");
        fs::write(&path, b"{").unwrap();
        assert!(AppliedVersions::load(path).is_err());
    }

    #[test]
    fn digests_are_stable_lowercase_hex() {
        assert_eq!(
            digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
