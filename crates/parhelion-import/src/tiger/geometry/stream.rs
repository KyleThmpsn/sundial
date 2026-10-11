//! Checked source vertex streams, with float cloth normalized for native carriers.
use super::*;
use std::{fs, path::Path};

pub(crate) struct Streams {
    /// Source packed position, normal and tangent records, including skin selectors.
    pub positions: Vec<u8>,
    pub auxiliary: Vec<u8>,
    /// Retain exact float positions for source exports and shader contract selection.
    pub float_positions: Option<Payload>,
}

pub(crate) fn raw(root: &Path, tag: u32) -> Result<Payload> {
    Ok(Payload(fs::read(root.join(format!("raw/{tag:08X}.bin")))?))
}

pub(crate) fn buffer(root: &Path, manifest: &Value, tag: u32) -> Result<Payload> {
    let reference = u32::try_from(
        manifest["tags"][format!("{tag:08X}")]["reference"]
            .as_u64()
            .context("source buffer provenance")?,
    )?;
    raw(root, reference)
}

pub(crate) fn streams(
    root: &Path,
    manifest: &Value,
    model: &Payload,
    mesh: usize,
) -> Result<Streams> {
    read(model, mesh, &mut |tag| {
        Ok((raw(root, tag)?, buffer(root, manifest, tag)?))
    })
}

fn checked(header: &Payload, data: &Payload, stride: usize, code: u16) -> Result<()> {
    ensure!(
        header.u16(4)? as usize == stride && header.u16(6)? == code,
        "source vertex declaration differs from its stream"
    );
    ensure!(
        header.u32(0)? as usize == data.0.len() && data.0.len().is_multiple_of(stride),
        "source vertex payload differs from its header"
    );
    Ok(())
}

pub(super) fn read(
    model: &Payload,
    mesh: usize,
    read: &mut dyn FnMut(u32) -> Result<(Payload, Payload)>,
) -> Result<Streams> {
    let (header, positions) = read(model.u32(mesh)?)?;
    let stride = header.u16(4)? as usize;
    let code = header.u16(6)?;
    ensure!(
        matches!((stride, code), (24, 0 | 1) | (48, 1)),
        "unsupported source vertex declaration {stride}/{code}"
    );
    checked(&header, &positions, stride, code)?;
    let count = positions.0.len() / stride;
    ensure!(count > 0, "source vertex stream is empty");
    let (header, uv) = read(model.u32(mesh + 4)?)?;
    checked(&header, &uv, 4, code)?;
    ensure!(uv.0.len() / 4 == count, "source vertex counts differ");
    if stride == 48 {
        let (header, skin) = read(model.u32(mesh + 8)?)?;
        checked(&header, &skin, 8, 1)?;
        ensure!(
            skin.0.len() / 8 == count,
            "cloth skin count differs from its vertices"
        );
        return float(positions, &skin);
    }
    if code == 0 {
        ensure!(
            (0..24).all(|stage| model.u8(mesh + 98 + stage).ok() == Some(7)),
            "unskinned packed input layout differs"
        );
        let mut positions = positions.0;
        for vertex in positions.chunks_exact_mut(24) {
            ensure!(
                i16::from_le_bytes(vertex[6..8].try_into()?) == 32767,
                "unskinned position lacks homogeneous W"
            );
            // This declaration stores a homogeneous position, not a skin selector.
            vertex[6..8].fill(0);
        }
        return Ok(Streams {
            positions,
            auxiliary: Vec::new(),
            float_positions: None,
        });
    }
    let auxiliary =
        if positions.0.chunks_exact(24).any(|v| {
            crate::tiger::skinning::selector(v).is_ok_and(crate::tiger::skinning::weighted)
        }) {
            read(model.u32(mesh + 24)?)?.1.0
        } else {
            Vec::new()
        };
    crate::tiger::skinning::used(&positions.0, &auxiliary)?;
    Ok(Streams {
        positions: positions.0,
        auxiliary,
        float_positions: None,
    })
}

fn quantized(value: f32) -> Result<[u8; 2]> {
    ensure!(
        value.is_finite() && value.abs() <= 1.000_01,
        "float cloth component exceeds normalized vertex storage"
    );
    Ok(((value.clamp(-1., 1.) * 32767.).round_ties_even() as i16).to_le_bytes())
}

fn float(original: Payload, skin: &Payload) -> Result<Streams> {
    let count = original.0.len() / 48;
    let mut positions = Vec::with_capacity(count * 24);
    // Four influences occupy two four-byte source auxiliary records per vertex.
    let mut auxiliary = vec![0; count.div_ceil(4) * 32];
    for index in 0..count {
        crate::cancellation::check()?;
        let at = index * 48;
        ensure!(
            original.f32(at + 12)? == 1. && original.f32(at + 28)? == 0.,
            "float cloth position or normal metadata differs"
        );
        for axis in 0..3 {
            positions.extend_from_slice(&quantized(original.f32(at + axis * 4)?)?);
        }
        let selector = -i16::try_from(0x800 + index / 4)?;
        positions.extend_from_slice(&selector.to_le_bytes());
        for lane in 4..12 {
            positions.extend_from_slice(&quantized(original.f32(at + lane * 4)?)?);
        }
        let row = &skin.0[index * 8..index * 8 + 8];
        ensure!(
            row[..4].iter().map(|v| u16::from(*v)).sum::<u16>() == 255,
            "cloth skin weights do not sum to 255"
        );
        for lane in 0..4 {
            let weight = row[lane];
            let bone = row[lane + 4];
            ensure!(
                weight == 0 || bone < 254,
                "nonzero cloth weight uses a missing bone"
            );
            let start = index * 8 + (lane / 2) * 4;
            auxiliary[start + lane % 2] = if weight == 0 { 0 } else { bone };
            auxiliary[start + lane % 2 + 2] = weight;
        }
    }
    Ok(Streams {
        positions,
        auxiliary,
        float_positions: Some(original),
    })
}
