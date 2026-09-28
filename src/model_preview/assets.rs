//! Non-mesh parts of an object. These are inspected independently of geometry so an effects
//! sequence remains useful when it has no triangles.
use super::*;
mod audio;
mod effects;
mod particle_program;
mod particle_shaders;
pub(crate) use audio::{decoded_wave, wave_duration};
pub(crate) use particle_program::{Program, Registers};
pub(crate) use particle_shaders::PixelKind;

#[derive(Default)]
pub(crate) struct Assets {
    pub source: u32,
    pub class: u32,
    pub file_type: u8,
    pub size: u32,
    pub particles: Vec<Particle>,
    pub sounds: Vec<Sound>,
    pub lights: Vec<super::light::Info>,
    pub children: Vec<u32>,
    pub components: Vec<Component>,
    pub effect_nodes: Vec<effects::Node>,
    pub image: Option<texture::Texture>,
    pub references: Vec<Reference>,
}

impl Assets {
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
            && self.sounds.is_empty()
            && self.lights.is_empty()
            && self.children.is_empty()
            && self.components.is_empty()
            && self.effect_nodes.is_empty()
            && self.image.is_none()
            && self.references.is_empty()
    }
}

pub(crate) struct Particle {
    pub tag: u32,
    pub name: Option<String>,
    pub definition: Option<u32>,
    pub program: Option<Program>,
    pub emitter: Option<u32>,
    pub emitter_model: Option<u32>,
    pub point_emitter: bool,
    pub material: Option<u32>,
    pub texture: Option<texture::Texture>,
    pub gradient: Option<texture::Texture>,
    pub material_textures: Vec<(u32, texture::Texture)>,
    pub material_samplers: Vec<texture::Sampler>,
    pub material_slot_omissions: usize,
    pub compute_passes: Vec<particle_shaders::ComputePass>,
    pub pixel_kind: Option<PixelKind>,
    pub notice: Option<String>,
}

pub(crate) struct Sound {
    pub tag: u32,
    pub name: Option<String>,
    pub clips: Vec<AudioClip>,
    pub notice: Option<String>,
}

pub(crate) struct AudioClip {
    pub tag: u32,
    pub name: Option<String>,
    pub size: u32,
    pub codec: u16,
    pub channels: u16,
    pub sample_rate: u32,
}

pub(crate) struct Reference {
    pub tag: u32,
    pub class: u32,
    pub file_type: u8,
    pub name: Option<String>,
}

pub(crate) struct Component {
    pub tag: u32,
    pub header: Option<u32>,
    pub data: Option<u32>,
}

impl AudioClip {
    pub fn format(&self) -> String {
        let codec = match self.codec {
            1 => "PCM",
            2 => "ADPCM",
            0xFFFF => "Wwise Vorbis",
            _ => "Encoded Audio",
        };
        format!(
            "{codec} · {} Hz · {} channels",
            self.sample_rate, self.channels
        )
    }
}

pub(super) fn read(
    manager: &PackageManager,
    source: u32,
    inventory: &Inventory,
    load: &Load,
) -> Result<Assets, String> {
    let entry = manager.get_entry(source);
    let mut particles = Vec::new();
    let mut sounds = Vec::new();
    let lights = inventory
        .lights
        .iter()
        .filter_map(|&tag| super::light::info(manager, tag).ok())
        .collect();
    let references = component_references(manager, source, inventory, load)?;
    const DETAIL_LIMIT: usize = 16;
    let total = inventory.particle_systems.len().min(DETAIL_LIMIT)
        + inventory.sounds.len().min(DETAIL_LIMIT);
    for (index, &tag) in inventory.particle_systems.iter().enumerate() {
        if index < DETAIL_LIMIT {
            load.check()?;
            load.say("Reading particle systems", index, total);
            particles.push(particle(manager, tag));
        } else {
            particles.push(particle_summary(manager, tag));
        }
    }
    for (index, &tag) in inventory.sounds.iter().enumerate() {
        if index < DETAIL_LIMIT {
            load.check()?;
            load.say(
                "Reading sounds",
                inventory.particle_systems.len().min(DETAIL_LIMIT) + index,
                total,
            );
            sounds.push(sound(manager, tag));
        } else {
            sounds.push(sound_summary(manager, tag));
        }
    }
    Ok(Assets {
        source,
        class: entry.as_ref().map_or(0, |entry| entry.reference),
        file_type: entry.as_ref().map_or(0, |entry| entry.file_type),
        size: entry.as_ref().map_or(0, |entry| entry.file_size),
        particles,
        sounds,
        lights,
        children: inventory.children.iter().copied().collect(),
        components: inventory
            .components
            .iter()
            .map(|component| Component {
                tag: component.tag,
                header: component.header,
                data: component.data,
            })
            .collect(),
        effect_nodes: effects::read(manager, &inventory.components),
        image: None,
        references,
    })
}

