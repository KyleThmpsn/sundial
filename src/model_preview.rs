//! Read-only Shadowkeep entity geometry. Layout references: Charm's EntityStructs,
//! EntityModel and VertexBuffer readers (MontagueM/Charm, GPL-3.0).
//! Supports stored geometry and a bounded subset of native skeletal animation codecs.
use crate::{package_payload::*, package_runtime::reader::PackageManager};
use std::{collections::BTreeSet, path::Path};
pub(crate) mod animation;
pub(crate) mod assets;
mod decode;
pub(crate) mod export;
pub(crate) mod gpu;
mod light;
mod particle_material;
mod particles;
#[cfg(test)]
mod projectile_tests;
pub(crate) mod render;
mod shader;
mod statics;
#[cfg(test)]
mod tests;
pub(crate) mod texture;
pub(crate) mod weapon;

#[derive(Default)]
pub(crate) struct Model {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub tags: Vec<u32>,
    pub uvs: Vec<[f32; 2]>,
    pub triangle_textures: Vec<Option<usize>>,
    /// Emitter shape triangles are shown only in solid and wireframe inspection modes.
    pub triangle_emitter: Vec<bool>,
    /// Light volume outlines are kept for direct light previews, but excluded from mesh framing.
    pub triangle_light: Vec<bool>,
    pub particle_sources: Vec<particles::Source>,
    pub textures: Vec<texture::Texture>,
    pub notices: Vec<String>,
    pub weights: Vec<Option<animation::Weights>>,
    pub animation: Option<animation::Animation>,
    pub animation_notice: Option<String>,
    /// Every clip the object can play, for the viewer's picker.
    pub clips: Vec<animation::Clip>,
    pub assets: assets::Assets,
    /// The recovered emitter shape is a static mesh, not simulated particle output.
    pub particle_geometry: bool,
    /// Outlines of native light volumes, without the game's illumination shader.
    pub light_geometry: bool,
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
    /// Surfaces drawn with an editor's colors instead of their dyes'. A shown model takes new
    /// ones in place, so a color edit never reads the model again.
    surface_overrides: std::sync::Mutex<Vec<SurfaceOverride>>,
}

/// A surface drawn with other values than its dye's, for a dye an editor has not built yet.
/// `slot` counts surfaces: armor primary, armor secondary, cloth primary, cloth secondary, suit
/// primary and suit secondary.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceOverride {
    pub slot: usize,
    /// Values written into the dye's 27 material vectors where a build writes them: a vector,
    /// its lane and the value.
    pub writes: Vec<(usize, usize, f32)>,
}

impl Model {
    /// Draws surfaces with an editor's colors.
    pub(crate) fn set_surface_overrides(&self, overrides: &[SurfaceOverride]) {
        let mut current = self
            .surface_overrides
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.as_slice() != overrides {
            *current = overrides.to_vec();
        }
    }

    pub(crate) fn has_shader_animation(&self) -> bool {
        !self.dye_animations.is_empty()
    }

    pub(crate) fn has_particle_material_study(&self) -> bool {
        self.particle_geometry
            && self.assets.particles.len() == 1
            && self.assets.particles.iter().any(|particle| {
                particle.pixel_kind.is_some()
                    && particle.material_samplers.len() >= 3
                    && particle
                        .program
                        .as_ref()
                        .and_then(|program| program.lifetime_default())
                        .is_some()
                    && [0, 1, 2].into_iter().all(|slot| {
                        particle
                            .material_textures
                            .iter()
                            .any(|(index, _)| *index == slot)
                    })
            })
    }

    pub(crate) fn has_surface_mesh(&self) -> bool {
        self.triangles
            .iter()
            .enumerate()
            .any(|(index, _)| !self.triangle_light.get(index).copied().unwrap_or(false))
    }

    pub(crate) fn has_object_mesh(&self) -> bool {
        self.triangles.iter().enumerate().any(|(index, _)| {
            !self.triangle_light.get(index).copied().unwrap_or(false)
                && !self.triangle_emitter.get(index).copied().unwrap_or(false)
        })
    }
}

const ENTITY: u32 = 0x8080_9C0F;
const RESOURCE: u32 = 0x8080_9C36;
const MODEL: u32 = 0x8080_73A5;
const MAX_VERTICES: usize = 500_000;
const MAX_TRIANGLES: usize = 500_000;
const MAX_TEXTURES: usize = 24;

/// What the reader is doing right now, for the waiting viewer.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Stage {
    pub message: String,
    /// Steps finished and expected. Both zero while the size of the job is still unknown.
    pub done: usize,
    pub total: usize,
}

/// Shared handle between a preview and the thread reading its model. Reading a whole object
/// takes seconds, so the reader publishes what it is doing and checks whether the viewer has
/// gone away, instead of running to completion in silence for a window nobody is watching.
#[derive(Clone, Default)]
pub(crate) struct Load {
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    stage: std::sync::Arc<std::sync::Mutex<Stage>>,
}

/// Returned when a load stops early. Callers discard it rather than showing it.
pub(crate) const CANCELLED: &str = "The model load was cancelled.";

