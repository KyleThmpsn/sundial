//! Shadowkeep GPU input format codes, not DXGI enumeration values.
use super::*;

pub(super) fn size(format: u8) -> Result<usize, String> {
    match format {
        1 | 5 | 6 | 7 | 10 | 12 | 14 | 15 | 16 | 17 | 18 | 21 | 31 | 32 => Ok(4),
        2 | 8 | 9 | 11 | 13 | 19 | 22 | 33 => Ok(8),
        3 => Ok(12),
        4 | 20 | 23 => Ok(16),
        24 => Ok(2),
        25 => Ok(1),
        _ => Err(format!("Unsupported native vertex format {format}")),
    }
}

pub(super) fn read(format: u8, bytes: &[u8]) -> Result<[f32; 4], String> {
    let bytes = bytes
        .get(..size(format)?)
        .ok_or("Vertex attribute exceeds its buffer")?;
    let mut value = [0.0; 4];
    match format {
        1..=4 => {
            for (i, row) in bytes.chunks_exact(4).enumerate() {
                value[i] = f32::from_le_bytes(row.try_into().unwrap());
            }
        }
        5 | 6 | 14 | 15 | 25 | 31 => {
            for (i, &v) in bytes.iter().enumerate() {
                value[i] = match format {
                    5 | 25 | 31 => f32::from(v) / 255.0,
                    6 => f32::from(v),
                    14 => f32::from(v as i8),
                    _ => (f32::from(v as i8) / 127.0).max(-1.0),
                };
            }
        }
        7..=13 | 24 | 33 => {
            for (i, row) in bytes.chunks_exact(2).enumerate() {
                let v = u16::from_le_bytes(row.try_into().unwrap());
                value[i] = match format {
                    7 | 8 | 24 => f32::from(v as i16),
                    9 => f32::from(v),
                    12 | 13 => half(v),
                    _ => (f32::from(v as i16) / 32767.0).max(-1.0),
                };
            }
        }
        16 | 17 => {
            let packed = u32_at(bytes, 0)?;
            for (i, out) in value.iter_mut().enumerate() {
                let max = if i == 3 { 3 } else { 1023 };
                *out = ((packed >> (i * 10)) & max) as f32;
                if format == 17 {
                    *out /= max as f32;
                }
            }
        }
        18..=23 => {
            for (i, row) in bytes.chunks_exact(4).enumerate() {
                let v = u32::from_le_bytes(row.try_into().unwrap());
                value[i] = if format <= 20 {
                    v as i32 as f32
                } else {
                    v as f32
                };
            }
        }
        32 => {
            let packed = u32_at(bytes, 0)?;
            value[0] = unsigned_float(packed & 0x7FF, 6);
            value[1] = unsigned_float((packed >> 11) & 0x7FF, 6);
            value[2] = unsigned_float(packed >> 22, 5);
        }
        _ => return Err(format!("Unsupported native vertex format {format}")),
    }
    if value.iter().any(|v| !v.is_finite()) {
        return Err("The vertex attribute contains a non-finite value".into());
    }
    Ok(value)
}

fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    sign * unsigned_float(u32::from(bits & 0x7FFF), 10)
}
fn unsigned_float(bits: u32, mantissa_bits: u32) -> f32 {
    let exponent = (bits >> mantissa_bits) & 31;
    let mantissa = bits & ((1 << mantissa_bits) - 1);
    if exponent == 31 {
        return if mantissa == 0 {
            f32::INFINITY
        } else {
            f32::NAN
        };
    }
    let fraction = mantissa as f32 / (1 << mantissa_bits) as f32;
    if exponent == 0 {
        fraction * 2.0f32.powi(-14)
    } else {
        (1.0 + fraction) * 2.0f32.powi(exponent as i32 - 15)
    }
}