fn component_references(
    manager: &PackageManager,
    source: u32,
    inventory: &Inventory,
    load: &Load,
) -> Result<Vec<Reference>, String> {
    let mut seen = BTreeSet::new();
    let mut references = Vec::new();
    for component in inventory.components.iter().take(32) {
        load.check()?;
        let Some(entry) = manager.get_entry(component.tag) else {
            continue;
        };
        if entry.reference != RESOURCE || entry.file_type != 8 || entry.file_size > 256 * 1024 {
            continue;
        }
        let Ok(bytes) = manager.read_tag(component.tag) else {
            continue;
        };
        if u64_at(&bytes, 0).ok() != Some(bytes.len() as u64) {
            continue;
        }
        let Ok(tags) = declared_references(manager, &bytes, RESOURCE) else {
            continue;
        };
        for tag in tags {
            if references.len() == 256 {
                return Ok(references);
            }
            if tag == source
                || inventory.components.iter().any(|item| item.tag == tag)
                || inventory.particle_systems.contains(&tag)
                || inventory.sounds.contains(&tag)
                || inventory.lights.contains(&tag)
                || inventory.children.contains(&tag)
                || !seen.insert(tag)
            {
                continue;
            }
            let Some(entry) = manager.get_entry(tag) else {
                continue;
            };
            references.push(Reference {
                tag,
                class: entry.reference,
                file_type: entry.file_type,
                name: manager.get_tag_name(tag),
            });
        }
    }
    Ok(references)
}

/// A readable fallback for native resources with no known geometry adapter.
pub(super) fn generic(manager: &PackageManager, tag: u32) -> Result<Assets, String> {
    let entry = manager
        .get_entry(tag)
        .ok_or("The selected resource is missing")?;
    let mut assets = Assets {
        source: tag,
        class: entry.reference,
        file_type: entry.file_type,
        size: entry.file_size,
        ..Default::default()
    };
    if entry.file_type == 32 {
        assets.image = texture::load(manager, tag).ok();
    }
    if entry.file_size <= 2 * 1024 * 1024 {
        let bytes = manager.read_tag(tag)?;
        if matches!(entry.file_type, 20..=22 | 26) {
            if let Some((codec, channels, sample_rate)) = wave_info(&bytes) {
                assets.sounds.push(Sound {
                    tag,
                    name: manager.get_tag_name(tag),
                    clips: vec![AudioClip {
                        tag,
                        name: manager.get_tag_name(tag),
                        size: entry.file_size,
                        codec,
                        channels,
                        sample_rate,
                    }],
                    notice: None,
                });
            }
        }
        if matches!(entry.file_type, 8 | 16) {
            assets.references = declared_references(manager, &bytes, entry.reference)
                .unwrap_or_default()
                .into_iter()
                .filter(|&candidate| candidate != tag)
                .filter_map(|tag| {
                    let entry = manager.get_entry(tag)?;
                    Some(Reference {
                        tag,
                        class: entry.reference,
                        file_type: entry.file_type,
                        name: manager.get_tag_name(tag),
                    })
                })
                .collect();
        }
    }
    Ok(assets)
}

