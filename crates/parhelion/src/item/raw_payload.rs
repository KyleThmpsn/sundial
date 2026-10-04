//! Native raw payload operations with independent validation.
use super::*;

pub(super) fn apply_raw_payload_target(
    payload: &mut [u8],
    target_kind: WeaponRawPayloadTarget,
    patches: &[WeaponRawPayloadPatch],
) -> AuthoringResult<()> {
    for (index, patch) in patches
        .iter()
        .enumerate()
        .filter(|(_, patch)| patch.target == target_kind)
    {
        let start = usize::try_from(patch.offset)
            .map_err(|_| invalid(format!("Raw payload patch {index} offset is too large")))?;
        let end = start
            .checked_add(patch.bytes.len())
            .ok_or_else(|| invalid(format!("Raw payload patch {index} range overflows")))?;
        let payload_len = payload.len();
        let target = payload.get_mut(start..end).ok_or_else(|| {
            invalid(format!(
                "Raw payload patch {index} range 0x{start:X}..0x{end:X} exceeds its {target_kind:?} payload (0x{payload_len:X} bytes)"
            ))
        })?;
        target.copy_from_slice(&patch.bytes);
    }
    Ok(())
}

pub(super) fn validate_raw_payload_target(
    payload: &[u8],
    target_kind: WeaponRawPayloadTarget,
    patches: &[WeaponRawPayloadPatch],
) -> AuthoringResult<()> {
    for (index, patch) in patches
        .iter()
        .enumerate()
        .filter(|(_, patch)| patch.target == target_kind)
    {
        let start = usize::try_from(patch.offset)
            .map_err(|_| validation(format!("Raw payload patch {index} offset is too large")))?;
        let end = start
            .checked_add(patch.bytes.len())
            .ok_or_else(|| validation(format!("Raw payload patch {index} range overflows")))?;
        if payload.get(start..end) != Some(patch.bytes.as_slice()) {
            return Err(validation(format!(
                "Raw payload patch {index} was not retained at {target_kind:?} offset 0x{start:X}"
            )));
        }
    }
    Ok(())
}

pub(super) fn item_definition_raw_subtarget_range(
    definition: &[u8],
    target: WeaponRawPayloadTarget,
) -> AuthoringResult<Option<std::ops::Range<usize>>> {
    match target {
        WeaponRawPayloadTarget::ItemInventoryBlock => {
            return definition
                .get(0xB0..0xD8)
                .map(|_| Some(0xB0..0xD8))
                .ok_or_else(|| invalid("Item definition is missing its inline inventory block"));
        }
        WeaponRawPayloadTarget::ItemTraitsDescriptor => {
            return definition
                .get(ITEM_TRAITS_DESCRIPTOR_OFFSET..ITEM_TRAITS_DESCRIPTOR_OFFSET + 16)
                .map(|_| Some(ITEM_TRAITS_DESCRIPTOR_OFFSET..ITEM_TRAITS_DESCRIPTOR_OFFSET + 16))
                .ok_or_else(|| invalid("Item definition is missing its traits descriptor"));
        }
        WeaponRawPayloadTarget::ItemTraitRows => {
            let (count, _, rows, class) = array_at(definition, ITEM_TRAITS_DESCRIPTOR_OFFSET)?;
            if class != ITEM_TRAIT_ROW_CLASS {
                return Err(invalid("Item trait rows have an unexpected native class"));
            }
            if count == 0 {
                return Ok(None);
            }
            let end = rows
                .checked_add(
                    count
                        .checked_mul(ITEM_TRAIT_ROW_SIZE)
                        .ok_or_else(|| invalid("Item trait row range overflowed"))?,
                )
                .ok_or_else(|| invalid("Item trait row range overflowed"))?;
            definition
                .get(rows..end)
                .ok_or_else(|| invalid("Item trait rows exceed the definition"))?;
            return Ok(Some(rows..end));
        }
        _ => {}
    }

    let Some((_, pointer)) = ITEM_ROOT_RAW_TARGETS
        .iter()
        .find(|(candidate, _)| *candidate == target)
    else {
        return Err(invalid(format!(
            "{target:?} is not an item-definition subtarget"
        )));
    };
    if read_i64(definition, *pointer)? == 0 {
        return Ok(None);
    }
    let start = relative_target(definition, *pointer)?;
    let mut end = definition.len();
    for (_, other_pointer) in ITEM_ROOT_RAW_TARGETS {
        if read_i64(definition, other_pointer)? == 0 {
            continue;
        }
        let candidate = relative_target(definition, other_pointer)?;
        if candidate > start {
            end = end.min(candidate);
        }
    }
    for candidate in [0xB0, ITEM_TRAITS_DESCRIPTOR_OFFSET] {
        if candidate > start {
            end = end.min(candidate);
        }
    }
    if let Ok((trait_count, _, trait_rows, trait_class)) =
        array_at(definition, ITEM_TRAITS_DESCRIPTOR_OFFSET)
        && trait_count > 0
        && trait_class == ITEM_TRAIT_ROW_CLASS
        && trait_rows > start
    {
        end = end.min(trait_rows);
    }
    if start >= end || definition.get(start..end).is_none() {
        return Err(invalid(format!(
            "{target:?} does not resolve to a bounded item-definition block"
        )));
    }
    Ok(Some(start..end))
}

