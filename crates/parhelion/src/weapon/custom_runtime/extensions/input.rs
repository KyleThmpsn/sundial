//! Package defaults for a separate melee input alongside a gun input.
use super::*;

const MELEE_BINDING: u32 = 0xCC0A_DA9F;
const INPUT_BINDING: u32 = 0xC18B_D28D;

/// The native input filter treats zero initial, press and release thresholds as
/// disabled primary input. Secondary buttons and labels have separate fields.
/// This does not claim to translate melee execution, camera or guard behavior.
pub(super) fn secondary_only(
    manager: &PackageManager,
    entity: &[u8],
    owners: &BTreeSet<u32>,
) -> AuthoringResult<WeaponRuntimeResourcePatch> {
    let guns = weapon_component_bindings(entity, INPUT_BINDING).map_err(invalid)?;
    let melee = weapon_component_bindings(entity, MELEE_BINDING).map_err(invalid)?;
    let ([gun], [melee]) = (guns.as_slice(), melee.as_slice()) else {
        return Err(invalid(
            "Secondary input policy requires one gun and one melee input",
        ));
    };
    if gun.concrete_class != 0x8080_43DA
        || melee.concrete_class != 0x8080_43D2
        || gun.owner_tag == melee.owner_tag
        || !owners.contains(&melee.owner_tag)
    {
        return Err(invalid(
            "Secondary input policy requires distinct native gun and melee components",
        ));
    }
    let payload = read_tag(manager, TagHash(melee.owner_tag), "secondary input owner")?;
    let instance = relative(&payload, 0x10)?;
    let resource = relative(&payload, 0x18)?;
    let size = usize::try_from(read_u64(&payload, 0x48)?)
        .map_err(|_| invalid("Secondary input instance size is too large"))?;
    let end = instance
        .checked_add(size)
        .filter(|end| *end <= resource && *end <= payload.len())
        .ok_or_else(|| invalid("Secondary input instance exceeds its owner"))?;
    if read_u64(&payload, 0)? != payload.len() as u64
        || instance < 4
        || resource < 4
        || instance as u64 != melee.resource_offset
        || size < 0x170
        || read_u32(&payload, instance - 4)? != 0x8080_43D2
        || read_u32(&payload, resource - 4)? != 0x8080_2D66
    {
        return Err(invalid(
            "Secondary input owner has an unsupported native layout",
        ));
    }
    reference(&payload, instance, melee.owner_tag, 0x8080_2D66, resource)?;
    let input = instance + 0x110;
    let input_resource = resource
        .checked_add(0x258)
        .filter(|at| at.checked_add(0x88).is_some_and(|end| end <= payload.len()))
        .ok_or_else(|| invalid("Secondary input resource exceeds its owner"))?;
    reference(
        &payload,
        input,
        melee.owner_tag,
        0x8080_43D8,
        input_resource,
    )?;
    reference(
        &payload,
        input_resource,
        melee.owner_tag,
        0x8080_43DA,
        input,
    )?;
    if input + 0x60 > end {
        return Err(invalid("Secondary input exceeds its instance"));
    }
    for offset in [0x30, 0x34, 0x38] {
        let value = f32::from_bits(read_u32(&payload, input + offset)?);
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(invalid(
                "Secondary input has unsupported trigger thresholds",
            ));
        }
    }
    Ok(WeaponRuntimeResourcePatch {
        binding_hash: MELEE_BINDING,
        resource_index: 0,
        offset: 0x140,
        bytes: vec![0; 12],
        graph_values: Vec::new(),
    })
}

pub(super) fn relative(payload: &[u8], at: usize) -> AuthoringResult<usize> {
    let displacement = read_u64(payload, at)? as i64;
    let target = (at as i64)
        .checked_add(displacement)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value <= payload.len())
        .ok_or_else(|| invalid("Secondary input owner has an invalid relative pointer"))?;
    Ok(target)
}

fn reference(
    payload: &[u8],
    at: usize,
    owner: u32,
    class: u32,
    offset: usize,
) -> AuthoringResult<()> {
    if read_u32(payload, at)? != owner
        || read_u32(payload, at + 4)? != class
        || read_u64(payload, at + 8)? != offset as u64
    {
        return Err(invalid(
            "Secondary input resource and instance references do not agree",
        ));
    }
    Ok(())
}
