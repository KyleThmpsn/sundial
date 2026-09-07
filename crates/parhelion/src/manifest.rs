use std::{
    collections::BTreeSet,
    fmt,
    path::{Component, Path},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};
use sundial::package_authoring::{FNV1_EMPTY_HASH, is_valid_package_tag};
use tiger_pkg::TagHash;

use crate::{
    NewWeaponPlan, SunriseProjectMetadata,
    artifact::ArtifactMetadata,
    package_profile::CANONICAL_ARTIFACT_FILE_NAMES,
    recipe::{WeaponRecipe, validate_parhelion_namespace},
};

pub(crate) const MANIFEST_FILE_NAME: &str = "parhelion-manifest.json";
pub(crate) const MANIFEST_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestDocument {
    pub(crate) schema: u32,
    pub(crate) source_package_directory: String,
    pub(crate) source_artifacts: Vec<ArtifactMetadata>,
    pub(crate) ignored_authored_files: Vec<String>,
    pub(crate) selection_fingerprint: String,
    pub(crate) selected_recipe_files: Vec<String>,
    pub(crate) project: ManifestProject,
    pub(crate) artifacts: Vec<ArtifactMetadata>,
}

impl ManifestDocument {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema != MANIFEST_SCHEMA {
            return Err(format!(
                "Unsupported Parhelion manifest schema {}; expected {MANIFEST_SCHEMA}",
                self.schema
            ));
        }
        if self.source_package_directory.trim().is_empty() {
            return Err("The manifest source package directory is empty".to_owned());
        }
        validate_sha256("selection fingerprint", &self.selection_fingerprint)?;
        validate_ignored_authored_files(&self.ignored_authored_files)?;
        validate_selected_recipe_files(&self.selected_recipe_files)?;
        if self.selected_recipe_files.len() != self.project.weapons.len() {
            return Err(format!(
                "The manifest lists {} selected recipe files for {} authored weapons",
                self.selected_recipe_files.len(),
                self.project.weapons.len()
            ));
        }
        validate_artifact_metadata("source artifact", &self.source_artifacts)?;
        validate_artifact_metadata("artifact", &self.artifacts)?;
        self.project.validate()
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ManifestHash(u32);

impl ManifestHash {
    pub(crate) const fn new(value: u32) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for ManifestHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{:08X}", self.0)
    }
}

impl Serialize for ManifestHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("0x{:08X}", self.0))
    }
}

impl<'de> Deserialize<'de> for ManifestHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        parse_canonical_hash(&encoded)
            .map(Self)
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestProject {
    pub(crate) weapons: Vec<ManifestWeapon>,
    pub(crate) sunrise: ManifestSunrise,
}

