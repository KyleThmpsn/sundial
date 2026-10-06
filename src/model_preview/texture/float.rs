//! Preserve the native linear values of half and packed unsigned-float textures.
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
        _ => return Ok(None),
    };
    if pixels.iter().flatten().any(|v| !v.is_finite()) {
        return Err("The float texture has non-finite pixel values".into());
    }
    Ok(Some(pixels))
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
