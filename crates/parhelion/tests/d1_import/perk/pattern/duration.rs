//! A standalone Duration host, checked against the source host's Duration fields.
//!
//! Source host `808029C3` embeds its Duration at definition `+60`, with the 20-byte policy at
//! `+A0`, the Timer flag at Timer `+78` (`+B8` further) and the lifetime expression at `+138`.
//! Native host `80803B30` embeds its Duration at definition `+48`, with the policy at `+98`, the
//! Timer flag at `+120` and the lifetime at `+128`. Each field keeps its source value.
use super::*;

pub(super) fn check(original: &Payload, target: &Payload) -> CheckResult<()> {
    let s = original.pointer(24)?;
    let n = target.pointer(24)?;
    assert_eq!(original.u32(s - 4)?, 0x808029C3);
    assert_eq!(target.u32(n - 4)?, 0x80803B30);
    assert_eq!(target.u32(target.pointer(16)? - 4)?, 0x80803B2F);
    let (sd, nd) = (s + 0x60, n + 0x48);
    assert_eq!(
        original.bytes::<20>(sd + 0xA0)?,
        target.bytes::<20>(nd + 0x98)?,
        "Duration policy"
    );
    assert_eq!(
        original.bytes::<8>(sd + 0xB8 + 0x78)?,
        target.bytes::<8>(nd + 0x120)?,
        "Duration Timer flag"
    );
    let code = |p: &Payload, at: usize| -> CheckResult<Vec<u8>> {
        p.array(at, 1, Some(0x80800009))?
            .into_iter()
            .map(|row| p.u8(row).map_err(Into::into))
            .collect()
    };
    let constants = |p: &Payload, at: usize| -> CheckResult<Vec<[u8; 16]>> {
        p.array(at, 16, Some(0x80800090))?
            .into_iter()
            .map(|row| p.bytes::<16>(row).map_err(Into::into))
            .collect()
    };
    // Source expression fields sit eight bytes further into their object than native ones.
    let (sl, nl) = (sd + 0x138 + 8, nd + 0x128);
    assert_eq!(
        code(original, sl + 16)?,
        code(target, nl + 16)?,
        "Lifetime code"
    );
    assert_eq!(
        constants(original, sl + 32)?,
        constants(target, nl + 32)?,
        "Lifetime constants"
    );
    let source_inputs = original.array(sl + 64, 48, Some(0x80800927))?;
    let native_inputs = target.array(nl + 64, 40, Some(0x80809789))?;
    assert_eq!(
        source_inputs.len(),
        native_inputs.len(),
        "Lifetime input count"
    );
    for (&from, &to) in source_inputs.iter().zip(&native_inputs) {
        assert_eq!(
            original.u32(from + 40)?,
            target.u32(to + 32)?,
            "Lifetime input name"
        );
    }
    Ok(())
}
