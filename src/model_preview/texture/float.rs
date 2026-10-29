//! Preserve native float values, including signed block-compressed normal channels.
use super::*;

pub(in crate::model_preview) fn decode(
    bytes: &[u8],
    format: u32,
    width: usize,
    height: usize,
) -> Result<Option<Vec<[f32; 4]>>, String> {
    let count = width
        .checked_mul(height)
        .ok_or("Invalid float texture dimensions")?;
    let pixels = match format {
        10 => bytes
            .get(..count * 8)
            .ok_or("Truncated half-float texture")?
            .chunks_exact(8)
            .map(|p| {
                std::array::from_fn(|i| half_to_f32(u16::from_le_bytes([p[i * 2], p[i * 2 + 1]])))
            })
            .collect::<Vec<_>>(),
        26 => bytes
            .get(..count * 4)
            .ok_or("Truncated packed-float texture")?
            .chunks_exact(4)
            .map(|p| {
                let value = u32::from_le_bytes(p.try_into().unwrap());
                [
                    unsigned(value & 0x7FF, 6),
                    unsigned((value >> 11) & 0x7FF, 6),
                    unsigned((value >> 22) & 0x3FF, 5),
                    1.0,
                ]
            })
            .collect(),
        84 => super::banded(bytes, 16, [width, height], |band, rows| {
            bc5(band, width, rows)
        })?,
        _ => return Ok(None),
    };
    if pixels.iter().flatten().any(|v| !v.is_finite()) {
        return Err("The float texture has non-finite pixel values".into());
    }
    Ok(Some(pixels))
}

fn bc5(bytes: &[u8], width: usize, height: usize) -> Result<Vec<[f32; 4]>, String> {
    let columns = width.div_ceil(4);
    let rows = height.div_ceil(4);
    let source = bytes
        .get(..columns * rows * 16)
        .ok_or("Truncated signed BC5 texture")?;
    let mut pixels = vec![[0.0, 0.0, 0.0, 1.0]; width * height];
    for (index, block) in source.chunks_exact(16).enumerate() {
        let mut values = [0.0; 32];
        bcdec_rs::bc5_float(block, &mut values, 8, true);
        for y in 0..4 {
            for x in 0..4 {
                let px = (index % columns) * 4 + x;
                let py = (index / columns) * 4 + y;
                if px < width && py < height {
                    let from = (y * 4 + x) * 2;
                    pixels[py * width + px] = [values[from], values[from + 1], 0.0, 1.0];
                }
            }
        }
    }
    Ok(pixels)
}

fn unsigned(bits: u32, fraction: u32) -> f32 {
    let exponent = (bits >> fraction) as i32;
    let mantissa = (bits & ((1 << fraction) - 1)) as f32 / (1 << fraction) as f32;
    match exponent {
        0 => mantissa * 2f32.powi(-14),
        31 => {
            if mantissa == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + mantissa) * 2f32.powi(exponent - 15),
    }
}