impl Load {
    pub fn stop(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn stopped(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// The latest stage, or an empty one if a reader panicked while holding the lock.
    pub fn stage(&self) -> Stage {
        self.stage.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub(crate) fn say(&self, message: impl Into<String>, done: usize, total: usize) {
        if let Ok(mut stage) = self.stage.lock() {
            *stage = Stage {
                message: message.into(),
                done,
                total,
            };
        }
    }
    fn check(&self) -> Result<(), String> {
        if self.stopped() {
            return Err(CANCELLED.into());
        }
        Ok(())
    }
}

/// The whole object with its default clip, for tests that do not watch progress.
#[cfg(test)]
pub(crate) fn load(packages: &Path, tag: u32) -> Result<Model, String> {
    load_reported(packages, tag, &Load::default(), None)
}

/// `clip` names an animation to play instead of the object's default one.
pub(crate) fn load_reported(
    packages: &Path,
    tag: u32,
    load: &Load,
    clip: Option<u32>,
) -> Result<Model, String> {
    load.say("Opening packages", 0, 0);
    let manager = crate::investment::discovery::open_packages(packages)?;
    load_with_manager(&manager, tag, load, clip)
}

fn load_with_manager(
    manager: &PackageManager,
    tag: u32,
    cancel: &Load,
    clip: Option<u32>,
) -> Result<Model, String> {
    cancel.check()?;
    cancel.say("Reading object", 0, 0);
    let entry = manager
        .get_entry(tag)
        .ok_or("The selected resource is missing")?;
    let mut tags = BTreeSet::new();
    let mut resources = Vec::new();
    let mut components = Vec::new();
    let mut inventory = Inventory::default();
    let mut emitter_only = false;
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
            return load_with_manager(manager, entity, cancel, clip);
        }
        MODEL => {
            tags.insert(tag);
        }
        RESOURCE => {
            let bytes = checked(manager, tag, RESOURCE)?;
            collect_component(&bytes, &mut tags, &mut resources)?;
            inventory.record_component(tag, &bytes);
            inventory.scan(manager, &bytes);
            components.push(bytes);
        }
        ENTITY => {
            emitter_only = collect_entity_preview(
                manager,
                tag,
                cancel,
                &mut tags,
                &mut resources,
                &mut components,
                &mut inventory,
            )?;
        }
        // Map and prop geometry sits outside the entity family and reads its own tables.
        class if statics::is_static(class) => {
            cancel.say("Reading static mesh", 0, 0);
            return statics::load(manager, tag);
        }
        light::SHADOWING_LIGHT | light::LIGHT_COLLECTION => {
            return light::load(manager, tag);
        }
        PARTICLE_SYSTEM => {
            inventory.particle_systems.insert(tag);
            tags.extend(assets::emitter_models(manager, &inventory));
            emitter_only = !tags.is_empty();
        }
        SOUND | SOUND_COLLECTION => {
            inventory.sounds.insert(tag);
        }
        SOUND_BANK => {
            inventory.scan_sound_bank(manager, tag);
        }
        _ => {
            return Ok(Model {
                assets: assets::generic(manager, tag)?,
                ..Default::default()
            });
        }
    }
    if tags.is_empty() {
        let mut model = Model {
            assets: if inventory.particle_systems.is_empty()
                && inventory.sounds.is_empty()
                && inventory.lights.is_empty()
                && inventory.children.is_empty()
                && inventory.components.is_empty()
            {
                assets::generic(manager, tag)?
            } else {
                assets::read(manager, tag, &inventory, cancel)?
            },
            ..Default::default()
        };
        model.clips = animation::clips(manager, &resources);
        particles::collect_points(&mut model);
        light::append(manager, &inventory.lights, &mut model);
        return Ok(model);
    }
    let mut model = Model {
        assets: assets::read(manager, tag, &inventory, cancel)?,
        ..Default::default()
    };
    let total = tags.len();
    for (done, tag) in tags.into_iter().enumerate() {
        cancel.check()?;
        cancel.say(
            if total > 1 {
                format!("Decoding mesh {} of {total}", done + 1)
            } else {
                "Decoding mesh".to_owned()
            },
            done,
            total,
        );
        let component = components.iter().find(|bytes| {
            pointer(bytes, 0x18)
                .ok()
                .and_then(|data| u32_at(bytes, data + 0x1DC).ok())
                == Some(tag)
        });
        let first_triangle = model.triangles.len();
        match decode::append(manager, tag, component.map(Vec::as_slice), &mut model) {
            Ok(()) => {
                model.tags.push(tag);
                if emitter_only {
                    model.triangle_emitter.resize(model.triangles.len(), false);
                    model.triangle_emitter[first_triangle..].fill(true);
                }
            }
            Err(error) if emitter_only => model.notices.push(format!(
                "Particle geometry 0x{tag:08X} could not be drawn: {error}"
            )),
            Err(error) => return Err(error),
        }
    }
    if model.triangles.is_empty() {
        model.clips = animation::clips(manager, &resources);
        light::append(manager, &inventory.lights, &mut model);
        if !model.triangles.is_empty() {
            return Ok(model);
        }
        if emitter_only {
            return Ok(model);
        }
        return Err("The model contains no supported triangles.".into());
    }
    if emitter_only {
        model.particle_geometry = true;
    }
    particles::collect_points(&mut model);
    cancel.say("Reading animation", total, total);
    model.clips = animation::clips(manager, &resources);
    match clip {
        Some(tag) => match animation::load_clip(manager, &resources, &model, tag) {
            Ok(animation) => model.animation = Some(animation),
            Err(error) => model.animation_notice = Some(error),
        },
        None => match animation::load(manager, &resources, &model) {
            Ok(animation) => model.animation = animation,
            Err(error) => model.animation_notice = Some(error),
        },
    }
    light::append(manager, &inventory.lights, &mut model);
    Ok(model)
}

fn collect_entity_preview(
    manager: &PackageManager,
    tag: u32,
    cancel: &Load,
    tags: &mut BTreeSet<u32>,
    resources: &mut Vec<Vec<u8>>,
    components: &mut Vec<Vec<u8>>,
    inventory: &mut Inventory,
) -> Result<bool, String> {
    collect_entity(manager, tag, tags, resources, components, inventory)?;
    let has_own_model = !tags.is_empty();
    // Effects, sequences and animation patterns can keep their visible parts in child entities.
    let mut visited = BTreeSet::from([tag]);
    let mut queue = vec![(tag, 0usize)];
    cancel.say("Looking in child objects", 0, 0);
    while let Some((parent, depth)) = queue.pop() {
        cancel.check()?;
        for child in child_entities(manager, parent, inventory) {
            if visited.len() >= MAX_CHILD_ENTITIES {
                break;
            }
            if !visited.insert(child) || depth >= MAX_CHILD_DEPTH {
                continue;
            }
            let mut child_tags = BTreeSet::new();
            if collect_entity(
                manager,
                child,
                &mut child_tags,
                resources,
                components,
                inventory,
            )
            .is_err()
            {
                continue;
            }
            if !has_own_model && tags.len() < MAX_CHILD_MODELS {
                tags.extend(child_tags);
            }
            queue.push((child, depth + 1));
        }
    }
    if tags.is_empty() {
        tags.extend(assets::emitter_models(manager, inventory));
        return Ok(!tags.is_empty());
    }
    Ok(false)
}

const MAX_CHILD_DEPTH: usize = 3;
const MAX_CHILD_MODELS: usize = 16;
const MAX_CHILD_ENTITIES: usize = 64;
const PARTICLE_SYSTEM: u32 = 0x8080_6E28;
const SOUND: u32 = 0x8080_9802;
const SOUND_COLLECTION: u32 = 0x8080_9738;
const SOUND_BANK: u32 = 0x8080_8D54;

/// What a model-less object is made of, for the message when nothing can be drawn.
#[derive(Default)]
struct Inventory {
    particle_systems: BTreeSet<u32>,
    sounds: BTreeSet<u32>,
    sound_banks: BTreeSet<u32>,
    lights: BTreeSet<u32>,
    children: BTreeSet<u32>,
    components: Vec<assets::Component>,
}

impl Inventory {
    fn scan_sound_bank(&mut self, manager: &PackageManager, tag: u32) {
        if !self.sound_banks.insert(tag) {
            return;
        }
        let Some(entry) = manager.get_entry(tag) else {
            return;
        };
        if entry.reference != SOUND_BANK || entry.file_type != 8 || entry.file_size > 1024 * 1024 {
            return;
        }
        let Ok(bytes) = manager.read_tag(tag) else {
            return;
        };
        if u64_at(&bytes, 0).ok() != Some(bytes.len() as u64) {
            return;
        }
        self.scan(manager, &bytes);
    }