impl ManifestProject {
    pub(crate) fn from_build(
        recipes: &[WeaponRecipe],
        plans: &[NewWeaponPlan],
        sunrise: &SunriseProjectMetadata,
    ) -> Result<Self, String> {
        if recipes.len() != plans.len() {
            return Err(format!(
                "Compiler returned {} weapon plans for {} selected recipes",
                plans.len(),
                recipes.len()
            ));
        }
        let weapons = recipes
            .iter()
            .zip(plans)
            .map(|(recipe, plan)| {
                let recipe_item_hash = recipe.identity.item_hash.parse_u32().map_err(|error| {
                    format!("Recipe {:?} has an invalid item hash: {error}", recipe.name)
                })?;
                if recipe_item_hash != plan.item_hash {
                    return Err(format!(
                        "Compiler plan order mismatch: recipe {:?} is 0x{recipe_item_hash:08X}, plan is 0x{:08X}",
                        recipe.name, plan.item_hash
                    ));
                }
                Ok(ManifestWeapon {
                    namespace: recipe.namespace.clone(),
                    name: recipe.name.clone(),
                    item: ManifestTaggedIdentity {
                        hash: ManifestHash::new(plan.item_hash),
                        index: plan.item_index,
                        definition_tag: ManifestHash::new(plan.definition_tag.0),
                        string_tag: ManifestHash::new(plan.string_tag.0),
                    },
                    collectible: ManifestIdentity {
                        hash: ManifestHash::new(plan.collectible_hash),
                        index: plan.collectible_index,
                    },
                    unlock: ManifestUnlockIdentity {
                        hash: ManifestHash::new(plan.unlock_hash),
                        definition_index: plan.unlock_definition_index,
                        bank: plan.unlock_bank,
                        slot: plan.unlock_slot,
                    },
                    donor: ManifestDonorIdentity {
                        item_hash: ManifestHash::new(plan.template_item_hash),
                        definition_tag: ManifestHash::new(plan.template_definition_tag.0),
                        string_tag: ManifestHash::new(plan.template_string_tag.0),
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            weapons,
            sunrise: ManifestSunrise {
                badge_node_hashes: sunrise.badge_node_hashes.map(ManifestHash::new),
                badge_name_hash: ManifestHash::new(sunrise.badge_name_hash),
                badge_description_hash: ManifestHash::new(sunrise.badge_description_hash),
                badge_icon_tag: ManifestHash::new(sunrise.badge_icon_tag.0),
                watermark_layer_tag: ManifestHash::new(sunrise.watermark_layer_tag.0),
                watermarked_icon_containers: sunrise
                    .watermarked_icon_containers
                    .iter()
                    .map(|tag| ManifestHash::new(tag.0))
                    .collect(),
            },
        })
    }

    fn validate(&self) -> Result<(), String> {
        let mut namespaces = BTreeSet::new();
        let mut item_hashes = BTreeSet::new();
        let mut item_indices = BTreeSet::new();
        let mut collectible_hashes = BTreeSet::new();
        let mut collectible_indices = BTreeSet::new();
        let mut unlock_hashes = BTreeSet::new();
        let mut unlock_definitions = BTreeSet::new();
        let mut unlock_slots = BTreeSet::new();
        let mut authored_tags = BTreeSet::new();

        for weapon in &self.weapons {
            weapon.validate()?;
            if !namespaces.insert(weapon.namespace.as_str()) {
                return Err(format!(
                    "The manifest repeats weapon namespace {:?}",
                    weapon.namespace
                ));
            }
            insert_unique(&mut item_hashes, weapon.item.hash.get(), "item hash")?;
            insert_unique(&mut item_indices, weapon.item.index, "item index")?;
            insert_unique(
                &mut collectible_hashes,
                weapon.collectible.hash.get(),
                "collectible hash",
            )?;
            insert_unique(
                &mut collectible_indices,
                weapon.collectible.index,
                "collectible index",
            )?;
            insert_unique(&mut unlock_hashes, weapon.unlock.hash.get(), "unlock hash")?;
            insert_unique(
                &mut unlock_definitions,
                weapon.unlock.definition_index,
                "unlock definition index",
            )?;
            if !unlock_slots.insert((weapon.unlock.bank, weapon.unlock.slot)) {
                return Err(format!(
                    "The manifest repeats authored unlock bank {}, slot {}",
                    weapon.unlock.bank, weapon.unlock.slot
                ));
            }
            for (label, tag) in [
                ("definition tag", weapon.item.definition_tag),
                ("string tag", weapon.item.string_tag),
            ] {
                if !authored_tags.insert(tag.get()) {
                    return Err(format!(
                        "The manifest repeats authored {label} 0x{:08X}",
                        tag.get()
                    ));
                }
            }
        }
        self.sunrise.validate(&mut authored_tags)?;
        // Identical appearance/edit/rarity requests intentionally share a private icon.
        // The compiler validates every weapon-to-container reference; this list inventories
        // distinct resources, not one resource per weapon.
        if self.sunrise.watermarked_icon_containers.len() > self.weapons.len()
            || (!self.weapons.is_empty() && self.sunrise.watermarked_icon_containers.is_empty())
        {
            return Err(format!(
                "The manifest has {} authored weapons but {} watermarked icon containers",
                self.weapons.len(),
                self.sunrise.watermarked_icon_containers.len()
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestWeapon {
    pub(crate) namespace: String,
    pub(crate) name: String,
    pub(crate) item: ManifestTaggedIdentity,
    pub(crate) collectible: ManifestIdentity,
    pub(crate) unlock: ManifestUnlockIdentity,
    pub(crate) donor: ManifestDonorIdentity,
}

impl ManifestWeapon {
    fn validate(&self) -> Result<(), String> {
        validate_parhelion_namespace(&self.namespace)
            .map_err(|error| format!("Manifest {error}"))?;
        if self.name.trim().is_empty() || self.name.contains('\0') {
            return Err(format!(
                "Manifest weapon {} has an empty or malformed name",
                self.namespace
            ));
        }
        for (label, hash) in [
            ("item hash", self.item.hash),
            ("collectible hash", self.collectible.hash),
            ("unlock hash", self.unlock.hash),
            ("donor item hash", self.donor.item_hash),
        ] {
            validate_identity_hash(&self.namespace, label, hash)?;
        }
        for (label, tag) in [
            ("item definition tag", self.item.definition_tag),
            ("item string tag", self.item.string_tag),
            ("donor definition tag", self.donor.definition_tag),
            ("donor string tag", self.donor.string_tag),
        ] {
            validate_package_tag(&self.namespace, label, tag)?;
        }
        if self.item.hash == self.donor.item_hash {
            return Err(format!(
                "Manifest weapon {} reuses its donor item hash",
                self.namespace
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestIdentity {
    pub(crate) hash: ManifestHash,
    pub(crate) index: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestTaggedIdentity {
    pub(crate) hash: ManifestHash,
    pub(crate) index: u16,
    pub(crate) definition_tag: ManifestHash,
    pub(crate) string_tag: ManifestHash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestUnlockIdentity {
    pub(crate) hash: ManifestHash,
    pub(crate) definition_index: u16,
    pub(crate) bank: u8,
    pub(crate) slot: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestDonorIdentity {
    pub(crate) item_hash: ManifestHash,
    pub(crate) definition_tag: ManifestHash,
    pub(crate) string_tag: ManifestHash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestSunrise {
    pub(crate) badge_node_hashes: [ManifestHash; 4],
    pub(crate) badge_name_hash: ManifestHash,
    pub(crate) badge_description_hash: ManifestHash,
    pub(crate) badge_icon_tag: ManifestHash,
    pub(crate) watermark_layer_tag: ManifestHash,
    pub(crate) watermarked_icon_containers: Vec<ManifestHash>,
}

impl ManifestSunrise {
    fn validate(&self, authored_tags: &mut BTreeSet<u32>) -> Result<(), String> {
        let mut identity_hashes = BTreeSet::new();
        for hash in self
            .badge_node_hashes
            .into_iter()
            .chain([self.badge_name_hash, self.badge_description_hash])
        {
            validate_identity_hash("Project Sunrise", "identity hash", hash)?;
            if !identity_hashes.insert(hash.get()) {
                return Err(
                    "The manifest contains a repeated Project Sunrise identity hash".to_owned(),
                );
            }
        }
        for (label, tag) in [
            ("badge icon tag", self.badge_icon_tag),
            ("watermark layer tag", self.watermark_layer_tag),
        ] {
            validate_package_tag("Project Sunrise", label, tag)?;
            if !authored_tags.insert(tag.get()) {
                return Err(format!(
                    "The manifest has a repeated Project Sunrise {label}"
                ));
            }
        }
        let mut icon_containers = BTreeSet::new();
        for container in &self.watermarked_icon_containers {
            validate_package_tag("Project Sunrise", "watermarked icon container", *container)?;
            if !icon_containers.insert(container.get()) {
                return Err(
                    "The manifest contains a repeated watermarked icon container".to_owned(),
                );
            }
            if !authored_tags.insert(container.get()) {
                return Err(format!(
                    "The manifest repeats authored icon tag 0x{:08X}",
                    container.get()
                ));
            }
        }
        Ok(())
    }
}

fn validate_identity_hash(owner: &str, label: &str, hash: ManifestHash) -> Result<(), String> {
    if matches!(hash.get(), 0 | FNV1_EMPTY_HASH) {
        return Err(format!(
            "Manifest {owner} has reserved {label} 0x{:08X}",
            hash.get()
        ));
    }
    Ok(())
}

fn validate_package_tag(owner: &str, label: &str, tag: ManifestHash) -> Result<(), String> {
    if !is_valid_package_tag(TagHash(tag.get())) {
        return Err(format!(
            "Manifest {owner} has malformed {label} 0x{:08X}",
            tag.get()
        ));
    }
    Ok(())
}

pub(crate) fn recipe_selection_fingerprint(recipes: &[WeaponRecipe]) -> Result<String, String> {
    let mut digest = Sha256::new();
    for recipe in recipes {
        let encoded = recipe
            .to_json_pretty()
            .map_err(|error| format!("Could not normalize recipe: {error}"))?;
        digest.update(encoded.len().to_le_bytes());
        digest.update(encoded.as_bytes());
    }
    Ok(format!("{:X}", digest.finalize()))
}

fn parse_canonical_hash(encoded: &str) -> Result<u32, String> {
    if encoded.len() != 10
        || !encoded.starts_with("0x")
        || !encoded[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'A'..=b'F'))
    {
        return Err(format!(
            "manifest hash {encoded:?} must use canonical 0x00000000 uppercase hexadecimal form"
        ));
    }
    u32::from_str_radix(&encoded[2..], 16)
        .map_err(|error| format!("manifest hash {encoded:?} is invalid: {error}"))
}

fn validate_sha256(label: &str, encoded: &str) -> Result<(), String> {
    if encoded.len() != 64
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'A'..=b'F'))
    {
        return Err(format!(
            "The manifest {label} is not a canonical uppercase SHA-256 digest"
        ));
    }
    Ok(())
}

fn validate_artifact_metadata(label: &str, artifacts: &[ArtifactMetadata]) -> Result<(), String> {
    for artifact in artifacts {
        if artifact.file_name.is_empty() || artifact.byte_length == 0 {
            return Err(format!(
                "The manifest {label} has an empty name or zero byte length"
            ));
        }
        validate_sha256(
            &format!("{label} {} digest", artifact.file_name),
            &artifact.sha256,
        )?;
    }
    Ok(())
}

fn validate_ignored_authored_files(files: &[String]) -> Result<(), String> {
    let canonical = CANONICAL_ARTIFACT_FILE_NAMES
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for file in files {
        if !canonical.contains(file.as_str()) {
            return Err(format!(
                "The manifest ignored-authored list contains unexpected file {file:?}"
            ));
        }
        if !seen.insert(file.as_str()) {
            return Err(format!(
                "The manifest ignored-authored list repeats file {file:?}"
            ));
        }
    }
    Ok(())
}

fn validate_selected_recipe_files(files: &[String]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for file in files {
        let normalized = file.replace('\\', "/");
        let path = Path::new(&normalized);
        let components = path.components().collect::<Vec<_>>();
        let valid = components.len() == 2
            && matches!(components[0], Component::Normal(value) if value == "recipes")
            && matches!(components[1], Component::Normal(_))
            && normalized.ends_with(".parhelion.json")
            && !normalized.contains('\0');
        if !valid {
            return Err(format!(
                "Manifest selected recipe path {file:?} is not a direct recipes/*.parhelion.json path"
            ));
        }
        if !seen.insert(normalized) {
            return Err(format!(
                "The manifest repeats selected recipe file {file:?}"
            ));
        }
    }
    Ok(())
}

fn insert_unique<T>(values: &mut BTreeSet<T>, value: T, label: &str) -> Result<(), String>
where
    T: Copy + fmt::Display + Ord,
{
    if !values.insert(value) {
        return Err(format!("The manifest repeats {label} {value}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_manifest_json() -> serde_json::Value {
        json!({
            "schema": MANIFEST_SCHEMA,
            "source_package_directory": "C:\\Destiny2\\packages",
            "source_artifacts": [{
                "file_name": "stock.pkg",
                "byte_length": 1,
                "sha256": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
            }],
            "ignored_authored_files": [],
            "selection_fingerprint": recipe_selection_fingerprint(&[]).unwrap(),
            "selected_recipe_files": [],
            "project": {
                "weapons": [],
                "sunrise": {
                    "badge_node_hashes": [
                        "0x53554E42",
                        "0x53554E54",
                        "0x53554E48",
                        "0x53554E57"
                    ],
                    "badge_name_hash": "0x53554E4E",
                    "badge_description_hash": "0x53554E44",
                    "badge_icon_tag": "0x81D40005",
                    "watermark_layer_tag": "0x81A29560",
                    "watermarked_icon_containers": []
                }
            },
            "artifacts": [{
                "file_name": "authored.pkg",
                "byte_length": 1,
                "sha256": "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
            }]
        })
    }

    #[test]
    fn manifest_hash_requires_the_emitted_canonical_form() {
        for invalid in [r#""53554E44""#, r#""0x53554e44""#, r#""0X53554E44""#] {
            assert!(serde_json::from_str::<ManifestHash>(invalid).is_err());
        }
        let hash: ManifestHash = serde_json::from_str(r#""0x53554E44""#).unwrap();
        assert_eq!(hash.get(), 0x5355_4E44);
        assert_eq!(serde_json::to_string(&hash).unwrap(), r#""0x53554E44""#);
    }

    #[test]
    fn strict_model_rejects_unknown_and_missing_fields() {
        let mut unknown = valid_manifest_json();
        unknown["project"]["sunrise"]["unexpected"] = json!(true);
        assert!(serde_json::from_value::<ManifestDocument>(unknown).is_err());

        let mut missing = valid_manifest_json();
        missing
            .as_object_mut()
            .unwrap()
            .remove("selection_fingerprint");
        assert!(serde_json::from_value::<ManifestDocument>(missing).is_err());
    }

    #[test]
    fn complete_shared_model_validates_the_emitted_profile() {
        let manifest: ManifestDocument = serde_json::from_value(valid_manifest_json()).unwrap();
        manifest.validate().unwrap();

        let mut wrong_profile = manifest;
        wrong_profile.project.sunrise.badge_icon_tag =
            wrong_profile.project.sunrise.watermark_layer_tag;
        assert!(wrong_profile.validate().is_err());
    }

    #[test]
    fn manifest_rejects_reserved_identity_hashes_and_incomplete_icon_sets() {
        let mut reserved: ManifestDocument = serde_json::from_value(valid_manifest_json()).unwrap();
        reserved.project.sunrise.badge_name_hash = ManifestHash::new(FNV1_EMPTY_HASH);
        assert!(reserved.validate().unwrap_err().contains("reserved"));

        let mut incomplete: ManifestDocument =
            serde_json::from_value(valid_manifest_json()).unwrap();
        incomplete
            .project
            .sunrise
            .watermarked_icon_containers
            .push(ManifestHash::new(0x81D4_0006));
        assert!(
            incomplete
                .validate()
                .unwrap_err()
                .contains("0 authored weapons but 1 watermarked icon containers")
        );
    }
}
