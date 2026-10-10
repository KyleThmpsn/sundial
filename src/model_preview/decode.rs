use super::*;
mod owner;
pub(super) use owner::{Owner, empty, finish, owners, specialized};

mod cloth;

pub(super) fn append(
    manager: &PackageManager,
    tag: u32,
    component: Option<&[u8]>,
    inputs: &Result<Vec<effects::ObjectInput>, String>,
    model: &mut Model,
    simulate_cloth: bool,
) -> Result<Option<super::cloth::Pending>, String> {
    let vertices = model.vertices.len();
    let triangles = model.triangles.len();
    let textures = model.textures.len();
    let effects = model.effects.len();
    let motions = model.motions.len();
    let result = (|| {
        let layouts = vertex::Layouts::read(manager)?;
        let mut pending = append_stages(
            manager,
            tag,
            component,
            inputs,
            model,
            &layouts,
            false,
            simulate_cloth,
        )?;
        if model.triangles.len() == triangles {
            pending = append_stages(
                manager,
                tag,
                component,
                inputs,
                model,
                &layouts,
                true,
                simulate_cloth,
            )?;
        }
        Ok(pending)
    })();
    if result.is_err() {
        model.vertices.truncate(vertices);
        model.normals.truncate(vertices);
        model.tangents.truncate(vertices);
        model.colors.truncate(vertices);
        model.uvs.truncate(vertices);
        model.detail_uvs.truncate(vertices);
        model.triangle_detail_uv.truncate(triangles);
        model.weights.truncate(vertices);
        model.triangles.truncate(triangles);
        model.triangle_textures.truncate(triangles);
        model.triangle_dyes.truncate(triangles);
        model.triangle_dye_maps.truncate(triangles);
        model.triangle_clip.truncate(triangles);
        model.triangle_cutoff.truncate(triangles);
        model.triangle_constant.truncate(triangles);
        model.triangle_effects.truncate(triangles);
        model.effects.truncate(effects);
        model.motions.truncate(motions);
        model.triangle_gearstacks.truncate(triangles);
        model.triangle_normals.truncate(triangles);
        model.textures.truncate(textures);
    }
    result
}

/// A part's detail flags, mesh row, part row and render stage.
type StagedPart = (u8, usize, usize, Option<usize>);

/// Every part of every mesh.
fn staged_parts(bytes: &[u8]) -> Result<Vec<StagedPart>, String> {
    let (count, rows) = array(bytes, 0x10, 0x8080_7378, 0x88, 1024)?;
    let mut parts = Vec::new();
    for mesh in 0..count {
        let mesh = rows + mesh * 0x88;
        let (count, rows) = array(bytes, mesh + 0x18, 0x8080_737E, 0x20, 65536)?;
        // Parts are listed by render stage; the table at 0x28 holds each stage's first part.
        // Shadow and depth stages are auxiliary. Transparent stages have their own material
        // programs and may intentionally draw the same geometry after the gbuffer.
        let ranges = (0..24)
            .map(|stage| bytes_at(bytes, mesh + 0x28 + stage * 2).map(i16::from_le_bytes))
            .collect::<Result<Vec<_>, _>>()?;
        for part in 0..count {
            let stage = (0..23).find(|&stage| {
                let (start, end) = (ranges[stage], ranges[stage + 1]);
                start >= 0 && end >= start && part >= start as usize && part < end as usize
            });
            let part = rows + part * 0x20;
            parts.push((bytes[part + 0x1B], mesh, part, stage));
        }
    }
    Ok(parts)
}

/// The most detailed level any part draws at, noted on the model when it is not the first.
/// None when no part draws, which only the lenient pass refuses.
fn detail_level(
    parts: &[StagedPart],
    lenient: bool,
    model: &mut Model,
) -> Result<Option<u8>, String> {
    let level = (0..4).find(|level| {
        parts
            .iter()
            .any(|p| lod_visible(p.0, *level) && (lenient || stage_drawn(p.3)))
    });
    match level {
        None if lenient => Err("This model has no supported detail level".into()),
        None => Ok(None),
        Some(level) => {
            if level != 0 {
                model.notices.push(format!(
                    "Showing detail level {level}. Higher-detail geometry is absent."
                ));
            }
            Ok(Some(level))
        }
    }
}

fn position_scale(bytes: &[u8]) -> Result<f32, String> {
    let scale = f32::from_bits(u32_at(bytes, 0x6C)?);
    if !scale.is_finite() || scale < 0.0 {
        return Err("Invalid model position scale".into());
    }
    Ok(scale)
}

