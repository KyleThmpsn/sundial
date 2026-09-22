use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub attributes: Vec<[f32; 10]>,
    pub groups: Vec<(u32, Vec<[u16; 3]>)>,
}
fn transform(v: [f32; 3], m: &[f32; 16], point: bool) -> [f32; 3] {
    std::array::from_fn(|i| {
        m[i * 4] * v[0]
            + m[i * 4 + 1] * v[1]
            + m[i * 4 + 2] * v[2]
            + if point { m[i * 4 + 3] } else { 0. }
    })
}
pub fn read(r: &mut Reader, entries: &[Value]) -> Result<Mesh> {
    let mut out = Mesh {
        positions: vec![],
        attributes: vec![],
        groups: vec![],
    };
    for entry in entries {
        let tag = hash(entry, "model")?;
        let b = r.tag(tag, Some(0x8080881C))?;
        let mat: [f32; 16] = serde_json::from_value(entry["transform"].clone())?;
        ensure!(
            mat.iter().all(|x| x.is_finite()),
            "nonfinite attachment transform"
        );
        for m in b.array(16, 0x88, Some(0x808087CB))? {
            let pt = b.u32(m)?;
            let ut = b.u32(m + 4)?;
            let it = b.u32(m + 16)?;
            let ph = r.tag(pt, None)?;
            let uh = r.tag(ut, None)?;
            let ih = r.tag(it, None)?;
            ensure!(
                (ph.u16(4)?, uh.u16(4)?) == (24, 4),
                "Marathon vertex stride differs"
            );
            let p = r.tag(r.reference(pt)?, None)?;
            let uv = r.tag(r.reference(ut)?, None)?;
            let ib = r.tag(r.reference(it)?, None)?;
            ensure!(
                p.0.len() == ph.u32(0)? as usize
                    && uv.0.len() == uh.u32(0)? as usize
                    && ib.0.len() == ih.u32(8)? as usize,
                "buffer lengths differ"
            );
            ensure!(
                p.0.len() % 24 == 0 && uv.0.len() == p.0.len() / 24 * 4,
                "vertex counts differ"
            );
            let width = if ih.u8(1)? == 0 { 2 } else { 4 };
            let indices = (0..ib.0.len())
                .step_by(width)
                .map(|o| {
                    if width == 2 {
                        Ok(ib.u16(o)? as u32)
                    } else {
                        ib.u32(o)
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let parts = b.array(m + 32, 40, Some(0x808087D1))?;
            let mut selected = vec![];
            let mut seen = BTreeSet::new();
            // Opaque base and inventory decals. Lighting and compute records repeat their ranges.
            for stage in [0, 2] {
                ensure!(
                    b.u8(m + 0x64 + stage)? == 7,
                    "unhandled Marathon input layout"
                );
                let start = b.u16(m + 0x30 + stage * 2)? as usize;
                let end = b.u16(m + 0x32 + stage * 2)? as usize;
                for &part in parts.get(start..end).context("part stage range")? {
                    if b.u8(part + 33)? > 3 {
                        continue;
                    }
                    let start = b.u32(part + 8)? as usize;
                    let count = b.u32(part + 12)? as usize;
                    if !seen.insert((start, count)) {
                        continue;
                    }
                    let values = indices
                        .get(start..start + count)
                        .context("draw outside buffer")?;
                    let faces = match b.u16(part + 6)? {
                        5 => crate::d2_mot::geometry::triangles(
                            values,
                            p.0.len() / 24,
                            if width == 2 { 65535 } else { u32::MAX },
                        )?,
                        3 => {
                            ensure!(
                                count % 3 == 0
                                    && values.iter().all(|i| (*i as usize) < p.0.len() / 24),
                                "invalid triangle list"
                            );
                            values.chunks_exact(3).map(|f| [f[0], f[1], f[2]]).collect()
                        }
                        n => anyhow::bail!("unhandled Marathon primitive {n} in {tag:08X}"),
                    };
                    selected.push((b.u32(part)?, faces));
                }
            }
            let used = selected
                .iter()
                .flat_map(|(_, f)| f.iter().flatten().copied())
                .collect::<BTreeSet<_>>();
            let base = out.positions.len();
            ensure!(
                base + used.len() < 65535,
                "assembled model exceeds 16-bit vertex capacity"
            );
            let map = used
                .iter()
                .enumerate()
                .map(|(i, v)| (*v, (base + i) as u16))
                .collect::<BTreeMap<_, _>>();
            for v in used {
                let o = v as usize * 24;
                // This adapter bakes bind-pose positions to native root zero. W is a
                // skinning selector and does not change the stored XYZ coordinates.
                let mut xyz = [0.; 3];
                let mut normal = [0.; 3];
                let mut tangent = [0.; 3];
                let mut tex = [0.; 2];
                for a in 0..3 {
                    xyz[a] = (p.i16(o + a * 2)? as f32 / 32767.).max(-1.) * b.f32(0xa0 + a * 4)?
                        + b.f32(0xb0 + a * 4)?;
                    normal[a] = (p.i16(o + 8 + a * 2)? as f32 / 32767.).max(-1.);
                    tangent[a] = (p.i16(o + 16 + a * 2)? as f32 / 32767.).max(-1.);
                }
                for (a, value) in tex.iter_mut().enumerate() {
                    *value = (uv.i16(v as usize * 4 + a * 2)? as f32 / 32767.).max(-1.)
                        * b.f32(0xc0 + a * 4)?
                        + b.f32(0xc8 + a * 4)?;
                }
                xyz = transform(xyz, &mat, true);
                normal = transform(normal, &mat, false);
                tangent = transform(tangent, &mat, false);
                ensure!(
                    xyz.iter()
                        .chain(normal.iter())
                        .chain(tangent.iter())
                        .chain(tex.iter())
                        .all(|v| v.is_finite()),
                    "nonfinite decoded vertex"
                );
                out.positions.push(xyz);
                out.attributes.push([
                    tex[0],
                    tex[1],
                    normal[0],
                    normal[1],
                    normal[2],
                    p.i16(o + 14)? as f32 / 32767.,
                    tangent[0],
                    tangent[1],
                    tangent[2],
                    p.i16(o + 22)? as f32 / 32767.,
                ]);
            }
            for (mat, faces) in selected {
                if !faces.is_empty() {
                    out.groups
                        .push((mat, faces.iter().map(|f| f.map(|i| map[&i])).collect()));
                }
            }
        }
    }
    ensure!(!out.groups.is_empty(), "no render geometry");
    Ok(out)
}
pub struct Encoded {
    pub header: Vec<u8>,
    pub streams: [Vec<u8>; 3],
    pub patches: Vec<Value>,
}
#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub fn encode(mesh: &Mesh, native: &Payload, native_mesh: usize) -> Result<Encoded> {
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
    for (v, a) in mesh.positions.iter().zip(&mesh.attributes) {
        for i in 0..3 {
            positions.extend(quant((v[i] - center[i]) / scale));
        }
        positions.extend(0u16.to_le_bytes());
        for i in 0..2 {
            attributes.extend(quant((a[i] - uc[i]) / us[i]));
        }
        for v in &a[2..] {
            attributes.extend(quant(*v));
        }
        attributes.extend([0; 4]);
    }
    let mut records = vec![];
    let mut patches = vec![
        json!({"offset":0xb0,"symbol":"positions-header"}),
        json!({"offset":0xb4,"symbol":"attributes-header"}),
        json!({"offset":0xc0,"symbol":"indices-header"}),
    ];
    let mut ranges = vec![0u16];
    for stage in 0..23 {
        if stage == 0 || stage == 3 {
            for (mat, faces) in &mesh.groups {
                let mut rec = vec![0u8; 32];
                put(&mut rec, 0, &u32::MAX.to_le_bytes())?;
                put(&mut rec, 4, &u16::MAX.to_le_bytes())?;
                put(&mut rec, 6, &5u16.to_le_bytes())?;
                put(&mut rec, 8, &(indices.len() as u32 / 2).to_le_bytes())?;
                put(&mut rec, 12, &(faces.len() as u32 * 4).to_le_bytes())?;
                put(&mut rec, 16, &(faces.len() as u32).to_le_bytes())?;
                // These static assemblies use the donors' base variant and
                // native opaque weapon flags, rather than an absent variant.
                put(&mut rec, 20, &0u16.to_le_bytes())?;
                put(&mut rec, 22, &(records.len() as u16).to_le_bytes())?;
                put(&mut rec, 24, &5u16.to_le_bytes())?;
                rec[26] = 0;
                rec[27] = 0;
                rec[28] = 0x7f;
                // The native draw iterator advances by this group length.
                // Zero leaves it on the first record indefinitely.
                rec[29] = 1;
                let symbol = if stage == 0 {
                    format!("material-{mat:08X}")
                } else {
                    "material-shadow".into()
                };
                patches.push(json!({"offset":0x150+records.len()*32,"symbol":symbol}));
                records.push(rec);
                for face in faces {
                    for i in face {
                        indices.extend(i.to_le_bytes());
                    }
                    indices.extend(u16::MAX.to_le_bytes());
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
    put(&mut h, 0x40, &1u32.to_le_bytes())?;
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
            &(if [0, 3].contains(&i) {
                139u16
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
    Ok(Encoded {
        header: h,
        streams: [positions, attributes, indices],
        patches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_translation_does_not_move_normals() {
        let mut m = [0.; 16];
        m[0] = 1.;
        m[5] = 1.;
        m[10] = 1.;
        m[3] = 9.;
        assert_eq!(transform([1., 2., 3.], &m, true), [10., 2., 3.]);
        assert_eq!(transform([1., 2., 3.], &m, false), [1., 2., 3.]);
    }
    #[test]
    #[expect(
        clippy::cognitive_complexity,
        reason = "Round-trip verification compares each native geometry stream with the source."
    )]
    fn native_geometry_round_trip_preserves_shape_uv_and_draws() {
        let mesh = Mesh {
            positions: vec![[0., -0.25, 1.], [3., 0.5, 2.], [1., 0.25, 0.]],
            attributes: vec![
                [0., 0., 0., 0., 1., 1., 1., 0., 0., 1.],
                [1., 2., 0., 0., 1., 1., 1., 0., 0., 1.],
                [-1., 1., 0., 0., 1., 1., 1., 0., 0., 1.],
            ],
            groups: vec![(123, vec![[0, 1, 2]])],
        };
        let Encoded {
            header: bytes,
            streams,
            patches,
        } = encode(&mesh, &Payload(vec![0; 0x150]), 0xb0).unwrap();
        let h = Payload(bytes);
        let p = Payload(streams[0].clone());
        let a = Payload(streams[1].clone());
        assert_eq!(h.array(16, 136, Some(0x80807378)).unwrap(), vec![0xb0]);
        assert_eq!(h.array(0xc8, 32, Some(0x8080737e)).unwrap().len(), 2);
        assert_eq!(
            streams.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![24, 72, 16]
        );
        assert_eq!(patches.len(), 5);
        for i in 0..3 {
            let mut radius = 0.;
            for axis in 0..3 {
                let decoded = p.i16(i * 8 + axis * 2).unwrap() as f32 / 32767.
                    * h.f32(0x50 + axis * 4).unwrap()
                    + h.f32(0x60 + axis * 4).unwrap();
                assert!((decoded - mesh.positions[i][axis]).abs() < 0.0001);
                radius += (decoded - h.f32(0x80 + axis * 4).unwrap()).powi(2);
            }
            assert!(radius.sqrt() <= h.f32(0x8c).unwrap());
            for axis in 0..2 {
                let uv = a.i16(i * 24 + axis * 2).unwrap() as f32 / 32767.
                    * h.f32(0x70 + axis * 4).unwrap()
                    + h.f32(0x78 + axis * 4).unwrap();
                assert!((uv - mesh.attributes[i][axis]).abs() < 0.0001);
            }
            assert_eq!(p.u16(i * 8 + 6).unwrap(), 0);
        }
        let ib = Payload(streams[2].clone());
        assert_eq!(
            (0..16)
                .step_by(2)
                .map(|i| ib.u16(i).unwrap())
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 65535, 0, 1, 2, 65535]
        );
        assert_eq!(h.u16(0x108).unwrap(), 139);
        assert_eq!(h.u16(0x10a).unwrap(), 65535);
        for row in h.array(0xc8, 32, Some(0x8080737e)).unwrap() {
            assert_eq!(h.u32(row + 16).unwrap(), 1);
            assert_eq!(h.u16(row + 20).unwrap(), 0);
            assert_eq!(h.u16(row + 24).unwrap(), 5);
            assert_eq!(h.u8(row + 29).unwrap(), 1);
        }
    }
}
