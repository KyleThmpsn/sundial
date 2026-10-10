//! Installed investment appearance assignments, separate from gameplay entities.
use super::*;
use crate::package_authoring::resolve_live_named_tag;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub arrangement: u16,
    /// Effective channel/reference pairs after default, custom, shader and locked precedence.
    pub dyes: Vec<(i8, u16)>,
    /// Detail textures an editor gives dyes it has not built yet.
    pub dye_textures: Vec<DyeTextureOverride>,
}

/// The detail textures one dye draws with instead of its own, by texture tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DyeTextureOverride {
    /// The dye's channel within the item's gear type, from 0 to 2.
    pub channel: usize,
    pub detail: Option<u32>,
    pub normal: Option<u32>,
}

/// The composed appearance with no progress reporting, for tests.
#[cfg(test)]
pub(crate) fn load(packages: &Path, appearance: &Appearance) -> Result<Model, String> {
    load_reported(packages, appearance, &super::Load::default())
}

pub(crate) fn load_reported(
    packages: &Path,
    appearance: &Appearance,
    cancel: &super::Load,
) -> Result<Model, String> {
    load_clip_reported(packages, appearance, cancel, None)
}

pub(crate) fn load_clip_reported(
    packages: &Path,
    appearance: &Appearance,
    cancel: &super::Load,
    clip: Option<u32>,
) -> Result<Model, String> {
    cancel.say("Opening packages", 0, 0);
    let manager = crate::investment::discovery::open_packages(packages)?;
    let globals = manager.read_tag(resolve_live_named_tag(
        &manager,
        "investment_globals",
        None,
    )?)?;
    let table = checked(&manager, u32_at(&globals, 0x430)?, 0x8080_5DF5)?;
    let assignments = assignment_hashes(&table, appearance.arrangement)?;
    let assets = manager.read_tag(resolve_live_named_tag(&manager, "investment_assets", None)?)?;
    let map = checked(&manager, u32_at(&assets, 0x20)?, 0x8080_56EA)?;
    let (count, rows) = array(&map, 8, 0x8080_56EC, 8, 100_000)?;
    let mut entities = BTreeSet::new();
    for hash in assignments {
        for index in 0..count {
            let row = rows + index * 8;
            if u32_at(&map, row)? != hash {
                continue;
            }
            let relation = checked(&manager, u32_at(&map, row + 4)?, 0x8080_744A)?;
            entities.insert(u32_at(&relation, 0x10)?);
            break;
        }
    }
    if entities.is_empty() {
        return Err("This appearance has no installed model assignment".into());
    }
    if entities.len() > 64 {
        return Err("This appearance exceeds the preview object budget".into());
    }
    let mut result = Model::default();
    let total = entities.len();
    for (done, entity) in entities.into_iter().enumerate() {
        if cancel.stopped() {
            return Err(super::CANCELLED.into());
        }
        cancel.say(
            if total > 1 {
                format!("Reading part {} of {total}", done + 1)
            } else {
                "Reading weapon".to_owned()
            },
            done,
            total,
        );
        let mut model = match load_with_manager(&manager, entity, cancel, None) {
            Ok(model) => model,
            Err(error) => {
                result
                    .notices
                    .push(format!("Object 0x{entity:08X}: {error}"));
                continue;
            }
        };
        if let Some(tag) = clip.filter(|tag| model.clips.iter().any(|c| c.tag == *tag)) {
            model = load_with_manager(&manager, entity, cancel, Some(tag))?;
        }
        append(&mut result, model, &format!("Part {}", done + 1))?;
    }
    if result.triangles.is_empty() {
        return Err(format!(
            "This appearance has no supported geometry. {}",
            result.notices.join(" ")
        ));
    }
    apply_colors(&manager, &globals, appearance, &mut result)?;
    if !appearance.dyes.is_empty() {
        result.iridescence = texture::iridescence(&manager);
    }
    result.notices.sort();
    result.notices.dedup();
    let mut seen = BTreeSet::new();
    result
        .assets
        .particles
        .retain(|particle| seen.insert(particle.tag));
    seen.clear();
    result.assets.sounds.retain(|sound| seen.insert(sound.tag));
    seen.clear();
    result.assets.lights.retain(|light| seen.insert(light.tag));
    seen.clear();
    result.assets.children.retain(|tag| seen.insert(*tag));
    seen.clear();
    result
        .assets
        .components
        .retain(|component| seen.insert(component.tag));
    seen.clear();
    result
        .assets
        .references
        .retain(|reference| seen.insert(reference.tag));
    let mut nodes = BTreeSet::new();
    result
        .assets
        .effect_nodes
        .retain(|node| nodes.insert((node.source, node.index)));
    Ok(result)
}