fn declared_references(
    manager: &PackageManager,
    bytes: &[u8],
    class: u32,
) -> Result<Vec<u32>, String> {
    use crate::package_runtime::references::{schema::Registry, walk};

    let mut registry = Registry::new()?;
    let fields = walk(bytes, class, |handle| {
        registry.record(handle, |schema_tag| {
            let entry = manager
                .get_entry(schema_tag)
                .ok_or("Generated reference schema is missing")?;
            if entry.file_type != 8 || entry.reference != 0x8080_0000 {
                return Err("Generated reference schema has an unexpected type".into());
            }
            manager.read_tag(schema_tag)
        })
    })?;
    Ok(fields
        .into_keys()
        .filter(|&tag| manager.get_entry(tag).is_some())
        .collect())
}

/// Particle emitters may draw a native model even when their owner has no model component.
/// Follow only the validated particle -> emitter -> model chain.
pub(super) fn emitter_models(manager: &PackageManager, inventory: &Inventory) -> BTreeSet<u32> {
    let mut models = BTreeSet::new();
    for &system in inventory.particle_systems.iter().take(MAX_CHILD_MODELS) {
        let Some(entry) = manager.get_entry(system) else {
            continue;
        };
        if entry.reference != PARTICLE_SYSTEM || entry.file_type != 8 {
            continue;
        }
        let Ok(bytes) = manager.read_tag(system) else {
            continue;
        };
        let Ok(emitter) = u32_at(&bytes, 0x18) else {
            continue;
        };
        if manager
            .get_entry(emitter)
            .is_none_or(|entry| entry.reference != 0x8080_6E2E || entry.file_type != 8)
        {
            continue;
        }
        let Ok(bytes) = manager.read_tag(emitter) else {
            continue;
        };
        if u64_at(&bytes, 0).ok() != Some(bytes.len() as u64) {
            continue;
        }
        let Ok(model) = u32_at(&bytes, 0x40) else {
            continue;
        };
        if manager
            .get_entry(model)
            .is_some_and(|entry| entry.reference == MODEL && entry.file_type == 8)
        {
            models.insert(model);
        }
    }
    models
}

fn particle(manager: &PackageManager, tag: u32) -> Particle {
    let mut particle = particle_summary(manager, tag);
    let outcome = (|| {
        let entry = manager.get_entry(tag).ok_or("Particle system is missing")?;
        if entry.reference != PARTICLE_SYSTEM || entry.file_type != 8 {
            return Err("Particle system has an unexpected type".to_owned());
        }
        let bytes = manager.read_tag(tag)?;
        particle.compute_passes = particle_shaders::passes(manager, &bytes);
        let definition = u32_at(&bytes, 0)?;
        if manager
            .get_entry(definition)
            .is_some_and(|entry| entry.reference == 0x8080_6E2C && entry.file_type == 8)
        {
            particle.definition = Some(definition);
            particle.program = Some(
                Program::read(&manager.read_tag(definition)?)
                    .map_err(|error| format!("Particle program 0x{definition:08X}: {error}"))?,
            );
        }
        let emitter = u32_at(&bytes, 0x18)?;
        if manager
            .get_entry(emitter)
            .is_some_and(|entry| entry.reference == 0x8080_6E2E && entry.file_type == 8)
        {
            particle.emitter = Some(emitter);
            if let Ok(emitter_bytes) = manager.read_tag(emitter)
                && u64_at(&emitter_bytes, 0).ok() == Some(emitter_bytes.len() as u64)
            {
                particle.point_emitter = emitter_bytes.len() == 32
                    && u32_at(&emitter_bytes, 0x08).ok() == Some(u32::MAX)
                    && u32_at(&emitter_bytes, 0x0C).ok() == Some(0)
                    && emitter_bytes[0x10..].iter().all(|&byte| byte == 0);
                if let Ok(model) = u32_at(&emitter_bytes, 0x40)
                    && manager
                        .get_entry(model)
                        .is_some_and(|entry| entry.reference == MODEL && entry.file_type == 8)
                {
                    particle.emitter_model = Some(model);
                }
            }
        }
        // This native record stores its material at +0x14. Its envelope differs from the
        // size-prefixed entity records, so validate the record type and field separately.
        let material = u32_at(&bytes, 0x14)?;
        if manager
            .get_entry(material)
            .is_none_or(|entry| entry.reference != 0x8080_71E8 || entry.file_type != 8)
        {
            return Err("Particle system has no supported material".to_owned());
        }
        particle.material = Some(material);
        particle.pixel_kind = particle_shaders::pixel_kind(manager, material);
        let (textures, omitted) = texture::material_slots(manager, material)?;
        particle.material_textures = textures;
        particle.material_samplers = texture::material_samplers(manager, material);
        particle.material_slot_omissions = omitted;
        particle.gradient = texture::material_color_ramp(manager, material)
            .ok()
            .flatten();
        let mut preview = Model::default();
        let index = texture::material(manager, material, &mut preview)?;
        particle.texture = Some(preview.textures.swap_remove(index));
        Ok::<(), String>(())
    })();
    if let Err(error) = outcome {
        particle.notice = Some(error);
    }
    particle
}

