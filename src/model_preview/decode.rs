use super::*;

pub(super) fn append(
    manager: &PackageManager,
    tag: u32,
    component: Option<&[u8]>,
    model: &mut Model,
) -> Result<(), String> {
    let before = model.triangles.len();
    append_stages(manager, tag, component, model, false)?;
    if model.triangles.len() == before {
        // Every part sat in a stage the strict pass skips: transparent-only effects such as
        // seeker projectiles. Their own materials drawn opaque beat an empty preview.
        append_stages(manager, tag, component, model, true)?;
    }
    Ok(())
}

#[expect(
    clippy::cognitive_complexity,
    reason = "One pass over the part list keeps stage, LOD and material decisions together"
)]
fn append_stages(
    manager: &PackageManager,
    tag: u32,
    component: Option<&[u8]>,
    model: &mut Model,
    lenient: bool,
) -> Result<(), String> {
    let bytes = checked(manager, tag, MODEL)?;
    let scale = vector(&bytes, 0x50)?;
    let translation = vector(&bytes, 0x60)?;
    let (count, rows) = array(&bytes, 0x10, 0x8080_7378, 0x88, 1024)?;
    let mut parts = Vec::new();
    for mesh in 0..count {
        let mesh = rows + mesh * 0x88;
        let (count, rows) = array(&bytes, mesh + 0x18, 0x8080_737E, 0x20, 65536)?;
        // Parts are listed by render stage; the table at 0x28 holds each stage's first part.
        // Only the gbuffer stage and the decal stages are drawn. Shadow, depth prepass and
        // transparent stages repeat geometry with their own materials and dye indices.
        let ranges = (0..24)
            .map(|stage| bytes_at(&bytes, mesh + 0x28 + stage * 2).map(i16::from_le_bytes))
            .collect::<Result<Vec<_>, _>>()?;
        for part in 0..count {
            let stage = (0..23).find(|&stage| {
                let (start, end) = (ranges[stage], ranges[stage + 1]);
                start >= 0 && end >= start && (part as i16) >= start && (part as i16) < end
            });
            let part = rows + part * 0x20;
            parts.push((bytes[part + 0x1B], mesh, part, stage));
        }
    }
    let level = (0..4)
        .find(|level| {
            parts
                .iter()
                .any(|p| lod_visible(p.0, *level) && (lenient || stage_drawn(p.3)))
        })
        .ok_or("This model has no supported detail level")?;
    if level != 0 {
        model.notices.push(format!(
            "Showing detail level {level}. Higher-detail geometry is absent."
        ));
    }
    let mut drawn = BTreeSet::new();
    let mut materials = std::collections::BTreeMap::new();
    let mut loaded = std::collections::BTreeMap::new();
    let plate = match component
        .map(|c| texture::albedo(manager, c, model))
        .transpose()
    {
        Ok(index) => index.flatten(),
        Err(error) => {
            model.notices.push(format!("Gear Texture: {error}"));
            None
        }
    };
    let gearstack = match component
        .map(|c| texture::gearstack(manager, c, model))
        .transpose()
    {
        Ok(index) => index.flatten(),
        Err(error) => {
            model.notices.push(format!("Gear Material Mask: {error}"));
            None
        }
    };
    let normal = match component
        .map(|c| texture::normal(manager, c, model))
        .transpose()
    {
        Ok(index) => index.flatten(),
        Err(error) => {
            model.notices.push(format!("Normal Map: {error}"));
            None
        }
    };
    for (_, mesh, part, stage) in parts
        .into_iter()
        .filter(|p| lod_visible(p.0, level) && (lenient || stage_drawn(p.3)))
    {
        let offset = u32_at(&bytes, part + 8)? as usize;
        let count = u32_at(&bytes, part + 12)? as usize;
        let primitive = u16_at(&bytes, part + 6)?;
        if !drawn.insert((mesh, offset, count, primitive)) {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(slot) = loaded.entry(mesh) {
            slot.insert(read_mesh(manager, &bytes, mesh, scale, translation, model)?);
        }
        let (base, vertices, width, indices, has_uv) = &loaded[&mesh];
        let triangles = triangles(indices, *width, offset, count, primitive, *vertices)?;
        if model.triangles.len() + triangles.len() > MAX_TRIANGLES {
            return Err("This model exceeds the preview triangle budget.".into());
        }
        let variant = i16::from_le_bytes(bytes_at(&bytes, part + 4)?);
        let material = if variant >= 0 {
            component
                .ok_or_else(|| "This model needs its parent component's material table".to_owned())
                .and_then(|component| external_material(component, variant as usize))
        } else {
            u32_at(&bytes, part)
        };
        // Transparent-stage parts with no textures are flat emissive panels, coloured by the
        // second pixel constant (Better Devils' cylinder screens: white at 3.0). Other
        // transparents need their own shader and are skipped.
        let constant = if stage == Some(7) {
            match material
                .as_ref()
                .ok()
                .and_then(|&tag| constant_emissive(manager, tag))
            {
                Some(colour) => Some(colour),
                None if lenient => None,
                None => continue,
            }
        } else {
            None
        };
        let texture = if *has_uv && bytes[part + 0x1A] < 6 && plate.is_some() {
            plate
        } else if *has_uv {
            *materials.entry(material.clone()).or_insert_with(|| {
                match material.and_then(|tag| super::texture::material(manager, tag, model)) {
                    Ok(index) => Some(index),
                    Err(error) => {
                        model.notices.push(error);
                        None
                    }
                }
            })
        } else {
            None
        };
        model
            .triangle_textures
            .extend(std::iter::repeat_n(texture, triangles.len()));
        model
            .triangle_dyes
            .extend(std::iter::repeat_n(bytes[part + 0x1A], triangles.len()));
        model
            .triangle_constant
            .extend(std::iter::repeat_n(constant, triangles.len()));
        // Decal-stage parts are cut out by the gearstack blue channel.
        let clip = matches!(stage, Some(1 | 2 | 6));
        model
            .triangle_clip
            .extend(std::iter::repeat_n(clip, triangles.len()));
        model.triangle_gearstacks.extend(std::iter::repeat_n(
            gearstack.filter(|_| *has_uv && plate.is_some() && texture == plate),
            triangles.len(),
        ));
        model.triangle_normals.extend(std::iter::repeat_n(
            normal.filter(|_| *has_uv && plate.is_some() && texture == plate),
            triangles.len(),
        ));
        model.triangles.extend(
            triangles
                .into_iter()
                .map(|tri| tri.map(|v| v + *base as u32)),
        );
    }
    Ok(())
}

/// A textureless material's colour constant, for flat emissive panels.
fn constant_emissive(manager: &PackageManager, tag: u32) -> Option<[f32; 3]> {
    let bytes = checked(manager, tag, 0x8080_71E8).ok()?;
    // Pixel stage at 0x2C8: texture bindings at +0x8, constants at +0x50.
    if u64_at(&bytes, 0x2D0).ok()? != 0 {
        return None;
    }
    let (count, _, rows, _) = native_array_at(&bytes, 0x2C8 + 0x50).ok()?;
    if count < 2 || rows + count * 16 > bytes.len() {
        return None;
    }
    let colour: [f32; 3] =
        std::array::from_fn(|i| f32::from_bits(u32_at(&bytes, rows + 16 + i * 4).unwrap_or(0)));
    (colour.iter().all(|v| v.is_finite()) && colour.iter().any(|&v| v > 0.0)).then_some(colour)
}

/// Gbuffer (0), decals (1), investment decals (2), additive decals (6) and the flat emissive
/// subset of transparents (7).
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

type MeshBuffers = (usize, usize, usize, Vec<u8>, bool);

fn read_mesh(
    manager: &PackageManager,
    bytes: &[u8],
    mesh: usize,
    scale: [f32; 3],
    translation: [f32; 3],
    model: &mut Model,
) -> Result<MeshBuffers, String> {
    let vertex = buffer(manager, u32_at(bytes, mesh)?, 4)?;
    let stride = usize::from(u16_at(&vertex.0, 4)?);
    if !matches!(stride, 8 | 12 | 16 | 28 | 32)
        || vertex.1.len() % stride != 0
        || u32_at(&vertex.0, 0)? as usize != vertex.1.len()
    {
        return Err("Unsupported model position buffer layout".into());
    }
    let base = model.vertices.len();
    let uv = read_uvs(manager, bytes, mesh, &vertex, stride);
    let has_uv = uv.is_ok();
    match uv {
        Ok(uv) => model.uvs.extend(uv),
        Err(error) => {
            model.uvs.resize(base + vertex.1.len() / stride, [0.0; 2]);
            if !model.notices.contains(&error) {
                model.notices.push(error);
            }
        }
    }
    if base + vertex.1.len() / stride > MAX_VERTICES {
        return Err("This model exceeds the preview vertex budget.".into());
    }
    for row in vertex.1.chunks_exact(stride) {
        let mut position = [0.0; 3];
        for axis in 0..3 {
            let packed = i16::from_le_bytes(bytes_at(row, axis * 2)?);
            position[axis] =
                (f32::from(packed) / 32767.0).max(-1.0) * scale[axis] + translation[axis];
        }
        if position.iter().any(|v| !v.is_finite()) {
            return Err("The model contains invalid positions".into());
        }
        model.vertices.push(position);
        model
            .weights
            .push((stride == 16).then(|| animation::Weights {
                values: row[8..12].try_into().unwrap(),
                bones: row[12..16].try_into().unwrap(),
            }));
    }
    match read_normals(manager, bytes, mesh, &vertex, stride, scale) {
        Ok(normals) => model.normals.extend(normals),
        Err(_) => model.normals.resize(model.vertices.len(), [0.0; 3]),
    }
    let index = buffer(manager, u32_at(bytes, mesh + 0x10)?, 6)?;
    let width = if bool_at(&index.0, 1)? { 4 } else { 2 };
    if u64_at(&index.0, 8)? != index.1.len() as u64 {
        return Err("The model index buffer has an invalid size".into());
    }
    Ok((base, vertex.1.len() / stride, width, index.1, has_uv))
}

fn buffer(manager: &PackageManager, tag: u32, subtype: u8) -> Result<(Vec<u8>, Vec<u8>), String> {
    let entry = manager
        .get_entry(tag)
        .ok_or("The model buffer is missing")?;
    if entry.file_type != 32 || entry.file_subtype != subtype {
        return Err("Unsupported model buffer type".into());
    }
    let data = manager
        .get_entry(entry.reference)
        .ok_or("The model buffer payload is missing")?;
    if data.file_size > 32 * 1024 * 1024 {
        return Err("This buffer exceeds the preview size budget.".into());
    }
    Ok((manager.read_tag(tag)?, manager.read_tag(entry.reference)?))
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
        3 if count % 3 == 0 => {
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

fn read_uvs(
    manager: &PackageManager,
    model: &[u8],
    mesh: usize,
    positions: &(Vec<u8>, Vec<u8>),
    stride: usize,
) -> Result<Vec<[f32; 2]>, String> {
    let second = buffer(manager, u32_at(model, mesh + 4)?, 4)?;
    let other_stride = usize::from(u16_at(&second.0, 4)?);
    let (data, uv_stride, offset) =
        if matches!(stride, 28 | 32) || (stride == 12 && other_stride == 16) {
            (&positions.1, stride, 8)
        } else if matches!(other_stride, 4 | 12 | 20 | 24 | 28) {
            (&second.1, other_stride, 0)
        } else {
            return Err("This vertex layout has no supported texture coordinates.".into());
        };
    if data.len() % uv_stride != 0 || data.len() / uv_stride != positions.1.len() / stride {
        return Err("The texture coordinate buffer does not match the positions.".into());
    }
    let transform = (0..4)
        .map(|i| u32_at(model, 0x70 + i * 4).map(f32::from_bits))
        .collect::<Result<Vec<_>, _>>()?;
    if transform.iter().any(|v| !v.is_finite()) {
        return Err("Invalid texture coordinate transform".into());
    }
    data.chunks_exact(uv_stride)
        .map(|row| {
            let u = f32::from(i16::from_le_bytes(bytes_at(row, offset)?)) / 32767.0;
            let v = f32::from(i16::from_le_bytes(bytes_at(row, offset + 2)?)) / 32767.0;
            Ok([
                u * transform[0] + transform[2],
                v * transform[1] + transform[3],
            ])
        })
        .collect()
}

// Native signed-normalized normals. Tangents are derived from the actual UVs so
// mirrored UV islands and the preview camera use the same coordinate convention.
fn read_normals(
    manager: &PackageManager,
    model: &[u8],
    mesh: usize,
    positions: &(Vec<u8>, Vec<u8>),
    stride: usize,
    scale: [f32; 3],
) -> Result<Vec<[f32; 3]>, String> {
    let second = buffer(manager, u32_at(model, mesh + 4)?, 4)?;
    let other = usize::from(u16_at(&second.0, 4)?);
    let (data, row_stride, offset) = if matches!(stride, 28 | 32) {
        (&positions.1, stride, 12)
    } else {
        let offset = match other {
            8 | 16 => 0,
            12 | 20 => 4,
            24 => 8,
            _ => return Err("Unsupported vertex normal layout".into()),
        };
        (&second.1, other, offset)
    };
    if data.len() % row_stride != 0 || data.len() / row_stride != positions.1.len() / stride {
        return Err("Normal buffer does not match positions".into());
    }
    data.chunks_exact(row_stride)
        .map(|row| {
            let mut n = [0.0; 3];
            for i in 0..3 {
                n[i] = f32::from(i16::from_le_bytes(bytes_at(row, offset + i * 2)?)) / 32767.0;
                if scale[i].abs() > 1e-8 {
                    n[i] /= scale[i];
                }
            }
            Ok(super::shader::normal::normalize(n).unwrap_or([0.0; 3]))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lower_detail_fallback_preserves_shared_parts_without_overlaying_other_levels() {
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
