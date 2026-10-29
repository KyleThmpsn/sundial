//! Neutral absolute profiles, without inherited filters or response behaviors.
use super::*;
use crate::package_payload::{u64_at, write_bytes};

/// Qualify the complete static owner envelope, not just its amount fields.
/// Native +55 retains the stock default. Every response, label and feedback list is empty.
pub fn neutral_profile(tag: u32, payload: &[u8]) -> Result<Profile, String> {
    let shape = profile(tag, payload).ok_or("The resource is not an outgoing damage profile")?;
    if payload.len() != 0x1B0 || shape.paired != 0x70 || shape.root != 0xA0 {
        return Err("The outgoing profile has another runtime envelope".into());
    }
    let amount = u32_at(payload, shape.root + 0x14)?;
    if !f32::from_bits(amount).is_finite() || f32::from_bits(amount) <= 0.0 {
        return Err("The neutral profile needs a positive absolute amount".into());
    }
    let allocation = u32_at(payload, 0x44)?;
    if matches!(allocation, 0 | u32::MAX) {
        return Err("The outgoing profile has no native runtime allocation".into());
    }
    let expected = envelope(tag, allocation, amount, shape.mode)?;
    if expected != payload {
        return Err(
            "The outgoing profile has additional scaling, filters, responses or feedback".into(),
        );
    }
    Ok(shape)
}

pub(super) fn envelope(
    tag: u32,
    allocation: u32,
    amount: u32,
    mode: u8,
) -> Result<Vec<u8>, String> {
    let mut expected = vec![0u8; 0x1B0];
    for (at, value) in [
        (0, 0x1B0u64),
        (16, 0x60),
        (24, 0x88),
        (0x48, 0x30),
        (0x58, 0x0000_2605_0000_0010),
        (0x78, 0xA0),
        (0xA8, 0x70),
    ] {
        write_bytes(&mut expected, at, &value.to_le_bytes())?;
    }
    for (at, value) in [
        (0x40, u32::MAX),
        (0x44, allocation),
        (0x6C, PAIRED_CLASS),
        (0x70, tag),
        (0x74, ROOT_CLASS),
        (0x9C, ROOT_CLASS),
        (0xA0, tag),
        (0xA4, PAIRED_CLASS),
        (0xB4, amount),
        (0xB8, amount),
        (0xEC, 1.0f32.to_bits()),
        (0x114, 1.0f32.to_bits()),
        (0x150, u32::MAX),
        (0x170, 1.0f32.to_bits()),
    ] {
        write_bytes(&mut expected, at, &value.to_le_bytes())?;
    }
    expected[0xC0] = mode;
    expected[0xF5] = 1;
    Ok(expected)
}

/// Read the qualified profile and its empty stock allocation before making a private copy.
pub fn read_neutral_profile(
    manager: &PackageManager,
    tag: u32,
) -> Result<(Vec<u8>, Profile), String> {
    read_profile(manager, tag, neutral_profile)
}

pub(super) fn read_profile(
    manager: &PackageManager,
    tag: u32,
    qualify: fn(u32, &[u8]) -> Result<Profile, String>,
) -> Result<(Vec<u8>, Profile), String> {
    let entry = manager
        .get_entry(tiger_pkg::TagHash(tag))
        .ok_or("The outgoing profile template is missing")?;
    if entry.file_type != 8 || entry.reference != DIRECTORY_CLASS {
        return Err("The outgoing profile template has another directory class".into());
    }
    let payload = manager.read_tag(tag)?;
    let shape = qualify(tag, &payload)?;
    let allocation = u32_at(&payload, 0x44)?;
    let entry = manager
        .get_entry(tiger_pkg::TagHash(allocation))
        .ok_or("The outgoing profile allocation is missing")?;
    if entry.file_type != 8 || entry.reference != 0x80809BBB {
        return Err("The outgoing profile allocation has another directory class".into());
    }
    let data = manager.read_tag(allocation)?;
    if data.len() != 48
        || u64_at(&data, 0)? != 48
        || u64_at(&data, 8)? != 0x811C9DC5
        || u64_at(&data, 16)? != 0
        || u64_at(&data, 24)? != u64::from(u32::MAX)
        || data[32..] != [0; 16]
    {
        return Err("The outgoing profile allocation is not empty".into());
    }
    Ok((payload, shape))
}
