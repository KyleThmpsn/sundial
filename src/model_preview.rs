//! Read-only Shadowkeep entity geometry. Layout references: Charm's EntityStructs,
//! EntityModel and VertexBuffer readers (MontagueM/Charm, GPL-3.0).
//! Supports stored geometry and a bounded subset of native skeletal animation codecs.
use crate::{package_payload::*, package_runtime::reader::PackageManager};
use std::{collections::BTreeSet, path::Path};
pub(crate) mod animation;
mod decode;
pub(crate) mod gpu;
#[cfg(test)]
mod projectile_tests;
pub(crate) mod render;
mod shader;
#[cfg(test)]
mod tests;
mod texture;
pub(crate) mod weapon;

#[derive(Default)]
pub(crate) struct Model {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub tags: Vec<u32>,
    pub uvs: Vec<[f32; 2]>,
    pub triangle_textures: Vec<Option<usize>>,
    pub textures: Vec<texture::Texture>,
    pub notices: Vec<String>,
    pub weights: Vec<Option<animation::Weights>>,
    pub animation: Option<animation::Animation>,
    pub animation_notice: Option<String>,
    pub triangle_dyes: Vec<u8>,
    /// Parts flagged alpha-clipped: the gearstack blue channel is coverage, not emission.
    pub triangle_clip: Vec<bool>,
    /// Flat emissive colour for textureless transparent parts.
    pub triangle_constant: Vec<Option<[f32; 3]>>,
    pub triangle_gearstacks: Vec<Option<usize>>,
    pub triangle_normals: Vec<Option<usize>>,
    pub normals: Vec<[f32; 3]>,
    /// The game's iridescence lookup: one row per dye id, view angle along the row.
    pub iridescence: Option<texture::Texture>,
    dyes: [Option<shader::Dye>; 6],
    dye_animations: Vec<(usize, crate::weapon_dyes::material::Animation)>,
}

impl Model {
    pub(crate) fn has_shader_animation(&self) -> bool {
        !self.dye_animations.is_empty()
    }
}

const ENTITY: u32 = 0x8080_9C0F;
const RESOURCE: u32 = 0x8080_9C36;
const MODEL: u32 = 0x8080_73A5;
const MAX_VERTICES: usize = 500_000;
const MAX_TRIANGLES: usize = 500_000;
const MAX_TEXTURES: usize = 24;

pub(crate) fn load(packages: &Path, tag: u32) -> Result<Model, String> {
    let manager = crate::investment::discovery::open_packages(packages)?;
    load_with_manager(&manager, tag)
}

