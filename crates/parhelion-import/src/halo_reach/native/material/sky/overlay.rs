//! Optional World diagnostics for camera fog and weather. Native TFX restores the enabled value
//! before every draw, so these surfaces also work without a diagnostic runtime.
use super::*;

#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum Kind {
    ScreenFog,
    PatchyFog,
    Weather,
}

pub(super) const DECLARATION: &str = "float4 overlayControl;float4 overlaySignature;";
pub(super) const DISCARD: &str = "clip(overlayControl.x-0.5);";

/// Row zero remains the depth projection. Rows one and two carry the enabled value and the
/// dedicated version-3 signature. Only these native constant loads supply the mutable rows.
pub(super) fn bind(bytes: &mut Vec<u8>, expression: &mut Vec<u8>, kind: Kind) -> Result<Vec<u8>> {
    let initial: Vec<u8> = [1., 0., 0., 0., 18607., 23091., 3., f32::from(kind as u8)]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    append(bytes, 0x2f8, 0x80800090, &initial, 16)?;
    expression.extend([0x34, 0, 0x43, 1, 0x34, 1, 0x43, 2]);
    let mut output = vec![0; 16];
    output.extend(initial);
    Ok(output)
}
