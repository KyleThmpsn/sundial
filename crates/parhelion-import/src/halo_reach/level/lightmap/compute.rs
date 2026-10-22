//! Row-bounded GPU work. The CPU still owns atlas placement and whole-level normalization.
use super::{LAYERS, Texel};
use crate::compute::{self, Input};
use anyhow::{Result, ensure};

const PIXELS: usize = 1_048_576;
const STRIDE: usize = 32;
const DECODE: &str = include_str!("compute/decode.hlsl");
const ENCODE: &str = include_str!("compute/encode.hlsl");

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

pub(super) fn decode(
    width: usize,
    height: usize,
    layers: &[Vec<u8>],
    direction: &[u8],
    brightness: f32,
) -> Result<Option<Vec<Texel>>> {
    if !compute::eligible(width * height)? {
        return Ok(None);
    }
    ensure!(
        layers.len() == LAYERS,
        "Lightmap intensity layer count differs"
    );
    compute::checked(decode_chunks(width, height, layers, direction, brightness))
}

fn decode_chunks(
    width: usize,
    height: usize,
    layers: &[Vec<u8>],
    direction: &[u8],
    brightness: f32,
) -> Result<Option<Vec<Texel>>> {
    let rows = (PIXELS / width / 4).max(1) * 4;
    let mut output = Vec::with_capacity(width * height);
    for row in (0..height).step_by(rows) {
        let count = rows.min(height - row);
        let start = row / 4 * (width / 4) * 16;
        let end = start + count / 4 * (width / 4) * 16;
        let inputs = [
            Input {
                bytes: &layers[0][start..end],
                stride: 16,
            },
            Input {
                bytes: &layers[1][start..end],
                stride: 16,
            },
            Input {
                bytes: &layers[2][start..end],
                stride: 16,
            },
            Input {
                bytes: &direction[start..end],
                stride: 16,
            },
        ];
        let Some(bytes) = compute::dispatch(
            DECODE,
            [width as u32, count as u32, brightness.to_bits(), 0],
            &inputs,
            STRIDE,
            width * count,
        )?
        else {
            return Ok(None);
        };
        for bytes in bytes.chunks_exact(STRIDE) {
            let lobe = [0, 4, 8].map(|at| f32::from_bits(word(bytes, at)));
            let ambient = [12, 16, 20].map(|at| f32::from_bits(word(bytes, at)));
            ensure!(
                lobe.into_iter()
                    .chain(ambient)
                    .all(|v| v.is_finite() && v >= 0.),
                "Hardware lightmap conversion produced invalid lighting"
            );
            ensure!(
                word(bytes, 28) <= 255,
                "Hardware lightmap conversion produced invalid visibility"
            );
            output.push(Texel {
                lobe,
                ambient,
                direction: [bytes[24], bytes[25], bytes[26]],
                bandwidth: bytes[27],
                sun: bytes[28],
            });
        }
    }
    ensure!(
        output.len() == width * height,
        "Hardware lightmap conversion is incomplete"
    );
    Ok(Some(output))
}

pub(super) fn encode(texels: &[Texel], peak: f32) -> Result<Option<Vec<u8>>> {
    if !compute::eligible(texels.len())? {
        return Ok(None);
    }
    ensure!(
        peak.is_finite() && peak > 0.,
        "Invalid lightmap encoding peak"
    );
    compute::checked(encode_chunks(texels, peak))
}

fn encode_chunks(texels: &[Texel], peak: f32) -> Result<Option<Vec<u8>>> {
    let mut output = Vec::with_capacity(texels.len() * 12);
    for chunk in texels.chunks(PIXELS) {
        let mut input = Vec::with_capacity(chunk.len() * STRIDE);
        for texel in chunk {
            for value in texel.lobe.into_iter().chain(texel.ambient) {
                input.extend_from_slice(&value.to_le_bytes());
            }
            input.extend_from_slice(&[
                texel.direction[0],
                texel.direction[1],
                texel.direction[2],
                texel.bandwidth,
            ]);
            input.extend_from_slice(&[texel.sun, 0, 0, 0]);
        }
        let Some(bytes) = compute::dispatch(
            ENCODE,
            [chunk.len() as u32, peak.to_bits(), 0, 0],
            &[Input {
                bytes: &input,
                stride: STRIDE,
            }],
            12,
            chunk.len(),
        )?
        else {
            return Ok(None);
        };
        output.extend(bytes);
    }
    ensure!(
        output.len() == texels.len() * 12,
        "Hardware lightmap encoding is incomplete"
    );
    Ok(Some(output))
}