#[allow(clippy::too_many_arguments)]
fn append_stages(
    manager: &PackageManager,
    tag: u32,
    component: Option<&[u8]>,
    inputs: &Result<Vec<effects::ObjectInput>, String>,
    model: &mut Model,
    layouts: &vertex::Layouts,
    lenient: bool,
    simulate_cloth: bool,
) -> Result<Option<super::cloth::Pending>, String> {
    let bytes = checked(manager, tag, MODEL)?;
    let scale = position_scale(&bytes)?;
    // Marker-only companion models retain buffers but collapse every triangle in game.
    if scale == 0.0 {
        return Ok(None);
    }
    let scale = [scale; 3];
    let translation = vector(&bytes, 0x60)?;
    let model_uv = [
        u32_at(&bytes, 0x70)?,
        u32_at(&bytes, 0x74)?,
        u32_at(&bytes, 0x78)?,
        u32_at(&bytes, 0x7C)?,
    ]
    .map(f32::from_bits);
    let (parts, mut pending) = cloth::select(
        manager,
        component,
        &bytes,
        staged_parts(&bytes)?,
        simulate_cloth,
        model,
    )?;
    let Some(level) = detail_level(&parts, lenient, model)? else {
        return Ok(None);
    };
    let mut drawn = BTreeSet::new();
    let mut materials = std::collections::BTreeMap::new();
    let mut gear_materials = std::collections::BTreeMap::new();
    let mut attributes = std::collections::BTreeMap::new();
    let mut cutoffs = std::collections::BTreeMap::new();
    let mut effects = std::collections::BTreeMap::new();
    let mut opaque_materials = std::collections::BTreeMap::new();
    let mut normal_materials = std::collections::BTreeMap::new();
    let mut surface_materials = std::collections::BTreeMap::new();
    let mut loaded = std::collections::BTreeMap::new();
    let plate = optional_texture(
        component
            .map(|c| texture::albedo(manager, c, model))
            .transpose(),
        model,
        "Gear Texture",
    );
    let gearstack = optional_texture(
        component
            .map(|c| texture::gearstack(manager, c, model))
            .transpose(),
        model,
        "Gear Material Mask",
    );
    let normal = optional_texture(
        component
            .map(|c| texture::normal(manager, c, model))
            .transpose(),
        model,
        "Normal Map",
    );
    for (_, mesh, part, stage) in parts
        .into_iter()
        .filter(|p| lod_visible(p.0, level) && (lenient || stage_drawn(p.3)))
    {
        let offset = u32_at(&bytes, part + 8)? as usize;
        let count = u32_at(&bytes, part + 12)? as usize;
        if count == 0 {
            continue;
        }
        let primitive = u16_at(&bytes, part + 6)?;
        let layout = u16_at(&bytes, mesh + 0x58 + stage.unwrap_or(0) * 2)?;
        let variant = i16::from_le_bytes(bytes_at(&bytes, part + 4)?);
        let material = if variant >= 0 {
            component
                .ok_or_else(|| "This model needs its parent component's material table".to_owned())
                .and_then(|component| external_material(component, variant as usize))
        } else {
            u32_at(&bytes, part)
        };
        let mut effect = if stage == Some(7) {
            let key = (material.clone(), bytes[part + 0x1A]);
            Some(*effects.entry(key).or_insert_with(|| {
                let decoded = material.as_ref().map_err(Clone::clone).and_then(|&tag| {
                    let mut decoded = effects::load(
                        manager,
                        tag,
                        inputs.as_ref().map(Vec::as_slice).map_err(String::as_str),
                        bytes[part + 0x1A],
                        model,
                    )?;
                    if let Some(native) = &mut decoded.native {
                        native.remap_stored_uv(model_uv)?;
                    }
                    Ok(decoded)
                });
                let decoded = decoded.unwrap_or_else(|error| {
                    model.notices.push(format!(
                        "Effect material: {error}. Its shape is available in Solid and Wireframe."
                    ));
                    effects::Material::default()
                });
                let index = model.effects.len();
                model.effects.push(decoded);
                index
            }))
        } else {
            None
        };
        effect = effect.or_else(|| {
            material
                .as_ref()
                .ok()
                .filter(|_| matches!(stage, Some(0 | 1 | 2 | 6)))
                .and_then(|&tag| {
                    *surface_materials
                        .entry((tag, bytes[part + 0x1A]))
                        .or_insert_with(|| {
                            surface(manager, tag, inputs, bytes[part + 0x1A], model_uv, model)
                        })
                })
        });
        let dyed = bytes[part + 0x1A] < 6 && plate.is_some();
        if !drawn.insert((
            mesh,
            layout,
            offset,
            count,
            primitive,
            stage,
            material.clone(),
        )) {
            continue;
        }
        let motion = if stage_drawn(stage) {
            material.as_ref().ok().and_then(|tag| {
                match effects::native::load_motion(
                    manager,
                    *tag,
                    inputs.as_ref().map(Vec::as_slice).map_err(String::as_str),
                ) {
                    Ok(motion) => motion,
                    Err(error) => {
                        model.notices.push(format!("Vertex motion: {error}"));
                        None
                    }
                }
            })
        } else {
            None
        };
        let cutoff = material.as_ref().ok().and_then(|tag| {
            *cutoffs.entry(*tag).or_insert_with(|| {
                match effects::native::load_cutoff(manager, *tag) {
                    Ok(cutoff) => cutoff,
                    Err(error) => {
                        model.notices.push(format!("Material coverage: {error}"));
                        None
                    }
                }
            })
        });
        let gear = explicit_gear(
            manager,
            &material,
            model_uv,
            (stage == Some(0) && bytes[part + 0x1A] < 6 && effect.is_none())
                || (cutoff.is_some() && matches!(stage, Some(1 | 2 | 6))),
            &mut gear_materials,
            model,
        );
        let uv = gear.and_then(|g| g.uv).unwrap_or(model_uv);
        if effect.is_none()
            && let Some(gear) = gear
            && let Some(map) = gear.map
        {
            let local_uv = map.transform.map(f32::from_bits);
            let key = (material.clone(), bytes[part + 0x1A], map.transform);
            effect = *opaque_materials.entry(key).or_insert_with(|| {
                let decoded = material.as_ref().ok().and_then(|&tag| {
                    match effects::native::load_opaque(
                        manager,
                        tag,
                        inputs.as_ref().map(Vec::as_slice).map_err(String::as_str),
                        bytes[part + 0x1A],
                        local_uv,
                        model,
                    ) {
                        Ok(material) => material,
                        Err(error) => {
                            model.notices.push(format!("Opaque color: {error}"));
                            None
                        }
                    }
                })?;
                let index = model.effects.len();
                model.effects.push(decoded);
                Some(index)
            });
        }
        effect = effect.or_else(|| {
            let tag = material
                .as_ref()
                .ok()
                .copied()
                .filter(|_| stage == Some(0) && (dyed || gear.is_some()));
            normal_material(
                manager,
                tag.map(|tag| (tag, bytes[part + 0x1A])),
                inputs,
                &mut normal_materials,
                model,
            )
        });
        let attributes = material.as_ref().ok().and_then(|tag| {
            attributes
                .entry(*tag)
                .or_insert_with(|| match effects::native::load_attributes(manager, *tag) {
                    Ok(attributes) => attributes,
                    Err(error) => {
                        model
                            .notices
                            .push(format!("Stored vertex attributes: {error}"));
                        None
                    }
                })
                .as_ref()
        });
        // Material-local geometry and atlas equations can differ on shared stored vertices.
        let key = (
            mesh,
            layout,
            (motion.is_some() || attributes.is_some())
                .then(|| material.as_ref().ok().copied())
                .flatten(),
            uv.map(f32::to_bits),
            offset,
            count,
            primitive,
        );
        if let std::collections::btree_map::Entry::Vacant(slot) = loaded.entry(key) {
            let read = read_mesh(
                manager,
                &bytes,
                mesh,
                layout,
                layouts,
                scale,
                translation,
                uv,
                (offset, count, primitive),
                attributes,
                model,
            )?;
            pending
                .as_mut()
                .map(|pending| pending.bind(read.0, &read.4))
                .transpose()?;
            if let Some(mut motion) = motion {
                motion.vertices = read.0..model.vertices.len();
                model.motions.push(motion);
            }
            slot.insert(read);
        }
        let (base, triangles, has_uv, native_detail, _) = &loaded[&key];
        let gear = gear.filter(|_| *has_uv);
        let map = bounded_map(gear.and_then(|g| g.map), triangles.len(), model);
        let passes = if map.is_some() { 6 } else { 1 };
        if model.triangles.len() + triangles.len() * passes > MAX_TRIANGLES {
            return Err("This model exceeds the preview triangle budget.".into());
        }
        let procedural = effect.is_some_and(|index| {
            matches!(
                model.effects[index].kind,
                effects::Kind::Gradient | effects::Kind::SoftGradient | effects::Kind::Unavailable
            )
        });
        let texture = if procedural || !*has_uv {
            None
        } else if let Some(gear) = gear {
            Some(gear.albedo)
        } else if dyed {
            plate
        } else {
            part_texture(manager, material, &mut materials, model)
        };
        append_part(
            model,
            triangles,
            *base as u32,
            passes,
            Part {
                native_detail: *native_detail,
                texture,
                map,
                dye: bytes[part + 0x1A],
                effect,
                // Decal-stage parts are cut out by the gearstack blue channel.
                clip: cutoff.is_some() || matches!(stage, Some(1 | 2 | 6)),
                cutoff,
                mask: gear.map(|g| g.mask).or_else(|| {
                    gearstack.filter(|_| *has_uv && plate.is_some() && texture == plate)
                }),
                normal: gear
                    .map(|g| g.normal)
                    .or_else(|| normal.filter(|_| *has_uv && plate.is_some() && texture == plate)),
            },
        );
    }
    Ok(pending)
}

