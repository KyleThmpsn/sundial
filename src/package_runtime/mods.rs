//! The managed Parhelion mod's on-disk contract and read-only package projection.
//!
//! Sunrise owns general mod resolution and merged cache files. This projection reads the stock
//! packages with the managed mod's packages over them. Other mods' packages are not read, since
//! their effect on a package Parhelion also patches depends on the loader's merge.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

pub const ID: &str = "io.github.kylethmpsn.parhelion";
pub const MANIFEST: &str = "mod.json";
pub const RECEIPT: &str = "parhelion-install.json";
pub const JOURNAL: &str = ".parhelion-mod-transaction.json";
/// Read-only package views, beside the mods folder rather than in it, so the loader never reads
/// them. The leading dot keeps them out of listings that skip hidden folders.
const VIEWS: &str = ".sundial-package-views";
const VIEW_PREFIX: &str = "view-";
const VIEW_OWNER: &str = "owner.lock";
const VIEW_GRACE: std::time::Duration = std::time::Duration::from_secs(120);

pub fn runtime_directory(install: &Path) -> PathBuf {
    install.join("bin/x64/Sunrise")
}

pub fn directory(install: &Path) -> PathBuf {
    runtime_directory(install).join("mods").join(ID)
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(rename = "manifestVersion")]
    pub manifest_version: u32,
    pub id: String,
    pub version: String,
    pub name: String,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            manifest_version: 3,
            id: ID.into(),
            version: "1.0.0".into(),
            name: "Parhelion".into(),
        }
    }
}

impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.manifest_version != 3
            || self.id != ID
            || self.name.is_empty()
            || self.name.len() > 64
        {
            return Err("The managed Parhelion mod manifest has an unsupported identity".into());
        }
        let parts = self.version.split('.').collect::<Vec<_>>();
        if parts.len() != 3
            || parts.iter().any(|part| {
                part.is_empty()
                    || !part.bytes().all(|b| b.is_ascii_digit())
                    || (part.len() > 1 && part.starts_with('0'))
                    || part.parse::<u16>().is_err()
            })
        {
            return Err(
                "The mod version must contain three numbers from 0 to 65535 without leading zeros"
                    .into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: String,
    pub byte_length: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: u32,
    pub mod_id: String,
    pub artifacts: Vec<Artifact>,
}

impl Receipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || self.mod_id != ID
            || self.artifacts.is_empty()
            || self.artifacts.len() > 1024
        {
            return Err(
                "The managed mod receipt has an unsupported schema, identity or package count"
                    .into(),
            );
        }
        let mut names = BTreeSet::new();
        let mut packages = BTreeSet::new();
        for artifact in &self.artifacts {
            let (folder, name) = artifact
                .path
                .split_once('/')
                .ok_or("A mod artifact needs its package folder")?;
            if !matches!(folder, "packages" | "patches")
                || name.contains(['/', '\\'])
                || name.contains("_unp1_")
            {
                return Err("The managed mod receipt contains an unsupported package path".into());
            }
            let parsed = tiger_pkg::manager::PackagePath::parse(name)
                .ok_or("Invalid mod package filename")?;
            let id = u16::from_str_radix(&parsed.id, 16).map_err(|error| error.to_string())?;
            if !(0x0100..=0x0CFF).contains(&id)
                || !name.starts_with("w64_")
                || !name.ends_with(".pkg")
                || (folder == "packages" && parsed.patch != 0)
                || (folder == "patches" && parsed.patch == 0)
                || !names.insert(name.to_owned())
                || !packages.insert(id)
                || artifact.byte_length == 0
                || artifact.sha256.len() != 64
                || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(
                    "The managed mod receipt has invalid or repeated package metadata".into(),
                );
            }
        }
        Ok(())
    }
}

