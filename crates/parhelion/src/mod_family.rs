//! A loader generation stages the main mod's files flat in the run directory and every other
//! member's files in a folder named for the member, so two members may each patch one stock
//! package under the same native name. A kept importer's unchanged files are read from the
//! installed family instead. The staged manifest binds those reads to the exact installed
//! receipt, so review never substitutes a later generation merely because it uses the same
//! filenames.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sundial::package_authoring::mod_content::{self, Importer, Manifest, Receipt};

use crate::artifact::ArtifactMetadata;
pub(crate) mod build;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Member {
    pub manifest: Manifest,
    pub receipt: Receipt,
    pub kept: bool,
}

/// One member's disposition in the reviewed or installed generation.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ModReport {
    pub id: String,
    pub name: String,
    pub status: ModStatus,
    pub packages: Vec<String>,
    /// Why a source builds into Parhelion, or why it needs Parhelion to load, when the build
    /// knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The shared tables Parhelion holds the source's rows of, which Sunrise can't merge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub enum ModStatus {
    Kept,
    Installed,
    Updated,
    Retired,
    /// A source whose items build into Parhelion's mod, as it shares data another mod changes in
    /// a way no shared table can hold. It has no folder of its own.
    InParhelion,
}

impl ModStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Kept => "Kept",
            Self::Installed => "Installed",
            Self::Updated => "Updated",
            Self::Retired => "Retired",
            Self::InParhelion => "In Parhelion",
        }
    }
}

/// A member's staged file name: the native name for the main mod, and `<member id>/<native name>`
/// for any other member.
pub(crate) fn staged_name(id: &str, native: &str) -> String {
    if id == mod_content::ID {
        native.to_owned()
    } else {
        format!("{id}/{native}")
    }
}

/// The member a staged file name belongs to and its native file name. A bare name is the main
/// mod's.
pub(crate) fn split(staged: &str) -> (&str, &str) {
    staged.split_once('/').unwrap_or((mod_content::ID, staged))
}

/// The native file name of a staged file, which package profiles and headers are read by.
pub(crate) fn native_name(staged: &str) -> &str {
    split(staged).1
}

/// The staged name of a receipt artifact, from the last part of its installed path.
fn staged_path(id: &str, artifact: &mod_content::Artifact) -> String {
    let native = artifact.path.rsplit('/').next().unwrap_or(&artifact.path);
    staged_name(id, native)
}

/// Whether a member registers content of its own: it patches stock packages. A member without
/// patches holds resource packages that another member's records name.
fn registers(receipt: &Receipt) -> bool {
    receipt
        .artifacts
        .iter()
        .any(|artifact| artifact.path.starts_with("patches/"))
}

/// Whether two receipts name the same files with the same bytes, in any order, and agree on
/// everything else they record.
pub(crate) fn same_receipt(left: &Receipt, right: &Receipt) -> bool {
    let files = |receipt: &Receipt| {
        let mut files = receipt
            .artifacts
            .iter()
            .map(|a| (a.path.clone(), a.byte_length, a.sha256.to_ascii_uppercase()))
            .collect::<Vec<_>>();
        files.sort();
        files
    };
    left.schema == right.schema
        && left.mod_id == right.mod_id
        && left.content == right.content
        && left.coupled == right.coupled
        && files(left) == files(right)
}