    fn record_component(&mut self, tag: u32, bytes: &[u8]) {
        let header = pointer(bytes, 0x10)
            .ok()
            .and_then(|at| at.checked_sub(4))
            .and_then(|at| u32_at(bytes, at).ok());
        let data = pointer(bytes, 0x18)
            .ok()
            .and_then(|at| at.checked_sub(4))
            .and_then(|at| u32_at(bytes, at).ok());
        if !self.components.iter().any(|component| component.tag == tag) {
            self.components
                .push(assets::Component { tag, header, data });
        }
    }

    fn scan(&mut self, manager: &PackageManager, bytes: &[u8]) {
        for word in bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        {
            if word & 0xFF00_0000 != 0x8000_0000 {
                continue;
            }
            if let Some(entry) = manager.get_entry(word) {
                match entry.reference {
                    PARTICLE_SYSTEM if entry.file_type == 8 => {
                        self.particle_systems.insert(word);
                    }
                    SOUND | SOUND_COLLECTION if entry.file_type == 8 => {
                        self.sounds.insert(word);
                    }
                    SOUND_BANK if entry.file_type == 8 => {
                        self.scan_sound_bank(manager, word);
                    }
                    light::SHADOWING_LIGHT if entry.file_type == 8 => {
                        self.lights.insert(word);
                    }
                    _ => {}
                }
            }
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
        inventory.record_component(resource, &bytes);
        collect_component(&bytes, tags, resources)?;
        inventory.scan(manager, &bytes);
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
