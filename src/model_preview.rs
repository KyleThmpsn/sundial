//! Read-only Shadowkeep entity geometry. Layout references: Charm's EntityStructs,
//! EntityModel and VertexBuffer readers (MontagueM/Charm, GPL-3.0).
//! Supports stored geometry and a bounded subset of native skeletal animation codecs.
use crate::{package_payload::*, package_runtime::reader::PackageManager};
use std::{collections::BTreeSet, path::Path};
pub(crate) mod animation;
pub(crate) mod appearance;
pub(crate) mod assets;
mod cloth;
mod decode;
mod effects;
pub(crate) mod export;
pub(crate) mod gpu;
mod light;
pub(crate) mod local;
mod output;
mod particle_material;
mod particles;
#[cfg(test)]
mod projectile_tests;
pub(crate) mod render;
pub(crate) mod shader;
mod statics;
mod terrain;
#[cfg(test)]
mod tests;
pub(crate) mod texture;
mod vertex;

#[derive(Default)]
pub(crate) struct Model {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub tags: Vec<u32>,
    pub uvs: Vec<[f32; 2]>,
    /// Native secondary UVs used by transparent gear passes.
    pub detail_uvs: Vec<[f32; 2]>,
    /// The material's stored vertex program supplied these secondary coordinates.
    pub triangle_detail_uv: Vec<bool>,
    pub triangle_textures: Vec<Option<usize>>,
    /// Composed textured previews exclude uninstanced particle meshes. Inspection modes
    /// and standalone resource previews retain their stored geometry.
    pub triangle_emitter: Vec<bool>,
    /// Light volume outlines are kept for direct light previews, but excluded from mesh framing.
    pub triangle_light: Vec<bool>,
    pub particle_sources: Vec<particles::Source>,
    pub textures: Vec<texture::Texture>,
    pub notices: Vec<String>,
    pub weights: Vec<Option<animation::Weights>>,
    pub(crate) motions: Vec<effects::Motion>,
    pub animation: Option<animation::Animation>,
    pub rigs: Vec<animation::Rig>,
    cloth: Vec<cloth::Timeline>,
    pub animation_notice: Option<String>,
    /// Every clip the object can play, for the viewer's picker.
    pub clips: Vec<animation::Clip>,
    pub assets: assets::Assets,
    /// Stored particle mesh geometry, without native instancing or simulation.
    pub particle_geometry: bool,
    /// Outlines of native light volumes, without the game's illumination shader.
    pub light_geometry: bool,
    pub triangle_dyes: Vec<u8>,
    pub(crate) triangle_dye_maps: Vec<Option<texture::DyeMap>>,
    /// Parts flagged alpha-clipped: the gearstack blue channel is coverage, not emission.
    pub triangle_clip: Vec<bool>,
    /// Native gear-mask cutoff, including cutout materials in the gbuffer stage.
    pub triangle_cutoff: Vec<Option<f32>>,
    /// Flat emissive colour for textureless transparent parts.
    pub triangle_constant: Vec<Option<[f32; 3]>>,
    pub triangle_effects: Vec<Option<usize>>,
    pub(crate) effects: Vec<effects::Material>,
    pub triangle_gearstacks: Vec<Option<usize>>,
    pub triangle_normals: Vec<Option<usize>>,
    pub normals: Vec<[f32; 3]>,
    /// Stored tangent handedness and vertex colors used by native transparent programs.
    pub tangents: Vec<[f32; 4]>,
    pub colors: Vec<[f32; 4]>,
    /// The game's iridescence lookup: one row per dye id, view angle along the row.
    pub iridescence: Option<texture::Texture>,
    pub(crate) dyes: [Option<shader::Dye>; 6],
    pub(crate) dye_animations: Vec<(usize, crate::dyes::material::Animation)>,
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
    /// Shared stored-pose, material motion and skeletal deformation for every renderer/export.
    pub(crate) fn pose(&self, seconds: f32) -> Option<animation::Deformed> {
        if self.motions.is_empty() && self.rigs.is_empty() && self.cloth.is_empty() {
            return self
                .animation
                .as_ref()
                .map(|a| a.sample(self, a.looped_seconds(seconds)));
        }
        let mut stored = animation::Deformed::stored(self);
        for motion in &self.motions {
            motion.apply(self, seconds, &mut stored);
        }
        if let Some(a) = &self.animation {
            a.apply(
                self,
                a.looped_seconds(seconds),
                0..self.vertices.len(),
                &mut stored,
                true,
            );
        }
        for rig in &self.rigs {
            rig.animation.apply(
                self,
                rig.animation.looped_seconds(seconds),
                rig.vertices.clone(),
                &mut stored,
                self.rigs.len() == 1 && rig.vertices.len() == self.vertices.len(),
            );
        }
        for cloth in &self.cloth {
            cloth.apply(seconds, &mut stored);
        }
        Some(stored)
    }

    pub(crate) fn has_animation(&self) -> bool {
        self.animation.is_some() || !self.rigs.is_empty()
    }

    pub(crate) fn has_cloth(&self) -> bool {
        !self.cloth.is_empty()
    }