pub(crate) fn validate(
    members: &[Member],
    artifacts: &[ArtifactMetadata],
    merge_data: &[ArtifactMetadata],
    diagnostics: Option<&ArtifactMetadata>,
    scripts: &[ArtifactMetadata],
) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    let mut listed = BTreeMap::new();
    for member in members {
        member.manifest.validate()?;
        member.receipt.validate()?;
        let id = &member.manifest.id;
        if id != &member.receipt.mod_id || !ids.insert(id.as_str()) {
            return Err("The staged family repeats or disagrees on a mod identity".into());
        }
        // A kept member's files stay out of the run, so only a fingerprinted importer that
        // registers nothing of its own is kept.
        if member.kept
            && (id == mod_content::ID
                || member.receipt.content.is_none()
                || registers(&member.receipt))
        {
            return Err(
                "Only a fingerprinted importer mod that holds only resource packages can be kept"
                    .into(),
            );
        }
        for artifact in &member.receipt.artifacts {
            let name = staged_path(id, artifact);
            if listed.insert(name.clone(), artifact).is_some() {
                return Err(format!("More than one managed mod owns {name}"));
            }
        }
    }
    let expected = || {
        artifacts
            .iter()
            .chain(merge_data)
            .chain(diagnostics)
            .chain(scripts)
    };
    // Only another member's files sit in a folder of the run, one level deep.
    for artifact in expected() {
        let (id, native) = split(&artifact.file_name);
        if native.is_empty()
            || native.contains(['/', '\\'])
            || matches!(native, "." | "..")
            || (artifact.file_name.contains('/') && (id == mod_content::ID || !ids.contains(id)))
        {
            return Err(format!(
                "Staged file {} names no member of the staged family",
                artifact.file_name
            ));
        }
    }
    if members.is_empty() {
        return Ok(());
    }
    for member in members {
        if let Some(dependency) = member
            .manifest
            .dependencies
            .iter()
            .find(|dependency| !ids.contains(dependency.id.as_str()))
        {
            return Err(format!(
                "{} depends on {}, which the staged family doesn't hold",
                member.manifest.name, dependency.id
            ));
        }
    }
    // An importer with its own patches is independent. One without them is coupled: its
    // resources serve another member's records, and that member requires it.
    for member in members
        .iter()
        .filter(|m| m.manifest.id != mod_content::ID && !registers(&m.receipt))
    {
        let id = &member.manifest.id;
        if !members.iter().any(|other| {
            other
                .manifest
                .dependencies
                .iter()
                .any(|dependency| dependency.id == *id)
        }) {
            return Err(format!(
                "No member of the staged family requires {}, which holds only resource packages",
                member.manifest.name
            ));
        }
    }
    for expected in expected() {
        let actual = listed
            .remove(expected.file_name.as_str())
            .ok_or_else(|| format!("No managed mod owns {}", expected.file_name))?;
        if expected.byte_length != actual.byte_length
            || !expected.sha256.eq_ignore_ascii_case(&actual.sha256)
        {
            return Err(format!(
                "The mod receipt disagrees on {}",
                expected.file_name
            ));
        }
    }
    if !listed.is_empty() {
        return Err("The mod family contains files outside the staged artifact list".into());
    }
    Ok(())
}

/// The file staged under `name`, a member-qualified staged name: a kept member's installed file,
/// or the file in the run directory.
pub(crate) fn source(
    members: &[Member],
    stage: &Path,
    target: &Path,
    name: &str,
) -> Result<PathBuf, String> {
    if let Some(member) = members.iter().find(|member| {
        member.kept
            && member
                .receipt
                .artifacts
                .iter()
                .any(|a| staged_path(&member.manifest.id, a) == name)
    }) {
        let install = target.parent().ok_or("Missing game root")?;
        let directory = mod_content::member_directory(install, &member.manifest.id);
        mod_content::checked_path(install, &directory)?;
        let receipt: Receipt = mod_content::read_json(&directory.join(mod_content::RECEIPT))?;
        let manifest: Manifest = mod_content::read_json(&directory.join(mod_content::MANIFEST))?;
        if receipt != member.receipt || manifest != member.manifest {
            return Err(format!(
                "{} changed after staging. Build again",
                member.manifest.name
            ));
        }
        let artifact = receipt
            .artifacts
            .iter()
            .find(|a| staged_path(&member.manifest.id, a) == name)
            .unwrap();
        let path = directory.join(&artifact.path);
        mod_content::checked_path(install, &path)?;
        return Ok(path);
    }
    Ok(stage.join(name))
}

