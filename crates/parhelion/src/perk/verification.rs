//! Append-only observations bound to the exact authored configuration and staged files.
use super::PerkRecipe;
use crate::{ItemKind, WeaponRecipe, artifact::ArtifactMetadata};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) fn recipe_hash(recipe: &PerkRecipe) -> Result<String, String> {
    serde_json::to_vec(recipe)
        .map(|bytes| format!("{:X}", Sha256::digest(bytes)))
        .map_err(|error| error.to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Check {
    Activation,
    Stacking,
    Cleanup,
    Reactivation,
    StockIsolation,
    Persistence,
}
impl Check {
    pub const ALL: [Self; 6] = [
        Self::Activation,
        Self::Stacking,
        Self::Cleanup,
        Self::Reactivation,
        Self::StockIsolation,
        Self::Persistence,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Activation => "Activation",
            Self::Stacking => "Stacking",
            Self::Cleanup => "Cleanup",
            Self::Reactivation => "Reactivation",
            Self::StockIsolation => "Stock Isolation",
            Self::Persistence => "Persistence",
        }
    }
    pub fn guidance(self) -> &'static str {
        match self {
            Self::Activation => {
                "Trigger the behavior on the bound destination and record its measured result."
            }
            Self::Stacking => {
                "Repeat the trigger and combine it with other active perks. Record caps and ordering."
            }
            Self::Cleanup => {
                "End the behavior, holster, swap equipment, and die. Record any remaining effects."
            }
            Self::Reactivation => {
                "Trigger again after expiry and after cooldown. Check it can start and end repeatedly."
            }
            Self::StockIsolation => {
                "Compare the unchanged stock perk and weapon before and after the private package is used."
            }
            Self::Persistence => {
                "Return to orbit and relaunch. Confirm the same saved perk and package behave consistently."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    #[default]
    NotTested,
    Passed,
    Failed,
    Inconclusive,
}
impl Outcome {
    pub const ALL: [Self; 4] = [
        Self::NotTested,
        Self::Passed,
        Self::Failed,
        Self::Inconclusive,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::NotTested => "Not Tested",
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Inconclusive => "Inconclusive",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub check: Check,
    pub outcome: Outcome,
    pub notes: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Destination {
    pub namespace: String,
    pub location: String,
    pub recipe_file: String,
    pub recipe_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub schema: u32,
    pub recipe: PerkRecipe,
    pub recipe_sha256: String,
    pub destination_kind: ItemKind,
    pub client_build: String,
    pub runtime_build: String,
    pub captured_at: u64,
    pub staged_manifest_sha256: Option<String>,
    pub destinations: Vec<Destination>,
    pub selected_destination: Option<usize>,
    pub packages: Vec<ArtifactMetadata>,
    pub observations: Vec<Observation>,
}

impl Record {
    pub fn new(recipe: &PerkRecipe, kind: ItemKind, client_build: &str) -> Result<Self, String> {
        recipe.validate_draft()?;
        Ok(Self {
            schema: 1,
            recipe: recipe.clone(),
            recipe_sha256: recipe_hash(recipe)?,
            destination_kind: kind,
            client_build: client_build.trim().into(),
            runtime_build: String::new(),
            captured_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs(),
            staged_manifest_sha256: None,
            destinations: Vec::new(),
            selected_destination: None,
            packages: Vec::new(),
            observations: Check::ALL
                .into_iter()
                .map(|check| Observation {
                    check,
                    outcome: Outcome::NotTested,
                    notes: String::new(),
                })
                .collect(),
        })
    }

    pub fn matches(&self, recipe: &PerkRecipe) -> bool {
        recipe_hash(recipe).is_ok_and(|hash| hash == self.recipe_sha256)
    }

    /// Uses the build's own recipe fingerprint and artifact digests before recording a binding.
    /// It reads staged content only and never installs it or changes a stock resource.
    pub fn bind_staged(&mut self, run: &Path) -> Result<(), String> {
        let manifest_path = run.join(crate::manifest::MANIFEST_FILE_NAME);
        let bytes = fs::read(&manifest_path).map_err(|e| e.to_string())?;
        let manifest: crate::manifest::ManifestDocument =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        manifest.validate()?;
        let mut recipes = Vec::new();
        let mut destinations = Vec::new();
        for relative in &manifest.selected_recipe_files {
            let path = run.join(relative);
            let weapon = WeaponRecipe::load_json(&path).map_err(|e| e.to_string())?;
            if weapon.kind == self.destination_kind {
                let locations = bound_locations(&weapon, &self.recipe);
                if !locations.is_empty() {
                    let digest = crate::artifact::digest_file(&path).map_err(|e| e.to_string())?;
                    for location in locations {
                        destinations.push(Destination {
                            namespace: weapon.namespace.clone(),
                            location,
                            recipe_file: relative.clone(),
                            recipe_sha256: digest.sha256.clone(),
                        });
                    }
                }
            }
            recipes.push(weapon);
        }
        if crate::manifest::recipe_selection_fingerprint(&recipes)?
            != manifest.selection_fingerprint
        {
            return Err("The staged recipe selection no longer matches its manifest.".into());
        }
        if destinations.is_empty() {
            return Err(
                "This staged build does not contain the current perk on this kind of destination."
                    .into(),
            );
        }
        for artifact in &manifest.artifacts {
            let digest = crate::artifact::digest_file(&run.join(&artifact.file_name))
                .map_err(|e| e.to_string())?;
            if digest.byte_length != artifact.byte_length || digest.sha256 != artifact.sha256 {
                return Err(format!(
                    "Staged package {} no longer matches its manifest.",
                    artifact.file_name
                ));
            }
        }
        if manifest.artifacts.is_empty() {
            return Err("The staged build has no compiled packages.".into());
        }
        self.staged_manifest_sha256 = Some(format!("{:X}", Sha256::digest(bytes)));
        self.destinations = destinations;
        self.selected_destination = (self.destinations.len() == 1).then_some(0);
        self.packages = manifest.artifacts;
        // A new package binding never inherits observations about an earlier build.
        for observation in &mut self.observations {
            observation.outcome = Outcome::NotTested;
            observation.notes.clear();
        }
        Ok(())
    }

    pub fn record(&mut self, check: Check, outcome: Outcome, notes: &str) -> Result<(), String> {
        if outcome != Outcome::NotTested {
            if self.client_build.trim().is_empty() || self.runtime_build.trim().is_empty() {
                return Err("Enter the client and runtime builds used for the observation.".into());
            }
            if self.packages.is_empty()
                || self.destinations.is_empty()
                || self.staged_manifest_sha256.is_none()
            {
                return Err("Bind the staged build before recording a gameplay result.".into());
            }
            if self
                .selected_destination
                .and_then(|index| self.destinations.get(index))
                .is_none()
            {
                return Err(
                    "Choose the exact destination used for the gameplay observation.".into(),
                );
            }
            if notes.trim().is_empty() {
                return Err(
                    "Describe what was observed, including the test setup and result.".into(),
                );
            }
        }
        let observation = self
            .observations
            .iter_mut()
            .find(|observation| observation.check == check)
            .ok_or("This check is missing from the record.")?;
        observation.outcome = outcome;
        observation.notes = notes.trim().into();
        Ok(())
    }

    pub fn gameplay_verified(&self) -> bool {
        !self.packages.is_empty()
            && !self.destinations.is_empty()
            && self
                .selected_destination
                .and_then(|index| self.destinations.get(index))
                .is_some()
            && self.staged_manifest_sha256.is_some()
            && !self.client_build.trim().is_empty()
            && !self.runtime_build.trim().is_empty()
            && Check::ALL.iter().all(|check| {
                self.observations.iter().any(|entry| {
                    entry.check == *check
                        && entry.outcome == Outcome::Passed
                        && !entry.notes.trim().is_empty()
                })
            })
    }

    fn validate(&self) -> Result<(), String> {
        self.recipe.validate_draft()?;
        if self.schema != 1 || !self.matches(&self.recipe) {
            return Err("The verification record does not match its recipe snapshot.".into());
        }
        if self.observations.len() != Check::ALL.len()
            || !Check::ALL.iter().all(|check| {
                self.observations
                    .iter()
                    .filter(|entry| entry.check == *check)
                    .count()
                    == 1
            })
        {
            return Err("The verification record has missing or repeated checks.".into());
        }
        let mut checked = self.clone();
        for observation in &self.observations {
            checked.record(observation.check, observation.outcome, &observation.notes)?;
        }
        Ok(())
    }

    pub fn save(&self, directory: &Path) -> Result<PathBuf, String> {
        self.validate()?;
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = directory.join(format!(
            "{}-{stamp}-{}.verification.json",
            self.recipe.id,
            std::process::id()
        ));
        sundial::storage::create_file(
            &path,
            &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let file = fs::File::open(path).map_err(|e| e.to_string())?;
        let record: Self =
            sundial::package_authoring::read_json(file).map_err(|e| e.to_string())?;
        record.validate()?;
        Ok(record)
    }
}

fn bound_locations(weapon: &WeaponRecipe, perk: &PerkRecipe) -> Vec<String> {
    let mut locations = Vec::new();
    for variant in &weapon.overrides.socket_plug_variants {
        let mut expected = perk.at_socket(variant.socket_index, variant.choice_index);
        if !weapon.kind.is_weapon() {
            expected
                .source_plug_hash
                .clone_from(&variant.source_plug_hash);
        }
        if *variant == expected {
            locations.push(format!(
                "Socket {}, Choice {}",
                variant.socket_index + 1,
                variant.choice_index + 1
            ));
        }
    }
    if let Some(abilities) = &weapon.overrides.subclass_abilities {
        for choice in &abilities.choices {
            if choice
                .edits
                .custom_perks
                .iter()
                .any(|candidate| candidate == perk)
            {
                locations.push(format!("Ability {}", choice.entry));
            }
        }
        for path in &abilities.attunements {
            for node in &path.nodes {
                if node
                    .edits
                    .custom_perks
                    .iter()
                    .any(|candidate| candidate == perk)
                {
                    locations.push(format!("{} Node {}", path.path.label(), node.position));
                }
            }
        }
    }
    locations
}
