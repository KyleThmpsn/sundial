//! MCC U13 transform channels. Masks keep absent bind channels distinct from identity.
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

mod curve;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Track {
    pub rotation: Vec<[f32; 4]>,
    pub translation: Vec<[f32; 3]>,
    pub scale: Vec<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Motion {
    pub frames: usize,
    pub tracks: Vec<Track>,
}

#[derive(Default)]
struct Channels {
    rotations: Vec<Vec<[f32; 4]>>,
    translations: Vec<Vec<[f32; 3]>>,
    scales: Vec<Vec<f32>>,
}

fn unit(q: [f32; 4]) -> Result<[f32; 4]> {
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    ensure!(
        length.is_finite() && length > 1e-6,
        "Invalid animation rotation"
    );
    Ok(q.map(|v| v / length))
}

fn floats<const N: usize>(p: &Payload, at: usize) -> Result<[f32; N]> {
    let mut values = [0.; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = p.f32(at + index * 4)?;
    }
    Ok(values)
}

fn flat(p: &Payload, frames: usize) -> Result<Channels> {
    let codec = p.u8(0)?;
    let counts = [p.u8(1)? as usize, p.u8(2)? as usize, p.u8(3)? as usize];
    let rotation_width = if codec == 8 { 16 } else { 8 };
    let sizes = [rotation_width * frames, 12 * frames, 4 * frames];
    // Static headers leave all three block strides zero. Their samples are consecutive.
    if codec != 1 {
        for (index, size) in sizes.iter().enumerate() {
            ensure!(
                p.u32(20 + index * 4)? as usize == *size,
                "Animation block stride differs"
            );
        }
    }
    let translations = 32 + counts[0] * sizes[0];
    let scales = translations + counts[1] * sizes[1];
    ensure!(
        p.u32(12)? as usize == translations && p.u32(16)? as usize == scales,
        "Animation channel offsets differ"
    );
    ensure!(
        scales + counts[2] * sizes[2] <= p.0.len(),
        "Animation samples exceed section"
    );
    let mut output = Channels::default();
    for node in 0..counts[0] {
        let mut values = Vec::with_capacity(frames);
        for frame in 0..frames {
            let at = 32 + node * sizes[0] + frame * rotation_width;
            let q = if codec == 8 {
                floats(p, at)?
            } else {
                [p.i16(at)?, p.i16(at + 2)?, p.i16(at + 4)?, p.i16(at + 6)?]
                    .map(|v| f32::from(v) / 32767.)
            };
            values.push(unit(q)?);
        }
        output.rotations.push(values);
    }
    for node in 0..counts[1] {
        output.translations.push(
            (0..frames)
                .map(|frame| floats(p, translations + node * sizes[1] + frame * 12))
                .collect::<Result<_>>()?,
        );
    }
    for node in 0..counts[2] {
        output.scales.push(
            (0..frames)
                .map(|frame| p.f32(scales + node * sizes[2] + frame * 4))
                .collect::<Result<_>>()?,
        );
    }
    Ok(output)
}

fn section(bytes: &[u8], frames: usize, fixed: bool) -> Result<Channels> {
    if bytes.is_empty() {
        return Ok(Channels::default());
    }
    let p = Payload(bytes.to_vec());
    let codec = p.u8(0)?;
    ensure!(
        (fixed && codec == 1) || (!fixed && matches!(codec, 3 | 8 | 9)),
        "Unsupported Reach animation codec {codec}"
    );
    if codec == 9 {
        curve::decode(&p, frames)
    } else {
        flat(&p, frames)
    }
}

fn masks(bytes: &[u8], nodes: usize) -> Result<[Vec<usize>; 3]> {
    if bytes.is_empty() {
        return Ok(Default::default());
    }
    ensure!(
        bytes.len().is_multiple_of(12) && bytes.len() / 3 * 8 >= nodes,
        "Animation masks do not cover the skeleton"
    );
    let mut output: [Vec<usize>; 3] = Default::default();
    for (kind, mask) in bytes.chunks_exact(bytes.len() / 3).enumerate() {
        for node in 0..mask.len() * 8 {
            if mask[node / 8] & (1 << (node % 8)) != 0 {
                ensure!(
                    node < nodes,
                    "Animation mask selects a node outside the skeleton"
                );
                output[kind].push(node);
            }
        }
    }
    Ok(output)
}

/// Decode transform channels without inventing a rest pose for omitted nodes.
pub(super) fn decode(bytes: &[u8], frames: usize, nodes: usize, sizes: &[u32]) -> Result<Motion> {
    ensure!(
        sizes.len() == 17
            && (1..=4096).contains(&frames)
            && (1..=255).contains(&nodes)
            && frames * nodes <= 500_000,
        "Animation exceeds the bounded motion profile"
    );
    let mut sections = Vec::new();
    let mut start = 0;
    for &size in &sizes[..4] {
        let end = start + size as usize;
        sections.push(
            bytes
                .get(start..end)
                .context("Animation section exceeds resource")?,
        );
        start = end;
    }
    let fixed = section(sections[0], 1, true)?;
    let dynamic = section(sections[1], frames, false)?;
    let fixed_masks = masks(sections[2], nodes)?;
    let dynamic_masks = masks(sections[3], nodes)?;
    let mut tracks = vec![Track::default(); nodes];
    for (channels, masks) in [(fixed, fixed_masks), (dynamic, dynamic_masks)] {
        ensure!(
            channels.rotations.len() == masks[0].len()
                && channels.translations.len() == masks[1].len()
                && channels.scales.len() == masks[2].len(),
            "Animation masks and channel counts disagree"
        );
        for (node, values) in masks[0].iter().copied().zip(channels.rotations) {
            ensure!(
                tracks[node].rotation.is_empty(),
                "Static and animated rotations overlap"
            );
            tracks[node].rotation = values;
        }
        for (node, values) in masks[1].iter().copied().zip(channels.translations) {
            ensure!(
                tracks[node].translation.is_empty(),
                "Static and animated positions overlap"
            );
            tracks[node].translation = values
                .into_iter()
                .map(|v| v.map(|v| v * super::super::rig::METRES_PER_UNIT))
                .collect();
        }
        for (node, values) in masks[2].iter().copied().zip(channels.scales) {
            ensure!(
                tracks[node].scale.is_empty(),
                "Static and animated scales overlap"
            );
            tracks[node].scale = values;
        }
    }
    Ok(Motion { frames, tracks })
}
