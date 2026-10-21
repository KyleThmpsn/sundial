//! The managed mod's on-disk contract and read-only views of independent Sunrise mods.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

mod config;
mod content;
mod family;
pub use family::{
    ImportContent, Importer, family_directories, has_family, is_family_id, member_directory,
};
pub mod map_diagnostics;
pub mod merge;
pub mod plaintext;
mod view;
pub use content::{Coexistence, Incoming};
pub use view::View;

pub const ID: &str = "dev.kylethmpsn.parhelion";
pub const MANIFEST: &str = "mod.json";
pub const RECEIPT: &str = "parhelion-install.json";
pub const JOURNAL: &str = ".parhelion-mod-transaction.json";
/// The loader reads at most this many mod folders, the ones whose names sort first.
pub(super) const FOLDER_LIMIT: usize = 64;

/// A mod the loader refuses once its configured values are written: a value doesn't fit the
/// content it binds. Readers leave the mod out and read again, as the loader does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Refusal {
    id: String,
    reason: String,
}

/// Sunrise's own folder, beside the DLL it runs from: the game root's when one is there, else
/// `bin/x64`'s. Sunrise reads its mods folder from there.
pub fn runtime_directory(install: &Path) -> PathBuf {
    super::sunrise_module_path(install)
        .parent()
        .map_or_else(|| install.join("bin/x64"), Path::to_path_buf)
        .join("Sunrise")
}

pub fn directory(install: &Path) -> PathBuf {
    runtime_directory(install).join("mods").join(ID)
}

/// Sundial's release as the loader's `MAJOR.MINOR.PATCH`, which admits no pre-release suffix.
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR"),
    ".",
    env!("CARGO_PKG_VERSION_PATCH")
);
const AUTHOR: &str = "KyleThmpsn";
const SOURCE: &str = "https://github.com/KyleThmpsn/sundial";

/// Sunrise's Mods page shows the name and version as the mod's title, then its authors and
/// source.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(rename = "manifestVersion")]
    pub manifest_version: u32,
    pub id: String,
    pub version: String,
    pub name: String,
    pub authors: Vec<String>,
    #[serde(rename = "src")]
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Dependency>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Dependency {
    pub id: String,
    pub min_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_version: Option<String>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            manifest_version: 3,
            id: ID.into(),
            version: VERSION.into(),
            name: "Parhelion".into(),
            authors: vec![AUTHOR.into()],
            source: SOURCE.into(),
            dependencies: Vec::new(),
        }
    }
}

impl Manifest {
    pub fn for_importer(importer: Importer) -> Self {
        Self {
            id: importer.id(),
            name: importer.name().into(),
            ..Self::default()
        }
    }

    pub fn require(&mut self, importer: &Self) {
        self.dependencies.push(Dependency {
            id: importer.id.clone(),
            min_version: importer.version.clone(),
            max_version: Some(importer.version.clone()),
        });
        self.dependencies
            .sort_by(|left, right| left.id.cmp(&right.id));
    }

    pub fn validate(&self) -> Result<(), String> {
        // The loader's text limits: names and authors up to 64 bytes, at most eight authors and a
        // source up to 256 bytes, none holding a control character.
        let text =
            |value: &str, limit: usize| value.len() <= limit && !value.contains(char::is_control);
        if self.manifest_version != 3
            || !is_family_id(&self.id)
            || self.name.is_empty()
            || !text(&self.name, 64)
            || self.authors.len() > 8
            || self
                .authors
                .iter()
                .any(|author| author.is_empty() || !text(author, 64))
            || !text(&self.source, 256)
        {
            return Err("The managed Parhelion mod manifest has an unsupported identity".into());
        }
        let mut dependencies = BTreeSet::new();
        if self.dependencies.len() > 32
            || self.dependencies.iter().any(|dependency| {
                !is_family_id(&dependency.id)
                    || dependency.id == self.id
                    || dependency.min_version != self.version
                    || dependency.max_version.as_deref() != Some(&self.version)
                    || !dependencies.insert(&dependency.id)
            })
            || (self.id != ID && !self.dependencies.is_empty())
        {
            return Err(
                "Managed mod dependencies must name each used importer at the same release".into(),
            );
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ImportContent>,
}

impl Receipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 || !is_family_id(&self.mod_id) || self.artifacts.is_empty() {
            return Err(
                "The managed mod receipt has an unsupported schema, identity or package count"
                    .into(),
            );
        }
        let mut names = BTreeSet::new();
        let mut packages = BTreeSet::new();
        for artifact in &self.artifacts {
            if artifact.path == map_diagnostics::FILE_NAME {
                if self.mod_id != ID
                    || !names.insert(artifact.path.clone())
                    || artifact.byte_length == 0
                    || artifact.byte_length > map_diagnostics::BYTE_LIMIT
                    || !valid_sha256(&artifact.sha256)
                {
                    return Err("Invalid map diagnostics receipt".into());
                }
                continue;
            }
            let (folder, name) = artifact
                .path
                .split_once('/')
                .ok_or("A mod artifact needs its package folder")?;
            if !matches!(folder, "packages" | "patches")
                || name.contains(['/', '\\'])
                || name.contains("_unp1_")
                || (self.mod_id != ID
                    && (folder != "packages" || name.starts_with("w64_parhelion_investment_")))
            {
                return Err("The managed mod receipt contains an unsupported package path".into());
            }
            // Merge data sits beside its package under the same name.
            if let Some(stem) = name.strip_suffix(".merge") {
                let package = format!("{folder}/{stem}.pkg");
                if !self.artifacts.iter().any(|other| other.path == package)
                    || !names.insert(name.to_owned())
                    || artifact.byte_length == 0
                    || !valid_sha256(&artifact.sha256)
                {
                    return Err("The managed mod receipt has invalid or repeated merge data".into());
                }
                continue;
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
                || !valid_sha256(&artifact.sha256)
            {
                return Err(
                    "The managed mod receipt has invalid or repeated package metadata".into(),
                );
            }
        }
        if packages.is_empty() || packages.len() > 1024 {
            return Err(
                "The managed mod receipt has an unsupported schema, identity or package count"
                    .into(),
            );
        }
        if let Some(content) = &self.content {
            if self.mod_id == ID {
                return Err("Importer content cannot belong to the main mod receipt".into());
            }
            content.validate(&packages)?;
        }
        Ok(())
    }
}

fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
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
    } else if path.file_name().is_some_and(|n| n == RECEIPT) {
        // Linked metadata carries one compact entry descriptor per imported resource. Large
        // map families can exceed the ordinary one-megabyte metadata limit.
        32 * 1024 * 1024
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
    let mut proposed = family_directories(install)?
        .into_keys()
        .collect::<BTreeSet<_>>();
    proposed.insert(ID.into());
    ensure_family_environment(install, &proposed)
}

