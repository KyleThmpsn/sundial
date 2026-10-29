//! Mode-three health responses on the checked zero-damage profile family.
use super::*;
use crate::package_payload::{native_array_at, u64_at, write_bytes};

/// The template carries only the two qualified categories and one mode-three response.
/// Its literal amount is a witness. Private copies receive the authored response operands.
pub fn healing_profile(tag: u32, payload: &[u8]) -> Result<Profile, String> {
    let shape = profile(tag, payload).ok_or("The resource is not an outgoing profile")?;
    if payload.len() != 0x23C || shape.root != 0xA0 || shape.paired != 0x70 {
        return Err("The healing profile has another runtime envelope".into());
    }
    let amount = u32_at(payload, 0x230)?;
    if !f32::from_bits(amount).is_finite() || f32::from_bits(amount) <= 0.0 {
        return Err("The healing template needs a finite positive response".into());
    }
    let allocation = u32_at(payload, 0x44)?;
    if matches!(allocation, 0 | u32::MAX) {
        return Err("The healing profile has no runtime allocation".into());
    }
    let mut expected = fixed::envelope(tag, allocation, 0, 0)?;
    expected.resize(0x23C, 0);
    for (at, value) in [
        (0, 0x23Cu64),
        (0x100, 2),
        (0x108, 0xB8),
        (0x118, 1),
        (0x120, 0xF0),
        (0x1C0, 2),
        (0x210, 1),
        (0x220, 0xC),
    ] {
        write_bytes(&mut expected, at, &value.to_le_bytes())?;
    }
    for (at, value) in [
        (0xC4, 0x800u32),
        (0xE4, 0x1000),
        (0x140, 1),
        (0x1BC, 0x80809FBD),
        (0x1C8, 0x808094B3),
        (0x1D0, 0x4EFCDCF5),
        (0x1E0, 0x80C70CA1),
        (0x1E8, 0xCD887E0F),
        (0x1F8, 0x80C70CA1),
        (0x20C, 0x80809FBD),
        (0x218, 0x80804B3D),
        (0x228, 0x80804B5D),
        (0x22C, 0x103),
        (0x230, amount),
        (0x238, 0x811C9DC5),
    ] {
        write_bytes(&mut expected, at, &value.to_le_bytes())?;
    }
    if expected != payload {
        return Err(
            "The healing profile has additional categories, scaling, responses or feedback".into(),
        );
    }
    Ok(shape)
}

/// Category names must still occupy the bits used by the qualified native template.
pub fn healing_labels(registry: &[u8]) -> Result<(), String> {
    let (count, _, rows, class) = native_array_at(registry, 8)?;
    if class != 0x80800070
        || count > 320
        || count <= 268
        || u32_at(registry, rows + 11 * 4)? != 0x4EFCDCF5
        || u32_at(registry, rows + 268 * 4)? != 0xCD887E0F
    {
        return Err("The healing categories have another native dictionary layout".into());
    }
    Ok(())
}

pub fn read_healing_profile(
    manager: &PackageManager,
    tag: u32,
) -> Result<(Vec<u8>, Profile), String> {
    healing_labels(&manager.read_tag(0x80C70CA1)?)?;
    fixed::read_profile(manager, tag, healing_profile)
}

/// Preserve mode three, both source flags and the two source Float32 operands.
/// This family has no damage amount. Region capacity and unit conversion remain native.
pub fn set_health_response(
    payload: &mut [u8],
    shape: Profile,
    amounts: [u32; 2],
) -> Result<(), String> {
    let tag = u32_at(payload, shape.root)?;
    if healing_profile(tag, payload)? != shape
        || amounts
            .iter()
            .any(|&bits| !f32::from_bits(bits).is_finite() || f32::from_bits(bits) < 0.0)
        || amounts.iter().all(|&bits| f32::from_bits(bits) == 0.0)
        || u64_at(payload, shape.root + 0x78)? != 1
    {
        return Err(
            "The health response needs a qualified profile and finite positive contribution".into(),
        );
    }
    write_bytes(payload, 0x22C, &[3, 1, 1, 0])?;
    write_bytes(payload, 0x230, &amounts[0].to_le_bytes())?;
    write_bytes(payload, 0x234, &amounts[1].to_le_bytes())
}
