use std::path::Path;

use crate::{AuthoringError, AuthoringResult, format::BLOCK_SIZE};

pub(crate) const FULL_RAW_FLAGS: u16 = 0;
const COMPRESSED_FLAGS: u16 = 1;

#[cfg(all(windows, target_pointer_width = "64"))]
mod oodle;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub(crate) struct EncodedPackageBlock {
    pub stored: Vec<u8>,
    pub flags: u16,
    pub gcm_tag: [u8; 16],
}

#[derive(Default)]
pub(crate) struct PackageBlockEncoder {
    #[cfg(all(windows, target_pointer_width = "64"))]
    compressor: Option<std::sync::Arc<oodle::Compressor>>,
}

/// One loaded encoder per DLL path for the whole process.
///
/// Every package a build emits opens the same DLL, and opening it is a `LoadLibrary` plus an
/// ABI check. The path is canonical by the time it reaches here, so it keys the cache.
#[cfg(all(windows, target_pointer_width = "64"))]
fn shared_compressor(runtime: &Path) -> AuthoringResult<std::sync::Arc<oodle::Compressor>> {
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, Mutex, OnceLock},
    };
    static ENCODERS: OnceLock<Mutex<HashMap<PathBuf, Arc<oodle::Compressor>>>> = OnceLock::new();
    let mut encoders = ENCODERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| {
            AuthoringError::InvalidInput("The package encoder cache is poisoned".into())
        })?;
    if let Some(compressor) = encoders.get(runtime) {
        return Ok(Arc::clone(compressor));
    }
    let compressor =
        Arc::new(oodle::Compressor::open(runtime).map_err(AuthoringError::InvalidInput)?);
    encoders.insert(runtime.to_path_buf(), Arc::clone(&compressor));
    Ok(compressor)
}

impl PackageBlockEncoder {
    pub(crate) fn open_for_packages(package_directory: &Path) -> AuthoringResult<Self> {
        if !package_directory.is_dir() {
            return Err(AuthoringError::InvalidInput(format!(
                "Package directory does not exist at {}",
                package_directory.display()
            )));
        }
        #[cfg(all(windows, target_pointer_width = "64"))]
        {
            // Shadowkeep's own version-3 runtime is the compatibility boundary. Never
            // pick an arbitrary DLL from the executable directory, PATH or another game.
            let runtime = package_directory
                .canonicalize()
                .map_err(|error| {
                    AuthoringError::io("resolve package directory", package_directory, error)
                })?
                .parent()
                .ok_or_else(|| {
                    AuthoringError::InvalidInput("Package directory has no install parent".into())
                })?
                .join("bin/x64/oo2core_3_win64.dll");
            let compressor = if runtime.is_file() {
                Some(shared_compressor(&runtime)?)
            } else {
                // Synthetic package views and platforms without a compatible encoder
                // retain the existing valid raw-block representation.
                None
            };
            Ok(Self { compressor })
        }
        #[cfg(not(all(windows, target_pointer_width = "64")))]
        Ok(Self::default())
    }

    pub(crate) fn encode(
        &self,
        _package_id: u16,
        plaintext: &[u8],
    ) -> AuthoringResult<EncodedPackageBlock> {
        if plaintext.is_empty() || plaintext.len() > BLOCK_SIZE {
            return Err(AuthoringError::InvalidInput(format!(
                "A package block must contain between 1 and {BLOCK_SIZE} plaintext bytes"
            )));
        }
        #[cfg(all(windows, target_pointer_width = "64"))]
        let compressed = self
            .compressor
            .as_ref()
            .map(|compressor| compressor.compress(plaintext))
            .transpose()
            .map_err(AuthoringError::InvalidInput)?;
        #[cfg(not(all(windows, target_pointer_width = "64")))]
        let compressed = None;
        select_verified_storage(plaintext, compressed)
    }
}

/// The native codec verifies the exact decoded bytes before returning a candidate here.
fn select_verified_storage(
    plaintext: &[u8],
    compressed: Option<Vec<u8>>,
) -> AuthoringResult<EncodedPackageBlock> {
    let (stored, flags) = match compressed {
        Some(bytes) if bytes.is_empty() => {
            return Err(AuthoringError::InvalidInput(
                "Package compression returned an empty block".into(),
            ));
        }
        Some(bytes) if bytes.len() < plaintext.len() => (bytes, COMPRESSED_FLAGS),
        _ => {
            // Flags-0 blocks store exactly their plaintext range, without padding to the
            // logical 0x40000-byte span. Incompressible blocks must not grow on disk.
            (plaintext.to_vec(), FULL_RAW_FLAGS)
        }
    };
    Ok(EncodedPackageBlock {
        stored,
        flags,
        gcm_tag: [0; 16],
    })
}
