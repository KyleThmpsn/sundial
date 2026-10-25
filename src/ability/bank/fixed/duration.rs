//! The movement duration modifier embeds a paired condition list, empty for a literal window.
use super::*;

pub(super) const CLASS: u32 = 0x80804125;

pub(super) fn check(p: &[u8], owner: u32, d: usize, i: usize) -> Result<(), String> {
    let layout = layout(p)?;
    if i.checked_add(80).is_none_or(|end| end > layout.instance)
        || d < layout.instance
        || p.get(d + 48..d + 64) != Some(&[0; 16])
        || u32_at(p, d + 28)? != 0
        || u32_at(p, d + 32)? != owner
        || u32_at(p, d + 36)? != 0x80803BFA
        || u64_at(p, d + 40)? != (i + 32) as u64
        || u32_at(p, d + 64)? != 1
        || u32_at(p, d + 68)? != 0
        || u32_at(p, i + 32)? != owner
        || u32_at(p, i + 36)? != 0x80803BFB
        || u64_at(p, i + 40)? != (d + 32) as u64
        || pointer(p, i + 48)? != i
        || p.get(i + 56..i + 80) != Some(&[0; 24])
    {
        return Err("Movement duration requires an empty paired condition list".into());
    }
    Ok(())
}

pub(super) fn write(out: &mut [u8], owner: u32, d: usize, i: usize) -> Result<(), String> {
    put_u32(out, d + 32, owner);
    put_u32(out, d + 36, 0x80803BFA);
    put_u64(out, d + 40, (i + 32) as u64);
    // The native reset flag has no effect on an empty condition list. Its evaluator
    // returns true and initialization/reset visit no entries (D5EFA0, D63290).
    put_u32(out, d + 64, 1);
    put_u32(out, i + 32, owner);
    put_u32(out, i + 36, 0x80803BFB);
    put_u64(out, i + 40, (d + 32) as u64);
    put_pointer(out, i + 48, i)
}
