use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[cfg(windows)]
use tiger_pkg::{DestinyVersion, GameVersion};
use tiger_pkg::{PackageManager, TagHash};

pub(crate) mod index_cache;
pub(crate) mod snapshot;
pub mod tft;

const MIN_RUNTIME_PACKAGE_ID: u16 = 0x0100;
const MAX_RUNTIME_PACKAGE_ID: u16 = 0x0CFF;

const PACKAGE_AUTHORING_RUNTIME_MARKERS: [(&[u8], &str); 3] = [
    (
        b"mode=package_integrity_bypass",
        "generated package-header trust",
    ),
    (b"SUNCMANF", "generated content manifest"),
    (
        b"ev=content_config stage=get route=manifest",
        "generated manifest routing",
    ),
];

pub(crate) fn sunrise_module_path(install: &Path) -> PathBuf {
    install.join("bin").join("x64").join("steam_api64.dll")
}

pub(crate) fn installed_sunrise_module_version(install: &Path) -> Option<String> {
    let bytes = fs::read(sunrise_module_path(install)).ok()?;
    sunrise_module_version(&bytes)
}

fn sunrise_module_version(bytes: &[u8]) -> Option<String> {
    let image = pelite::PeFile::from_bytes(bytes).ok()?;
    let version_info = image.resources().ok()?.version_info().ok()?.file_info();
    let is_sunrise = version_info.strings.values().any(|strings| {
        strings.iter().any(|(key, value)| {
            (key.eq_ignore_ascii_case("ProductName") || key.eq_ignore_ascii_case("FileDescription"))
                && value.trim().eq_ignore_ascii_case("Sunrise")
        })
    });
    if !is_sunrise {
        return None;
    }
    let fixed = version_info.fixed?;
    (fixed.dwSignature == pelite::image::VS_FIXEDFILEINFO_SIGNATURE)
        .then(|| normalize_sunrise_version(&fixed.dwProductVersion.to_string()))?
}

pub(crate) fn normalize_sunrise_version(version: &str) -> Option<String> {
    let mut components = version
        .trim()
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if components.len() < 2 || components.len() > 4 {
        return None;
    }
    while components.len() > 2 && components.last() == Some(&0) {
        components.pop();
    }
    Some(
        components
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("."),
    )
}

pub(crate) fn validate_package_authoring_runtime(install: &Path) -> Result<(), String> {
    let module = sunrise_module_path(install);
    let bytes = fs::read(&module).map_err(|error| {
        format!(
            "Could not read installed Sunrise module {}: {error}",
            module.display()
        )
    })?;
    let version = sunrise_module_version(&bytes).ok_or_else(|| {
        format!(
            "The installed Sunrise module has no valid Sunrise version resource: {}",
            module.display()
        )
    })?;
    // These embedded markers are a capability advertisement, not proof that a particular
    // generated manifest or package set has already loaded successfully.
    let missing = missing_package_authoring_runtime_features(&bytes);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Project Sunrise {version} does not advertise the package-authoring runtime features required by Parhelion (missing: {})",
            missing.join(", ")
        ))
    }
}

fn missing_package_authoring_runtime_features(bytes: &[u8]) -> Vec<&'static str> {
    PACKAGE_AUTHORING_RUNTIME_MARKERS
        .iter()
        .filter_map(|(marker, label)| (!contains_bytes(bytes, marker)).then_some(*label))
        .collect()
}

fn contains_bytes(bytes: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && bytes.windows(needle.len()).any(|window| window == needle)
}

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
    #[cfg(windows)]
    {
        PackageManager::new(
            install.join("packages"),
            GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
            None,
        )
        .map_err(|error| format!("Could not open the Shadowkeep packages: {error}"))
    }
}

/// Returns whether a value is a canonically encoded package tag accepted by the client.
///
/// Keep this local instead of using `tiger_pkg::TagHash::is_valid`: tiger-pkg 0.21 limits that
/// helper to values through package `0xBFF`, while the Shadowkeep header validator accepts
/// effective package ids through `0xCFF`.
pub fn is_valid_package_tag(tag: TagHash) -> bool {
    let package_id = tag.pkg_id();
    (MIN_RUNTIME_PACKAGE_ID..=MAX_RUNTIME_PACKAGE_ID).contains(&package_id)
        && TagHash::new(package_id, tag.entry_index()) == tag
}

/// Resolves a package name only when its row still matches the live entry directory.
///
/// The package reader does not expose the client's registered-context priority. More than one
/// distinct live candidate is therefore ambiguous and must not be selected by iteration order.
pub fn resolve_live_named_tag(
    manager: &PackageManager,
    name: &str,
    expected_class: Option<u32>,
) -> Result<TagHash, String> {
    let candidates = manager
        .lookup
        .named_tags
        .iter()
        .filter(|candidate| candidate.name == name)
        .filter(|candidate| expected_class.is_none_or(|expected| candidate.class_hash == expected))
        .filter(|candidate| {
            manager
                .get_entry(candidate.hash)
                .is_some_and(|entry| entry.reference == candidate.class_hash)
        })
        .map(|candidate| u32::from(candidate.hash))
        .collect::<BTreeSet<_>>();

    match candidates.len() {
        0 => Err(match expected_class {
            Some(class) => {
                format!("The install has no live named tag {name:?} with class 0x{class:08X}")
            }
            None => format!("The install has no live named tag {name:?}"),
        }),
        1 => Ok(TagHash(
            *candidates
                .first()
                .expect("one-candidate branch has one named tag"),
        )),
        count => Err(format!(
            "The install has {count} distinct live named tags called {name:?}; package-context priority is ambiguous"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_authoring_runtime_requires_each_loader_capability() {
        let compatible = PACKAGE_AUTHORING_RUNTIME_MARKERS
            .iter()
            .flat_map(|(marker, _)| marker.iter().copied().chain([0]))
            .collect::<Vec<_>>();
        assert!(missing_package_authoring_runtime_features(&compatible).is_empty());

        let incomplete = b"mode=package_integrity_bypass\0SUNCMANF";
        assert_eq!(
            missing_package_authoring_runtime_features(incomplete),
            vec!["generated manifest routing"]
        );
    }

    #[test]
    fn package_tag_validation_covers_the_complete_runtime_window() {
        for tag in [
            TagHash::new(MIN_RUNTIME_PACKAGE_ID, 0),
            TagHash::new(MIN_RUNTIME_PACKAGE_ID, 0x1FFF),
            TagHash::new(MAX_RUNTIME_PACKAGE_ID, 0),
            TagHash::new(MAX_RUNTIME_PACKAGE_ID, 0x1FFF),
        ] {
            assert!(is_valid_package_tag(tag), "{tag} should be valid");
        }

        assert!(!is_valid_package_tag(TagHash::new(
            MIN_RUNTIME_PACKAGE_ID - 1,
            0
        )));
        assert!(!is_valid_package_tag(TagHash::new(
            MAX_RUNTIME_PACKAGE_ID + 1,
            0
        )));
        assert!(!is_valid_package_tag(TagHash::NONE));

        // This is the dependency edge that motivated the shared validator.
        assert!(!TagHash::new(MAX_RUNTIME_PACKAGE_ID, 0).is_valid());
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
            .join("bin")
            .join("x64")
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
