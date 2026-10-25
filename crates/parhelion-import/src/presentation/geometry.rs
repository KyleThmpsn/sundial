use super::*;
use crate::tiger::payload::Payload;
use serde_json::json;

pub(crate) struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub attributes: Vec<[f32; 10]>,
    pub groups: Vec<(u32, Vec<[u32; 3]>)>,
    pub weights: Vec<[u8; 8]>,
    pub bones: u32,
}
pub struct Encoded {
    pub header: Vec<u8>,
    pub streams: [Vec<u8>; 3],
    pub patches: Vec<Value>,
}
pub fn encode_with_stages(
    mesh: &Mesh,
    native: &Payload,
    native_mesh: usize,
    stages: &std::collections::BTreeMap<u32, u8>,
) -> Result<Encoded> {
    ensure!(
        stages.values().all(|stage| matches!(stage, 0 | 7)),
        "Unsupported surface stage"
    );
    encode_passes(mesh, native, native_mesh, stages, true, None, None)
}

/// A static mesh's streams. Positions lie on one grid of a step from the origin: the mesh's center
/// lies on the grid and its range is 32,767 steps, so pieces that share a corner store it as the
/// same point, and a mesh too wide for the step takes the smallest power-of-two multiple of it
/// that holds the mesh. Texture coordinates take one range on both axes, since a static
/// instance's data scales both by one value. Each group's triangles are one list of 16-bit
/// indices, as stock statics store them.
pub(crate) struct StaticStreams {
    pub positions: Vec<u8>,
    /// Texture coordinates, normal and tangent as the entity encoder writes them, then four bytes
    /// for detail coordinates the caller fills.
    pub attributes: Vec<u8>,
    pub indices: Vec<u8>,
    /// Each group's first index and index count, in `Mesh::groups` order.
    pub ranges: Vec<(u32, u32)>,
    pub center: [f32; 3],
    pub scale: f32,
    pub uv_scale: f32,
    pub uv_center: [f32; 2],
    /// The positions' box.
    pub bounds: ([f32; 3], [f32; 3]),
}