pub(super) fn apply_item_definition_raw_subtargets(
    definition: &mut [u8],
    patches: &[WeaponRawPayloadPatch],
) -> AuthoringResult<()> {
    for target in ITEM_DEFINITION_RAW_SUBTARGETS {
        if !patches.iter().any(|patch| patch.target == target) {
            continue;
        }
        let range = item_definition_raw_subtarget_range(definition, target)?.ok_or_else(|| {
            invalid(format!(
                "Raw {target:?} patch requested but the gameplay donor has no such block"
            ))
        })?;
        apply_raw_payload_target(&mut definition[range], target, patches)?;
    }
    Ok(())
}

pub(super) fn validate_item_definition_raw_subtargets(
    definition: &[u8],
    patches: &[WeaponRawPayloadPatch],
) -> AuthoringResult<()> {
    for target in ITEM_DEFINITION_RAW_SUBTARGETS {
        if !patches.iter().any(|patch| patch.target == target) {
            continue;
        }
        let range = item_definition_raw_subtarget_range(definition, target)?.ok_or_else(|| {
            validation(format!(
                "Raw {target:?} patch target disappeared from the authored definition"
            ))
        })?;
        validate_raw_payload_target(&definition[range], target, patches)?;
    }
    Ok(())
}

pub(super) fn validate_item_root_holder_bounds(definition: &[u8]) -> AuthoringResult<()> {
    if definition.len() < 0xF0 {
        return Err(validation(
            "Authored item definition is shorter than its fixed 240-byte root record",
        ));
    }
    for (target, pointer) in ITEM_ROOT_RAW_TARGETS {
        if read_i64(definition, pointer)? == 0 {
            continue;
        }
        let resolved = relative_target(definition, pointer)?;
        if !(0xF0..definition.len()).contains(&resolved) {
            return Err(validation(format!(
                "Authored {target:?} holder resolves outside the definition's variable payload"
            )));
        }
    }
    Ok(())
}

pub(super) fn apply_weapon_raw_payload_patches(
    definition: &mut [u8],
    strings: &mut [u8],
    patches: &[WeaponRawPayloadPatch],
) -> AuthoringResult<()> {
    apply_raw_payload_target(definition, WeaponRawPayloadTarget::ItemDefinition, patches)?;
    apply_item_definition_raw_subtargets(definition, patches)?;
    apply_raw_payload_target(
        strings,
        WeaponRawPayloadTarget::ItemStringDefinition,
        patches,
    )
}

pub(super) fn validate_weapon_raw_payload_patches(
    definition: &[u8],
    strings: &[u8],
    patches: &[WeaponRawPayloadPatch],
) -> AuthoringResult<()> {
    validate_raw_payload_target(definition, WeaponRawPayloadTarget::ItemDefinition, patches)?;
    validate_item_definition_raw_subtargets(definition, patches)?;
    validate_raw_payload_target(
        strings,
        WeaponRawPayloadTarget::ItemStringDefinition,
        patches,
    )
}

pub(super) fn apply_array_row_raw_payload_patches(
    data: &mut [u8],
    descriptor: usize,
    row_index: usize,
    row_size: usize,
    expected_class: u32,
    target_kind: WeaponRawPayloadTarget,
    patches: &[WeaponRawPayloadPatch],
) -> AuthoringResult<()> {
    let (count, _, rows, class) = array_at(data, descriptor)?;
    if class != expected_class || row_index >= count {
        return Err(invalid(format!(
            "Raw {target_kind:?} patch target does not resolve to an authored native row"
        )));
    }
    let start = rows
        .checked_add(
            row_index
                .checked_mul(row_size)
                .ok_or_else(|| invalid("Raw authored-row offset overflowed"))?,
        )
        .ok_or_else(|| invalid("Raw authored-row offset overflowed"))?;
    let end = start
        .checked_add(row_size)
        .ok_or_else(|| invalid("Raw authored-row range overflowed"))?;
    let row = data
        .get_mut(start..end)
        .ok_or_else(|| invalid("Raw authored-row range exceeds its table"))?;
    apply_raw_payload_target(row, target_kind, patches)?;
    validate_raw_payload_target(row, target_kind, patches)
}
