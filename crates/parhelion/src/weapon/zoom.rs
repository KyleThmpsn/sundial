//! Private magnification through the selected Weapon Stats translation column.
use super::rig::{
    TRANSLATOR_KEY_CLASS, TRANSLATOR_KEY_HASH, TRANSLATOR_KEY_SIZE, TRANSLATOR_TABLE_CLASS,
    TRANSLATOR_TABLE_SIZE, translator_array,
};
use crate::tag_payload::{array_at, read_u32};
use crate::{AuthoringResult, error::invalid, item::WeaponRuntimeResourcePatch};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::entity::{
    WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, weapon_component_bindings,
};
use tiger_pkg::TagHash;

#[cfg(feature = "d2-model-importer")]
pub(crate) fn imported(graph: &serde_json::Value) -> AuthoringResult<Option<f32>> {
    let Some(aim) = graph.pointer("/source_gameplay/aim") else {
        return Ok(None);
    };
    let value = aim["magnification"]
        .as_f64()
        .ok_or_else(|| invalid("Imported aim has no magnification"))? as f32;
    validate(value)?;
    Ok(Some(value))
}

fn validate(value: f32) -> AuthoringResult<()> {
    if !(1.0..=10.0).contains(&value) {
        return Err(invalid("Magnification must be between 1 and 10"));
    }
    Ok(())
}

/// Set Weapon Controller input 3 without changing other users of a shared column.
/// The build applies these offsets to its private resource copy. Other columns
/// and weapon types keep their original cells, and player FOV stays independent.
pub(crate) fn patches(
    manager: &PackageManager,
    entity: &[u8],
    group: u32,
    magnification: f32,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    validate(magnification)?;
    let bindings =
        weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid("Magnification requires one stat translator"));
    };
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|error| invalid(error.to_string()))?;
    let resource = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("Stat translator offset overflow"))?;
    let keys = translator_array(&owner, TRANSLATOR_KEY_CLASS)?;
    let tables = translator_array(&owner, TRANSLATOR_TABLE_CLASS)?;
    if keys.0 != tables.0 || keys.0 == 0 {
        return Err(invalid(
            "Magnification translator keys and tables do not agree",
        ));
    }
    let mut selected = None;
    for index in 0..keys.0 {
        if read_u32(
            &owner,
            keys.1 + index * TRANSLATOR_KEY_SIZE + TRANSLATOR_KEY_HASH,
        )? == group
            && selected.replace(index).is_some()
        {
            return Err(invalid("Magnification has duplicate translation keys"));
        }
    }
    let table = tables.1 + selected.unwrap_or(0) * TRANSLATOR_TABLE_SIZE;
    let (translations, _, translation_rows, translation_class) = array_at(&owner, table)?;
    let (routes, _, route_rows, route_class) = array_at(&owner, table + 0x10)?;
    if translation_class != 0x8080_3979 || route_class != 0x8080_3981 {
        return Err(invalid("Unsupported magnification translation layout"));
    }
    let mut zoom = None;
    for route in (0..routes).map(|index| route_rows + index * 0x50) {
        if read_u32(&owner, route)? == 0
            && read_u32(&owner, route + 4)? == 3
            && zoom.replace(route).is_some()
        {
            return Err(invalid("Magnification has more than one input route"));
        }
    }
    let route = zoom.ok_or_else(|| invalid("The weapon has no magnification input route"))?;
    if read_u32(&owner, route + 0xc)? != 4 || read_u32(&owner, route + 0x20)? != 0x7f7f_ffff {
        return Err(invalid(
            "Magnification does not directly copy a translation output",
        ));
    }
    let output = read_u32(&owner, route + 0x1c)?;
    let ordinal = output
        .checked_sub(0x1e00)
        .ok_or_else(|| invalid("Magnification does not read a stat translation"))?
        as usize;
    let translation_index = ordinal / 32;
    let column = ordinal % 32;
    if translation_index >= translations {
        return Err(invalid("Magnification translation is absent"));
    }
    let mut shared = false;
    for other in (0..routes).map(|index| route_rows + index * 0x50) {
        for operand in [0x1c, 0x34, 0x4c] {
            if (other != route || operand != 0x1c) && read_u32(&owner, other + operand)? == output {
                shared = true;
            }
        }
    }
    // Named output channels are owner-wide, even when no routing entry shares it.
    for descriptor in (0..owner.len().saturating_sub(16)).step_by(8) {
        let Ok((count, _, rows, 0x8080_396b)) = array_at(&owner, descriptor) else {
            continue;
        };
        for at in (0..count).map(|index| rows + index * 12) {
            if read_u32(&owner, at + 4)? == output || read_u32(&owner, at + 8)? == output {
                shared = true;
            }
        }
    }
    let translation = translation_rows + translation_index * 0x38;
    if read_u32(&owner, translation + 0x20)? != 3 || read_u32(&owner, translation + 0x24)? != 0 {
        return Err(invalid(
            "Magnification does not follow the supported Zoom stat input",
        ));
    }
    let (tiers, _, tier_rows, tier_class) = array_at(&owner, translation + 0x10)?;
    if tier_class != 0x8080_9f2a || tiers == 0 {
        return Err(invalid("Magnification has no supported stat tiers"));
    }
    let cells = (0..tiers)
        .map(|index| {
            let (count, _, values, class) = array_at(&owner, tier_rows + index * 16)?;
            if class != 0x8080_000f || column >= count {
                return Err(invalid("Magnification tier has no output cell"));
            }
            let cell = values + column * 4;
            validate(f32::from_bits(read_u32(&owner, cell)?))?;
            Ok(cell)
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    // E83C40 returns the operand's +10 literal whenever it differs from the
    // Float32 maximum sentinel. This bypasses sampling for this input only.
    let cells = if shared { vec![route + 0x20] } else { cells };
    cells
        .into_iter()
        .map(|cell| {
            let offset = cell
                .checked_sub(resource)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| invalid("Magnification patch offset overflow"))?;
            Ok(WeaponRuntimeResourcePatch {
                binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
                resource_index: 0,
                offset,
                bytes: magnification.to_le_bytes().to_vec(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            })
        })
        .collect()
}