fn load_with_manager(manager: &PackageManager, tag: u32) -> Result<Model, String> {
    let entry = manager
        .get_entry(tag)
        .ok_or("The selected resource is missing")?;
    let mut tags = BTreeSet::new();
    let mut resources = Vec::new();
    let mut components = Vec::new();
    match entry.reference {
        0x8080_744A => {
            let relation = checked(manager, tag, 0x8080_744A)?;
            let entity = u32_at(&relation, 0x10)?;
            if manager
                .get_entry(entity)
                .is_none_or(|e| e.reference != ENTITY)
            {
                return Err("This assignment does not point to a renderable object".into());
            }
            return load_with_manager(manager, entity);
        }
        MODEL => {
            tags.insert(tag);
        }
        RESOURCE => {
            let bytes = checked(manager, tag, RESOURCE)?;
            collect_component(&bytes, &mut tags, &mut resources)?;
            components.push(bytes);
        }
        ENTITY => {
            let mut inventory = Inventory::default();
            collect_entity(
                manager,
                tag,
                &mut tags,
                &mut resources,
                &mut components,
                &mut inventory,
            )?;
            if tags.is_empty() {
                // Effects, sequences and animation patterns keep their visible parts in child
                // entities. Walk those before giving up on the object.
                let mut visited = BTreeSet::from([tag]);
                let mut queue = vec![(tag, 0usize)];
                while let Some((parent, depth)) = queue.pop() {
                    for child in child_entities(manager, parent, &mut inventory) {
                        if !visited.insert(child) || depth >= MAX_CHILD_DEPTH {
                            continue;
                        }
                        let mut child_tags = BTreeSet::new();
                        if collect_entity(
                            manager,
                            child,
                            &mut child_tags,
                            &mut resources,
                            &mut components,
                            &mut inventory,
                        )
                        .is_err()
                        {
                            continue;
                        }
                        if child_tags.is_empty() {
                            queue.push((child, depth + 1));
                        } else if tags.len() < MAX_CHILD_MODELS {
                            tags.extend(child_tags);
                        }
                    }
                }
            }
            if tags.is_empty() {
                return Err(inventory.describe());
            }
        }
        _ => return Err("This resource does not have a supported entity model.".into()),
    }
    if tags.is_empty() {
        return Err("This resource has no mesh geometry to draw.".into());
    }
    let mut model = Model::default();
    for tag in tags {
        let component = components.iter().find(|bytes| {
            pointer(bytes, 0x18)
                .ok()
                .and_then(|data| u32_at(bytes, data + 0x1DC).ok())
                == Some(tag)
        });
        decode::append(manager, tag, component.map(Vec::as_slice), &mut model)?;
        model.tags.push(tag);
    }
    if model.triangles.is_empty() {
        return Err("The model contains no supported triangles.".into());
    }
    match animation::load(manager, &resources, &model) {
        Ok(animation) => model.animation = animation,
        Err(error) => model.animation_notice = Some(error),
    }
    Ok(model)
}

const MAX_CHILD_DEPTH: usize = 3;
const MAX_CHILD_MODELS: usize = 16;
const PARTICLE_SYSTEM: u32 = 0x8080_6E28;
const SOUND: u32 = 0x8080_9802;

/// What a model-less object is made of, for the message when nothing can be drawn.
#[derive(Default)]
struct Inventory {
    particle_systems: BTreeSet<u32>,
    sounds: BTreeSet<u32>,
    children: BTreeSet<u32>,
}

impl Inventory {
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        for (count, noun) in [
            (self.particle_systems.len(), "particle system"),
            (self.sounds.len(), "sound"),
            (self.children.len(), "child object"),
        ] {
            match count {
                0 => {}
                1 => parts.push(format!("1 {noun}")),
                n => parts.push(format!("{n} {noun}s")),
            }
        }
        if parts.is_empty() {
            "This object has no mesh geometry to draw.".into()
        } else {
            format!(
                "This object has no mesh geometry to draw. It holds {}.",
                parts.join(", ")
            )
        }
    }
}

/// Every model component reachable from an entity's own resource list.
fn collect_entity(
    manager: &PackageManager,
    tag: u32,
    tags: &mut BTreeSet<u32>,
    resources: &mut Vec<Vec<u8>>,
    components: &mut Vec<Vec<u8>>,
    inventory: &mut Inventory,
) -> Result<(), String> {
    let entity = checked(manager, tag, ENTITY)?;
    let (count, rows) = array(&entity, 0x10, 0x8080_9C04, 12, 4096)?;
    for index in 0..count {
        let resource = u32_at(&entity, rows + index * 12)?;
        if manager
            .get_entry(resource)
            .is_none_or(|e| e.reference != RESOURCE)
        {
            continue;
        }
        let bytes = checked(manager, resource, RESOURCE)?;
        collect_component(&bytes, tags, resources)?;
        for word in bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        {
            if word & 0xFF00_0000 != 0x8000_0000 {
                continue;
            }
            if let Some(entry) = manager.get_entry(word) {
                match entry.reference {
                    PARTICLE_SYSTEM => {
                        inventory.particle_systems.insert(word);
                    }
                    SOUND => {
                        inventory.sounds.insert(word);
                    }
                    _ => {}
                }
            }
        }
        components.push(bytes);
    }
    Ok(())
}

