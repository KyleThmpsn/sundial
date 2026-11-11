//! Ability-roster additions and removals name the Shadowkeep version of their source ability.
use super::*;

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    p: &Payload,
    at: usize,
) -> Result<NativeNode> {
    let kind = p.u8(at + 5)?;
    ensure!(
        matches!((p.u32(at - 4)?, kind), (0x808031BF, 10) | (0x8080308E, 11))
            && (0.0..=1.0).contains(&p.f32(at)?)
            && p.u8(at + 4)? == 255
            && p.u8(at + 6)? == 0
            && p.u32(at + 20)? == 0,
        "Source ability-reference condition has unqualified state or metadata"
    );
    if p.u64(at + 8)? != 0 {
        let path = p.pointer(at + 8)?;
        ensure!(
            p.0[path..].contains(&0),
            "Source ability-reference path is unterminated"
        );
    }
    let pattern = p.u32(at + 16)?;
    let target = counterpart::find(source, native, pattern)?
        .context("Source ability-reference condition names an ability Shadowkeep lacks")?
        .entity;

    // Source 76AC40/76AC70 and native 107C4B0 compare event word zero with +10.
    // Roster replacement in source 58A0B0 and native EC4AE0 publishes removal (11)
    // before addition (10). Preserve the kind and resolve both operands identically.
    // Their lifecycle callbacks allocate no state. The path is authoring metadata.
    let mut bytes = vec![0; 24];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    bytes[16..20].copy_from_slice(&target.to_le_bytes());
    Ok(NativeNode { kind, bytes })
}