/// Compose independent model owners without merging their bone namespaces.
pub(super) fn append(result: &mut Model, model: Model, label: &str) -> Result<(), String> {
    if result.vertices.len() + model.vertices.len() > MAX_VERTICES
        || result.triangles.len() + model.triangles.len() > MAX_TRIANGLES
    {
        return Err("This appearance exceeds the preview geometry budget".into());
    }
    let base = result.vertices.len() as u32;
    let range = base as usize..base as usize + model.vertices.len();
    for mut cloth in model.cloth {
        cloth.offset(base as usize);
        result.cloth.push(cloth);
    }
    if let Some(animation) = model.animation {
        result.rigs.push(animation::Rig {
            vertices: range,
            animation,
        });
    }
    for mut rig in model.rigs {
        rig.vertices = rig.vertices.start + base as usize..rig.vertices.end + base as usize;
        result.rigs.push(rig);
    }
    if let Some(notice) = model.animation_notice {
        result.notices.push(format!("{label}: {notice}"));
    }
    for mut clip in model.clips {
        if result.clips.iter().any(|other| other.tag == clip.tag) {
            continue;
        }
        clip.name = format!("{label}: {}", clip.name);
        result.clips.push(clip);
    }
    let texture_map: Vec<_> = model
        .textures
        .into_iter()
        .map(|texture| {
            if let Some(index) = result.textures.iter().position(|t| {
                t.tag == texture.tag
                    && t.size == texture.size
                    && t.mips.as_ref().map(Vec::len) == texture.mips.as_ref().map(Vec::len)
            }) {
                return Some(index);
            }
            if result.textures.len() >= MAX_TEXTURES {
                let notice = "The preview texture budget is full".to_owned();
                if !result.notices.contains(&notice) {
                    result.notices.push(notice);
                }
                return None;
            }
            match texture::retain(result, texture) {
                Ok(index) => Some(index),
                Err(error) => {
                    result.notices.push(error);
                    None
                }
            }
        })
        .collect();
    let first_triangle = result.triangles.len();
    let effect_base = result.effects.len();
    for mut material in model.effects {
        material.remap_textures(&texture_map);
        result.effects.push(material);
    }
    let part_triangles = model.triangles.len();
    result.triangle_detail_uv.resize(first_triangle, false);
    result.triangle_detail_uv.extend(
        (0..part_triangles).map(|i| model.triangle_detail_uv.get(i).copied().unwrap_or(false)),
    );
    result
        .triangles
        .extend(model.triangles.into_iter().map(|t| t.map(|v| v + base)));
    result.triangle_light.resize(first_triangle, false);
    result.triangle_light.extend(
        (0..part_triangles).map(|index| model.triangle_light.get(index).copied().unwrap_or(false)),
    );
    result.triangle_emitter.resize(first_triangle, false);
    result.triangle_emitter.extend(
        (0..part_triangles)
            .map(|index| model.triangle_emitter.get(index).copied().unwrap_or(false)),
    );
    for mut source in model.particle_sources {
        let Some(texture) = texture_map.get(source.texture).copied().flatten() else {
            continue;
        };
        source.texture = texture;
        source.gradient = source
            .gradient
            .and_then(|index| texture_map.get(index).copied().flatten());
        result.particle_sources.push(source);
    }
    result.particle_geometry |= model.particle_geometry;
    result.light_geometry |= model.light_geometry;
    result.triangle_textures.extend(
        model
            .triangle_textures
            .into_iter()
            .map(|t| t.and_then(|i| texture_map[i])),
    );
    result.triangle_dyes.extend(model.triangle_dyes);
    result.triangle_dye_maps.resize(first_triangle, None);
    result
        .triangle_dye_maps
        .extend((0..part_triangles).map(|index| {
            model
                .triangle_dye_maps
                .get(index)
                .copied()
                .flatten()
                .and_then(|mut map| {
                    map.texture = texture_map[map.texture]?;
                    Some(map)
                })
        }));
    result.triangle_clip.extend(model.triangle_clip);
    result.triangle_cutoff.resize(first_triangle, None);
    result
        .triangle_cutoff
        .extend((0..part_triangles).map(|i| model.triangle_cutoff.get(i).copied().flatten()));
    result.triangle_constant.extend(model.triangle_constant);
    result.triangle_effects.resize(first_triangle, None);
    result
        .triangle_effects
        .extend((0..part_triangles).map(|index| {
            model
                .triangle_effects
                .get(index)
                .copied()
                .flatten()
                .map(|index| effect_base + index)
        }));
    result.triangle_gearstacks.extend(
        model
            .triangle_gearstacks
            .into_iter()
            .map(|t| t.and_then(|i| texture_map[i])),
    );
    result.triangle_normals.extend(
        model
            .triangle_normals
            .into_iter()
            .map(|t| t.and_then(|i| texture_map[i])),
    );
    result.normals.extend(model.normals);
    result.weights.resize_with(base as usize, || None);
    result.weights.extend(model.weights);
    for mut motion in model.motions {
        motion.vertices =
            motion.vertices.start + base as usize..motion.vertices.end + base as usize;
        result.motions.push(motion);
    }
    result
        .tangents
        .resize(result.vertices.len(), [0.0, 0.0, 0.0, 1.0]);
    result.colors.resize(result.vertices.len(), [1.0; 4]);
    result.tangents.extend((0..model.vertices.len()).map(|i| {
        model
            .tangents
            .get(i)
            .copied()
            .unwrap_or([0.0, 0.0, 0.0, 1.0])
    }));
    result.colors.extend(
        (0..model.vertices.len()).map(|i| model.colors.get(i).copied().unwrap_or([1.0; 4])),
    );
    result.vertices.extend(model.vertices);
    result.uvs.extend(model.uvs);
    result.detail_uvs.resize(base as usize, [0.0; 2]);
    result.detail_uvs.extend(model.detail_uvs);
    result.tags.extend(model.tags);
    result.notices.extend(model.notices);
    result.assets.particles.extend(model.assets.particles);
    result.assets.sounds.extend(model.assets.sounds);
    result.assets.lights.extend(model.assets.lights);
    result.assets.children.extend(model.assets.children);
    result.assets.components.extend(model.assets.components);
    result.assets.effect_nodes.extend(model.assets.effect_nodes);
    result.assets.references.extend(model.assets.references);
    if result.assets.image.is_none() {
        result.assets.image = model.assets.image;
    }
    Ok(())
}

