//! A constant protection factor uses the native value program, with no damage filter.
use super::*;
use crate::package_payload::{native_array_at, rows_fit};

pub(super) const CLASS: u32 = 0x80804520;
pub(super) const SIZE: usize = 176;

/// A native definition with a constant, unfiltered protection value. Its paired
/// owner header stays clear for the component builder to connect to its runtime.
pub fn constant_protection_definition(duration: f32, factor: f32) -> Result<Vec<u8>, String> {
    if duration.to_bits() != (-1.0f32).to_bits()
        || !factor.is_finite()
        || !(0.0..=1.0).contains(&factor)
    {
        return Err("Constant ability protection has unqualified duration or factor".into());
    }
    let mut out = vec![0; SIZE];
    put_u32(&mut out, 16, duration.to_bits());
    put_u32(&mut out, 20, factor.to_bits());
    write(&mut out, 0, factor.to_bits())?;
    Ok(out)
}

pub(super) fn check(p: &[u8], d: usize, i: usize) -> Result<(), String> {
    let split = layout(p)?.instance;
    if i.checked_add(40).is_none_or(|end| end > split)
        || d < split
        || p.get(d..d + 96).is_none()
        || u32_at(p, d + 16)? != (-1.0f32).to_bits()
        || u64_at(p, d + 24)? != 0
        || u64_at(p, d + 64)? != 1
        || u64_at(p, d + 72)? != 1
        || u64_at(p, d + 80)? != 0
        || u32_at(p, d + 92)? != 0
    {
        return Err("Ability protection has another native value-source layout".into());
    }
    for (offset, stride, expected) in [(32, 1, 0x80800009), (48, 16, 0x80800090)] {
        let (count, header, first, class) = native_array_at(p, d + offset)?;
        if count == 0
            || class != expected
            || header < split
            || header < 4
            || u32_at(p, header - 4)? != ARRAY_MARKER
            || u32_at(p, header + 12)? != 0
        {
            return Err("Ability protection has an invalid native value-source array".into());
        }
        rows_fit(p, first, count, stride)?;
    }
    Ok(())
}

pub(super) fn write(out: &mut [u8], d: usize, factor: u32) -> Result<(), String> {
    // FC01F0 binds this envelope at definition +0x18. The +0x14 factor prefix is not
    // consumed. FC5940 evaluates the program and only multiplies an optional filter
    // when its handle is present, so the absent filter preserves the source callback.
    put_u64(out, d + 32, 4);
    put_pointer(out, d + 40, d + 112)?;
    put_u64(out, d + 48, 1);
    put_pointer(out, d + 56, d + 144)?;
    put_u64(out, d + 64, 1);
    put_u64(out, d + 72, 1);
    put_u32(out, d + 88, u32::MAX);

    // Both typed arrays stay beside the definition in the same native clone region.
    put_u32(out, d + 108, ARRAY_MARKER);
    put_u64(out, d + 112, 4);
    put_u32(out, d + 120, 0x80800009);
    out[d + 128..d + 132].copy_from_slice(&[0x34, 0, 0x3E, 0]);
    put_u32(out, d + 140, ARRAY_MARKER);
    put_u64(out, d + 144, 1);
    put_u32(out, d + 152, 0x80800090);
    for lane in 0..4 {
        put_u32(out, d + 160 + lane * 4, factor);
    }
    Ok(())
}
