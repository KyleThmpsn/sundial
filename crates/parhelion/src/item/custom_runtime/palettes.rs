//! Recolored effects of an authored ability. Each edited palette gets a private copy with its
//! own or another stock palette's pixels, recolored, each material that binds it a copy binding the new palette, and each particle
//! system drawing with such a material a copy naming the new material. The copies take an asset
//! group of their own and are placed for the runtime dependency index. The ability's graph copies
//! then name the private systems through resource patches, so the stock ability and every other
//! effect drawing with the palette keep their colors.
use super::*;
use crate::subclass::PaletteEdit;
use sundial::package_authoring::ability_palette::{
    self, PaletteUse, SYSTEM_MATERIAL, ability_palettes, binding_tag_offset,
};

/// One private copy: its stock template, payload, the copy its package entry names, and the
/// places in its payload that name another copy.
struct Node {
    template: u32,
    payload: Vec<u8>,
    reference: Option<usize>,
    patches: Vec<(usize, usize)>,
}

/// Authors the private copies `edits` need below `source`, the ability's entity, and returns the
/// resource patches that make each graph of its tree name them, by graph.
pub(in crate::item) fn author(
    manager: &PackageManager,
    source: TagHash,
    edits: &[PaletteEdit],
    (packages, placed): (&mut crate::asset_packages::AssetPackages, &mut Vec<TagHash>),
) -> AuthoringResult<BTreeMap<u32, Vec<WeaponRuntimeResourcePatch>>> {
    if edits.is_empty() {
        return Ok(BTreeMap::new());
    }
    let found = ability_palettes(manager, source.0, SPAWN_DEPTH).map_err(invalid)?;
    let mut nodes = Vec::<Node>::new();
    let mut headers = BTreeMap::<u32, usize>::new();
    let mut uses = Vec::<PaletteUse>::new();
    for edit in edits {
        let palette = found
            .iter()
            .find(|palette| palette.header == edit.palette)
            .ok_or_else(|| {
                invalid(format!(
                    "No effect of {source} draws with palette 0x{:08X}",
                    edit.palette
                ))
            })?;
        // A taken palette must be one too, so its pixels fit the copy's header.
        let mut pixels =
            ability_palette::palette_pixels(manager, edit.source()).map_err(invalid)?;
        edit.apply(&mut pixels);
        let data = nodes.len();
        nodes.push(Node {
            template: palette.data,
            payload: pixels,
            reference: Some(data + 1),
            patches: Vec::new(),
        });
        nodes.push(Node {
            template: palette.header,
            payload: read_tag(manager, TagHash(palette.header), "palette texture header")?,
            reference: Some(data),
            patches: Vec::new(),
        });
        headers.insert(palette.header, data + 1);
        uses.extend(palette.uses.iter().copied());
    }
    // One copy of each material and system, however many uses reach it.
    let mut materials = BTreeMap::<u32, usize>::new();
    let mut systems = BTreeMap::<u32, usize>::new();
    let header_of = |material: &[u8], binding: u32| -> AuthoringResult<(usize, u32)> {
        let at = binding_tag_offset(material, binding).map_err(invalid)?;
        Ok((at, read_u32(material, at)?))
    };
    for palette_use in &uses {
        let material = match materials.get(&palette_use.material) {
            Some(index) => *index,
            None => {
                let payload = read_tag(manager, TagHash(palette_use.material), "effect material")?;
                nodes.push(Node {
                    template: palette_use.material,
                    payload,
                    reference: None,
                    patches: Vec::new(),
                });
                materials.insert(palette_use.material, nodes.len() - 1);
                nodes.len() - 1
            }
        };
        let (at, header) = header_of(&nodes[material].payload, palette_use.binding)?;
        if !nodes[material]
            .patches
            .iter()
            .any(|(offset, _)| *offset == at)
        {
            nodes[material].patches.push((at, headers[&header]));
        }
        if let std::collections::btree_map::Entry::Vacant(entry) =
            systems.entry(palette_use.site.system)
        {
            let payload = read_tag(
                manager,
                TagHash(palette_use.site.system),
                "effect particle system",
            )?;
            if read_u32(&payload, SYSTEM_MATERIAL)? != palette_use.material {
                return Err(invalid(format!(
                    "Particle system 0x{:08X} no longer draws with material 0x{:08X}",
                    palette_use.site.system, palette_use.material
                )));
            }
            nodes.push(Node {
                template: palette_use.site.system,
                payload,
                reference: None,
                patches: vec![(SYSTEM_MATERIAL, material)],
            });
            entry.insert(nodes.len() - 1);
        }
    }
    let index = packages.reserve_group(nodes.iter().map(|node| node.payload.len()))?;
    let package = &mut packages.packages[index];
    let allocator = AppendedTagAllocator::new(package.id, 0);
    let base = package.tags.len();
    let tags = (0..nodes.len())
        .map(|ordinal| allocator.assigned_tag(base + ordinal, "Effect color copy", "effect asset"))
        .collect::<AuthoringResult<Vec<_>>>()?;
    for (ordinal, mut node) in nodes.into_iter().enumerate() {
        for (offset, target) in &node.patches {
            write_u32(&mut node.payload, *offset, tags[*target].0)?;
        }
        if let Some(target) = node.reference {
            package.references.push(crate::NewTagReferenceOverride {
                new_tag_ordinal: base + ordinal,
                reference: crate::NewTagReference::Appended(tags[target].entry_index() as usize),
            });
        }
        package.tags.push(NewTagSpec {
            template_tag: TagHash(node.template),
            payload: node.payload,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        placed.push(tags[ordinal]);
    }
    let mut patches = BTreeMap::<u32, Vec<WeaponRuntimeResourcePatch>>::new();
    for palette_use in uses {
        let site = palette_use.site;
        let graph = patches.entry(palette_use.graph).or_default();
        let bytes = tags[systems[&site.system]].0.to_le_bytes().to_vec();
        if graph.iter().any(|patch| {
            (patch.binding_hash, patch.resource_index, patch.offset)
                == (site.binding_hash, site.resource_index, site.offset)
        }) {
            continue;
        }
        graph.push(WeaponRuntimeResourcePatch {
            binding_hash: site.binding_hash,
            resource_index: site.resource_index,
            offset: site.offset,
            bytes,
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        });
    }
    Ok(patches)
}