    pub(crate) fn animation_duration(&self) -> Option<f32> {
        self.animation
            .iter()
            .chain(self.rigs.iter().map(|r| &r.animation))
            .map(animation::Animation::duration)
            .chain(self.cloth.iter().map(cloth::Timeline::duration))
            .reduce(f32::max)
    }

    pub(crate) fn active_clip(&self) -> Option<&animation::Animation> {
        self.animation
            .as_ref()
            .or_else(|| self.rigs.first().map(|r| &r.animation))
    }
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
        self.motions.iter().any(effects::Motion::animated)
            || !self.dye_animations.is_empty()
            || self.effects.iter().any(|m| {
                m.program.as_ref().is_some_and(|p| p.animated())
                    || m.native.as_ref().is_some_and(|n| n.animated())
            })
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

    pub(crate) fn particle_simulation(&self) -> Option<&particles::simulation::Timeline> {
        if !self.has_particle_material_study() || !self.particle_sources.is_empty() {
            return None;
        }
        self.assets
            .particles
            .first()?
            .simulation
            .as_ref()
            .filter(|simulation| self.triangles.len().saturating_mul(simulation.peak) <= 200_000)
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
// Vehicle assemblies can exceed the old 24-image count with ordinary surface materials.
// Retained decoded bytes remain independently bounded in texture::retain/check_pending.
pub(crate) const MAX_TEXTURES: usize = 256;

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

/// Background package reads still running, such as a viewer's. Each keeps package files open
/// until it returns.
static PACKAGE_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Counts one background read of the packages for as long as it is alive, so an operation that
/// replaces packages waits for it. Move it into the reading thread so it ends with the read,
/// however the read ends.
pub struct PackageRead(());

impl PackageRead {
    #[must_use]
    pub fn start() -> Self {
        PACKAGE_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(())
    }
}

impl Drop for PackageRead {
    fn drop(&mut self) {
        PACKAGE_READS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Whether a background read of the packages is still running. Pausing a viewer stops it from
/// starting new reads, but one already started finishes, so a package operation waits for this.
pub(crate) fn package_reads_running() -> bool {
    PACKAGE_READS.load(std::sync::atomic::Ordering::SeqCst) != 0
}

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
    if let Some(model) = decode::specialized(manager, tag, entry.reference, cancel, clip)? {
        return Ok(model);
    }
    let mut tags = BTreeSet::new();
    let mut resources = Vec::new();
    let mut components = Vec::new();
    let mut inventory = Inventory::default();
    let mut emitter_only = false;
    let mut owners = Vec::new();
    match entry.reference {
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
            (emitter_only, owners) = collect_entity_preview(
                manager,
                tag,
                cancel,
                &mut tags,
                &mut resources,
                &mut components,
                &mut inventory,
            )?;
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
        return decode::empty(manager, tag, &inventory, &resources, cancel);
    }
    if owners.is_empty() {
        owners.push(decode::Owner {
            models: tags,
            resources: 0..resources.len(),
            components: 0..components.len(),
        });
    }
    let mut model = decode::owners(
        manager,
        &owners,
        &resources,
        &components,
        cancel,
        clip,
        emitter_only,
    )?;
    model.assets = assets::read(manager, tag, &inventory, cancel)?;
    decode::finish(manager, model, &inventory, &resources, emitter_only)
}

fn collect_entity_preview(
    manager: &PackageManager,
    tag: u32,
    cancel: &Load,
    tags: &mut BTreeSet<u32>,
    resources: &mut Vec<Vec<u8>>,
    components: &mut Vec<Vec<u8>>,
    inventory: &mut Inventory,
) -> Result<(bool, Vec<decode::Owner>), String> {
    collect_entity(manager, tag, tags, resources, components, inventory)?;
    let has_own_model = !tags.is_empty();
    let mut owners = Vec::new();
    if has_own_model {
        owners.push(decode::Owner {
            models: tags.clone(),
            resources: 0..resources.len(),
            components: 0..components.len(),
        });
    }
    let mut model_count = tags.len();
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
            let resource_start = resources.len();
            let component_start = components.len();
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
            if !has_own_model && !child_tags.is_empty() && model_count < MAX_CHILD_MODELS {
                child_tags = child_tags
                    .into_iter()
                    .take(MAX_CHILD_MODELS - model_count)
                    .collect();
                model_count += child_tags.len();
                tags.extend(&child_tags);
                owners.push(decode::Owner {
                    models: child_tags,
                    resources: resource_start..resources.len(),
                    components: component_start..components.len(),
                });
            }
            queue.push((child, depth + 1));
        }
    }
    if tags.is_empty() {
        tags.extend(assets::emitter_models(manager, inventory));
        return Ok((!tags.is_empty(), owners));
    }
    Ok((false, owners))
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
    if header < 4 {
        return Ok(());
    }
    let expected = match u32_at(bytes, header - 4)? {
        0x8080_72B8 => 0x8080_72BD,
        // Cloth owns a separate model, with the same model and plate fields in this prefix.
        0x8080_7273 => 0x8080_7286,
        _ => return Ok(()),
    };
    let data = pointer(bytes, 0x18)?;
    if data < 4 || u32_at(bytes, data - 4)? != expected {
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
