//! The color palettes an ability's effects draw with. An effect owner's particle nodes each name
//! a particle system, whose render material binds textures by slot. The checked ability shaders
//! take the effect's visible color from a 256 by 16 sRGB RGBA8 palette among those textures, so a
//! private copy of the palette recolors an effect while its particle program, emitter and
//! gameplay graphs stay stock. Evidence and its limits are in knowledge chapters 11 and 16.
use std::collections::BTreeMap;

use crate::entity::{weapon_component_binding_hashes, weapon_component_bindings};
use crate::package_payload::{i64_at, native_array_at, relative_offset, u16_at, u32_at, u64_at};
use crate::package_runtime::reader::PackageManager;

/// A component owner holding effect sequence nodes, its header and data roots, and its node
/// rows: the owner's tag, a row class, then a pointer to the node at `+0x10`.
pub const EFFECT_OWNER_CLASS: u32 = 0x8080_9C36;
const EFFECT_HEADER_CLASS: u32 = 0x8080_84D7;
const EFFECT_DATA_CLASS: u32 = 0x8080_84E9;
const NODES: usize = 0x168;
const NODE_ROW_CLASS: u32 = 0x8080_93E6;
const NODE_ROW_KIND: u32 = 0x8080_93E5;
const NODE_ROW_SIZE: usize = 24;
/// A particle node names its system at one of two offsets, by node variant.
const PARTICLE_NODE_CLASS: u32 = 0x8080_6CC5;
const PARTICLE_NODE_TARGETS: [usize; 2] = [0x158, 0x150];
pub const PARTICLE_SYSTEM_CLASS: u32 = 0x8080_6E28;
/// Where a particle system names its render material.
pub const SYSTEM_MATERIAL: usize = 0x14;
pub const MATERIAL_CLASS: u32 = 0x8080_71E8;
/// A material's texture bindings: a slot, then a texture header tag.
pub const MATERIAL_BINDINGS: usize = 0x2D0;
const BINDING_CLASS: u32 = 0x8080_7211;
const BINDING_SIZE: usize = 8;
/// A palette's texture: its header names its pixel data by package reference and the data names
/// the header back.
const TEXTURE_HEADER_SIZE: usize = 40;
const TEXTURE_MARKER: u16 = 0xCAFE;
pub const PALETTE_WIDTH: u16 = 256;
pub const PALETTE_HEIGHT: u16 = 16;
/// DXGI `R8G8B8A8_UNORM_SRGB`.
pub const PALETTE_FORMAT: u32 = 29;

/// Most nodes one effect owner may list, and bindings one material.
const NODE_LIMIT: usize = 512;
const BINDING_LIMIT: usize = 256;

/// One place an entity's effect owner names a particle system.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ParticleSite {
    pub owner: u32,
    pub binding_hash: u32,
    pub resource_index: u16,
    /// Byte offset of the system tag from the start of that resource, as a resource patch names
    /// a place.
    pub offset: u32,
    pub system: u32,
}

/// One place a palette reaches an ability: the graph and particle site, the system's render
/// material, and the material's binding row that names the palette.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PaletteUse {
    pub graph: u32,
    pub site: ParticleSite,
    pub material: u32,
    pub binding: u32,
}

/// A palette texture an entity's effects draw with, and every place they reach it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Palette {
    pub header: u32,
    pub data: u32,
    pub uses: Vec<PaletteUse>,
}

fn pointer(payload: &[u8], at: usize) -> Result<usize, String> {
    relative_offset(at, 0, i64_at(payload, at)?)
}

fn live(manager: &PackageManager, tag: u32, file_type: u8, class: u32) -> bool {
    manager
        .get_entry(tag)
        .is_some_and(|entry| entry.file_type == file_type && entry.reference == class)
}

