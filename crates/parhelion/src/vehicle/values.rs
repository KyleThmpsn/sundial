//! Checked scalar edits shared by the vehicle component writers.
use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{bounded_relative_target, read_u32, write_relative_pointer, write_u32},
};

pub(super) fn scale_float(
    payload: &mut [u8],
    at: usize,
    factor: f32,
    label: &str,
) -> AuthoringResult<()> {
    if factor == 1.0 {
        return Ok(());
    }
    let original = f32::from_bits(read_u32(payload, at)?);
    let scaled = original * factor;
    if !original.is_finite() || original < 0.0 || !scaled.is_finite() || scaled < 0.0 {
        return Err(invalid(format!(
            "{label} requires a finite nonnegative scalar at +0x{at:X}"
        )));
    }
    write_u32(payload, at, scaled.to_bits())
}

/// Numeric configuration values use a relative boxed number. Clone even a shared box so
/// another input cannot accidentally receive this factor. Native F3F8F0 reads these kinds.
pub(super) fn scale_number(
    payload: &mut Vec<u8>,
    pointer: usize,
    factor: f32,
    label: &str,
) -> AuthoringResult<bool> {
    if factor == 1.0 {
        return Ok(false);
    }
    let relative = crate::tag_payload::read_i64(payload, pointer)?;
    if relative == 0 {
        return Ok(false);
    }
    let at = bounded_relative_target(payload, pointer, label)?;
    let kind = *payload
        .get(at)
        .ok_or_else(|| invalid(format!("{label}: truncated numeric kind")))?;
    let value = match kind {
        0 => f32::from_bits(read_u32(payload, at + 4)?),
        1 => read_u32(payload, at + 4)? as i32 as f32,
        2 => f32::from(
            *payload
                .get(at + 1)
                .ok_or_else(|| invalid(format!("{label}: truncated byte value")))?
                as i8,
        ),
        _ => {
            return Err(invalid(format!(
                "{label}: unsupported boxed numeric kind {kind}"
            )));
        }
    };
    let scaled = value * factor;
    if !value.is_finite() || value < 0.0 || !scaled.is_finite() || scaled < 0.0 {
        return Err(invalid(format!(
            "{label}: requires finite nonnegative numeric values"
        )));
    }
    let new_at = (payload.len() + 7) & !7;
    payload.resize(new_at, 0);
    payload.extend_from_slice(&[0, 0, 0, 0]);
    payload.extend_from_slice(&scaled.to_le_bytes());
    write_relative_pointer(payload, pointer, new_at)?;
    Ok(true)
}
