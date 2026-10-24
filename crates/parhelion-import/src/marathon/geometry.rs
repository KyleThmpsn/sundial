use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod scope;
pub(super) use scope::Lens;

pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub attributes: Vec<[f32; 10]>,
    pub colors: Vec<[u8; 4]>,
    pub groups: Vec<(u32, Vec<[u16; 3]>)>,
    pub surfaces: BTreeMap<u32, Surface>,
    pub omitted: Vec<Value>,
    pub weights: Vec<[u8; 8]>,
    pub bones: u32,
    pub motion: String,
}

pub struct Surface {
    pub material: u32,
    pub model: u32,
    pub entity: u32,
    pub stage: u8,
    pub transform: [f32; 16],
    pub scale: [f32; 3],
    pub offset: [f32; 3],
    pub has_colors: bool,
    pub lens: Option<Lens>,
    pub cutout: Option<Lens>,
}

impl Surface {
    pub fn native_stage(&self) -> u8 {
        match self.stage {
            0 => 0,
            2 => 1,
            4 => 3,
            8 => 7,
            13 => 12,
            15 => {
                if self.lens.as_ref().is_some_and(|lens| lens.artwork) {
                    14
                } else {
                    7
                }
            }
            17 => 16,
            _ => unreachable!(),
        }
    }
}
fn transform(v: [f32; 3], m: &[f32; 16], point: bool) -> [f32; 3] {
    std::array::from_fn(|i| {
        m[i * 4] * v[0]
            + m[i * 4 + 1] * v[1]
            + m[i * 4 + 2] * v[2]
            + if point { m[i * 4 + 3] } else { 0. }
    })
}
pub fn read(r: &mut Reader, entries: &[Value], rig: &rig::Binding) -> Result<Mesh> {
    let mut out = Mesh {
        positions: vec![],
        attributes: vec![],
        colors: vec![],
        groups: vec![],
        surfaces: BTreeMap::new(),
        omitted: vec![],
        weights: vec![],
        bones: rig.bones,
        motion: rig.program.clone(),
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
            let weights = rig.vertices(r, &b, m, hash(entry, "entity")?, &p, &ph)?;
            let uv = r.tag(r.reference(ut)?, None)?;
            let ib = r.tag(r.reference(it)?, None)?;
            let color_tag = b.u32(m + 20)?;
            let colors = if [0, u32::MAX, 0x811c9dc5].contains(&color_tag) {
                None
            } else {
                let h = r.tag(color_tag, None)?;
                ensure!(
                    h.u16(4)? == 4 && h.u16(6)? == 5,
                    "Unsupported Marathon vertex-color format"
                );
                let data = r.tag(r.reference(color_tag)?, None)?;
                ensure!(
                    !data.0.is_empty()
                        && data.0.len().is_multiple_of(4)
                        && data.0.len() == h.u32(0)? as usize
                        && data.0.len() / 4 <= p.0.len() / 24,
                    "Invalid Marathon vertex-color buffer"
                );
                Some(data)
            };
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
            let aperture = scope::read(r, entry, &b, m, &parts, (&p, &indices, width))?;
            let mut selected = vec![];
            let mut seen = BTreeSet::new();
            for stage in 0..25 {
                let start = b.u16(m + 0x30 + stage * 2)? as usize;
                let end = b.u16(m + 0x32 + stage * 2)? as usize;
                let records = parts.get(start..end).context("part stage range")?;
                if records.is_empty() {
                    continue;
                }
                let optical = aperture
                    .as_ref()
                    .is_some_and(|lens| stage == 15 || (stage == 17 && lens.artwork));
                if ![0, 2, 4, 8, 13].contains(&stage) && !optical {
                    let active = records
                        .iter()
                        .filter(|part| b.u8(**part + 33).is_ok_and(|lod| lod <= 3))
                        .count();
                    if active > 0 {
                        out.omitted.push(json!({"model":format!("{tag:08X}"),"stage":stage,"draws":active,"reason":if stage == 24 { "Source skinning is evaluated by the native vertex program" } else { "Source renderer pass has no native adapter" }}));
                    }
                    continue;
                }
                ensure!(
                    b.u8(m + 0x64 + stage)? == 7,
                    "unhandled Marathon input layout"
                );
                for &part in parts.get(start..end).context("part stage range")? {
                    if b.u8(part + 33)? > 3 {
                        continue;
                    }
                    let start = b.u32(part + 8)? as usize;
                    let count = b.u32(part + 12)? as usize;
                    let material = b.u32(part)?;
                    // Equal index ranges can carry different decal, shadow or
                    // transparent work. Only an identical draw is redundant.
                    if !seen.insert((stage, material, start, count)) {
                        continue;
                    }
                    let faces = faces(&b, part, &indices, width, p.0.len() / 24)
                        .with_context(|| format!("Marathon model {tag:08X}, stage {stage}"))?;
                    if faces.is_empty() {
                        continue;
                    }
                    let key =
                        u32::try_from(out.surfaces.len()).context("Marathon material count")?;
                    let scale = [b.f32(0xa0)?, b.f32(0xa4)?, b.f32(0xa8)?];
                    let offset = [b.f32(0xb0)?, b.f32(0xb4)?, b.f32(0xb8)?];
                    ensure!(
                        scale.iter().all(|v| v.is_finite() && *v > 0.)
                            && offset.iter().all(|v| v.is_finite()),
                        "Invalid source model decode transform"
                    );
                    out.surfaces.insert(
                        key,
                        Surface {
                            material,
                            model: tag,
                            entity: hash(entry, "entity")?,
                            stage: stage as u8,
                            transform: mat,
                            scale,
                            offset,
                            has_colors: colors.is_some(),
                            lens: aperture
                                .as_ref()
                                .filter(|lens| {
                                    stage == 15
                                        || (stage == 8 && scope::covers(lens, &b, &p, &faces))
                                })
                                .map(|lens| lens.placed(&mat, stage == 15)),
                            cutout: aperture
                                .as_ref()
                                .filter(|lens| {
                                    !lens.outline.is_empty() && matches!(stage, 0 | 4 | 13)
                                })
                                .map(|lens| lens.placed(&mat, false)),
                        },
                    );
                    selected.push((key, faces));
                }
            }
            append(
                &mut out,
                &b,
                &mat,
                &p,
                &uv,
                colors.as_deref(),
                &weights,
                selected,
            )?;
        }
    }
    ensure!(!out.groups.is_empty(), "no render geometry");
    Ok(out)
}
type Draw = (u32, Vec<[u32; 3]>);

