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
                        5 => crate::tiger::geometry::triangles(
                            values,
                            p.0.len() / 24,
                            if width == 2 { 65535 } else { u32::MAX },
                        )?,
                        3 => {
                            ensure!(
                                count.is_multiple_of(3)
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
pub use crate::presentation::geometry::Encoded;
pub fn encode(mesh: &Mesh, native: &Payload, native_mesh: usize) -> Result<Encoded> {
    crate::presentation::geometry::encode(
        &crate::presentation::geometry::Mesh {
            positions: mesh.positions.clone(),
            attributes: mesh.attributes.clone(),
            groups: mesh
                .groups
                .iter()
                .map(|(m, f)| (*m, f.iter().map(|f| f.map(u32::from)).collect()))
                .collect(),
            weights: Vec::new(),
            bones: 1,
        },
        native,
        native_mesh,
    )
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
        // A native carrier has an opaque body record, including its activation group.
        let mut carrier = vec![0; 0x170];
        carrier[0xc8..0xd0].copy_from_slice(&1u64.to_le_bytes());
        carrier[0xd0..0xd8].copy_from_slice(&0x70u64.to_le_bytes());
        carrier[0x140..0x148].copy_from_slice(&1u64.to_le_bytes());
        carrier[0x148..0x14c].copy_from_slice(&0x8080737eu32.to_le_bytes());
        for stage in 1..24 {
            carrier[0xb0 + 40 + stage * 2..0xb0 + 42 + stage * 2]
                .copy_from_slice(&1u16.to_le_bytes());
        }
        carrier[0x15c..0x160].copy_from_slice(&3u32.to_le_bytes());
        carrier[0x160..0x164].copy_from_slice(&1u32.to_le_bytes());
        carrier[0x16d] = 1;
        let Encoded {
            header: bytes,
            streams,
            patches,
        } = encode(&mesh, &Payload(carrier), 0xb0).unwrap();
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
