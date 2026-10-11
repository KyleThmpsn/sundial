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
pub fn encode(mesh: &Mesh, native: &Payload, native_mesh: usize) -> Result<Encoded> {
    encode_with_stages(mesh, native, native_mesh, &Default::default())
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
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
    let center = std::array::from_fn::<_, 3, _>(|a| (lo[a] + hi[a]) / 2.);
    let scale = (0..3)
        .map(|a| (hi[a] - lo[a]) / 2.)
        .fold(0.000001, f32::max);
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
    let visibility_group = crate::tiger::draws::body_group(native, native_mesh)?;
    let mut patches = vec![
        json!({"offset":0xb0,"symbol":"positions-header"}),
        json!({"offset":0xb4,"symbol":"attributes-header"}),
        json!({"offset":0xc0,"symbol":"indices-header"}),
    ];
    let mut ranges = vec![0u16];
    for stage in 0..23 {
        if [0, 3, 7].contains(&stage) {
            for (mat, faces) in &mesh.groups {
                let surface_stage = usize::from(*stages.get(mat).unwrap_or(&0));
                if stage != surface_stage && !(stage == 3 && surface_stage == 0) {
                    continue;
                }
                let mut rec = vec![0u8; 32];
                put(&mut rec, 0, &u32::MAX.to_le_bytes())?;
                put(&mut rec, 4, &u16::MAX.to_le_bytes())?;
                put(&mut rec, 6, &5u16.to_le_bytes())?;
                put(
                    &mut rec,
                    8,
                    &(indices.len() as u32 / if mesh.weights.is_empty() { 2 } else { 4 })
                        .to_le_bytes(),
                )?;
                put(&mut rec, 12, &(faces.len() as u32 * 4).to_le_bytes())?;
                put(&mut rec, 16, &(faces.len() as u32).to_le_bytes())?;
                // The imported assembly follows the carrier's body activation.
                put(&mut rec, 20, &visibility_group.to_le_bytes())?;
                put(&mut rec, 22, &(records.len() as u16).to_le_bytes())?;
                put(
                    &mut rec,
                    24,
                    &(if stage == 7 { 21u16 } else { 5u16 }).to_le_bytes(),
                )?;
                rec[26] = 0;
                rec[27] = 0;
                rec[28] = 0x7f;
                // The native draw iterator advances by this group length.
                // Zero leaves it on the first record indefinitely.
                rec[29] = 1;
                let symbol = if stage != 3 {
                    format!("material-{mat:08X}")
                } else {
                    "material-shadow".into()
                };
                patches.push(json!({"offset":0x150+records.len()*32,"symbol":symbol}));
                records.push(rec);
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
    for offset in [0x20, 0x24, 0x8c] {
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