/// Encodes rigid `mesh` as a static mesh's streams, positions on a grid of `step` meters.
pub(crate) fn encode_static(mesh: &Mesh, step: f32) -> Result<StaticStreams> {
    ensure!(
        !mesh.positions.is_empty() && mesh.positions.len() == mesh.attributes.len(),
        "Missing or mismatched static vertices"
    );
    ensure!(mesh.weights.is_empty(), "A static mesh is rigid");
    ensure!(
        mesh.positions.len() <= usize::from(u16::MAX),
        "A static mesh's indices are 16 bits"
    );
    ensure!(step > 0. && step.is_finite(), "Invalid position grid");
    ensure!(
        mesh.positions
            .iter()
            .flatten()
            .chain(mesh.attributes.iter().flatten())
            .all(|v| v.is_finite()),
        "Nonfinite static geometry"
    );
    let lo = std::array::from_fn::<_, 3, _>(|a| {
        mesh.positions
            .iter()
            .map(|p| p[a])
            .fold(f32::INFINITY, f32::min)
    });
    let hi = std::array::from_fn::<_, 3, _>(|a| {
        mesh.positions
            .iter()
            .map(|p| p[a])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let half = (0..3)
        .map(|a| (hi[a] - lo[a]) / 2.)
        .fold(0.000001, f32::max);
    // The snapped center moves by up to half a step, so the range holds the half extent and one
    // step more.
    let mut step = step;
    while half + step > 32767. * step {
        step *= 2.;
    }
    let center = std::array::from_fn::<_, 3, _>(|a| (((lo[a] + hi[a]) / 2.) / step).round() * step);
    let scale = 32767. * step;
    let ulo = std::array::from_fn::<_, 2, _>(|a| {
        mesh.attributes
            .iter()
            .map(|p| p[a])
            .fold(f32::INFINITY, f32::min)
    });
    let uhi = std::array::from_fn::<_, 2, _>(|a| {
        mesh.attributes
            .iter()
            .map(|p| p[a])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let uv_scale = (0..2)
        .map(|a| (uhi[a] - ulo[a]) / 2.)
        .fold(0.000001, f32::max);
    let uv_center = std::array::from_fn::<_, 2, _>(|a| (uhi[a] + ulo[a]) / 2.);
    let quant = |v: f32| ((v * 32767.).round().clamp(-32767., 32767.) as i16).to_le_bytes();
    let mut positions = Vec::with_capacity(mesh.positions.len() * 8);
    let mut attributes = Vec::with_capacity(mesh.positions.len() * 24);
    for (v, a) in mesh.positions.iter().zip(&mesh.attributes) {
        for i in 0..3 {
            positions.extend(quant((v[i] - center[i]) / scale));
        }
        positions.extend(i16::MAX.to_le_bytes());
        for i in 0..2 {
            attributes.extend(quant((a[i] - uv_center[i]) / uv_scale));
        }
        for v in &a[2..] {
            attributes.extend(quant(*v));
        }
        attributes.extend([0; 4]);
    }
    let mut indices = Vec::new();
    let mut ranges = Vec::with_capacity(mesh.groups.len());
    for (_, faces) in &mesh.groups {
        let first = u32::try_from(indices.len() / 2)?;
        for face in faces {
            for &i in face {
                ensure!(
                    (i as usize) < mesh.positions.len(),
                    "Face outside static vertices"
                );
                indices.extend(u16::try_from(i)?.to_le_bytes());
            }
        }
        ranges.push((first, u32::try_from(faces.len() * 3)?));
    }
    Ok(StaticStreams {
        positions,
        attributes,
        indices,
        ranges,
        center,
        scale,
        uv_scale,
        uv_center,
        // Rounding can move a corner outwards by half a grid step. Keep the full step for
        // the additional float rounding in the native instance transform as well.
        bounds: (lo.map(|v| v - step), hi.map(|v| v + step)),
    })
}

/// Bind each authored primitive to a declared component activation group.
pub(crate) fn encode_grouped(
    mesh: &Mesh,
    native: &Payload,
    native_mesh: usize,
    stages: &std::collections::BTreeMap<u32, u8>,
    groups: &[u16],
) -> Result<Encoded> {
    ensure!(
        stages.values().all(|stage| matches!(stage, 0 | 7)),
        "Unsupported surface stage"
    );
    ensure!(
        groups.len() == mesh.groups.len() && groups.iter().all(|&group| group <= i16::MAX as u16),
        "Authored activation groups differ from primitive groups"
    );
    encode_passes(mesh, native, native_mesh, stages, true, Some(groups), None)
}

/// Keep explicitly supplied surface, decal, shadow, transparent, depth and reticle
/// records. Source backends own pass selection and material translation.
pub fn encode_source_passes(
    mesh: &Mesh,
    native: &Payload,
    native_mesh: usize,
    stages: &std::collections::BTreeMap<u32, u8>,
) -> Result<Encoded> {
    ensure!(
        mesh.groups.iter().all(|(key, _)| stages.contains_key(key)),
        "Source draw has no native stage"
    );
    ensure!(
        stages
            .values()
            .all(|stage| matches!(stage, 0 | 1 | 3 | 7 | 9 | 12 | 14 | 16)),
        "Unsupported source presentation pass"
    );
    encode_passes(mesh, native, native_mesh, stages, false, None, None)
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Encode the complete native model and its cross-referenced streams together."
)]
fn encode_passes(
    mesh: &Mesh,
    native: &Payload,
    native_mesh: usize,
    stages: &std::collections::BTreeMap<u32, u8>,
    automatic_shadows: bool,
    groups: Option<&[u16]>,
    grid: Option<(f32, &std::collections::BTreeSet<u32>)>,
) -> Result<Encoded> {
    ensure!(
        !mesh.positions.is_empty() && mesh.positions.len() == mesh.attributes.len(),
        "Missing or mismatched native vertices"
    );
    ensure!(
        mesh.weights.is_empty() || mesh.weights.len() == mesh.positions.len(),
        "Mismatched native skin weights"
    );
    ensure!(
        mesh.bones <= 256,
        "Native bone palette exceeds byte indices"
    );
    ensure!(
        mesh.positions
            .iter()
            .flatten()
            .chain(mesh.attributes.iter().flatten())
            .all(|v| v.is_finite()),
        "Nonfinite native geometry"
    );
    ensure!(
        mesh.groups.iter().all(|(_, faces)| faces
            .iter()
            .flatten()
            .all(|i| (*i as usize) < mesh.positions.len())),
        "Face outside native vertices"
    );
    let lo = std::array::from_fn::<_, 3, _>(|a| {
        mesh.positions
            .iter()
            .map(|p| p[a])
            .fold(f32::INFINITY, f32::min)
    });
    let hi = std::array::from_fn::<_, 3, _>(|a| {
        mesh.positions
            .iter()
            .map(|p| p[a])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let mut center = std::array::from_fn::<_, 3, _>(|a| (lo[a] + hi[a]) / 2.);
    let mut scale = (0..3)
        .map(|a| (hi[a] - lo[a]) / 2.)
        .fold(0.000001, f32::max);
    if let Some((step, _)) = grid {
        // The snapped center moves by up to half a step, so the range holds the half extent and
        // one step more.
        let mut step = step;
        while scale + step > 32767. * step {
            step *= 2.;
        }
        center = center.map(|value| (value / step).round() * step);
        scale = 32767. * step;
    }
    let ulo = std::array::from_fn::<_, 2, _>(|a| {
        mesh.attributes
            .iter()
            .map(|p| p[a])
            .fold(f32::INFINITY, f32::min)
    });
    let uhi = std::array::from_fn::<_, 2, _>(|a| {
        mesh.attributes
            .iter()
            .map(|p| p[a])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let us = std::array::from_fn::<_, 2, _>(|a| ((uhi[a] - ulo[a]) / 2.).max(0.000001));
    let uc = std::array::from_fn::<_, 2, _>(|a| (uhi[a] + ulo[a]) / 2.);
    let quant = |v: f32| ((v * 32767.).round().clamp(-32767., 32767.) as i16).to_le_bytes();
    let mut positions = vec![];
    let mut attributes = vec![];
    let mut indices = vec![];
    for (vertex, (v, a)) in mesh.positions.iter().zip(&mesh.attributes).enumerate() {
        for i in 0..3 {
            positions.extend(quant((v[i] - center[i]) / scale));
        }
        positions.extend(0u16.to_le_bytes());
        if !mesh.weights.is_empty() {
            positions.extend(mesh.weights[vertex]);
        }
        for i in 0..2 {
            attributes.extend(quant((a[i] - uc[i]) / us[i]));
        }
        for v in &a[2..] {
            attributes.extend(quant(*v));
        }
        attributes.extend([0; 4]);
    }
    let mut records = vec![];
    let visibility_group =
        if !automatic_shadows && !stages.is_empty() && stages.values().all(|stage| *stage == 16) {
            crate::tiger::draws::stage_group(native, native_mesh, 16)?
        } else {
            crate::tiger::draws::drawn_group(native, native_mesh)?
        };
    let mut patches = vec![
        json!({"offset":0xb0,"symbol":"positions-header"}),
        json!({"offset":0xb4,"symbol":"attributes-header"}),
        json!({"offset":0xc0,"symbol":"indices-header"}),
    ];
    let mut ranges = vec![0u16];
    // Surface and shadow records draw the same source group. Native part ranges can share one
    // index span, avoiding a second resident copy of every shadow-casting triangle.
    let mut group_starts = vec![None; mesh.groups.len()];
    for stage in 0..23 {
        if [0, 1, 3, 7, 9, 12, 14, 16].contains(&stage) {
            for (group_index, (mat, faces)) in mesh.groups.iter().enumerate() {
                let surface_stage = usize::from(*stages.get(mat).unwrap_or(&0));
                let shadowless = grid.is_some_and(|(_, shadowless)| shadowless.contains(mat));
                if stage != surface_stage
                    && !(automatic_shadows && stage == 3 && surface_stage == 0 && !shadowless)
                {
                    continue;
                }
                let mut rec = vec![0u8; 32];
                put(&mut rec, 0, &u32::MAX.to_le_bytes())?;
                put(&mut rec, 4, &u16::MAX.to_le_bytes())?;
                put(&mut rec, 6, &5u16.to_le_bytes())?;
                let first = *group_starts[group_index].get_or_insert(u32::try_from(
                    indices.len() / if mesh.weights.is_empty() { 2 } else { 4 },
                )?);
                put(&mut rec, 8, &first.to_le_bytes())?;
                put(&mut rec, 12, &(faces.len() as u32 * 4).to_le_bytes())?;
                put(&mut rec, 16, &(faces.len() as u32).to_le_bytes())?;
                let group = groups.map_or(visibility_group, |groups| groups[group_index]);
                put(&mut rec, 20, &group.to_le_bytes())?;
                put(&mut rec, 22, &(records.len() as u16).to_le_bytes())?;
                put(
                    &mut rec,
                    24,
                    &(match stage {
                        1 => 39u16,
                        14 => 0x2004,
                        7 | 9 | 16 => 21,
                        _ => 5,
                    })
                    .to_le_bytes(),
                )?;
                rec[26] = 0;
                // This writer emits one complete detail level. Category 3 covers all
                // four native bands, including the coarser view used during inspection.
                rec[27] = 3;
                rec[28] = 0x7f;
                // The native draw iterator advances by this group length.
                // Zero leaves it on the first record indefinitely.
                rec[29] = 1;
                let symbol = if stage != 3 || !automatic_shadows {
                    format!("material-{mat:08X}")
                } else {
                    "material-shadow".into()
                };
                patches.push(json!({"offset":0x150+records.len()*32,"symbol":symbol}));
                records.push(rec);
                if first as usize != indices.len() / if mesh.weights.is_empty() { 2 } else { 4 } {
                    continue;
                }
                for face in faces {
                    for i in face {
                        if mesh.weights.is_empty() {
                            indices.extend(u16::try_from(*i)?.to_le_bytes());
                        } else {
                            indices.extend(i.to_le_bytes());
                        }
                    }
                    if mesh.weights.is_empty() {
                        indices.extend(u16::MAX.to_le_bytes());
                    } else {
                        indices.extend(u32::MAX.to_le_bytes());
                    }
                }
            }
        }
        ranges.push(u16::try_from(records.len())?);
    }
    let mut h = vec![0; 0x150 + records.len() * 32];
    put(&mut h, 0, &native.bytes::<160>(0)?)?;
    let len = h.len() as u64;
    put(&mut h, 0, &len.to_le_bytes())?;
    put(&mut h, 16, &1u64.to_le_bytes())?;
    put(&mut h, 24, &(0xa0i64 - 24).to_le_bytes())?;
    put(&mut h, 0x40, &mesh.bones.to_le_bytes())?;
    for (a, value) in center.iter().enumerate() {
        put(&mut h, 0x50 + a * 4, &scale.to_le_bytes())?;
        put(&mut h, 0x60 + a * 4, &value.to_le_bytes())?;
    }
    put(&mut h, 0x6c, &scale.to_le_bytes())?;
    let radius = mesh
        .positions
        .iter()
        .map(|p| {
            (0..3)
                .map(|a| (p[a] - center[a]).powi(2))
                .sum::<f32>()
                .sqrt()
        })
        .fold(0f32, f32::max)
        + scale / 32767.;
    // Stock weapon headers hold the bounding radius at +0x20, +0x24 and +0x28 and keep 1.0 at
    // +0x2C. The copied donor header must not keep its own +0x28 under a larger model.
    for offset in [0x20, 0x24, 0x28, 0x8c] {
        put(&mut h, offset, &radius.to_le_bytes())?;
    }
    for (a, value) in center.iter().enumerate() {
        put(&mut h, 0x80 + a * 4, &value.to_le_bytes())?;
    }
    for a in 0..2 {
        put(&mut h, 0x70 + a * 4, &us[a].to_le_bytes())?;
        put(&mut h, 0x78 + a * 4, &uc[a].to_le_bytes())?;
    }
    put(&mut h, 0x9c, &0x80809fbdu32.to_le_bytes())?;
    put(&mut h, 0xa0, &1u64.to_le_bytes())?;
    put(&mut h, 0xa8, &0x80807378u64.to_le_bytes())?;
    put(&mut h, 0xb0, &native.bytes::<136>(native_mesh)?)?;
    for o in [0xb0, 0xb4, 0xb8, 0xc0] {
        put(&mut h, o, &u32::MAX.to_le_bytes())?;
    }
    put(&mut h, 0xc8, &(records.len() as u64).to_le_bytes())?;
    put(&mut h, 0xd0, &(0x140i64 - 0xd0).to_le_bytes())?;
    for (i, n) in ranges.iter().enumerate() {
        put(&mut h, 0xd8 + i * 2, &n.to_le_bytes())?;
    }
    for i in 0..23 {
        put(
            &mut h,
            0x108 + i * 2,
            &(if ranges[i] != ranges[i + 1] {
                if mesh.weights.is_empty() {
                    139u16
                } else {
                    28u16
                }
            } else {
                u16::MAX
            })
            .to_le_bytes(),
        )?;
    }
    put(&mut h, 0x13c, &0x80809fbdu32.to_le_bytes())?;
    put(&mut h, 0x140, &(records.len() as u64).to_le_bytes())?;
    put(&mut h, 0x148, &0x8080737eu64.to_le_bytes())?;
    for (i, r) in records.iter().enumerate() {
        put(&mut h, 0x150 + i * 32, r)?;
    }
    crate::tiger::draws::declare_draw_indices(&mut h, records.len())?;
    Ok(Encoded {
        header: h,
        streams: [positions, attributes, indices],
        patches,
    })
}
