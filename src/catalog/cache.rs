//! Serialized catalog cache envelope and compatibility checks.

use std::{collections::HashMap, fs, io::Read, path::Path};

use serde::{Deserialize, Serialize};

use super::{
    CollectibleDef, CollectionConditionTokenDef, InventoryMetadata, ItemDef,
    ItemMaterialRequirementSetIndices, ItemPackageMetadata, ItemStatDefinition, ItemStatGroup,
    MaterialRequirementSetDef, ObjectiveDef, ObjectiveOwnerTraitDef, PresentationNode,
    ProgressionDefinition, RecordDefinition, UnlockDefinition,
};

pub(super) const CACHE_SCHEMA: u32 = 135;
pub(super) const SUNDIAL_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize)]
pub(super) struct CatalogCache {
    pub(super) schema: u32,
    pub(super) sundial_version: String,
    pub(super) fingerprint: String,
    pub(super) contents: CatalogContents,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(Default))]
pub(super) struct CatalogContents {
    pub(super) items: Vec<ItemDef>,
    pub(super) names: HashMap<u64, String>,
    pub(super) type_names: HashMap<u64, String>,
    #[serde(default)]
    pub(super) package_item_names: HashMap<u64, String>,
    #[serde(default)]
    pub(super) package_item_type_names: HashMap<u64, String>,
    #[serde(default)]
    pub(super) descriptions: HashMap<u64, String>,
    pub(super) perk_descriptions: HashMap<u16, String>,
    #[serde(default)]
    pub(super) icon_containers: HashMap<u64, u32>,
    /// The icon containers of the Primary, Special and Heavy ammunition marks.
    #[serde(default)]
    pub(super) ammo_icon_containers: [Option<u32>; 3],
    #[serde(default)]
    pub(super) item_package_metadata: HashMap<u64, ItemPackageMetadata>,
    #[serde(default)]
    pub(super) item_stat_definitions: Vec<ItemStatDefinition>,
    /// The six character stat rows in character screen order; `None` only in test catalogs.
    #[serde(default)]
    pub(super) character_stat_rows: Option<[u16; 6]>,
    pub(super) power_cap_definitions: Vec<super::PowerCapDefinition>,
    pub(super) item_stat_groups: Vec<ItemStatGroup>,
    #[serde(default)]
    pub(super) trait_definitions: Vec<ObjectiveOwnerTraitDef>,
    #[serde(default)]
    pub(super) reusable_plug_set_count: usize,
    #[serde(default)]
    pub(super) socket_entry_list_count: usize,
    /// Every row of the ability tables, with its entity and bank.
    #[serde(default)]
    pub(super) ability_rows: Vec<crate::investment::AbilityRowSummary>,
    #[serde(default)]
    pub(super) package_names: HashMap<u16, String>,
    #[serde(default)]
    pub(super) inventory_metadata: HashMap<u64, InventoryMetadata>,
    pub(super) objectives: Vec<ObjectiveDef>,
    pub(super) presentation_nodes: Vec<PresentationNode>,
    #[serde(default)]
    pub(super) records: Option<Vec<RecordDefinition>>,
    #[serde(with = "unlock_definitions")]
    pub(super) unlock_flag_definitions: Vec<UnlockDefinition>,
    #[serde(with = "unlock_definitions")]
    pub(super) unlock_value_definitions: Vec<UnlockDefinition>,
    pub(super) collectibles: Vec<CollectibleDef>,
    pub(super) shared_expression_pool: Vec<Vec<CollectionConditionTokenDef>>,
    pub(super) material_requirement_sets: Vec<MaterialRequirementSetDef>,
    pub(super) item_material_requirement_set_indices:
        HashMap<u64, ItemMaterialRequirementSetIndices>,
    pub(super) progression_definitions: Vec<ProgressionDefinition>,
    pub(super) seasonal: Option<crate::investment::seasonal::Definition>,
    #[serde(default)]
    pub(super) progression_package_error: Option<String>,
    pub(super) plug_pools: Vec<Vec<u64>>,
}