fn surface(
    manager: &PackageManager,
    tag: u32,
    inputs: &Result<Vec<effects::ObjectInput>, String>,
    dye: u8,
    model_uv: [f32; 4],
    model: &mut Model,
) -> Option<usize> {
    let decoded = match effects::native::load_surface(
        manager,
        tag,
        inputs.as_ref().map(Vec::as_slice).map_err(String::as_str),
        dye,
        model_uv,
        model,
    ) {
        Ok(material) => material?,
        Err(error) => {
            model.notices.push(format!("Surface material: {error}"));
            return None;
        }
    };
    let index = model.effects.len();
    model.effects.push(decoded);
    Some(index)
}

fn normal_material(
    manager: &PackageManager,
    key: Option<(u32, u8)>,
    inputs: &Result<Vec<effects::ObjectInput>, String>,
    materials: &mut std::collections::BTreeMap<(u32, u8), Option<usize>>,
    model: &mut Model,
) -> Option<usize> {
    let (tag, surface) = key?;
    *materials.entry((tag, surface)).or_insert_with(|| {
        let decoded = match effects::native::load_normal(
            manager,
            tag,
            inputs.as_ref().map(Vec::as_slice).map_err(String::as_str),
            surface,
        ) {
            Ok(material) => material?,
            Err(error) => {
                model.notices.push(format!("Surface normal: {error}"));
                return None;
            }
        };
        let index = model.effects.len();
        model.effects.push(decoded);
        Some(index)
    })
}

