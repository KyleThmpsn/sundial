//! Initialize saved D1 static banks through Shadowkeep's native upload gate.
use super::super::payload::{Payload, write_array};
use anyhow::{Context, Result, ensure};

pub(super) fn bind(bytes: &mut Vec<u8>, stage: usize) -> Result<()> {
    let payload = Payload(bytes.clone());
    if payload.u64(stage + 0x20)? != 0
        || payload.u64(stage + 0x50)? == 0
        || !matches!(payload.u32(stage + 0x84)?, 0 | u32::MAX)
    {
        return Ok(());
    }
    ensure!(
        matches!(payload.u8(stage + 0x74)?, 0 | 0x10) && payload.u32(stage + 0x80)? == 0,
        "static initial bank has an unsupported allocation or slot"
    );
    let values = payload.array_range(stage + 0x50, 16, Some(0x80800090))?;
    ensure!(
        values.len() <= 65536,
        "static initial bank exceeds a native constant buffer"
    );
    let first = &payload.0[values.start..values.start + 16];
    let range = payload.array_range(stage + 0x30, 16, Some(0x80800090))?;
    let mut constants = payload.0[range].to_vec();
    let index = constants
        .chunks_exact(16)
        .position(|value| value == first)
        .unwrap_or(constants.len() / 16);
    let encoded = u8::try_from(index).context("static initializer exceeds 256 constants")?;
    if index == constants.len() / 16 {
        constants.extend_from_slice(first);
        write_array(
            bytes,
            stage + 0x30,
            0x80800090,
            constants.len() / 16,
            &constants,
        )?;
    }
    // Older prepared D1 graphs retained initial vectors without requesting a writable bank.
    // The checked slot has no external buffer, so allocate its own native upload storage.
    bytes[stage + 0x74] = 0x10;
    // 011D8900 returns before Map for an empty expression. A value-preserving
    // store makes it copy the entire initial bank before evaluating this code.
    // Appending keeps every existing graph relocation at its original offset.
    write_array(
        bytes,
        stage + 0x20,
        0x80800009,
        4,
        &[0x34, encoded, 0x43, 0],
    )
}