/// Reject links, junctions and other reparse points below the caller's canonical boundary,
/// including in prospective paths whose final components do not exist yet.
pub fn checked_path(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Mod path escaped its installation")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("Mod path is not relative to its installation".into());
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                #[cfg(windows)]
                let reparse = {
                    use std::os::windows::fs::MetadataExt;
                    metadata.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let reparse = false;
                if metadata.file_type().is_symlink() || reparse {
                    return Err(format!(
                        "Mod paths cannot pass through links or junctions: {}",
                        current.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not inspect {}: {error}", current.display())),
        }
    }
    Ok(())
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let file =
        File::open(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Mod metadata must be a regular file".into());
    }
    let limit = if path.file_name().is_some_and(|n| n == MANIFEST) {
        256 * 1024
    } else {
        1024 * 1024
    };
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Mod metadata exceeds its size limit".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    crate::strict_json::from_str(text)
        .map_err(|error| format!("Invalid {}: {error}", path.display()))
}

/// Checks what would keep the loader from reading Parhelion's mod beside the others: another
/// folder claiming Parhelion's id, or more folders than the loader reads. Other mods' packages
/// are allowed, since Sunrise merges patches from several mods. A mod the loader would refuse
/// for its own name or manifest is the loader's to report and is skipped here.
pub fn ensure_resolved_environment(install: &Path) -> Result<(), String> {
    let mods = runtime_directory(install).join("mods");
    checked_path(install, &mods)?;
    let entries = match fs::read_dir(&mods) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let mut count = 0;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        checked_path(install, &entry.path())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        count += 1;
        if name == ID {
            continue;
        }
        let manifest = entry.path().join(MANIFEST);
        checked_path(install, &manifest)?;
        if manifest.exists()
            && read_json::<serde_json::Value>(&manifest)
                .is_ok_and(|value| value.get("id").and_then(serde_json::Value::as_str) == Some(ID))
        {
            return Err(format!(
                "Mod folder {name} declares Parhelion's mod id. Remove or rename that mod"
            ));
        }
    }
    let own = directory(install);
    let proposed_count = count + usize::from(!own.exists());
    if proposed_count > 64 {
        return Err("Installing Parhelion would exceed the loader's 64-mod limit".into());
    }
    Ok(())
}

/// Metadata and package paths contributing to the managed content identity. This does not
/// hash all payloads at catalog startup. The installer verifies the receipt's full digests.
pub fn files(install: &Path) -> Result<Vec<PathBuf>, String> {
    ensure_readable(install)?;
    let folder = directory(install);
    checked_path(install, &folder)?;
    // Readers see stock packages and Parhelion's mod. Other mods' packages are not read, so
    // they never stop a reader.
    if !folder.try_exists().map_err(|e| e.to_string())? {
        return Ok(Vec::new());
    }
    let runtime = super::installed_runtime(install)?;
    if !runtime.uses_mod_loader()? {
        return Err("This installation has a Parhelion mod that requires Sunrise 0.6 or later. Restore a compatible Sunrise runtime before editing its content.".into());
    }
    checked_path(install, &folder.join(MANIFEST))?;
    checked_path(install, &folder.join(RECEIPT))?;
    let manifest: Manifest = read_json(&folder.join(MANIFEST))?;
    manifest.validate()?;
    let receipt: Receipt = read_json(&folder.join(RECEIPT))?;
    receipt.validate()?;
    let expected = receipt
        .artifacts
        .iter()
        .map(|a| a.path.clone())
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    for child in ["packages", "patches"] {
        let path = folder.join(child);
        checked_path(install, &path)?;
        if !path.exists() {
            continue;
        }
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            checked_path(install, &entry.path())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                return Err("Unexpected directory in the managed package set".into());
            }
            actual.insert(format!(
                "{child}/{}",
                entry
                    .file_name()
                    .to_str()
                    .ok_or("Non-Unicode mod filename")?
            ));
        }
    }
    if actual != expected {
        return Err("The managed package set differs from its installation receipt".into());
    }
    let mut files = vec![folder.join(MANIFEST), folder.join(RECEIPT)];
    for artifact in receipt.artifacts {
        let path = folder.join(&artifact.path);
        checked_path(install, &path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() != artifact.byte_length {
            return Err(format!(
                "Managed mod package changed or is missing: {}",
                path.display()
            ));
        }
        let name = path.file_name().ok_or("Mod package has no filename")?;
        if install.join("packages").join(name).exists() {
            return Err("Both legacy and mod-folder Parhelion packages are present. Recover or review the upgrade before reading content.".into());
        }
        files.push(path);
    }
    super::verify_installed_runtime(install, &runtime)?;
    Ok(files)
}