/// Checks the complete proposed family, excluding members the transaction retires.
pub fn ensure_family_environment(
    install: &Path,
    proposed: &BTreeSet<String>,
) -> Result<(), String> {
    if proposed.iter().any(|id| !is_family_id(id)) {
        return Err("The proposed managed mod family contains an unknown identity".into());
    }
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
        if is_family_id(name) {
            continue;
        }
        count += 1;
        let manifest = entry.path().join(MANIFEST);
        checked_path(install, &manifest)?;
        if manifest.exists()
            && read_json::<serde_json::Value>(&manifest).is_ok_and(|value| {
                value
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(is_family_id)
            })
        {
            return Err(format!(
                "Mod folder {name} declares a Parhelion family mod ID. Remove or rename that mod"
            ));
        }
    }
    let proposed_count = count + proposed.len();
    if proposed_count > FOLDER_LIMIT {
        return Err("Installing Parhelion would exceed the loader's 64-mod limit".into());
    }
    Ok(())
}

/// Metadata and package paths of the mod folder Parhelion installed, checked against its receipt.
/// Installation replaces and removes the folder whichever runtime is active, though only Sunrise
/// 0.6 or later loads it. This does not hash every payload at catalog startup. The installer
/// verifies the receipt's full digests.
pub fn files(install: &Path) -> Result<Vec<PathBuf>, String> {
    ensure_readable(install)?;
    let mut files = Vec::new();
    for (id, folder) in family_directories(install)? {
        files.extend(member_files(install, &id, &folder)?);
    }
    Ok(files)
}

fn member_files(install: &Path, id: &str, folder: &Path) -> Result<Vec<PathBuf>, String> {
    checked_path(install, folder)?;
    if !folder.try_exists().map_err(|e| e.to_string())? {
        return Ok(Vec::new());
    }
    checked_path(install, &folder.join(MANIFEST))?;
    checked_path(install, &folder.join(RECEIPT))?;
    let manifest: Manifest = read_json(&folder.join(MANIFEST))?;
    manifest.validate()?;
    let receipt: Receipt = read_json(&folder.join(RECEIPT))?;
    receipt.validate()?;
    if manifest.id != id || receipt.mod_id != id {
        return Err(format!(
            "Managed mod {id} has a manifest or receipt for another family member"
        ));
    }
    let expected = receipt
        .artifacts
        .iter()
        .map(|a| a.path.clone())
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    let diagnostics = folder.join(map_diagnostics::FILE_NAME);
    checked_path(install, &diagnostics)?;
    if diagnostics.try_exists().map_err(|e| e.to_string())? {
        actual.insert(map_diagnostics::FILE_NAME.to_owned());
    }
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
        if artifact.path != map_diagnostics::FILE_NAME
            && install.join("packages").join(name).exists()
        {
            return Err("Both legacy and mod-folder Parhelion packages are present. Recover or review the upgrade before reading content.".into());
        }
        files.push(path);
    }
    Ok(files)
}

/// The files that decide what the active runtime loads beyond stock packages: every Sunrise mod's
/// manifest, packages and merge data, and the saved configuration. A runtime that doesn't load
/// Sunrise mods has none. `files` is the stricter ownership contract installation and removal use.
pub fn content_files(install: &Path) -> Result<Vec<PathBuf>, String> {
    content::files(install)
}

/// How Parhelion's build would load beside the other mods Sunrise loads, with `incoming` in place
/// of Parhelion's installed folder: what would keep its content from loading intact, and the
/// entries it shares that Sunrise merges.
pub fn coexistence(install: &Path, incoming: &Incoming) -> Result<Coexistence, String> {
    Ok(content::Plan::read(install, &[], Some(incoming))?.coexistence)
}

/// What readers leave out of the installation's content, and why, for diagnostics. `None` until
/// a reader has read the current content. This never reads it, so it returns at once.
pub fn notices(install: &Path) -> Result<Option<Vec<String>>, String> {
    if !loads_mods(install)? {
        let installed = directory(install).try_exists().map_err(|e| e.to_string())?;
        return Ok(Some(
            installed
                .then(|| {
                    "Parhelion's mod folder is installed, but the active runtime doesn't load \
                     Sunrise mods, so its content isn't read"
                        .to_owned()
                })
                .into_iter()
                .collect(),
        ));
    }
    View::current_notices(install)
}

/// Whether the installation's runtime loads Sunrise mods, as Sunrise 0.6 and later do.
fn loads_mods(install: &Path) -> Result<bool, String> {
    super::installed_runtime(install)?.uses_mod_loader()
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

fn is_package(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"))
}
