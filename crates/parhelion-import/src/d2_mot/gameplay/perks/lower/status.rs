//! State-gated, constant-count HUD reports with the original consumers checked on both eras.
use anyhow::{Result, ensure};

use super::super::controller::{Auxiliary, STATE_AUXILIARY};
use crate::d2_mot::payload::Payload;

/// Translate one report naming only the controller's active state into native 80804085.
/// The caller must bind its name key to private native HUD text and artwork.
///
/// Modern 181DD10 (build 239845) and retained-image 1841DA0 use the state pointer list,
/// key +18, signed priority +1C and default count one. Shadowkeep EF9950 uses mask 06
/// for runtime state one and forwards the corresponding fields to HUD collector D86920.
/// Dynamic count providers and additional reporting flags need separate translations.
pub fn active_status(p: &Payload, auxiliary: &Auxiliary, active: usize) -> Result<Vec<u8>> {
    let at = auxiliary.offset;
    ensure!(
        at >= 4
            && p.u32(at - 4)? == STATE_AUXILIARY
            && auxiliary.class == STATE_AUXILIARY
            && auxiliary.state_offsets == [active],
        "source HUD report must name only the translated active state"
    );
    let key = p.u32(at + 24)?;
    ensure!(
        ![0, u32::MAX, 0x811C_9DC5].contains(&key)
            && p.bytes::<8>(at)? == [0, 255, 0, 0, 0, 0, 0, 0]
            && p.u64(at + 32)? == 0
            && p.u32(at + 40)? == 255
            && p.u32(at + 44)? == 0x811C_9DC5
            && p.u64(at + 48)? == 0
            && p.bytes::<8>(at + 56)? == [0; 8],
        "source HUD report has unsupported count inputs, grouping or reporting flags"
    );
    let mut bytes = vec![0; 24];
    bytes[0] = 6;
    bytes[1] = 255;
    bytes[4..8].copy_from_slice(&key.to_le_bytes());
    bytes[8..12].copy_from_slice(&p.bytes::<4>(at + 28)?);
    bytes[20] = 255;
    Ok(bytes)
}
