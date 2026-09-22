use super::{load, put};
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use half::f16;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

pub struct Part {
    header: Payload,
    positions: Payload,
    attributes: Payload,
    indices: Payload,
    mapping: Value,
    materials: BTreeMap<String, String>,
}

pub struct Merged {
    pub header: Vec<u8>,
    pub streams: [Vec<u8>; 3],
    pub patches: Vec<Value>,
    pub count: usize,
    pub position_error: f64,
    pub radius: f32,
}

fn detail_half(value: f64) -> u16 {
    // Keep every discarded bit when resolving ties. Hardware conversion can
    // round through f32, and truncated-mantissa fallbacks can lose a sticky bit.
    let bits = value.to_bits();
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exponent = ((bits >> 52) & 0x7FF) as i32 - 1023;
    if exponent < -25 {
        return sign;
    }
    let significand = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
    let shift = if exponent < -14 {
        (28 - exponent) as u32
    } else {
        42
    };
    let retained = significand >> shift;
    let discarded = significand & ((1u64 << shift) - 1);
    let halfway = 1u64 << (shift - 1);
    let rounded =
        retained + u64::from(discarded > halfway || (discarded == halfway && retained & 1 != 0));
    let magnitude = if exponent < -14 {
        rounded
    } else {
        (((exponent + 15) as u64) << 10) + rounded - 1024
    };
    sign | magnitude as u16
}

pub fn load_raw(path: &Path, materials: BTreeMap<String, String>) -> Result<Part> {
    Ok(Part {
        header: Payload(fs::read(path.join("model.unlinked.bin"))?),
        positions: Payload(fs::read(path.join("positions.bin"))?),
        attributes: Payload(fs::read(path.join("attributes.bin"))?),
        indices: Payload(fs::read(path.join("indices.bin"))?),
        mapping: load(&path.join("mapping.json"))?,
        materials,
    })
}

pub fn load_part(
    path: &Path,
    materials: BTreeMap<String, String>,
    rect: [usize; 4],
    size: [usize; 2],
) -> Result<(Part, f64)> {
    let mut h = Payload(fs::read(path.join("model.unlinked.bin"))?);
    let mut a = Payload(fs::read(path.join("attributes.bin"))?);
    let mut error = 0f64;
    ensure!(a.0.len() % 24 == 0, "attribute stride differs");
    for at in (0..a.0.len()).step_by(24) {
        for axis in 0..2 {
            let uv = a.i16(at + axis * 2)? as f64 / 32767.0 * h.f32(0x70 + axis * 4)? as f64
                + h.f32(0x78 + axis * 4)? as f64;
            let detail = uv * f16::from_bits(a.u16(at + 20 + axis * 2)?).to_f64();
            ensure!(
                detail.is_finite() && detail.abs() <= 65504.0,
                "detail UV exceeds finite half precision"
            );
            let half = f16::from_bits(detail_half(detail));
            error = error.max((detail - half.to_f64()).abs());
            put(&mut a.0, at + 20 + axis * 2, &half.to_bits().to_le_bytes())?;
        }
    }
    for axis in 0..2 {
        let scale =
            (h.f32(0x70 + axis * 4)? as f64 * rect[axis + 2] as f64 / size[axis] as f64) as f32;
        let offset = ((h.f32(0x78 + axis * 4)? as f64 * rect[axis + 2] as f64 + rect[axis] as f64)
            / size[axis] as f64) as f32;
        put(&mut h.0, 0x70 + axis * 4, &scale.to_le_bytes())?;
        put(&mut h.0, 0x78 + axis * 4, &offset.to_le_bytes())?;
    }
    Ok((
        Part {
            header: h,
            positions: Payload(fs::read(path.join("positions.bin"))?),
            attributes: a,
            indices: Payload(fs::read(path.join("indices.bin"))?),
            mapping: load(&path.join("mapping.json"))?,
            materials,
        },
        error,
    ))
}