pub fn ensure_readable(install: &Path) -> Result<(), String> {
    let journal = runtime_directory(install).join(JOURNAL);
    checked_path(install, &journal)?;
    if journal.try_exists().map_err(|e| e.to_string())? {
        let record: serde_json::Value = read_json(&journal)?;
        if record.get("state").and_then(serde_json::Value::as_str) != Some("committed") {
            return Err("A Parhelion mod operation was interrupted. Recover it before reading or changing installed content.".into());
        }
    }
    Ok(())
}

/// Retain this view until every reader using it closes. The source packages are never edited.
pub struct View {
    /// Held while the view lives, so another opener knows it is in use. Declared first so it
    /// closes before the folder holding it is removed.
    _owner: File,
    directory: tempfile::TempDir,
    packages: PathBuf,
    /// The installation's own packages directory the view projects.
    source: PathBuf,
}

impl View {
    pub fn open(install: &Path) -> Result<Option<Self>, String> {
        ensure_readable(install)?;
        let additions = files(install)?;
        if additions.is_empty() {
            return Ok(None);
        }
        super::reader::initialize_source_decoder(&install.join("packages"))?;
        // Hard links work only within one volume, so the view sits inside the installation. In
        // the system temp folder on another drive, every installed package was copied instead.
        let root = runtime_directory(install).join(VIEWS);
        checked_path(install, &root)?;
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        remove_stale_views(&root);
        let directory = tempfile::Builder::new()
            .prefix(VIEW_PREFIX)
            .tempdir_in(&root)
            .map_err(|e| e.to_string())?;
        let owner = File::create(directory.path().join(VIEW_OWNER)).map_err(|e| e.to_string())?;
        fs2::FileExt::try_lock_exclusive(&owner).map_err(|e| e.to_string())?;
        let packages = directory.path().join("packages");
        fs::create_dir(&packages).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(install.join("packages")).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if is_package(&path) {
                checked_path(install, &path)?;
                link(&path, &packages.join(entry.file_name()))?;
            }
        }
        for path in additions.iter().filter(|p| is_package(p)) {
            link_or_copy(
                path,
                &packages.join(path.file_name().ok_or("Package has no filename")?),
            )?;
        }
        // A projected installation retains the source decoder location expected by readers.
        let bin = directory.path().join("bin/x64");
        fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        for name in ["oo2core_3_win64.dll", "oo2core_9_win64.dll"] {
            let source = install.join("bin/x64").join(name);
            if source.is_file() {
                link_or_copy(&source, &bin.join(name))?;
            }
        }
        Ok(Some(Self {
            _owner: owner,
            directory,
            packages,
            source: install.join("packages"),
        }))
    }

    pub fn packages(&self) -> &Path {
        &self.packages
    }
    pub fn source(&self) -> &Path {
        &self.source
    }
    pub(crate) fn install(&self) -> &Path {
        self.directory.path()
    }
}

/// Removes views left behind by a crash, or by a file still open when its reader closed. A view
/// in use holds its owner lock. A new one gets a grace period, because its owner lock is created
/// just after its folder. Removing a view removes only links, never installed packages.
fn remove_stale_views(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let view = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(VIEW_PREFIX))
            && entry.file_type().is_ok_and(|kind| kind.is_dir());
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > VIEW_GRACE);
        if !view || !old {
            continue;
        }
        let unowned = match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(entry.path().join(VIEW_OWNER))
        {
            Ok(owner) => fs2::FileExt::try_lock_exclusive(&owner).is_ok(),
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
        if unowned {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn is_package(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"))
}

/// Installed packages are never copied: a copy of every one would take as much space again.
fn link(source: &Path, target: &Path) -> Result<(), String> {
    if target.try_exists().map_err(|e| e.to_string())? {
        return Err("A mod package aliases an installed package file".into());
    }
    fs::hard_link(source, target).map_err(|error| {
        format!(
            "Could not read the installed packages together with the Sunrise mod, because {} could not be linked: {error}. The game's drive must support hard links.",
            source.display()
        )
    })
}

fn link_or_copy(source: &Path, target: &Path) -> Result<(), String> {
    if target.try_exists().map_err(|e| e.to_string())? {
        return Err("A mod package aliases an installed package file".into());
    }
    if fs::hard_link(source, target).is_ok() {
        return Ok(());
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| e.to_string())?;
    let mut input = File::open(source).map_err(|e| e.to_string())?;
    std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    Ok(())
}
