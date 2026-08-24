//! Serialized catalog cache envelope and compatibility checks.

use std::{collections::HashMap, fs, io::Read, path::Path};

use serde::{Deserialize, Serialize};

use crate::orbit_map;

use super::{
    CollectibleDef, InventoryMetadata, ItemDef, ItemMaterialRequirementSetIndices,
    ItemPackageMetadata, ItemStatDefinition, MaterialRequirementSetDef, ObjectiveDef,
    ProgressionDefinition, SandboxPerkDefinition, UnlockDefinition,
};

pub(super) const CACHE_SCHEMA: u32 = 81;
pub(super) const SUNDIAL_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize)]
pub(super) struct CatalogCache {
    pub(super) schema: u32,
    pub(super) sundial_version: String,
    pub(super) fingerprint: String,
    pub(super) contents: CatalogContents,
}

#[derive(Serialize, Deserialize)]
pub(super) struct CatalogContents {
    pub(super) items: Vec<ItemDef>,
    pub(super) orbit_backdrops: Vec<String>,
    pub(super) orbit_map_entries: Vec<orbit_map::Entry>,
    pub(super) names: HashMap<u64, String>,
    pub(super) type_names: HashMap<u64, String>,
    #[serde(default)]
    pub(super) package_item_names: HashMap<u64, String>,
    #[serde(default)]
    pub(super) package_item_type_names: HashMap<u64, String>,
    #[serde(default)]
    pub(super) descriptions: HashMap<u64, String>,
    #[serde(default)]
    pub(super) icon_containers: HashMap<u64, u32>,
    #[serde(default)]
    pub(super) item_package_metadata: HashMap<u64, ItemPackageMetadata>,
    #[serde(default)]
    pub(super) item_stat_definitions: Vec<ItemStatDefinition>,
    pub(super) sandbox_perk_definitions: Vec<SandboxPerkDefinition>,
    #[serde(default)]
    pub(super) package_names: HashMap<u16, String>,
    #[serde(default)]
    pub(super) inventory_metadata: HashMap<u64, InventoryMetadata>,
    pub(super) objectives: Vec<ObjectiveDef>,
    pub(super) unlock_flag_definitions: Vec<UnlockDefinition>,
    pub(super) unlock_value_definitions: Vec<UnlockDefinition>,
    pub(super) collectibles: Vec<CollectibleDef>,
    pub(super) material_requirement_sets: Vec<MaterialRequirementSetDef>,
    pub(super) item_material_requirement_set_indices:
        HashMap<u64, ItemMaterialRequirementSetIndices>,
    pub(super) progression_definitions: Vec<ProgressionDefinition>,
    #[serde(default)]
    pub(super) progression_package_error: Option<String>,
    pub(super) plug_pools: Vec<Vec<u64>>,
}

pub(crate) fn cache_is_current(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut prefix = [0u8; 128];
    let Ok(read) = file.read(&mut prefix) else {
        return false;
    };
    cache_header_is_current(&String::from_utf8_lossy(&prefix[..read]))
}

fn cache_header_is_current(prefix: &str) -> bool {
    prefix.contains(&format!("\"schema\":{CACHE_SCHEMA}"))
        && prefix.contains(&format!("\"sundial_version\":\"{SUNDIAL_VERSION}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_requires_the_current_sundial_version() {
        assert!(cache_header_is_current(&format!(
            "{{\"schema\":{CACHE_SCHEMA},\"sundial_version\":\"{SUNDIAL_VERSION}\"}}"
        )));
        assert!(!cache_header_is_current(&format!(
            "{{\"schema\":{CACHE_SCHEMA},\"sundial_version\":\"older\"}}"
        )));
        assert!(!cache_header_is_current(&format!(
            "{{\"schema\":{},\"sundial_version\":\"{SUNDIAL_VERSION}\"}}",
            CACHE_SCHEMA - 1
        )));
    }
}