/// Entities referenced anywhere in an entity's resources. The resource layouts vary by
/// type, so this reads every aligned word and keeps the ones that name an entity.
fn child_entities(manager: &PackageManager, tag: u32, inventory: &mut Inventory) -> Vec<u32> {
    let mut children = Vec::new();
    let Ok(entity) = checked(manager, tag, ENTITY) else {
        return children;
    };
    let Ok((count, rows)) = array(&entity, 0x10, 0x8080_9C04, 12, 4096) else {
        return children;
    };
    for index in 0..count {
        let Ok(resource) = u32_at(&entity, rows + index * 12) else {
            continue;
        };
        let Ok(bytes) = manager.read_tag(resource) else {
            continue;
        };
        for word in bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        {
            if word & 0xFF00_0000 != 0x8000_0000
                || word & 0x00FF_FFFF == 0
                || word == tag
                || word == resource
                || children.contains(&word)
            {
                continue;
            }
            if manager
                .get_entry(word)
                .is_some_and(|entry| entry.reference == ENTITY && entry.file_type == 8)
            {
                children.push(word);
                inventory.children.insert(word);
            }
        }
    }
    children
}

fn collect_component(
    bytes: &[u8],
    tags: &mut BTreeSet<u32>,
    resources: &mut Vec<Vec<u8>>,
) -> Result<(), String> {
    if pointer(bytes, 0x18)
        .ok()
        .and_then(|offset| offset.checked_sub(4))
        .and_then(|offset| u32_at(bytes, offset).ok())
        .is_some_and(|class| matches!(class, 0x8080_8546 | 0x8080_344B))
    {
        resources.push(bytes.to_vec());
    }
    let header = pointer(bytes, 0x10)?;
    if header < 4 || u32_at(bytes, header - 4)? != 0x8080_72B8 {
        return Ok(());
    }
    let data = pointer(bytes, 0x18)?;
    if data < 4 || u32_at(bytes, data - 4)? != 0x8080_72BD {
        return Err("The entity model component has an unsupported layout".into());
    }
    tags.insert(u32_at(bytes, data + 0x1DC)?);
    Ok(())
}

fn pointer(bytes: &[u8], offset: usize) -> Result<usize, String> {
    relative_offset(offset, 0, i64_at(bytes, offset)?)
}

fn checked(manager: &PackageManager, tag: u32, class: u32) -> Result<Vec<u8>, String> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| format!("Resource 0x{tag:08X} is missing"))?;
    if (entry.file_type != 8 && !(class == 0x8080_744A && entry.file_type == 16))
        || entry.reference != class
    {
        return Err(format!(
            "Resource 0x{tag:08X} has an unsupported model type"
        ));
    }
    let bytes = manager.read_tag(tag)?;
    if bytes.len() != entry.file_size as usize || u64_at(&bytes, 0)? != bytes.len() as u64 {
        return Err(format!("Resource 0x{tag:08X} has an invalid size"));
    }
    Ok(bytes)
}

fn array(
    bytes: &[u8],
    offset: usize,
    class: u32,
    stride: usize,
    limit: usize,
) -> Result<(usize, usize), String> {
    let (count, _, rows, actual) = native_array_at(bytes, offset)?;
    if count > limit
        || count
            .checked_mul(stride)
            .and_then(|n| rows.checked_add(n))
            .is_none_or(|end| end > bytes.len())
    {
        return Err(format!(
            "Model array at {offset:#x} is unsupported: class {actual:08X} (expected {class:08X}), {count} rows of {stride} bytes (limit {limit}) in {} bytes",
            bytes.len()
        ));
    }
    // Another class with the same stride still reads; a wrong read fails on its indices later,
    // and refusing here hid whole models behind one differing sub-table.
    Ok((count, rows))
}
