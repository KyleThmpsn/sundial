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
}