fn particle_summary(manager: &PackageManager, tag: u32) -> Particle {
    Particle {
        tag,
        name: manager.get_tag_name(tag),
        definition: None,
        program: None,
        emitter: None,
        emitter_model: None,
        point_emitter: false,
        material: None,
        texture: None,
        gradient: None,
        material_textures: Vec::new(),
        material_samplers: Vec::new(),
        material_slot_omissions: 0,
        compute_passes: Vec::new(),
        pixel_kind: None,
        notice: None,
    }
}

fn sound(manager: &PackageManager, tag: u32) -> Sound {
    let mut sound = sound_summary(manager, tag);
    let outcome = (|| {
        let entry = manager.get_entry(tag).ok_or("Sound is missing")?;
        if !matches!(entry.reference, SOUND | SOUND_COLLECTION) || entry.file_type != 8 {
            return Err("Sound has an unexpected type".to_owned());
        }
        let bytes = manager.read_tag(tag)?;
        let tags = sound_clip_tags(&bytes, entry.reference)?;
        sound.clips = tags
            .into_iter()
            .take(32)
            .filter_map(|tag| {
                let entry = manager.get_entry(tag)?;
                if entry.file_size > 32 * 1024 * 1024 {
                    return None;
                }
                let bytes = manager.read_tag(tag).ok()?;
                let (codec, channels, sample_rate) = wave_info(&bytes)?;
                Some(AudioClip {
                    tag,
                    name: manager.get_tag_name(tag),
                    size: entry.file_size,
                    codec,
                    channels,
                    sample_rate,
                })
            })
            .collect();
        if sound.clips.is_empty() {
            return Err("No packaged audio clips were found in this sound".to_owned());
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = outcome {
        sound.notice = Some(error);
    }
    sound
}

fn sound_summary(manager: &PackageManager, tag: u32) -> Sound {
    Sound {
        tag,
        name: manager.get_tag_name(tag),
        clips: Vec::new(),
        notice: None,
    }
}

/// Native sound events store their variants in a typed TagHash array. Other integer fields can
/// happen to match audio tags, so only the declared stream array is used for playback.
fn sound_clip_tags(bytes: &[u8], class: u32) -> Result<Vec<u32>, String> {
    let descriptor = match class {
        SOUND => 0x18,
        SOUND_COLLECTION => 0x20,
        _ => return Err("Unsupported sound record".into()),
    };
    if u64_at(bytes, 0)? != bytes.len() as u64 {
        return Err("Sound has an invalid native envelope".into());
    }
    let (count, rows, kind) = array_at(bytes, descriptor)?;
    if kind != 0x8080_0014 || count > 32 {
        return Err("Sound has an unsupported stream array".into());
    }
    let end = rows
        .checked_add(count.checked_mul(4).ok_or("Sound stream count overflow")?)
        .ok_or("Sound stream offset overflow")?;
    if end > bytes.len() {
        return Err("Sound streams exceed the record".into());
    }
    (0..count)
        .map(|index| u32_at(bytes, rows + index * 4))
        .collect::<Result<Vec<_>, _>>()
}

/// Only native RIFF/WAVE payloads are playable clips. Wwise bank metadata shares file type 26.
fn wave_info(bytes: &[u8]) -> Option<(u16, u16, u32)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let declared =
        8usize.checked_add(u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize)?;
    if declared > bytes.len() {
        return None;
    }
    let mut offset = 12usize;
    let mut format = None;
    let mut data = false;
    while offset.checked_add(8)? <= declared {
        let kind = bytes.get(offset..offset + 4)?;
        let size = u32::from_le_bytes(bytes.get(offset + 4..offset + 8)?.try_into().ok()?) as usize;
        let start = offset + 8;
        let end = start.checked_add(size)?;
        if end > declared {
            return None;
        }
        if kind == b"fmt " {
            if size < 16 {
                return None;
            }
            let codec = u16::from_le_bytes(bytes.get(start..start + 2)?.try_into().ok()?);
            let channels = u16::from_le_bytes(bytes.get(start + 2..start + 4)?.try_into().ok()?);
            let sample_rate = u32::from_le_bytes(bytes.get(start + 4..start + 8)?.try_into().ok()?);
            if !(1..=8).contains(&channels) || !(8_000..=192_000).contains(&sample_rate) {
                return None;
            }
            format = Some((codec, channels, sample_rate));
        } else if kind == b"data" {
            data = true;
        }
        offset = end.checked_add(size & 1)?;
    }
    format.filter(|_| data)
}

pub(crate) fn clip_bytes(packages: &Path, tag: u32) -> Result<Vec<u8>, String> {
    let manager = crate::investment::discovery::open_packages(packages)?;
    let entry = manager.get_entry(tag).ok_or("Audio clip is missing")?;
    if !matches!(entry.file_type, 20..=22 | 26) || entry.file_size > 32 * 1024 * 1024 {
        return Err("Unsupported audio clip type or size".into());
    }
    let bytes = manager.read_tag(tag)?;
    if bytes.len() != entry.file_size as usize || wave_info(&bytes).is_none() {
        return Err("Audio clip is not a valid RIFF/WAVE payload".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{SOUND, sound_clip_tags, wave_info};

    #[test]
    fn sound_uses_declared_variants_instead_of_incidental_audio_handles() {
        let mut sound = vec![0; 0x58];
        sound[..8].copy_from_slice(&0x58u64.to_le_bytes());
        sound[0x10..0x14].copy_from_slice(&0x80C7_ACBEu32.to_le_bytes());
        sound[0x18..0x20].copy_from_slice(&2u64.to_le_bytes());
        sound[0x20..0x28].copy_from_slice(&0x20u64.to_le_bytes());
        sound[0x3C..0x40].copy_from_slice(&0x8080_9FBDu32.to_le_bytes());
        sound[0x40..0x48].copy_from_slice(&2u64.to_le_bytes());
        sound[0x48..0x4C].copy_from_slice(&0x8080_0014u32.to_le_bytes());
        sound[0x50..0x54].copy_from_slice(&0x80C7_ACBFu32.to_le_bytes());
        sound[0x54..0x58].copy_from_slice(&0x80C7_ACC0u32.to_le_bytes());
        assert_eq!(
            sound_clip_tags(&sound, SOUND),
            Ok(vec![0x80C7_ACBF, 0x80C7_ACC0])
        );
        sound[0x48..0x4C].copy_from_slice(&0x8080_0009u32.to_le_bytes());
        assert!(sound_clip_tags(&sound, SOUND).is_err());
    }

    #[test]
    fn riff_audio_requires_a_bounded_format_chunk() {
        let mut wave =
            b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\xff\xff\x02\0\x80\xbb\0\0\0\0\0\0\0\0\0\0data\0\0\0\0".to_vec();
        assert_eq!(wave_info(&wave), Some((0xFFFF, 2, 48_000)));
        wave[16] = 0xFF;
        assert_eq!(wave_info(&wave), None);
        assert_eq!(wave_info(b"BKHD\x18\0\0\0"), None);
    }
}
