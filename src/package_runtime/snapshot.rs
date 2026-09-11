//! A package-directory identity used to invalidate read-only native indexes.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    path: PathBuf,
    files: Vec<(OsString, u64, SystemTime)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_snapshot_tracks_patches_and_languages_without_unrelated_invalidations() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "w64_test_1234_0.pkg",
            "w64_test_1234_1.pkg",
            "w64_test_1234_en_0.pkg",
            "w64_other_5678_0.pkg",
        ] {
            std::fs::write(dir.path().join(name), b"original").unwrap();
        }
        let before = Snapshot::read(dir.path()).unwrap();
        assert_eq!(before.for_package(0x1234).files.len(), 3);
        std::fs::write(
            dir.path().join("w64_other_5678_0.pkg"),
            b"changed other package",
        )
        .unwrap();
        let after = Snapshot::read(dir.path()).unwrap();
        assert_ne!(before, after);
        assert_eq!(before.for_package(0x1234), after.for_package(0x1234));
        std::fs::write(dir.path().join("w64_test_1234_1.pkg"), b"changed patch").unwrap();
        assert_ne!(
            before.for_package(0x1234),
            Snapshot::read(dir.path()).unwrap().for_package(0x1234)
        );
        std::fs::write(dir.path().join("unknown.pkg"), b"unknown source").unwrap();
        assert_ne!(
            after.for_package(0x5678),
            Snapshot::read(dir.path()).unwrap().for_package(0x5678)
        );
    }
}

impl Snapshot {
    pub(crate) fn read(packages: &Path) -> Result<Self, String> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(packages).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry
                .path()
                .extension()
                .is_none_or(|extension| !extension.eq_ignore_ascii_case("pkg"))
            {
                continue;
            }
            let metadata = entry.metadata().map_err(|error| error.to_string())?;
            files.push((
                entry.file_name(),
                metadata.len(),
                metadata.modified().map_err(|error| error.to_string())?,
            ));
        }
        files.sort();
        Ok(Self {
            path: packages.canonicalize().map_err(|error| error.to_string())?,
            files,
        })
    }

    pub(crate) fn key(&self) -> Result<String, String> {
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    /// Include every patch and language variant of this package ID. Unknown
    /// filenames conservatively invalidate all shards rather than being ignored.
    pub(crate) fn for_package(&self, package: u16) -> Self {
        let package_id = |name: &OsString| {
            name.to_str()
                .and_then(tiger_pkg::manager::PackagePath::parse)
                .and_then(|path| u16::from_str_radix(&path.id, 16).ok())
        };
        if !self
            .files
            .iter()
            .any(|(name, _, _)| package_id(name) == Some(package))
        {
            return self.clone();
        }
        Self {
            path: self.path.clone(),
            files: self
                .files
                .iter()
                .filter(|(name, _, _)| package_id(name).is_none_or(|id| id == package))
                .cloned()
                .collect(),
        }
    }
}
