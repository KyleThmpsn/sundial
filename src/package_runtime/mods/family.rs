//! Ownership shared by compilation, readers and directory transactions.
use super::*;
use std::collections::BTreeMap;

/// A converter's source identity. Native authoring and mixed-source groups have no importer.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum Importer {
    HaloReach,
    D2Mot,
    Marathon,
    D1Roi,
}

impl Importer {
    pub fn from_source(source: &str) -> Option<Self> {
        match source {
            "halo_reach" => Some(Self::HaloReach),
            "d2_mot" => Some(Self::D2Mot),
            "marathon" => Some(Self::Marathon),
            "d1_roi" => Some(Self::D1Roi),
            _ => None,
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::from_source(id.strip_prefix(ID)?.strip_prefix('-')?)
    }

    pub const fn source(self) -> &'static str {
        match self {
            Self::HaloReach => "halo_reach",
            Self::D2Mot => "d2_mot",
            Self::Marathon => "marathon",
            Self::D1Roi => "d1_roi",
        }
    }

    pub fn id(self) -> String {
        format!("{ID}-{}", self.source())
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::HaloReach => "Parhelion Halo: Reach",
            Self::D2Mot => "Parhelion Destiny 2: Monument of Triumph",
            Self::Marathon => "Parhelion Marathon",
            Self::D1Roi => "Parhelion Destiny 1: Rise of Iron",
        }
    }
}

/// The namespace is owned as a family, including a member from a newer editor. A prefix without
/// a valid suffix does not grant ownership. Manifests and receipts still have to agree.
pub fn is_family_id(id: &str) -> bool {
    id == ID
        || (content::valid_id(id)
            && id
                .strip_prefix(ID)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|source| !source.is_empty() && !source.contains('.')))
}

/// The path of a validated family identity. Readers and transactions check the resolved path
/// beneath the installation before accessing it.
pub fn member_directory(install: &Path, id: &str) -> PathBuf {
    runtime_directory(install).join("mods").join(id)
}

/// Existing family folders, in ordinal order. This is ownership discovery on every runtime,
/// independent of whether the current loader admits their content.
pub fn family_directories(install: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let root = runtime_directory(install).join("mods");
    checked_path(install, &root)?;
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("Could not inspect managed mods: {error}")),
    };
    let mut members = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_family_id(&id) {
            continue;
        }
        checked_path(install, &entry.path())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            return Err(format!("Managed mod {id} is not a directory"));
        }
        members.insert(id, entry.path());
    }
    Ok(members)
}

pub fn has_family(install: &Path) -> Result<bool, String> {
    Ok(!family_directories(install)?.is_empty())
}

/// Compiler-specific state is opaque to the reader. The compiler validates its own schema and
/// binds it, the input hashes and allocations into `fingerprint` before reusing installed bytes.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ImportContent {
    pub fingerprint: String,
    pub inputs: String,
    pub allocation: Vec<u16>,
    pub state: serde_json::Value,
}

impl ImportContent {
    pub(super) fn validate(&self, packages: &BTreeSet<u16>) -> Result<(), String> {
        if !valid_sha256(&self.fingerprint)
            || !valid_sha256(&self.inputs)
            || self.allocation.is_empty()
            || self.allocation.iter().copied().collect::<BTreeSet<_>>() != *packages
            || self.allocation.len() != packages.len()
        {
            return Err(
                "The importer receipt has invalid fingerprints or package allocations".into(),
            );
        }
        Ok(())
    }
}