/// The dye keys of each gear type's armor, cloth and suit channels: armor, weapons, ships,
/// Sparrows and Ghost Shells.
const GEAR_KEYS: [[i8; 3]; 5] = [[0, 1, 2], [4, 5, 6], [7, 8, 9], [10, 11, 12], [13, 14, 15]];

/// The keys that paint this appearance. An item's own dyes name only its gear type's keys. A
/// shader names every gear type's, and a shader's dyes alone preview on a weapon.
fn gear_keys(dyes: &[(i8, u16)]) -> [i8; 3] {
    let named = GEAR_KEYS
        .iter()
        .filter(|keys| dyes.iter().any(|(key, _)| keys.contains(key)))
        .collect::<Vec<_>>();
    match named.as_slice() {
        [only] => **only,
        _ => GEAR_KEYS[1],
    }
}

fn apply_colors(
    manager: &PackageManager,
    globals: &[u8],
    appearance: &Appearance,
    model: &mut Model,
) -> Result<(), String> {
    if appearance.dyes.is_empty() {
        return Ok(());
    }
    let channels = checked(manager, u32_at(globals, 0x420)?, 0x8080_5BDE)?;
    let (count, _) = array(&channels, 8, 0x8080_5BE2, 4, 4096)?;
    let indices: Vec<_> = appearance.dyes.iter().map(|row| row.1).collect();
    let colors = crate::dyes::material::load(manager, &indices)?;
    let keys = gear_keys(&appearance.dyes);
    let mut slots = [None; 6];
    for &(channel, reference) in &appearance.dyes {
        if channel < 0 || channel as usize >= count {
            continue;
        }
        let Some(slot) = keys.iter().position(|&key| key == channel) else {
            continue;
        };
        model.dye_animations.retain(|(s, _)| *s != slot);
        match colors.get(&reference) {
            None => continue,
            Some(Ok(material)) => {
                let replaced = appearance
                    .dye_textures
                    .iter()
                    .find(|texture| texture.channel == slot);
                let mut load =
                    |replacement: Option<u32>, own: &Result<Option<u32>, String>| match replacement
                        .map(|tag| Ok(Some(tag)))
                        .unwrap_or_else(|| own.clone())
                    {
                        Ok(Some(tag)) => load_detail(manager, tag, model),
                        Ok(None) => None,
                        Err(error) => {
                            model.notices.push(error);
                            None
                        }
                    };
                let detail = load(
                    replaced.and_then(|texture| texture.detail),
                    &material.detail,
                );
                let normal = load(
                    replaced.and_then(|texture| texture.normal),
                    &material.normal,
                );
                match &material.animation {
                    Ok(Some(animation)) => model.dye_animations.push((slot, animation.clone())),
                    Ok(None) => {}
                    Err(error) => model.notices.push(format!(
                        "Dye {reference}: {error}. Showing its static material."
                    )),
                }
                // A part's dye index counts channel pairs: armor primary, armor secondary,
                // cloth primary, cloth secondary, suit primary, suit secondary.
                for index in 0..2 {
                    slots[slot * 2 + index] = Some(shader::Dye {
                        surface: material.surfaces[index],
                        detail,
                        normal,
                        normal_transform: material.normal_transform,
                        transform: material.detail_transform,
                        vectors: material.vectors,
                    });
                }
            }
            Some(Err(error)) => model.notices.push(format!("Dye {reference}: {error}")),
        }
    }
    model.dyes = slots;
    if model
        .triangle_gearstacks
        .iter()
        .enumerate()
        .any(|(index, mask)| {
            mask.is_none()
                && model
                    .triangle_effects
                    .get(index)
                    .copied()
                    .flatten()
                    .is_none_or(|e| model.effects[e].native.is_none())
        })
    {
        model
            .notices
            .push("Parts without material masks use a basic color preview.".into());
    }
    model.notices.push("Shader preview includes dye masks, detail color, wear, roughness, metal, emission, iridescence and supported transparent effects. Lighting is approximate. Gameplay-driven effects use their stored initial values.".into());
    Ok(())
}

