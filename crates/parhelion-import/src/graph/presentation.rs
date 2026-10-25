//! Persistent appearance components retain the lifetime of their enclosing part.
use crate::tiger::payload::Payload;
use anyhow::{Result, ensure};

pub(super) fn validate(bytes: &[u8]) -> Result<()> {
    let Some(relative) = bytes.get(16..24) else {
        return Ok(());
    };
    let Some(instance) = i64::from_le_bytes(relative.try_into()?)
        .checked_add(16)
        .and_then(|at| usize::try_from(at).ok())
        .filter(|&at| at >= 4)
    else {
        return Ok(());
    };
    if bytes.get(instance - 4..instance) != Some(0x808084D7u32.to_le_bytes().as_slice()) {
        return Ok(());
    }
    let p = Payload(bytes.to_vec());
    // Follow both the instance's reciprocal definition and the sealed loader boundary.
    // Older graphs can carry the dangerous template flag without a changed fingerprint.
    for definition in [usize::try_from(p.u64(instance + 8)?)?, p.pointer(24)?] {
        ensure!(
            definition >= 4 && p.u32(definition - 4)? == 0x808084E9,
            "Presentation sequence definition differs"
        );
        ensure!(
            p.u8(definition + 0x2E0)? & 2 == 0,
            "This imported sequence can retire its appearance part. Import the item again"
        );
    }
    Ok(())
}