/// Unlock definitions as the cache stores them: every context once, and each definition's list
/// as positions in that table. Written out in full, a condition that names hundreds of
/// definitions was repeated in each of them, which made up nearly nine tenths of the file.
mod unlock_definitions {
    use std::{collections::HashMap, sync::Arc};

    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    use crate::catalog::{ProgressionContextDef, UnlockDefinition};

    #[derive(Serialize)]
    struct Written<'a> {
        contexts: Vec<&'a ProgressionContextDef>,
        tested_by: Vec<Vec<u32>>,
        definitions: Vec<UnlockDefinition>,
    }

    #[derive(Deserialize)]
    struct Read {
        contexts: Vec<ProgressionContextDef>,
        tested_by: Vec<Vec<u32>>,
        definitions: Vec<UnlockDefinition>,
    }

    pub(super) fn serialize<S: Serializer>(
        definitions: &[UnlockDefinition],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut positions = HashMap::<*const ProgressionContextDef, u32>::new();
        let mut contexts = Vec::new();
        let mut tested_by = Vec::with_capacity(definitions.len());
        for definition in definitions {
            let mut list = Vec::with_capacity(definition.tested_by.len());
            for context in &definition.tested_by {
                let position = match positions.get(&Arc::as_ptr(context)) {
                    Some(position) => *position,
                    None => {
                        let position = u32::try_from(contexts.len()).map_err(|_| {
                            <S::Error as serde::ser::Error>::custom("too many unlock contexts")
                        })?;
                        positions.insert(Arc::as_ptr(context), position);
                        contexts.push(&**context);
                        position
                    }
                };
                list.push(position);
            }
            tested_by.push(list);
        }
        // The lists travel beside the definitions, so each definition is written without its own.
        let definitions = definitions
            .iter()
            .map(|definition| UnlockDefinition {
                tested_by: Vec::new(),
                ..definition.clone()
            })
            .collect();
        Written {
            contexts,
            tested_by,
            definitions,
        }
        .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<UnlockDefinition>, D::Error> {
        let Read {
            contexts,
            tested_by,
            mut definitions,
        } = Read::deserialize(deserializer)?;
        if tested_by.len() != definitions.len() {
            return Err(D::Error::custom(
                "unlock context lists do not match their definitions",
            ));
        }
        let contexts = contexts.into_iter().map(Arc::new).collect::<Vec<_>>();
        for (definition, positions) in definitions.iter_mut().zip(tested_by) {
            definition.tested_by = positions
                .into_iter()
                .map(|position| {
                    usize::try_from(position)
                        .ok()
                        .and_then(|position| contexts.get(position))
                        .cloned()
                        .ok_or_else(|| D::Error::custom("unlock context position out of range"))
                })
                .collect::<Result<_, _>>()?;
        }
        Ok(definitions)
    }
}

/// Whether the cache at `path` was written by this build's schema and version. The file is
/// compressed, so only its start is inflated to read the header. A plain file from an older
/// build is read as it is.
pub(crate) fn cache_is_current(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 2];
    let compressed = file.read_exact(&mut magic).is_ok() && magic == [0x1F, 0x8B];
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut prefix = Vec::with_capacity(128);
    let read = if compressed {
        flate2::read::GzDecoder::new(file)
            .take(128)
            .read_to_end(&mut prefix)
    } else {
        file.take(128).read_to_end(&mut prefix)
    };
    read.is_ok() && cache_header_is_current(&String::from_utf8_lossy(&prefix))
}

fn cache_header_is_current(prefix: &str) -> bool {
    prefix.contains(&format!("\"schema\":{CACHE_SCHEMA}"))
        && prefix.contains(&format!("\"sundial_version\":\"{SUNDIAL_VERSION}\""))
}