fn part_texture(
    manager: &PackageManager,
    material: Result<u32, String>,
    cache: &mut std::collections::BTreeMap<Result<u32, String>, Option<usize>>,
    model: &mut Model,
) -> Option<usize> {
    *cache.entry(material.clone()).or_insert_with(|| {
        match material.and_then(|tag| texture::material(manager, tag, model)) {
            Ok(index) => Some(index),
            Err(error) => {
                model.notices.push(error);
                None
            }
        }
    })
}

struct Part {
    native_detail: bool,
    texture: Option<usize>,
    map: Option<texture::DyeMap>,
    dye: u8,
    effect: Option<usize>,
    clip: bool,
    cutoff: Option<f32>,
    mask: Option<usize>,
    normal: Option<usize>,
}

fn append_part(model: &mut Model, triangles: &[[u32; 3]], base: u32, passes: usize, part: Part) {
    for pass in 0..passes {
        let count = triangles.len();
        model.triangle_dye_maps.resize(model.triangles.len(), None);
        model
            .triangle_dye_maps
            .extend(std::iter::repeat_n(part.map, count));
        model
            .triangle_textures
            .extend(std::iter::repeat_n(part.texture, count));
        model.triangle_dyes.extend(std::iter::repeat_n(
            if part.map.is_some() {
                pass as u8
            } else {
                part.dye
            },
            count,
        ));
        model
            .triangle_constant
            .extend(std::iter::repeat_n(None, count));
        model.triangle_effects.resize(model.triangles.len(), None);
        model
            .triangle_effects
            .extend(std::iter::repeat_n(part.effect, count));
        model
            .triangle_clip
            .extend(std::iter::repeat_n(part.clip, count));
        model.triangle_cutoff.resize(model.triangles.len(), None);
        model
            .triangle_cutoff
            .extend(std::iter::repeat_n(part.cutoff, count));
        model
            .triangle_gearstacks
            .extend(std::iter::repeat_n(part.mask, count));
        model
            .triangle_normals
            .extend(std::iter::repeat_n(part.normal, count));
        model
            .triangle_detail_uv
            .resize(model.triangles.len(), false);
        model
            .triangle_detail_uv
            .extend(std::iter::repeat_n(part.native_detail, count));
        model
            .triangles
            .extend(triangles.iter().map(|tri| tri.map(|v| v + base)));
    }
}

