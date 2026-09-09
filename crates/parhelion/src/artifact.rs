use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read},
    path::Path,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactMetadata {
    pub file_name: String,
    pub byte_length: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileDigest {
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
}

pub(crate) fn digest_file(path: &Path) -> io::Result<FileDigest> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut byte_length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        byte_length += read as u64;
    }
    Ok(FileDigest {
        byte_length,
        sha256: format!("{:X}", digest.finalize()),
    })
}

pub(crate) fn has_pkg_extension(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"))
}
