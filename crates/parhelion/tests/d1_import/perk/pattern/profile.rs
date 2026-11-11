//! A profile event and the healing profile it applies, checked against the source event.
//!
//! Source kind 24 names its profile owner at `+28` and holds bound programs at `+30` and `+70`.
//! Native kind 27 names the owner at `+48` and holds them at `+50` and `+110`. Its other programs
//! keep the values every stock event paired by name holds: `+90` empty, `+D0` and `+150` constant
//! zero. The applied profile's health response keeps the source mode, flags and operands.
use super::*;

/// A bound program's bytecode and constants. Rise of Iron lists its one builtin input as
/// `FFFFFFFF`, native lists none.
fn program(p: &Payload, at: usize) -> CheckResult<(Vec<u8>, Vec<[u8; 16]>, u64)> {
    let code = p
        .array(at, 1, Some(0x80800009))?
        .into_iter()
        .map(|row| p.u8(row))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let constants = p
        .array(at + 16, 16, Some(0x80800090))?
        .into_iter()
        .map(|row| p.bytes::<16>(row))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok((code, constants, p.u64(at + 32)?))
}

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    original: &Payload,
    from: usize,
    owner: &Payload,
    to: usize,
) -> CheckResult<()> {
    assert_eq!(owner.u32(to - 4)?, 0x80804AC4);
    assert_eq!(owner.u32(to + 0x30)?, 27);
    for (s, n) in [(0x30, 0x50), (0x70, 0x110)] {
        assert_eq!(
            program(original, from + s)?,
            program(owner, to + n)?,
            "Profile program"
        );
        for slot in original.array(from + s + 48, 4, Some(0x80800007))? {
            assert_eq!(original.u32(slot)?, u32::MAX);
        }
        assert!(owner.array(to + n + 48, 4, Some(0x80800007))?.is_empty());
    }
    assert_eq!(owner.bytes::<64>(to + 0x90)?, [0; 64]);
    let zero = (vec![0x34, 0, 0x3E, 0], vec![[0; 16]], 1);
    assert_eq!(program(owner, to + 0xD0)?, zero);
    assert_eq!(program(owner, to + 0x150)?, zero);
    assert_eq!(owner.u64(to + 0x40)?, 0);
    assert_eq!(owner.u32(to + 0x194)?, 0x811C_9DC5);

    // The applied profile is private and carries the source health response.
    let tag = owner.u32(to + 0x48)?;
    assert!(stock.tag(tag, None).is_err(), "Profile must be private");
    let profile = emitted.tag(tag, Some(0x80809C36))?;
    let source = reader.read(original.u32(from + 0x28)?)?;
    let root = source.pointer(24)?;
    let rows = source.array(root + 0x28, 8, Some(0x8080276E))?;
    assert_eq!(rows.len(), 1, "Source healing response count");
    let response = source.pointer(rows[0])?;
    assert_eq!(source.u32(response - 4)?, 0x80802976);
    assert_eq!(profile.u32(0x228)?, 0x80804B5D);
    assert_eq!(
        source.bytes::<12>(response)?,
        profile.bytes::<12>(0x22C)?,
        "Health response mode, flags and operands"
    );
    Ok(())
}