fn explicit_gear(
    manager: &PackageManager,
    material: &Result<u32, String>,
    model_uv: [f32; 4],
    allowed: bool,
    cache: &mut std::collections::BTreeMap<u32, Option<texture::Gear>>,
    model: &mut Model,
) -> Option<texture::Gear> {
    if !allowed {
        return None;
    }
    let &tag = material.as_ref().ok()?;
    *cache
        .entry(tag)
        .or_insert_with(|| match texture::gear(manager, tag, model_uv, model) {
            Ok(gear) => gear,
            Err(error) => {
                model.notices.push(format!("Gear material: {error}"));
                None
            }
        })
}

fn bounded_map(
    map: Option<texture::DyeMap>,
    triangles: usize,
    model: &mut Model,
) -> Option<texture::DyeMap> {
    map.filter(|_| {
        if model.triangles.len() + triangles * 6 <= MAX_TRIANGLES {
            return true;
        }
        model.notices.push(
            "Per-pixel dyes exceed the preview triangle budget. Showing the part's base dye."
                .into(),
        );
        false
    })
}

fn optional_texture(
    result: Result<Option<Option<usize>>, String>,
    model: &mut Model,
    label: &str,
) -> Option<usize> {
    match result {
        Ok(index) => index.flatten(),
        Err(error) => {
            model.notices.push(format!("{label}: {error}"));
            None
        }
    }
}

/// Gbuffer, decals, investment decals, additive decals and transparent material passes.
fn stage_drawn(stage: Option<usize>) -> bool {
    matches!(stage, Some(0 | 1 | 2 | 6 | 7) | None)
}

fn lod_visible(category: u8, level: u8) -> bool {
    // Bungie's LOD categories are coverage sets (01, 012, ...), not a sorted rank.
    const MASKS: [u8; 11] = [1, 3, 7, 15, 2, 6, 14, 4, 12, 8, 1];
    MASKS
        .get(category as usize)
        .is_some_and(|mask| mask & (1 << level) != 0)
}

fn external_material(component: &[u8], variant: usize) -> Result<u32, String> {
    let data = pointer(component, 0x18)?;
    let (count, rows) = array(component, data + 0x2D0, 0x8080_72C4, 12, 65536)?;
    if variant >= count {
        return Err("The model material variant is outside its table".into());
    }
    let row = rows + variant * 12;
    let count = u32_at(component, row)? as usize;
    let start = u32_at(component, row + 4)? as usize;
    let (total, rows) = array(component, data + 0x310, 0x8080_0014, 4, 1 << 20)?;
    if count == 0 || start.checked_add(count).is_none_or(|end| end > total) {
        return Err("The model material range is invalid".into());
    }
    u32_at(component, rows + start * 4)
}

type MeshBuffers = (usize, Vec<[u32; 3]>, bool, bool, Vec<u32>);

