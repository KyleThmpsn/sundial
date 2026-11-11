//! Native material constants for optional World lighting diagnostics.
//!
//! The material works without a runtime override. Its first two pixel rows hold lighting values
//! and a versioned signature. Ordinary TFX constant loads restore both on every evaluation.
//! Live object inputs follow them. Surfaces and additional analytical lights share this exact bank.
use crate::presentation::{append, put};
use crate::tiger::payload::Payload;
use anyhow::Result;

pub(super) const ROWS: usize = 2;
pub(super) const DECLARATION: &str = "float4 bakedControl;float4 bakedSignature;";

/// Strength, near and far fade distances in metres, and source baked-light scale. A zero strength
/// leaves the sun mask to native shadows. Native surface visibility couples direct and indirect light.
const DEFAULTS: [f32; 4] = [0., 30., 60., 1.];
/// Exactly represented finite values identifying the bounded lighting-control contract, version 2.
const SIGNATURE: [f32; 4] = [18607., 23091., 2., 0.];

pub(super) fn initial() -> Vec<u8> {
    DEFAULTS
        .into_iter()
        .chain(SIGNATURE)
        .flat_map(f32::to_le_bytes)
        .collect()
}

pub(super) fn bind(bytes: &mut Vec<u8>, expression: &mut Vec<u8>) -> Result<()> {
    append(bytes, 0x2f8, 0x80800090, &initial(), 16)?;
    expression.extend([0x34, 0, 0x43, 0, 0x34, 1, 0x43, 1]);
    bytes[0x33c] = 0x10;
    put(bytes, 0x348, &0u32.to_le_bytes())
}

/// Identify the material's native stage in both copies of the signature. Diagnostics admit only
/// the deferred surface and diffuse companion, not a shadow or glow program that discards lighting.
pub(super) fn stage(bytes: &mut [u8], stage: u8) -> Result<()> {
    let payload = Payload(bytes.to_owned());
    for descriptor in [0x2f8, 0x318] {
        let rows = payload.array(descriptor, 16, Some(0x80800090))?;
        if rows.len() >= ROWS && payload.bytes::<12>(rows[1])? == initial()[16..28] {
            put(bytes, rows[1] + 12, &f32::from(stage).to_le_bytes())?;
        }
    }
    Ok(())
}
