//! A loader generation can read unchanged importer files from the installed family. The staged
//! manifest binds those reads to the exact installed receipt, so review never substitutes a later
//! generation merely because it uses the same filenames.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sundial::package_authoring::mod_content::{self, Manifest, Receipt};

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
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub enum ModStatus {
    Kept,
    Installed,
    Updated,
    Retired,
}

impl ModStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Kept => "Kept",
            Self::Installed => "Installed",
            Self::Updated => "Updated",
            Self::Retired => "Retired",
        }
    }
}

pub(crate) fn validate(
    members: &[Member],
    artifacts: &[ArtifactMetadata],
    merge_data: &[ArtifactMetadata],
    diagnostics: Option<&ArtifactMetadata>,
    scripts: &[ArtifactMetadata],
) -> Result<(), String> {
    if members.is_empty() {
        return Ok(());
    }
    let mut ids = BTreeSet::new();
    let mut listed = BTreeMap::new();
    for member in members {
        member.manifest.validate()?;
        member.receipt.validate()?;
        let id = &member.manifest.id;
        if id != &member.receipt.mod_id || !ids.insert(id.as_str()) {
            return Err("The staged family repeats or disagrees on a mod identity".into());
        }
        if member.kept && (id == mod_content::ID || member.receipt.content.is_none()) {
            return Err("Only a fingerprinted importer mod can be kept".into());
        }
        for artifact in &member.receipt.artifacts {
            let name = artifact.path.rsplit('/').next().unwrap();
            if listed.insert(name, artifact).is_some() {
                return Err(format!("More than one managed mod owns {name}"));
            }
        }
    }
    let main = members
        .iter()
        .find(|member| member.manifest.id == mod_content::ID)
        .ok_or("A staged family needs its main mod")?;
    let dependencies = main
        .manifest
        .dependencies
        .iter()
        .map(|dependency| dependency.id.as_str())
        .collect::<BTreeSet<_>>();
    ids.remove(mod_content::ID);
    if ids != dependencies {
        return Err("The main mod must depend on every importer in the staged family".into());
    }
    for expected in artifacts
        .iter()
        .chain(merge_data)
        .chain(diagnostics)
        .chain(scripts)
    {
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
                .any(|a| a.path.rsplit('/').next() == Some(name))
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
            .find(|a| a.path.rsplit('/').next() == Some(name))
            .unwrap();
        let path = directory.join(&artifact.path);
        mod_content::checked_path(install, &path)?;
        return Ok(path);
    }
    Ok(stage.join(name))
}

pub(crate) fn incoming(
    members: &[Member],
    sources: &[(ArtifactMetadata, PathBuf)],
    retired: Vec<String>,
) -> Result<mod_content::Incoming, String> {
    let mut main = mod_content::Incoming {
        retired,
        ..Default::default()
    };
    if members.is_empty() {
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
            let name = artifact.path.rsplit('/').next().unwrap();
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
        if member.manifest.id == mod_content::ID {
            main.manifest = incoming.manifest;
            main.packages = incoming.packages;
            main.patches = incoming.patches;
        } else {
            main.family.push(incoming);
        }
    }
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
        });
    }
    for member in members {
        let prior = installed.remove(&member.manifest.id).is_some();
        reports.push(ModReport {
            id: member.manifest.id.clone(),
            name: member.manifest.name.clone(),
            status: if member.kept {
                ModStatus::Kept
            } else if prior {
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
        });
    }
    for (id, directory) in installed {
        let manifest: Manifest = mod_content::read_json(&directory.join(mod_content::MANIFEST))?;
        reports.push(ModReport {
            id,
            name: manifest.name,
            status: ModStatus::Retired,
            packages: vec![],
        });
    }
    Ok(reports)
}