#[allow(clippy::too_many_arguments)]
fn read_mesh(
    manager: &PackageManager,
    bytes: &[u8],
    mesh: usize,
    layout: u16,
    layouts: &vertex::Layouts,
    scale: [f32; 3],
    translation: [f32; 3],
    uv: [f32; 4],
    draw: (usize, usize, u16),
    attributes: Option<&effects::native::Attributes>,
    model: &mut Model,
) -> Result<MeshBuffers, String> {
    let tags = [
        u32_at(bytes, mesh)?,
        u32_at(bytes, mesh + 4)?,
        u32_at(bytes, mesh + 8)?,
        u32_at(bytes, mesh + 12)?,
    ];
    let mut decoded = layouts.read_vertices(manager, layout, tags, MAX_VERTICES)?;
    let (width, indices) = vertex::indices(manager, u32_at(bytes, mesh + 0x10)?)?;
    let mut triangles = triangles(
        &indices,
        width,
        draw.0,
        draw.1,
        draw.2,
        decoded.positions.len(),
    )?;
    let selected: Vec<_> = triangles
        .iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if selected.len() > MAX_VERTICES - model.vertices.len() {
        return Err("This model exceeds the preview vertex budget.".into());
    }
    // Material-local attributes and motion require separate vertices, but only for the
    // actual draw. Keep native SV_VertexID before remapping these sparse indices.
    decoded.select(&selected);
    for triangle in &mut triangles {
        for index in triangle {
            *index = selected.binary_search(index).expect("selected draw vertex") as u32;
        }
    }
    let applied =
        attributes.and_then(
            |attributes| match attributes.apply(&mut decoded, &selected) {
                Ok(()) => Some(attributes),
                Err(error) => {
                    decoded
                        .notices
                        .push(format!("Stored vertex attributes: {error}"));
                    None
                }
            },
        );
    let detail = applied
        .filter(|a| a.has_detail())
        .map(|_| decoded.detail_uvs.clone());
    decoded.transform(scale, translation, uv)?;
    let native_detail = detail.is_some() || applied.is_some_and(|a| a.scaled_detail());
    if let Some(detail) = detail {
        decoded.detail_uvs = detail;
    }
    let base = model.vertices.len();
    let has_uv = decoded.has_uv;
    model
        .vertices
        .extend(decoded.positions.into_iter().map(|p| [p[0], p[1], p[2]]));
    model.uvs.extend(decoded.uvs);
    model.detail_uvs.resize(base, [0.0; 2]);
    model.detail_uvs.extend(decoded.detail_uvs);
    model.normals.extend(decoded.normals);
    model.tangents.resize(
        model.vertices.len() - decoded.tangents.len(),
        [0.0, 0.0, 0.0, 1.0],
    );
    model
        .colors
        .resize(model.vertices.len() - decoded.colors.len(), [1.0; 4]);
    model.tangents.extend(decoded.tangents);
    model.colors.extend(decoded.colors);
    model.weights.extend(decoded.weights);
    for notice in decoded.notices {
        if !model.notices.contains(&notice) {
            model.notices.push(notice);
        }
    }
    Ok((base, triangles, has_uv, native_detail, selected))
}

fn vector(bytes: &[u8], offset: usize) -> Result<[f32; 3], String> {
    let values = [
        f32::from_bits(u32_at(bytes, offset)?),
        f32::from_bits(u32_at(bytes, offset + 4)?),
        f32::from_bits(u32_at(bytes, offset + 8)?),
    ];
    if values.iter().any(|v| !v.is_finite()) {
        return Err("Invalid model transform".into());
    }
    Ok(values)
}

pub(super) fn triangles(
    bytes: &[u8],
    width: usize,
    offset: usize,
    count: usize,
    primitive: u16,
    vertices: usize,
) -> Result<Vec<[u32; 3]>, String> {
    if !matches!(width, 2 | 4) || count > MAX_TRIANGLES * 3 {
        return Err("Invalid model index count".into());
    }
    let start = offset.checked_mul(width).ok_or("Index offset overflow")?;
    let end = count
        .checked_mul(width)
        .and_then(|n| start.checked_add(n))
        .ok_or("Index range overflow")?;
    let rows = bytes
        .get(start..end)
        .ok_or("Model indices exceed their buffer")?;
    let indices = rows
        .chunks_exact(width)
        .map(|row| {
            if width == 2 {
                u16_at(row, 0).map(u32::from)
            } else {
                u32_at(row, 0)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = Vec::new();
    match primitive {
        3 if count.is_multiple_of(3) => {
            for row in indices.chunks_exact(3) {
                result.push([row[0], row[1], row[2]]);
            }
        }
        5 => {
            let sentinel = if width == 2 {
                u16::MAX as u32
            } else {
                u32::MAX
            };
            for strip in indices.split(|v| *v == sentinel) {
                for (index, row) in strip.windows(3).enumerate() {
                    result.push(if index % 2 == 0 {
                        [row[0], row[1], row[2]]
                    } else {
                        [row[1], row[0], row[2]]
                    });
                }
            }
        }
        _ => return Err(format!("Unsupported model primitive {primitive}")),
    }
    if result.iter().flatten().any(|&v| v as usize >= vertices) {
        return Err("Model triangle references a missing vertex".into());
    }
    result.retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lod_visibility_respects_shared_and_exclusive_categories() {
        let categories = [4, 6, 8, 9];
        let level = (0..4)
            .find(|&l| categories.iter().any(|&c| lod_visible(c, l)))
            .unwrap();
        assert_eq!(level, 1);
        assert_eq!(
            categories
                .into_iter()
                .filter(|&c| lod_visible(c, level))
                .collect::<Vec<_>>(),
            [4, 6]
        );
        assert!(!lod_visible(255, 0));
    }
}