fn load_detail(manager: &PackageManager, tag: u32, model: &mut Model) -> Option<usize> {
    if let Some(index) = model.textures.iter().position(|t| t.tag == tag) {
        return Some(index);
    }
    // Object effects must leave room for a color and normal image per gear dye channel.
    let loaded = if model.textures.len() >= MAX_TEXTURES + GEAR_KEYS[0].len() * 2 {
        Err("The preview texture budget is full".into())
    } else {
        texture::load_model(manager, tag, model)
    };
    match loaded {
        Ok(index) => Some(index),
        Err(error) => {
            model
                .notices
                .push(format!("Detail Texture 0x{tag:08X}: {error}"));
            None
        }
    }
}

fn assignment_hashes(table: &[u8], index: u16) -> Result<BTreeSet<u32>, String> {
    let (count, rows) = array(table, 8, 0x8080_5DFB, 0x20, 65536)?;
    if usize::from(index) >= count {
        return Err("Appearance row is outside the installed table".into());
    }
    let row = rows + usize::from(index) * 0x20;
    let mut hashes = BTreeSet::new();
    if u64_at(table, row + 0x10)? == 0 {
        hashes.insert(u32_at(table, row + 8)?);
        hashes.insert(u32_at(table, row + 12)?);
    } else {
        let (count, rows) = array(table, row + 0x10, 0x8080_5DFE, 8, 4096)?;
        for index in 0..count {
            let resource = pointer(table, rows + index * 8)?;
            let (count, rows) = array(table, resource + 8, 0x8080_5E01, 4, 65536)?;
            // Each region lists alternatives, not simultaneous geometry. Use its base
            // attachment until socket-driven region selection is supported.
            if count != 0 {
                hashes.insert(u32_at(table, rows)?);
            }
        }
    }
    hashes.remove(&0);
    hashes.remove(&u32::MAX);
    hashes.remove(&0x811C_9DC5);
    Ok(hashes)
}

#[cfg(test)]
mod tests;
