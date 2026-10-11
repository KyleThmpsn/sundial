//! Imported particle assets: converted particle systems with their programs, materials,
//! shaders and textures. They take a group in the asset packages, where each node gets its
//! tag, its package reference and the tags of the nodes it names.
use super::imports::Inputs;
use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod projectile;
mod replication;

struct Node {
    symbol: String,
    template: u32,
    payload: Vec<u8>,
    reference: Option<String>,
    storage: crate::NewTagStorageMode,
    patches: Vec<(usize, String)>,
}

pub(in crate::item) struct ImportedParticles {
    nodes: Vec<Node>,
    projectile: Option<String>,
    attachments: BTreeSet<String>,
    presentation: Option<Presentation>,
}

struct Presentation {
    entity: u32,
    components: Vec<PresentationComponent>,
}

struct PresentationComponent {
    source_owner: u32,
    owner: String,
    class: u32,
}

fn presentation_number(part: &serde_json::Value, field: &str) -> AuthoringResult<u32> {
    part[field]
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .filter(|number| ![0, u32::MAX].contains(number))
        .ok_or_else(|| invalid(format!("Imported runtime presentation lacks {field}")))
}

fn presentation_component(
    part: &serde_json::Value,
    nodes: &[Node],
    required_class: Option<u32>,
) -> AuthoringResult<PresentationComponent> {
    let source_owner = presentation_number(part, "source_owner")?;
    let class = match required_class {
        Some(class) => class,
        None => presentation_number(part, "class")?,
    };
    if class & 0xffff0000 != 0x80800000 {
        return Err(invalid("Imported runtime component lacks its native class"));
    }
    let owner = part["owner"]
        .as_str()
        .filter(|symbol| !symbol.is_empty())
        .ok_or_else(|| invalid("Imported runtime component lacks its owner"))?
        .to_owned();
    let node = nodes
        .iter()
        .find(|node| node.symbol == owner)
        .ok_or_else(|| invalid("Imported runtime component owner is absent"))?;
    if node.template != source_owner || node.reference.is_some() {
        return Err(invalid(
            "Imported runtime component does not match its source template",
        ));
    }
    Ok(PresentationComponent {
        source_owner,
        owner,
        class,
    })
}

fn presentation(part: &serde_json::Value, nodes: &[Node]) -> AuthoringResult<Presentation> {
    let entity = presentation_number(part, "source_entity")?;
    let mut components = vec![presentation_component(part, nodes, Some(0x808072bd))?];
    if let Some(entries) = part.get("components") {
        for entry in entries
            .as_array()
            .ok_or_else(|| invalid("Imported runtime components are not an array"))?
        {
            let component = presentation_component(entry, nodes, None)?;
            if components.iter().any(|existing| {
                existing.source_owner == component.source_owner || existing.owner == component.owner
            }) {
                return Err(invalid("Imported runtime component is duplicated"));
            }
            components.push(component);
        }
    }
    Ok(Presentation { entity, components })
}

