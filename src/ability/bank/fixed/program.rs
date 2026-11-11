//! A constant value program body. The native glide bank binds the horizontal burst factor as a
//! program reference (`808044CC`, marker `808044CD`), which the glide multiplies by impulse lane 2
//! along the movement direction when the product exceeds 0.0001 (`FB1F12..FB1FE6`). A program of
//! bytecode `34 00 3E 00` returns its one constant whatever the speed it is given, as the source's
//! literal factor did. Both typed arrays sit inside the body block, behind relative pointers.
use super::*;

pub(in crate::ability::bank) const CLASS: u32 = 0x8080_44CC;
/// The body runs to the constant's last lane. The next block's marker follows it.
pub(super) const SIZE: usize = 0x98;
const BYTECODE: [u8; 4] = [0x34, 0, 0x3E, 0];
const BYTECODE_CLASS: u32 = 0x8080_0009;
const CONSTANT_CLASS: u32 = 0x8080_0090;
const BYTECODE_HEADER: usize = 0x58;
const CONSTANT_HEADER: usize = 0x78;
/// The constant's four lanes, all equal for a constant program.
pub(in crate::ability::bank) const LANES: usize = 0x88;

/// The constant a stock or written body returns, after checking every field of its layout.
pub(in crate::ability::bank) fn constant(p: &[u8], d: usize) -> Result<u32, String> {
    let fields = [
        (0x10, u64_at(p, d + 0x10)?, 0),
        (0x18, u64_at(p, d + 0x18)?, 4),
        (0x28, u64_at(p, d + 0x28)?, 1),
        (0x38, u64_at(p, d + 0x38)?, 1),
        (0x40, u64_at(p, d + 0x40)?, 1),
        (0x48, u64_at(p, d + 0x48)?, 0),
        (0x50, u64::from(u32_at(p, d + 0x50)?), 0),
        (BYTECODE_HEADER, u64_at(p, d + BYTECODE_HEADER)?, 4),
        (CONSTANT_HEADER, u64_at(p, d + CONSTANT_HEADER)?, 1),
    ];
    if fields.iter().any(|&(_, value, expected)| value != expected)
        || pointer(p, d + 0x20)? != d + BYTECODE_HEADER
        || pointer(p, d + 0x30)? != d + CONSTANT_HEADER
        || u32_at(p, d + BYTECODE_HEADER - 4)? != ARRAY_MARKER
        || u32_at(p, d + CONSTANT_HEADER - 4)? != ARRAY_MARKER
        || u32_at(p, d + BYTECODE_HEADER + 8)? != BYTECODE_CLASS
        || u32_at(p, d + BYTECODE_HEADER + 12)? != 0
        || u32_at(p, d + CONSTANT_HEADER + 8)? != CONSTANT_CLASS
        || u32_at(p, d + CONSTANT_HEADER + 12)? != 0
        || p.get(d + 0x68..d + 0x6C) != Some(&BYTECODE[..])
        || p.get(d + 0x6C..d + 0x74) != Some(&[0; 8][..])
    {
        return Err("Value program body is not a single constant program".into());
    }
    let value = u32_at(p, d + LANES)?;
    for lane in 1..4 {
        if u32_at(p, d + LANES + lane * 4)? != value {
            return Err("Value program constant has unequal lanes".into());
        }
    }
    if !f32::from_bits(value).is_finite() {
        return Err("Value program constant is not a number".into());
    }
    Ok(value)
}

pub(super) fn write(out: &mut [u8], d: usize, value: u32) -> Result<(), String> {
    put_u64(out, d + 0x10, 0);
    put_u64(out, d + 0x18, 4);
    put_pointer(out, d + 0x20, d + BYTECODE_HEADER)?;
    put_u64(out, d + 0x28, 1);
    put_pointer(out, d + 0x30, d + CONSTANT_HEADER)?;
    put_u64(out, d + 0x38, 1);
    put_u64(out, d + 0x40, 1);
    put_u32(out, d + BYTECODE_HEADER - 4, ARRAY_MARKER);
    put_u64(out, d + BYTECODE_HEADER, 4);
    put_u32(out, d + BYTECODE_HEADER + 8, BYTECODE_CLASS);
    out[d + 0x68..d + 0x6C].copy_from_slice(&BYTECODE);
    put_u32(out, d + CONSTANT_HEADER - 4, ARRAY_MARKER);
    put_u64(out, d + CONSTANT_HEADER, 1);
    put_u32(out, d + CONSTANT_HEADER + 8, CONSTANT_CLASS);
    for lane in 0..4 {
        put_u32(out, d + LANES + lane * 4, value);
    }
    Ok(())
}
