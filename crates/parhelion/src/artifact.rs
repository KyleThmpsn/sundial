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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct FileDigest {
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
}

pub(crate) fn digest_file(path: &Path) -> io::Result<FileDigest> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut byte_length = 0u64;
    // Reads of 64 KiB held SHA-256 to about two thirds of its speed on large packages.
    let mut buffer = vec![0u8; 1024 * 1024];
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
        sha256: hex::encode_upper(digest.finalize()),
    })
}

/// Hashes `paths` on up to eight threads, each taking the next file no thread has taken, and hands
/// each result to `done` in the order of `paths` as soon as it and every earlier one finish.
/// Hashed in fixed batches, each batch waited for its largest file, and the 8.3 GB of a Shadowkeep
/// install's stock packages took 12 seconds where the disk and the hash allow about 2.
pub(crate) fn digest_files(
    paths: &[&Path],
    mut done: impl FnMut(usize, io::Result<FileDigest>) -> Result<(), String>,
) -> Result<(), String> {
    let total = paths.len();
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .clamp(1, 8)
        .min(total);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (sender, finished) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let next = &next;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(path) = paths.get(index) else {
                        return;
                    };
                    if sender.send((index, digest_file(path))).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);
        let mut digests = (0..total).map(|_| None).collect::<Vec<_>>();
        let mut handed = 0;
        for (index, digest) in finished {
            digests[index] = Some(digest);
            while let Some(digest) = digests.get_mut(handed).and_then(Option::take) {
                done(handed, digest)?;
                handed += 1;
            }
        }
        if handed != total {
            return Err("A file hash worker stopped".to_owned());
        }
        Ok(())
    })
}

/// As [`digest_file`], for bytes already read, so a check that also parses a file reads it once.
pub(crate) fn digest_bytes(bytes: &[u8]) -> FileDigest {
    FileDigest {
        byte_length: bytes.len() as u64,
        sha256: hex::encode_upper(Sha256::digest(bytes)),
    }
}

pub(crate) fn has_pkg_extension(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"))
}
