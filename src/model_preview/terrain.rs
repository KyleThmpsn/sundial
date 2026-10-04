//! Shadowkeep terrain patches. The position and UV transforms follow the native vertex
//! shader: integer XY / 64 and (integer Z + W * 65536) / 8192 after the terrain offset.
//! Terrain layers and displacement remain a material approximation in the preview.
use super::*;
use std::collections::BTreeMap;

pub(super) const TERRAIN: u32 = 0x8080_714F;
pub(super) const RESOURCE: u32 = 0x8080_714B;

pub(super) fn load(manager: &PackageManager, tag: u32) -> Result<Model, String> {
    let bytes = checked(manager, tag, TERRAIN)?;
    let layouts = vertex::Layouts::read(manager)?;
    let decoded = layouts.read_vertices(
        manager,
        60,
        [u32_at(&bytes, 0x68)?, u32_at(&bytes, 0x6C)?, 0, 0],
        MAX_VERTICES,
    )?;
    let (width, indices) = vertex::indices(manager, u32_at(&bytes, 0x70)?)?;
    let (group_count, groups) = vertex::table(&bytes, 0x58, 0x8080_7154, 0x60, 4096)?;
    let (part_count, parts) = vertex::table(&bytes, 0x80, 0x8080_7152, 12, 65536)?;
    let level = (0..part_count)
        .map(|i| bytes[parts + i * 12 + 11])
        .filter(|&lod| lod < 4)
        .min()
        .ok_or("This terrain has no supported detail level")?;
    let offset = vector4(&bytes, 0x30)?;
    let mut model = Model {
        tags: vec![tag],
        notices: decoded.notices.clone(),
        ..Default::default()
    };
    model.notices.push(
        "Terrain uses a simplified material. Layer blending and displacement are not simulated."
            .into(),
    );
    if level != 0 {
        model.notices.push(format!(
            "Showing detail level {level}. Higher-detail geometry is absent."
        ));
    }
    let mut vertices = BTreeMap::new();
    let mut materials = BTreeMap::new();
    for row in (0..part_count)
        .map(|i| parts + i * 12)
        .filter(|&row| bytes[row + 11] == level)
    {
        let group = usize::from(bytes[row + 10]);
        if group >= group_count {
            return Err("A terrain part names a missing mesh group".into());
        }
        let uv = vector4(&bytes, groups + group * 0x60 + 0x20)?;
        let triangles = decode::triangles(
            &indices,
            width,
            u32_at(&bytes, row + 4)? as usize,
            usize::from(u16_at(&bytes, row + 8)?),
            5,
            decoded.positions.len(),
        )?;
        if model.triangles.len() + triangles.len() > MAX_TRIANGLES {
            return Err("This terrain exceeds the preview triangle budget.".into());
        }
        let technique = u32_at(&bytes, row)?;
        let texture = if decoded.has_uv {
            *materials
                .entry(technique)
                .or_insert_with(|| texture::material(manager, technique, &mut model).ok())
        } else {
            None
        };
        for triangle in triangles {
            let mut placed = [0; 3];
            for (slot, source) in triangle.into_iter().enumerate() {
                let key = (group, source);
                if let std::collections::btree_map::Entry::Vacant(entry) = vertices.entry(key) {
                    entry.insert(append_vertex(
                        &decoded,
                        source as usize,
                        offset,
                        uv,
                        &mut model,
                    )?);
                }
                placed[slot] = vertices[&key];
            }
            model.triangles.push(placed);
            model.triangle_textures.push(texture);
        }
    }
    if model.triangles.is_empty() {
        return Err("This terrain has no supported triangles.".into());
    }
    let triangles = model.triangles.len();
    model.triangle_dyes.resize(triangles, 0);
    model.triangle_clip.resize(triangles, false);
    model.triangle_constant.resize(triangles, None);
    model.triangle_gearstacks.resize(triangles, None);
    model.triangle_normals.resize(triangles, None);
    if [0x90, 0x94, 0x98]
        .iter()
        .any(|&at| u32_at(&bytes, at).is_ok_and(|v| !matches!(v, 0 | u32::MAX)))
    {
        model
            .notices
            .push("Secondary terrain detail buffers are not shown.".into());
    }
    Ok(model)
}

fn append_vertex(
    decoded: &vertex::Vertices,
    source: usize,
    offset: [f32; 4],
    uv: [f32; 4],
    model: &mut Model,
) -> Result<u32, String> {
    if model.vertices.len() >= MAX_VERTICES {
        return Err("This terrain exceeds the preview vertex budget.".into());
    }
    let p = decoded.positions[source];
    let position = [
        (p[0] + offset[0]) / 64.0,
        (p[1] + offset[1]) / 64.0,
        (p[2] + offset[2] + (p[3] + offset[3]) * 65536.0) / 8192.0,
    ];
    let t = decoded.uvs[source];
    let coordinate = [t[0] * uv[0] + uv[2], t[1] * uv[1] + uv[3]];
    if position.iter().chain(&coordinate).any(|v| !v.is_finite()) {
        return Err("The terrain contains invalid coordinates".into());
    }
    let index = model.vertices.len() as u32;
    model.vertices.push(position);
    model.uvs.push(coordinate);
    model
        .normals
        .push(shader::normal::normalize(decoded.normals[source]).unwrap_or([0.0; 3]));
    model.weights.push(None);
    Ok(index)
}

fn vector4(bytes: &[u8], at: usize) -> Result<[f32; 4], String> {
    let mut values = [0.0; 4];
    for (i, value) in values.iter_mut().enumerate() {
        *value = f32::from_bits(u32_at(bytes, at + i * 4)?);
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err("Invalid terrain transform".into());
    }
    Ok(values)
}
