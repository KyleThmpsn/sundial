use std::path::Path;

use tiger_pkg::PackageManager;
#[cfg(not(target_os = "linux"))]
use tiger_pkg::{DestinyVersion, GameVersion};

pub(crate) fn open_shadowkeep_packages(install: &Path) -> Result<PackageManager, String> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        linux::open_shadowkeep_packages(install)
    }
    #[cfg(all(target_os = "linux", not(target_arch = "x86_64")))]
    {
        let _ = install;
        Err("Shadowkeep package decompression is currently supported only on x86-64 Linux".into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        PackageManager::new(
            install.join("packages"),
            GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
            None,
        )
        .map_err(|error| format!("Could not open the Shadowkeep packages: {error}"))
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    use std::{
        env, fs,
        os::unix::fs::symlink,
        path::{Path, PathBuf},
        sync::{Mutex, OnceLock},
    };

    use sha2::{Digest, Sha256};
    use tiger_pkg::{DestinyVersion, GameVersion, PackageManager};

    const LINOODLE_URL: &str = "https://raw.githubusercontent.com/v4nguard/tiger-pkg/657f41c0851001b2d371592b2f7a5cb9c686ddb4/liblinoodle3.so";
    const LINOODLE_FILE_NAME: &str = "liblinoodle3.so";
    const OODLE_DLL_FILE_NAME: &str = "oo2core_3_win64.dll";
    const LINOODLE_MAX_BYTES: usize = 2 * 1024 * 1024;
    const LINOODLE_SHA256: [u8; 32] = [
        0x01, 0x67, 0xcf, 0xd2, 0xb3, 0x16, 0x23, 0xce, 0x15, 0xaf, 0xd1, 0x29, 0xc3, 0xad, 0x56,
        0x87, 0x76, 0xd6, 0x3a, 0xf2, 0x23, 0x82, 0x4a, 0xb0, 0x85, 0x25, 0x4b, 0x7e, 0xa0, 0x73,
        0x0e, 0x35,
    ];

    static INITIALIZED: OnceLock<()> = OnceLock::new();
    static INITIALIZE_LOCK: Mutex<()> = Mutex::new(());

    pub(super) fn open_shadowkeep_packages(install: &Path) -> Result<PackageManager, String> {
        let install = install.canonicalize().map_err(|error| {
            format!(
                "Could not resolve the Shadowkeep installation {}: {error}",
                install.display()
            )
        })?;
        if INITIALIZED.get().is_some() {
            return create_manager(&install);
        }

        let _initialization = INITIALIZE_LOCK
            .lock()
            .map_err(|_| "The Linux package-support initializer stopped unexpectedly")?;
        if INITIALIZED.get().is_some() {
            return create_manager(&install);
        }

        let runtime = prepare_runtime(&install)?;
        let previous_directory = env::current_dir()
            .map_err(|error| format!("Could not read Sundial's working directory: {error}"))?;
        env::set_current_dir(&runtime).map_err(|error| {
            format!(
                "Could not enter the Linux package-support folder {}: {error}",
                runtime.display()
            )
        })?;
        let manager = create_manager(&install);
        let restore = env::set_current_dir(&previous_directory);
        if let Err(error) = restore {
            return Err(format!(
                "Could not restore Sundial's working directory to {}: {error}",
                previous_directory.display()
            ));
        }
        let manager = manager?;
        let _ = INITIALIZED.set(());
        Ok(manager)
    }

    fn create_manager(install: &Path) -> Result<PackageManager, String> {
        PackageManager::new(
            install.join("packages"),
            GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
            None,
        )
        .map_err(|error| format!("Could not open the Shadowkeep packages: {error}"))
    }

    fn prepare_runtime(install: &Path) -> Result<PathBuf, String> {
        let runtime = crate::paths::cache_dir()
            .ok_or("Could not locate Sundial's Linux cache folder")?
            .join("runtime")
            .join("linoodle3-0167cfd2");
        fs::create_dir_all(&runtime).map_err(|error| {
            format!(
                "Could not create the Linux package-support folder {}: {error}",
                runtime.display()
            )
        })?;

        let library = runtime.join(LINOODLE_FILE_NAME);
        if !file_has_expected_hash(&library) {
            let bytes = crate::http::get(LINOODLE_URL, LINOODLE_MAX_BYTES).map_err(|error| {
                format!("Could not download Linux Shadowkeep package support: {error}")
            })?;
            if !bytes_have_expected_hash(&bytes) {
                return Err(
                    "The downloaded Linux package-support library failed its SHA-256 verification"
                        .into(),
                );
            }
            crate::storage::replace_file(&library, &bytes).map_err(|error| {
                format!(
                    "Could not save the Linux package-support library to {}: {error}",
                    library.display()
                )
            })?;
        }

        let installed_dll = install
            .join("bin/x64")
            .join(OODLE_DLL_FILE_NAME)
            .canonicalize()
            .map_err(|error| {
                format!("Could not resolve the installed Shadowkeep Oodle library: {error}")
            })?;
        ensure_dll_link(&runtime.join(OODLE_DLL_FILE_NAME), &installed_dll)?;
        Ok(runtime)
    }

    fn file_has_expected_hash(path: &Path) -> bool {
        fs::read(path)
            .ok()
            .is_some_and(|bytes| bytes_have_expected_hash(&bytes))
    }

    fn bytes_have_expected_hash(bytes: &[u8]) -> bool {
        Sha256::digest(bytes).as_slice() == LINOODLE_SHA256
    }

    fn ensure_dll_link(link: &Path, target: &Path) -> Result<(), String> {
        match fs::symlink_metadata(link) {
            Ok(metadata) if !metadata.file_type().is_symlink() => {
                return Err(format!(
                    "The Linux package-support path is not a symbolic link: {}",
                    link.display()
                ));
            }
            Ok(_) => {
                let existing = fs::read_link(link)
                    .map_err(|error| format!("Could not inspect {}: {error}", link.display()))?;
                if existing == target {
                    return Ok(());
                }
                fs::remove_file(link)
                    .map_err(|error| format!("Could not replace {}: {error}", link.display()))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!("Could not inspect {}: {error}", link.display()));
            }
        }
        symlink(target, link).map_err(|error| {
            format!(
                "Could not connect Linux package support to {}: {error}",
                target.display()
            )
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn linoodle_hash_rejects_untrusted_bytes() {
            assert!(!bytes_have_expected_hash(b"not linoodle"));
        }
    }
}
