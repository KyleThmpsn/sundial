//! Keep complete source hand motion through the native arm postpass.
use super::hash;
use crate::{
    presentation::put,
    tiger::{animation::control::Rig, payload::Payload},
};
use anyhow::{Context, Result, ensure};

pub(super) fn release(clip: &mut Payload, rig: &Rig) -> Result<usize> {
    ensure!(
        clip.u8(0x181)? == 0,
        "Source hand targets require a complete absolute clip"
    );
    if clip.u64(0x30)? == 0 {
        return Ok(0);
    }
    let resource = clip.pointer(0x30)?;
    ensure!(
        clip.u32(resource.checked_sub(4).context("Arm constraint resource")?)? == 0x80808F59,
        "Unsupported native arm constraint resource"
    );
    let rows = clip.array(resource, 64, Some(0x80808F5C))?;
    if rows.is_empty() {
        return Ok(0);
    }
    let mut pairs = Vec::new();
    for side in ["l", "r"] {
        let slot = |role: &str| -> Result<u16> {
            let bone = rig
                .names
                .iter()
                .position(|&name| name == hash(&format!("b_{side}_{role}")))
                .context("Native arm constraint joint missing")?;
            Ok(u16::try_from(rig.slot(bone)?)?)
        };
        pairs.push((slot("upperarm")?, slot("hand")?));
    }
    for &row in &rows {
        ensure!(
            pairs.contains(&(clip.u16(row)?, clip.u16(row + 2)?))
                && clip.u16(row + 0x10)? == u16::MAX
                && clip.u8(row + 0x12)? == 3
                && clip.u8(row + 0x13)? == 0,
            "Native clip has constraints outside the translated hand targets"
        );
    }
    // The postpass otherwise replaces source wrists with named donor targets and
    // donor release curves. A null binding stream skips those authored targets.
    // The control rows still request guided two-bone solving from the source FK
    // wrists and upper-arm planes. Auxiliary streams and action events stay intact.
    put(&mut clip.0, 0x30, &0u64.to_le_bytes())?;
    Ok(rows.len())
}