fn bounds<const N: usize>(values: &[[f64; N]]) -> Result<([f64; N], [f64; N])> {
    ensure!(!values.is_empty(), "no model vertices");
    let mut scale = [0.; N];
    let mut offset = [0.; N];
    for axis in 0..N {
        let low = values.iter().map(|v| v[axis]).fold(f64::INFINITY, f64::min);
        let high = values
            .iter()
            .map(|v| v[axis])
            .fold(f64::NEG_INFINITY, f64::max);
        scale[axis] = ((high - low) / 2.).max(1e-6);
        offset[axis] = (high + low) / 2.;
    }
    Ok((scale, offset))
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub fn merge(parts: Vec<Part>, max_bone: u16) -> Result<Merged> {
    ensure!(!parts.is_empty(), "no converted models");
    let mut decoded = vec![];
    let mut uvs = vec![];
    let mut positions = vec![];
    let mut attributes = vec![];
    let mut bone_count = parts[0].header.u32(0x40)?;
    for part in &parts {
        ensure!(
            part.positions.0.len() / 8 == part.attributes.0.len() / 24,
            "vertex counts differ"
        );
        for v in 0..part.positions.0.len() / 8 {
            let bone = part.positions.u16(v * 8 + 6)?;
            ensure!(bone <= max_bone, "bone selector exceeds validated rig");
            bone_count = bone_count.max(u32::from(bone) + 1);
            let mut xyz = [0.; 3];
            for (axis, value) in xyz.iter_mut().enumerate() {
                *value = part.positions.i16(v * 8 + axis * 2)? as f64 / 32767.
                    * part.header.f32(0x50 + axis * 4)? as f64
                    + part.header.f32(0x60 + axis * 4)? as f64;
            }
            let mut uv = [0.; 2];
            for (axis, value) in uv.iter_mut().enumerate() {
                *value = part.attributes.i16(v * 24 + axis * 2)? as f64 / 32767.
                    * part.header.f32(0x70 + axis * 4)? as f64
                    + part.header.f32(0x78 + axis * 4)? as f64;
            }
            decoded.push(xyz);
            uvs.push(uv);
        }
        positions.extend_from_slice(&part.positions.0);
        attributes.extend_from_slice(&part.attributes.0);
    }
    ensure!(decoded.len() < 65535, "merged model exceeds 16-bit indices");
    let (scale, offset) = bounds(&decoded)?;
    let scale = scale.into_iter().fold(0., f64::max) as f32 as f64;
    let offset = offset.map(|v| v as f32 as f64);
    let (us, uo) = bounds(&uvs)?;
    let us = us.map(|v| v as f32 as f64);
    let uo = uo.map(|v| v as f32 as f64);
    let mut error = 0f64;
    for (v, (xyz, uv)) in decoded.iter().zip(&uvs).enumerate() {
        for axis in 0..3 {
            let q = ((xyz[axis] - offset[axis]) / scale * 32767.)
                .round_ties_even()
                .clamp(-32767., 32767.) as i16;
            put(&mut positions, v * 8 + axis * 2, &q.to_le_bytes())?;
            error = error.max((q as f64 / 32767. * scale + offset[axis] - xyz[axis]).abs());
        }
        for axis in 0..2 {
            let q = ((uv[axis] - uo[axis]) / us[axis] * 32767.)
                .round_ties_even()
                .clamp(-32767., 32767.) as i16;
            put(&mut attributes, v * 24 + axis * 2, &q.to_le_bytes())?;
        }
    }
    let mut h = parts[0]
        .header
        .0
        .get(..0x150)
        .context("short native model")?
        .to_vec();
    // A rigid carrier can declare only one matrix. Merged attachments may use
    // more of the validated rig, so cover their selectors in the model palette.
    put(&mut h, 0x40, &bone_count.to_le_bytes())?;
    for (axis, value) in offset.iter().enumerate() {
        put(&mut h, 0x50 + axis * 4, &(scale as f32).to_le_bytes())?;
        put(&mut h, 0x60 + axis * 4, &(*value as f32).to_le_bytes())?;
    }
    put(&mut h, 0x6C, &(scale as f32).to_le_bytes())?;
    for axis in 0..2 {
        put(&mut h, 0x70 + axis * 4, &(us[axis] as f32).to_le_bytes())?;
        put(&mut h, 0x78 + axis * 4, &(uo[axis] as f32).to_le_bytes())?;
    }
    let mut ib = vec![];
    let mut patches = vec![
        json!({"offset":0xB0,"symbol":"positions-header"}),
        json!({"offset":0xB4,"symbol":"attributes-header"}),
        json!({"offset":0xC0,"symbol":"indices-header"}),
    ];
    let mut count = 0;
    let mut counts = vec![0usize];
    for stage in 0..23 {
        let mut base = 0;
        for part in &parts {
            let rows = part.header.array(0xC8, 32, None)?;
            let groups = part.mapping["plated_groups"]
                .as_array()
                .context("plated groups")?;
            ensure!(
                rows.len() == groups.len(),
                "material provenance differs from draw count"
            );
            for (&row, group) in rows.iter().zip(groups) {
                if group["stage"].as_u64() != Some(stage as u64) {
                    continue;
                }
                let mut record = part.header.bytes::<32>(row)?.to_vec();
                let start = part.header.u32(row + 8)? as usize;
                let length = part.header.u32(row + 12)? as usize;
                put(&mut record, 8, &u32::try_from(ib.len() / 2)?.to_le_bytes())?;
                for i in start..start + length {
                    let index = part.indices.u16(i * 2)?;
                    let value = if index == 65535 {
                        index
                    } else {
                        u16::try_from(index as usize + base)?
                    };
                    ib.extend(value.to_le_bytes());
                }
                put(&mut record, 22, &u16::try_from(count)?.to_le_bytes())?;
                let symbol = if stage == 0 {
                    let key = format!(
                        "{}:{}",
                        group["source_material"]
                            .as_str()
                            .context("source material")?,
                        group["channel"]
                    );
                    part.materials
                        .get(&key)
                        .or_else(|| {
                            group["source_material"]
                                .as_str()
                                .and_then(|s| part.materials.get(s))
                        })
                        .context("missing native material")?
                        .as_str()
                } else {
                    "material-plated-80EC271D-stage-3"
                };
                put(&mut record, 0, &u32::MAX.to_le_bytes())?;
                patches.push(json!({"offset":h.len(),"symbol":symbol}));
                h.extend(record);
                count += 1;
            }
            base += part.positions.0.len() / 8;
        }
        counts.push(count);
    }
    let len = h.len() as u64;
    put(&mut h, 0, &len.to_le_bytes())?;
    put(&mut h, 0xC8, &(count as u64).to_le_bytes())?;
    put(&mut h, 0x140, &(count as u64).to_le_bytes())?;
    for (i, value) in counts.iter().enumerate() {
        put(&mut h, 0xD8 + i * 2, &u16::try_from(*value)?.to_le_bytes())?;
    }
    for stage in 0..23 {
        put(
            &mut h,
            0x108 + stage * 2,
            &(if [0, 3].contains(&stage) { 139i16 } else { -1 }).to_le_bytes(),
        )?;
    }
    for at in [0x9C, 0x13C] {
        put(&mut h, at, &0x80809FBDu32.to_le_bytes())?;
    }
    let radius = expand_bounds(&mut h, &positions)?;
    Ok(Merged {
        header: h,
        streams: [positions, attributes, ib],
        patches,
        count,
        position_error: error,
        radius,
    })
}

fn expand_bounds(header: &mut [u8], positions: &[u8]) -> Result<f32> {
    let h = Payload(header.to_vec());
    let old = h.f32(0x8C)?;
    ensure!(
        h.f32(0x20)? == old && h.f32(0x24)? == old,
        "native radius fields differ"
    );
    let scale = h.f32(0x6C)? as f64;
    let p = Payload(positions.to_vec());
    let mut radius = 0f64;
    for at in (0..positions.len()).step_by(8) {
        let mut squared = 0.;
        for axis in 0..3 {
            squared += (p.i16(at + axis * 2)? as f64 / 32767. * scale).powi(2);
        }
        radius = radius.max(squared.sqrt());
    }
    let mut rounded = radius as f32;
    if (rounded as f64) < radius {
        rounded = f32::from_bits(rounded.to_bits() + 1);
    }
    for at in [0x20, 0x24, 0x8C] {
        put(header, at, &rounded.to_le_bytes())?;
    }
    put(header, 0x80, &h.bytes::<12>(0x60)?)?;
    Ok(rounded)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachments_keep_distinct_transforms_indices_and_bones() {
        let part = |offset: f32, bone: u16| {
            let mut h = vec![0; 0x170];
            put(&mut h, 0x40, &1u32.to_le_bytes()).unwrap();
            for axis in 0..3 {
                put(&mut h, 0x50 + 4 * axis, &1f32.to_le_bytes()).unwrap();
            }
            put(&mut h, 0x60, &offset.to_le_bytes()).unwrap();
            put(&mut h, 0xC8, &1u64.to_le_bytes()).unwrap();
            put(&mut h, 0xD0, &0x70i64.to_le_bytes()).unwrap();
            put(&mut h, 0x140, &1u64.to_le_bytes()).unwrap();
            put(&mut h, 0x15C, &3u32.to_le_bytes()).unwrap();
            let positions = [[0i16, 0, 0], [32767, 0, 0], [0, 32767, 0]]
                .into_iter()
                .flat_map(|v| {
                    v.into_iter()
                        .flat_map(i16::to_le_bytes)
                        .chain(bone.to_le_bytes())
                })
                .collect();
            Part {
                header: Payload(h),
                positions: Payload(positions),
                attributes: Payload(vec![0; 72]),
                indices: Payload(
                    [0u16, 1, 2]
                        .into_iter()
                        .flat_map(u16::to_le_bytes)
                        .collect(),
                ),
                mapping: json!({"plated_groups":[{"stage":0,"source_material":"source"}]}),
                materials: BTreeMap::from([("source".into(), format!("material-{offset}"))]),
            }
        };
        assert!(merge(vec![part(0., 0), part(10., 2)], 0).is_err());
        let result = merge(vec![part(0., 0), part(10., 2)], 2).unwrap();
        assert_eq!(result.count, 2);
        assert_eq!(
            result.streams[2],
            [0u16, 1, 2, 3, 4, 5]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        let h = Payload(result.header);
        assert_eq!(h.u32(0x40).unwrap(), 3);
        let p = Payload(result.streams[0].clone());
        assert_eq!(p.u16(3 * 8 + 6).unwrap(), 2);
        let scale = h.f32(0x6C).unwrap() as f64;
        for (v, expected) in [
            [0., 0., 0.],
            [1., 0., 0.],
            [0., 1., 0.],
            [10., 0., 0.],
            [11., 0., 0.],
            [10., 1., 0.],
        ]
        .iter()
        .enumerate()
        {
            for (axis, value) in expected.iter().enumerate() {
                assert!(
                    (p.i16(v * 8 + axis * 2).unwrap() as f64 / 32767. * scale
                        + h.f32(0x60 + axis * 4).unwrap() as f64
                        - value)
                        .abs()
                        < 0.0002
                );
            }
        }
        let mut carrier = part(0., 0);
        put(&mut carrier.header.0, 0x40, &8u32.to_le_bytes()).unwrap();
        let result = merge(vec![carrier, part(1., 2)], 7).unwrap();
        assert_eq!(Payload(result.header).u32(0x40).unwrap(), 8);
    }
    #[test]
    fn detail_uv_does_not_round_through_single_precision() {
        let value = 1.00146484375 - 2f64.powi(-40);
        assert_eq!(detail_half(value), 0x3C01);
        assert_eq!(detail_half(1.00048828125 + 2f64.powi(-40)), 0x3C01);
        for bits in 0..0x7C00 {
            let value = f16::from_bits(bits).to_f64();
            assert_eq!(detail_half(value), bits);
            assert_eq!(detail_half(-value), bits | 0x8000);
        }
    }
    #[test]
    fn attachment_expands_sphere_outward_after_quantization() {
        let mut h = vec![0; 0x150];
        for at in [0x20, 0x24, 0x8C] {
            put(&mut h, at, &0.5f32.to_le_bytes()).unwrap();
        }
        put(&mut h, 0x6C, &1f32.to_le_bytes()).unwrap();
        let pos = [32767i16, 32767, 0, 0]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        let radius = expand_bounds(&mut h, &pos).unwrap();
        assert!((radius as f64).powi(2) >= 2.);
        assert!(radius < 1.415);
    }
}