/// The particle systems `entity`'s effect owners name, each where it is named.
pub fn particle_sites(
    manager: &PackageManager,
    entity: &[u8],
) -> Result<Vec<ParticleSite>, String> {
    let mut resources = BTreeMap::<u32, Vec<(u64, u32, u16)>>::new();
    for binding in weapon_component_binding_hashes(entity)? {
        for resource in weapon_component_bindings(entity, binding)? {
            let index = u16::try_from(resource.resource_index)
                .map_err(|_| "A component binding selects too many resources".to_owned())?;
            resources.entry(resource.owner_tag).or_default().push((
                resource.resource_offset,
                binding,
                index,
            ));
        }
    }
    let mut sites = Vec::new();
    for (owner, mut starts) in resources {
        if !live(manager, owner, 8, EFFECT_OWNER_CLASS) {
            continue;
        }
        starts.sort_unstable();
        starts.dedup_by_key(|(start, ..)| *start);
        let payload = manager.read_tag(owner)?;
        let context = |error: String| format!("Effect owner 0x{owner:08X}: {error}");
        if usize::try_from(u64_at(&payload, 0)?).ok() != Some(payload.len()) {
            return Err(context("its size field disagrees with its payload".into()));
        }
        let header = pointer(&payload, 0x10).map_err(context)?;
        let data = pointer(&payload, 0x18).map_err(context)?;
        // Owners of this class with other roots hold other component data, and no effect nodes.
        if header < 4
            || data < 4
            || u32_at(&payload, header - 4)? != EFFECT_HEADER_CLASS
            || u32_at(&payload, data - 4)? != EFFECT_DATA_CLASS
        {
            continue;
        }
        let (count, _, rows, class) = native_array_at(&payload, data + NODES).map_err(context)?;
        if count == 0 {
            continue;
        }
        if class != NODE_ROW_CLASS || count > NODE_LIMIT {
            return Err(context(format!("its node rows are class 0x{class:08X}")));
        }
        for index in 0..count {
            let row = rows + index * NODE_ROW_SIZE;
            if u32_at(&payload, row)? != owner || u32_at(&payload, row + 4)? != NODE_ROW_KIND {
                return Err(context(format!("node row {index} names another owner")));
            }
            let node = pointer(&payload, row + 16).map_err(context)?;
            if u32_at(&payload, node + 4)? != PARTICLE_NODE_CLASS {
                continue;
            }
            let Some(at) = PARTICLE_NODE_TARGETS
                .iter()
                .map(|slot| node + slot)
                .find(|at| {
                    u32_at(&payload, *at)
                        .is_ok_and(|tag| live(manager, tag, 8, PARTICLE_SYSTEM_CLASS))
                })
            else {
                continue;
            };
            let at = at as u64;
            let Some(&(start, binding_hash, resource_index)) =
                starts.iter().rev().find(|(start, ..)| *start <= at)
            else {
                continue;
            };
            sites.push(ParticleSite {
                owner,
                binding_hash,
                resource_index,
                offset: u32::try_from(at - start).map_err(|_| {
                    context("a particle node sits too far into its resource".into())
                })?,
                system: u32_at(&payload, at as usize)?,
            });
        }
    }
    Ok(sites)
}

/// A material's texture bindings: each row's slot and texture header tag.
pub fn bindings(material: &[u8]) -> Result<Vec<(u32, u32)>, String> {
    let (count, _, rows, class) = native_array_at(material, MATERIAL_BINDINGS)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    if class != BINDING_CLASS || count > BINDING_LIMIT {
        return Err(format!("Material bindings are class 0x{class:08X}"));
    }
    (0..count)
        .map(|index| {
            let row = rows + index * BINDING_SIZE;
            Ok((u32_at(material, row)?, u32_at(material, row + 4)?))
        })
        .collect()
}

/// The pixel data tag of `header` when it is a palette texture: 256 by 16 sRGB RGBA8, one
/// tightly packed surface, its header and data naming each other.
pub fn palette_data(manager: &PackageManager, header: u32) -> Result<Option<u32>, String> {
    let Some(entry) = manager.get_entry(header) else {
        return Ok(None);
    };
    if entry.file_type != 0x20 || entry.file_subtype != 0x01 {
        return Ok(None);
    }
    let bytes = manager.read_tag(header)?;
    if bytes.len() != TEXTURE_HEADER_SIZE
        || u32_at(&bytes, 4)? != PALETTE_FORMAT
        || u16_at(&bytes, 12)? != TEXTURE_MARKER
        || u16_at(&bytes, 14)? != PALETTE_WIDTH
        || u16_at(&bytes, 16)? != PALETTE_HEIGHT
    {
        return Ok(None);
    }
    let size = usize::from(PALETTE_WIDTH) * usize::from(PALETTE_HEIGHT) * 4;
    let data = entry.reference;
    let data_entry = manager
        .get_entry(data)
        .ok_or_else(|| format!("Palette 0x{header:08X} names missing data 0x{data:08X}"))?;
    if usize::try_from(u32_at(&bytes, 0)?).ok() != Some(size)
        || data_entry.file_size as usize != size
        || data_entry.file_type != 0x28
        || data_entry.file_subtype != 0x01
        || data_entry.reference != header
    {
        return Err(format!(
            "Palette 0x{header:08X} data 0x{data:08X} is not one {PALETTE_WIDTH} by \
             {PALETTE_HEIGHT} surface naming its header"
        ));
    }
    Ok(Some(data))
}