/// The family as the loader would read it from `sources`, staged files by member-qualified name.
/// Each member's patches and packages go to its own entry. The main mod leads when the family has
/// one, and otherwise its first member does, with the others in `family`.
pub(crate) fn incoming(
    members: &[Member],
    sources: &[(ArtifactMetadata, PathBuf)],
    retired: Vec<String>,
) -> Result<mod_content::Incoming, String> {
    if members.is_empty() {
        let mut main = mod_content::Incoming {
            retired,
            ..Default::default()
        };
        for (artifact, path) in sources
            .iter()
            .filter(|(a, _)| a.file_name.ends_with(".pkg"))
        {
            let profile =
                crate::package_profile::authored_package_for_file_name(&artifact.file_name)
                    .ok_or("Unknown managed package")?;
            if crate::package_profile::canonical_package(profile.package_id).is_some() {
                main.patches.push(path.clone());
            } else {
                main.packages.push(path.clone());
            }
        }
        return Ok(main);
    }
    let mut family = Vec::new();
    for member in members {
        let mut incoming = mod_content::Incoming {
            manifest: member.manifest.clone(),
            ..Default::default()
        };
        for artifact in member
            .receipt
            .artifacts
            .iter()
            .filter(|a| a.path.ends_with(".pkg"))
        {
            let name = staged_path(&member.manifest.id, artifact);
            let path = &sources
                .iter()
                .find(|(a, _)| a.file_name == name)
                .ok_or_else(|| format!("Missing managed package {name}"))?
                .1;
            if artifact.path.starts_with("patches/") {
                incoming.patches.push(path.clone());
            } else {
                incoming.packages.push(path.clone());
            }
        }
        family.push(incoming);
    }
    let lead = family
        .iter()
        .position(|member| member.manifest.id == mod_content::ID)
        .unwrap_or(0);
    let mut main = family.remove(lead);
    main.retired = retired;
    main.family = family;
    Ok(main)
}

pub(crate) fn reports(
    members: &[Member],
    install: &Path,
    artifacts: &[ArtifactMetadata],
) -> Result<Vec<ModReport>, String> {
    let mut installed = mod_content::family_directories(install)?;
    let mut reports = Vec::new();
    if members.is_empty() {
        if !installed.keys().any(|id| id != mod_content::ID) {
            return Ok(reports);
        }
        reports.push(ModReport {
            id: mod_content::ID.into(),
            name: Manifest::default().name,
            status: if installed.remove(mod_content::ID).is_some() {
                ModStatus::Updated
            } else {
                ModStatus::Installed
            },
            packages: artifacts.iter().map(|a| a.file_name.clone()).collect(),
            note: None,
            shared: Vec::new(),
        });
    }
    // A source that builds into Parhelion has no folder, and installation removes one it had.
    let mut combined = Vec::new();
    for source in members
        .iter()
        .flat_map(|member| member.receipt.coupled.keys())
    {
        let importer = Importer::from_source(source)
            .ok_or_else(|| format!("Unknown coupled source {source}"))?;
        let had_folder = installed.remove(&importer.id()).is_some();
        combined.push(ModReport {
            id: importer.id(),
            name: importer.name().to_owned(),
            status: ModStatus::InParhelion,
            packages: vec![],
            note: had_folder.then(|| "Its folder is removed.".to_owned()),
            shared: Vec::new(),
        });
    }
    for member in members {
        let prior = installed.remove(&member.manifest.id);
        // Installation keeps a member whose installed manifest and receipt already match.
        let unchanged = prior.as_ref().is_some_and(|directory| {
            let receipt = mod_content::read_json::<Receipt>(&directory.join(mod_content::RECEIPT));
            let manifest =
                mod_content::read_json::<Manifest>(&directory.join(mod_content::MANIFEST));
            matches!((receipt, manifest), (Ok(receipt), Ok(manifest))
                if manifest == member.manifest && same_receipt(&receipt, &member.receipt))
        });
        reports.push(ModReport {
            id: member.manifest.id.clone(),
            name: member.manifest.name.clone(),
            status: if member.kept || unchanged {
                ModStatus::Kept
            } else if prior.is_some() {
                ModStatus::Updated
            } else {
                ModStatus::Installed
            },
            packages: member
                .receipt
                .artifacts
                .iter()
                .filter(|a| a.path.ends_with(".pkg"))
                .map(|a| a.path.clone())
                .collect(),
            note: None,
            shared: Vec::new(),
        });
    }
    reports.extend(combined);
    for (id, directory) in installed {
        let manifest: Manifest = mod_content::read_json(&directory.join(mod_content::MANIFEST))?;
        reports.push(ModReport {
            id,
            name: manifest.name,
            status: ModStatus::Retired,
            packages: vec![],
            note: None,
            shared: Vec::new(),
        });
    }
    Ok(reports)
}
