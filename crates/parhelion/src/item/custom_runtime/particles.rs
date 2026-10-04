//! Imported particle assets: converted particle systems with their programs, materials,
//! shaders and textures. They take a group in the asset packages, where each node gets its
//! tag, its package reference and the tags of the nodes it names.
use super::*;
use parhelion_import::GraphReference;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

struct Node {
    symbol: String,
    template: u32,
    payload: Vec<u8>,
    reference: Option<String>,
    patches: Vec<(usize, String)>,
}

pub(in crate::item) struct ImportedParticles {
    nodes: Vec<Node>,
}

pub(in crate::item) fn load(graph: &GraphReference) -> AuthoringResult<Option<ImportedParticles>> {
    let bytes = fs::read(graph.directory.join("asset-graph.json"))
        .map_err(|error| invalid(format!("Imported particle graph: {error}")))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("Imported particle graph: {error}")))?;
    let Some(nodes) = value["particles"]["nodes"].as_array() else {
        return Ok(None);
    };
    if value["particles"]["installable"] != true {
        return Err(invalid("Imported particles are not marked installable"));
    }
    let mut symbols = BTreeSet::new();
    let mut result = Vec::with_capacity(nodes.len());
    for node in nodes {
        let symbol = node["symbol"]
            .as_str()
            .ok_or_else(|| invalid("Imported particle node lacks a symbol"))?;
        if !symbols.insert(symbol.to_owned()) {
            return Err(invalid(format!(
                "Imported particle node {symbol} is repeated"
            )));
        }
        let file = node["file"]
            .as_str()
            .ok_or_else(|| invalid("Imported particle node lacks a file"))?;
        let path = std::path::Path::new(file);
        if !path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return Err(invalid("Imported particle file leaves its graph folder"));
        }
        let payload = fs::read(graph.directory.join(path))
            .map_err(|error| invalid(format!("Imported particle {symbol}: {error}")))?;
        let template = node["template"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| invalid(format!("Imported particle {symbol} lacks a template")))?;
        let mut patches = Vec::new();
        for patch in node["patches"].as_array().into_iter().flatten() {
            let offset = patch["offset"]
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| invalid("Imported particle patch lacks an offset"))?;
            let target = patch["symbol"]
                .as_str()
                .ok_or_else(|| invalid("Imported particle patch lacks a symbol"))?;
            patches.push((offset, target.to_owned()));
        }
        result.push(Node {
            symbol: symbol.to_owned(),
            template,
            payload,
            reference: node["reference"].as_str().map(str::to_owned),
            patches,
        });
    }
    for node in &result {
        for target in node
            .patches
            .iter()
            .map(|(_, target)| target)
            .chain(node.reference.iter())
        {
            if !symbols.contains(target) {
                return Err(invalid(format!(
                    "Imported particle {} names missing node {target}",
                    node.symbol
                )));
            }
        }
    }
    Ok((!result.is_empty()).then_some(ImportedParticles { nodes: result }))
}

impl ImportedParticles {
    pub(in crate::item) fn asset_bounds(&self) -> Vec<usize> {
        self.nodes.iter().map(|node| node.payload.len()).collect()
    }

    /// Append every node to the reserved group's package, in graph order, and return each
    /// node's tag by symbol.
    pub(in crate::item) fn author(
        &self,
        manager: &PackageManager,
        package: &mut crate::asset_packages::AssetPackage,
    ) -> AuthoringResult<BTreeMap<String, TagHash>> {
        let allocator = AppendedTagAllocator::new(package.id, 0);
        let base = package.tags.len();
        let mut symbols = BTreeMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            symbols.insert(
                node.symbol.clone(),
                allocator.assigned_tag(
                    base + index,
                    "Imported particle asset",
                    "particle asset",
                )?,
            );
        }
        for node in &self.nodes {
            if manager.get_entry(TagHash(node.template)).is_none() {
                return Err(invalid(format!(
                    "Imported particle template 0x{:08X} is unavailable",
                    node.template
                )));
            }
            let mut payload = node.payload.clone();
            for (offset, target) in &node.patches {
                let slot = payload
                    .get_mut(*offset..offset + 4)
                    .ok_or_else(|| invalid("Imported particle patch exceeds its payload"))?;
                if slot != u32::MAX.to_le_bytes() {
                    return Err(invalid("Imported particle patch is not a placeholder"));
                }
                slot.copy_from_slice(&symbols[target].0.to_le_bytes());
            }
            if let Some(target) = &node.reference {
                package.references.push(crate::NewTagReferenceOverride {
                    new_tag_ordinal: package.tags.len(),
                    reference: crate::NewTagReference::Appended(
                        symbols[target].entry_index() as usize
                    ),
                });
            }
            package.tags.push(NewTagSpec {
                template_tag: TagHash(node.template),
                payload,
                storage: crate::NewTagStorageMode::InheritTemplate,
            });
        }
        Ok(symbols)
    }
}

/// The fired graph's particle slots as checked owner patches naming the authored systems.
pub(in crate::item) fn fired_graph_patches(
    fired: &crate::weapon::behavior::FiredGraph,
    symbols: &BTreeMap<String, TagHash>,
) -> AuthoringResult<Vec<sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch>>
{
    fired
        .particles
        .iter()
        .map(|slot| {
            let tag = symbols.get(&slot.system).ok_or_else(|| {
                invalid(format!(
                    "Fired graph particle {} is not among the imported particles",
                    slot.system
                ))
            })?;
            Ok(
                sundial::package_authoring::sandbox_perk::program::NativeAssetResourcePatch {
                    binding_hash: slot.binding_hash,
                    resource_index: slot.resource_index,
                    offset: slot.offset,
                    expected: slot.expected.to_le_bytes().to_vec(),
                    bytes: tag.0.to_le_bytes().to_vec(),
                    imported_particle: None,
                },
            )
        })
        .collect()
}