/// The palette pixels of `header`, as sRGB RGBA8 rows.
pub fn palette_pixels(manager: &PackageManager, header: u32) -> Result<Vec<u8>, String> {
    let data = palette_data(manager, header)?
        .ok_or_else(|| format!("0x{header:08X} is not a palette texture"))?;
    manager.read_tag(data)
}

/// The offset of a material's binding row `binding`'s texture tag.
pub fn binding_tag_offset(material: &[u8], binding: u32) -> Result<usize, String> {
    let (count, _, rows, class) = native_array_at(material, MATERIAL_BINDINGS)?;
    let binding = usize::try_from(binding).map_err(|_| "Binding row out of range")?;
    if class != BINDING_CLASS || binding >= count {
        return Err(format!("Material has no binding row {binding}"));
    }
    Ok(rows + binding * BINDING_SIZE + 4)
}

/// The palettes the effects of `entity`, graph `graph`, draw with, in the order their first use
/// is found.
pub fn palettes(
    manager: &PackageManager,
    graph: u32,
    entity: &[u8],
) -> Result<Vec<Palette>, String> {
    let mut found = Vec::<Palette>::new();
    let mut materials = BTreeMap::<u32, Vec<(u32, u32)>>::new();
    for site in particle_sites(manager, entity)? {
        let system = manager.read_tag(site.system)?;
        let material = u32_at(&system, SYSTEM_MATERIAL)?;
        if !live(manager, material, 8, MATERIAL_CLASS) {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = materials.entry(material) {
            let rows = bindings(&manager.read_tag(material)?)
                .map_err(|error| format!("Material 0x{material:08X}: {error}"))?;
            entry.insert(rows);
        }
        for (binding, (_, header)) in materials[&material].iter().enumerate() {
            let Some(data) = palette_data(manager, *header)? else {
                continue;
            };
            let palette_use = PaletteUse {
                graph,
                site,
                material,
                binding: u32::try_from(binding).map_err(|_| "Too many bindings")?,
            };
            match found.iter_mut().find(|palette| palette.header == *header) {
                Some(palette) => palette.uses.push(palette_use),
                None => found.push(Palette {
                    header: *header,
                    data,
                    uses: vec![palette_use],
                }),
            }
        }
    }
    Ok(found)
}

/// The graphs an ability's effects can be changed in: `entity` and the graphs below it, up to
/// `depth` levels, including those its ability bank and its impact tables name, each once, with
/// its payload.
pub fn ability_graphs(
    manager: &PackageManager,
    entity: u32,
    depth: usize,
) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let mut graphs = Vec::new();
    let mut seen = std::collections::BTreeSet::from([entity]);
    let mut level = vec![entity];
    for below in 0..=depth {
        let mut next = Vec::new();
        for graph in level {
            let payload = manager.read_tag(graph)?;
            if below < depth {
                for child in super::spawns::reached_graphs(manager, graph, &payload)? {
                    if seen.insert(child) {
                        next.push(child);
                    }
                }
            }
            graphs.push((graph, payload));
        }
        level = next;
    }
    Ok(graphs)
}

/// The palettes an ability's effects draw with: those of `entity` and of the graphs below it, up
/// to `depth` levels, including those its ability bank names, each graph once. Each palette lists
/// every use across them.
pub fn ability_palettes(
    manager: &PackageManager,
    entity: u32,
    depth: usize,
) -> Result<Vec<Palette>, String> {
    let mut found = Vec::<Palette>::new();
    for (graph, payload) in ability_graphs(manager, entity, depth)? {
        for palette in palettes(manager, graph, &payload)? {
            match found.iter_mut().find(|each| each.header == palette.header) {
                Some(each) => each.uses.extend(palette.uses),
                None => found.push(palette),
            }
        }
    }
    Ok(found)
}