pub(in crate::item) fn load(graph: &Inputs) -> AuthoringResult<Option<ImportedParticles>> {
    let value = graph.value();
    let mut nodes = Vec::new();
    for section in [
        "particles",
        "projectile",
        "attachments",
        "runtime_presentation",
    ] {
        if let Some(part) = value.get(section) {
            if part["installable"] != true {
                return Err(invalid(format!(
                    "Imported {section} is not marked installable"
                )));
            }
            nodes.extend(
                part["nodes"]
                    .as_array()
                    .ok_or_else(|| invalid(format!("Imported {section} lacks its asset nodes")))?,
            );
        }
    }
    let projectile = value
        .get("projectile")
        .map(|section| {
            section["root"]
                .as_str()
                .filter(|symbol| !symbol.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| invalid("Imported projectile lacks its root symbol"))
        })
        .transpose()?;
    let mut attachments = BTreeSet::new();
    if let Some(section) = value.get("attachments") {
        let roots = section["roots"]
            .as_array()
            .filter(|roots| !roots.is_empty())
            .ok_or_else(|| invalid("Imported attachments lack their root symbols"))?;
        for root in roots {
            let root = root
                .as_str()
                .filter(|root| !root.trim().is_empty())
                .ok_or_else(|| invalid("Imported attachment root is empty"))?;
            if !attachments.insert(root.to_owned()) {
                return Err(invalid("Imported attachment root is repeated"));
            }
        }
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
        let payload = graph
            .read(path)
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
            storage: match node["storage"].as_str() {
                None => crate::NewTagStorageMode::InheritTemplate,
                Some("audio_bank") => crate::NewTagStorageMode::AudioBank,
                Some("audio_media") => crate::NewTagStorageMode::AudioMedia,
                Some(_) => return Err(invalid("Imported asset has an unsupported storage mode")),
            },
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
    let presentation = value
        .get("runtime_presentation")
        .map(|part| presentation(part, &result))
        .transpose()?;
    let assets = ImportedParticles {
        nodes: result,
        projectile,
        attachments,
        presentation,
    };
    if let Some(root) = &assets.projectile {
        assets.projectile(root)?;
    }
    for root in &assets.attachments {
        assets.attachment(root)?;
    }
    Ok((!assets.nodes.is_empty()).then_some(assets))
}

impl ImportedParticles {
    /// Include older graphs' mutable arrays in the native loader's instance copy.
    fn repair_instances(&mut self, manager: &PackageManager) -> AuthoringResult<()> {
        for node in &mut self.nodes {
            if manager
                .get_entry(TagHash(node.template))
                .is_none_or(|entry| entry.reference != 0x80809c36)
            {
                continue;
            }
            let patches = node
                .patches
                .iter()
                .map(|(offset, symbol)| serde_json::json!({"offset":offset,"symbol":symbol}))
                .collect::<Vec<_>>();
            if let Some(fixed) = parhelion_import::tiger::instance::repair(&node.payload, &patches)
                .map_err(|error| {
                    invalid(format!("Imported component {}: {error:#}", node.symbol))
                })?
            {
                node.payload = fixed.payload;
                node.patches = fixed
                    .patches
                    .into_iter()
                    .map(|patch| {
                        Ok((
                            usize::try_from(
                                patch["offset"]
                                    .as_u64()
                                    .ok_or_else(|| invalid("Repaired component offset missing"))?,
                            )
                            .map_err(|_| invalid("Repaired component offset overflow"))?,
                            patch["symbol"]
                                .as_str()
                                .ok_or_else(|| invalid("Repaired component symbol missing"))?
                                .to_owned(),
                        ))
                    })
                    .collect::<AuthoringResult<_>>()?;
            }
        }
        Ok(())
    }
    /// Apply prepared components only to the exact native vehicle they were retargeted for.
    pub(in crate::item) fn apply_presentation(
        &self,
        manager: &PackageManager,
        source_entity: TagHash,
        entity: &mut [u8],
        symbols: &BTreeMap<String, TagHash>,
    ) -> AuthoringResult<()> {
        let Some(presentation) = &self.presentation else {
            return Ok(());
        };
        if source_entity.0 != presentation.entity {
            return Err(invalid(
                "Imported appearance was prepared for a different summoned vehicle. Reimport it for this vehicle.",
            ));
        }
        for component in &presentation.components {
            self.apply_component(manager, entity, symbols, component)?;
        }
        Ok(())
    }

    fn apply_component(
        &self,
        manager: &PackageManager,
        entity: &mut [u8],
        symbols: &BTreeMap<String, TagHash>,
        component: &PresentationComponent,
    ) -> AuthoringResult<()> {
        let original = manager
            .read_tag(TagHash(component.source_owner))
            .map_err(|e| invalid(e.to_string()))?;
        let original = parhelion_import::d2_mot::payload::Payload(original);
        let resource = original.pointer(24).map_err(|e| invalid(e.to_string()))?;
        if manager
            .get_entry(TagHash(component.source_owner))
            .is_none_or(|e| e.reference != 0x80809c36)
            || original
                .u32(
                    resource
                        .checked_sub(4)
                        .ok_or_else(|| invalid("Invalid runtime component resource"))?,
                )
                .map_err(|e| invalid(e.to_string()))?
                != component.class
        {
            return Err(invalid(
                "Imported runtime owner does not match its native component class",
            ));
        }
        let node = self
            .nodes
            .iter()
            .find(|node| node.symbol == component.owner)
            .ok_or_else(|| invalid("Imported runtime component payload is absent"))?;
        let replacement = parhelion_import::d2_mot::payload::Payload(node.payload.clone());
        let resource = replacement
            .pointer(24)
            .map_err(|e| invalid(e.to_string()))?;
        if replacement
            .u32(
                resource
                    .checked_sub(4)
                    .ok_or_else(|| invalid("Invalid imported runtime resource"))?,
            )
            .map_err(|e| invalid(e.to_string()))?
            != component.class
        {
            return Err(invalid(
                "Imported runtime payload changes its component class",
            ));
        }
        let before = parhelion_import::d2_mot::payload::Payload(entity.to_vec());
        parhelion_import::d2_mot::entity::owner_slots(&before, &original, component.source_owner)
            .map_err(|e| invalid(e.to_string()))?;
        let target = symbols
            .get(&component.owner)
            .ok_or_else(|| invalid("Imported runtime owner was not allocated"))?;
        sundial::package_authoring::entity::retarget_weapon_component_owner(
            entity,
            component.source_owner,
            target.0,
        )
        .map_err(invalid)?;
        parhelion_import::d2_mot::entity::reject_stale_owner(
            &parhelion_import::d2_mot::payload::Payload(entity.to_vec()),
            component.source_owner,
        )
        .map_err(|e| invalid(e.to_string()))?;
        Ok(())
    }

    /// A perk may bind only a declared attachment root from this group.
    pub(in crate::item) fn attachment(&self, symbol: &str) -> AuthoringResult<()> {
        let node = self
            .nodes
            .iter()
            .find(|node| node.symbol == symbol)
            .filter(|_| self.attachments.contains(symbol))
            .ok_or_else(|| {
                invalid(format!(
                    "Imported attachment {symbol} is not a declared root"
                ))
            })?;
        if node.reference.is_some() {
            return Err(invalid(
                "An imported attachment root cannot replace its entity class",
            ));
        }
        let (count, _, _, class) = array_at(&node.payload, 16)?;
        if count == 0
            || class != 0x80809C04
            || read_u64(&node.payload, 0)? != node.payload.len() as u64
        {
            return Err(invalid(
                "Imported attachment root is not a complete native entity",
            ));
        }
        Ok(())
    }

    pub(in crate::item) fn merge(&mut self, other: Self) -> AuthoringResult<()> {
        if self.presentation.is_some() && other.presentation.is_some() {
            return Err(invalid(
                "More than one imported group supplies the vehicle appearance",
            ));
        }
        if self.projectile.is_some() && other.projectile.is_some() {
            return Err(invalid(
                "More than one imported group supplies the fired projectile",
            ));
        }
        let existing = self
            .nodes
            .iter()
            .map(|node| node.symbol.as_str())
            .collect::<BTreeSet<_>>();
        if other
            .nodes
            .iter()
            .any(|node| existing.contains(node.symbol.as_str()))
        {
            return Err(invalid("Imported asset groups contain duplicate symbols"));
        }
        self.nodes.extend(other.nodes);
        self.attachments.extend(other.attachments);
        self.projectile = self.projectile.take().or(other.projectile);
        self.presentation = self.presentation.take().or(other.presentation);
        Ok(())
    }

    /// Validate the declared root independently of its eventual package address.
    pub(in crate::item) fn projectile(&self, symbol: &str) -> AuthoringResult<&[u8]> {
        if self.projectile.as_deref() != Some(symbol) {
            return Err(invalid(format!(
                "Imported projectile {symbol} is not the graph's declared projectile root"
            )));
        }
        let node = self
            .nodes
            .iter()
            .find(|node| node.symbol == symbol)
            .ok_or_else(|| invalid("Imported projectile root is absent from its asset group"))?;
        if node.reference.is_some()
            || sundial::package_authoring::sandbox_perk::entity::kind(&node.payload)
                .map_err(invalid)?
                != Some(sundial::package_authoring::sandbox_perk::entity::Kind::Projectile)
        {
            return Err(invalid(
                "Imported projectile root is not a native projectile entity",
            ));
        }
        replication::validate(&self.nodes, node)?;
        Ok(&node.payload)
    }

    /// Reserve and append the group, including its replication loading companion.
    /// Return the node symbols and the resources needed by the global runtime root.
    pub(in crate::item) fn author(
        &self,
        manager: &PackageManager,
        packages: &mut crate::asset_packages::AssetPackages,
    ) -> AuthoringResult<(BTreeMap<String, TagHash>, Vec<TagHash>)> {
        let loading = self
            .projectile
            .as_ref()
            .map(|symbol| {
                let root = self
                    .nodes
                    .iter()
                    .find(|node| node.symbol == *symbol)
                    .ok_or_else(|| invalid("Imported projectile root is missing"))?;
                replication::Loading::read(&self.nodes, root, manager)
            })
            .transpose()?;
        let mut bounds = self
            .nodes
            .iter()
            .map(|node| node.payload.len())
            .collect::<Vec<_>>();
        bounds.extend(loading.iter().map(|loading| loading.bound));
        let index = packages.reserve_group(bounds)?;
        let package = &mut packages.packages[index];
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
            if node.storage != crate::NewTagStorageMode::InheritTemplate
                && (node.reference.is_some()
                    || manager
                        .get_entry(TagHash(node.template))
                        .is_none_or(|entry| entry.file_type != 26))
            {
                return Err(invalid(
                    "Imported audio storage needs a raw audio template and its own identity",
                ));
            }
            let entity = self.projectile.as_deref() == Some(node.symbol.as_str())
                || self.attachments.contains(&node.symbol);
            if entity
                && manager
                    .get_entry(TagHash(node.template))
                    .is_none_or(|entry| entry.reference != 0x80809C0F)
            {
                return Err(invalid("Imported entity template is not a native entity"));
            }
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
            if entity {
                parhelion_import::d2_mot::entity::links::Graph::read(
                    &parhelion_import::d2_mot::payload::Payload(payload.clone()),
                    false,
                )
                .map_err(|error| invalid(format!("Imported entity {}: {error}", node.symbol)))?;
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
                storage: node.storage,
            });
        }
        if let Some(loading) = loading {
            loading.append(&self.nodes, &symbols, package)?;
        }
        // Native replication allocations and their root are eager. The companion
        // is reached through package enrollment rather than the global index.
        let eager = self
            .nodes
            .iter()
            .map(|node| symbols[&node.symbol])
            .collect();
        Ok((symbols, eager))
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