fn faces(
    model: &Payload,
    part: usize,
    indices: &[u32],
    width: usize,
    vertices: usize,
) -> Result<Vec<[u32; 3]>> {
    let start = model.u32(part + 8)? as usize;
    let count = model.u32(part + 12)? as usize;
    let values = indices
        .get(start..start + count)
        .context("draw outside buffer")?;
    match model.u16(part + 6)? {
        5 => crate::tiger::geometry::triangles(
            values,
            vertices,
            if width == 2 { 65535 } else { u32::MAX },
        ),
        3 => {
            ensure!(
                count.is_multiple_of(3) && values.iter().all(|i| (*i as usize) < vertices),
                "invalid triangle list"
            );
            Ok(values.chunks_exact(3).map(|f| [f[0], f[1], f[2]]).collect())
        }
        n => anyhow::bail!("unhandled Marathon primitive {n}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn append(
    out: &mut Mesh,
    b: &Payload,
    mat: &[f32; 16],
    p: &Payload,
    uv: &Payload,
    colors: Option<&Payload>,
    weights: &[[u8; 8]],
    selected: Vec<Draw>,
) -> Result<()> {
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
        // XYZ remains in source bind space. The palette retains every influence,
        // including an unskinned attachment's named parent mechanism.
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
        xyz = transform(xyz, mat, true);
        normal = transform(normal, mat, false);
        tangent = transform(tangent, mat, false);
        ensure!(
            xyz.iter()
                .chain(normal.iter())
                .chain(tangent.iter())
                .chain(tex.iter())
                .all(|v| v.is_finite()),
            "nonfinite decoded vertex"
        );
        out.positions.push(xyz);
        out.weights.push(weights[v as usize]);
        // The source vertex program clamps SV_VertexID to the last
        // color row. Reordering the geometry must reorder that lookup.
        out.colors.push(if let Some(colors) = colors {
            colors.bytes::<4>((v as usize).min(colors.0.len() / 4 - 1) * 4)?
        } else {
            [0; 4]
        });
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
    Ok(())
}

pub use crate::presentation::geometry::Encoded;
pub fn encode(mesh: &Mesh, native: &Payload, native_mesh: usize) -> Result<Encoded> {
    crate::presentation::geometry::encode_source_passes(
        &crate::presentation::geometry::Mesh {
            positions: mesh.positions.clone(),
            attributes: mesh.attributes.clone(),
            groups: mesh
                .groups
                .iter()
                .map(|(m, f)| (*m, f.iter().map(|f| f.map(u32::from)).collect()))
                .collect(),
            weights: mesh.weights.clone(),
            bones: mesh.bones,
        },
        native,
        native_mesh,
        &mesh
            .surfaces
            .iter()
            .map(|(key, surface)| (*key, surface.native_stage()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
            colors: vec![[0; 4]; 3],
            groups: vec![(123, vec![[0, 1, 2]]), (124, vec![[0, 1, 2]])],
            surfaces: [(123, 0), (124, 4)]
                .into_iter()
                .map(|(key, stage)| {
                    (
                        key,
                        Surface {
                            material: key,
                            model: 1,
                            entity: 2,
                            stage,
                            transform: [0.; 16],
                            scale: [1.; 3],
                            offset: [0.; 3],
                            has_colors: false,
                            lens: None,
                            cutout: None,
                        },
                    )
                })
                .collect(),
            omitted: vec![],
            weights: vec![],
            bones: 1,
            motion: String::new(),
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
