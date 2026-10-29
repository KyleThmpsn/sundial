//! The HUD's optional seconds transform, separate from the Duration timer.
use super::*;

pub(super) fn read(p: &Payload, hud: usize) -> Result<bool> {
    let at = hud + 0x108;
    let code = p
        .array(at + 24, 1, Some(0x80800009))?
        .into_iter()
        .map(|a| p.u8(a))
        .collect::<Result<Vec<_>>>()?;
    let values = p.array(at + 40, 16, Some(0x80800090))?;
    let enabled = p.u64(hud + 0xF8)?;
    let identity = code == [0x3C, 0, 0x3E, 0] && values.is_empty();
    let literal = if code == [0x34, 0, 0x3E, 0] && values.len() == 1 {
        let bits = p.u32(values[0])?;
        f32::from_bits(bits).is_finite()
            && f32::from_bits(bits) >= 0.0
            && (1..4).all(|lane| p.u32(values[0] + lane * 4).ok() == Some(bits))
    } else {
        false
    };
    // Original reporters pass remaining seconds to input zero. The source identity
    // program retains values above one, unlike the native carrier's default saturation.
    // Inactive literals are safe only because both reporters bypass the whole program.
    ensure!(
        enabled <= 1
            && p.u64(hud + 0x100)? == 0
            && (identity
                || (enabled == 0
                    && ((code == [0x3C, 0, 0x23, 0x3E, 0] && values.is_empty()) || literal)))
            && p.u64(at + 56)? == 1
            && p.u64(at + 64)? == 1
            && p.bytes::<16>(at + 72)? == [0; 16],
        "Source HUD has an unqualified display-seconds program"
    );
    Ok(enabled == 1)
}

pub(super) fn patch(
    p: &Payload,
    hud: usize,
    replace: &mut impl FnMut(usize, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let at = hud + 0xF8;
    let code = p.array(at + 0x10, 1, Some(0x80800009))?;
    ensure!(
        code.len() == 5
            && p.bytes::<5>(code[0])? == [0x3C, 0, 0x23, 0x3E, 0]
            && p.array(at + 0x20, 16, Some(0x80800090))?.is_empty()
            && p.u64(at + 0x30)? == 1
            && p.u64(at + 0x38)? == 1
            && p.bytes::<16>(at + 0x40)? == [0; 16],
        "Native status display program differs"
    );
    let header = code[0]
        .checked_sub(16)
        .context("Native status display code has no array header")?;
    ensure!(
        header >= p.pointer(24)?
            && p.u32(header - 4)? == 0x80809FBD
            && p.u64(header)? == 5
            && p.u32(header + 8)? == 0x80800009
            && p.u32(header + 12)? == 0,
        "Native status display code has a different typed allocation"
    );
    // The identity fits in the existing private code allocation. Shrink both counts,
    // retaining the last byte as padding and all original instance/definition links.
    replace(hud + 0xEC, vec![1])?;
    replace(at + 0x10, 4u64.to_le_bytes().to_vec())?;
    replace(header, 4u64.to_le_bytes().to_vec())?;
    replace(code[0], vec![0x3C, 0, 0x3E, 0, 0])
}
