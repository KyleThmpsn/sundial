use super::*;

pub(in crate::catalog) fn unlock_state_indices(
    definitions: &[UnlockDefinition],
) -> HashMap<(u8, u16), usize> {
    let mut indices = HashMap::new();
    for (index, definition) in definitions.iter().enumerate() {
        if let Some(slot) = definition.compact_slot {
            indices.entry((definition.bank(), slot)).or_insert(index);
        }
    }
    indices
}

pub(in crate::catalog) fn add_objective_owner(
    objectives: &mut [ObjectiveDef],
    objective_index: usize,
    owner: ObjectiveOwnerDef,
) {
    let Some(objective) = objectives.get_mut(objective_index) else {
        return;
    };
    if let Some(existing) = objective
        .owners
        .iter_mut()
        .find(|existing| existing.kind == owner.kind && existing.hash == owner.hash)
    {
        if existing.name.trim().is_empty() && !owner.name.trim().is_empty() {
            existing.name.clone_from(&owner.name);
        }
        if existing.type_name.trim().is_empty() && !owner.type_name.trim().is_empty() {
            existing.type_name.clone_from(&owner.type_name);
        }
        if existing.description.trim().is_empty() && !owner.description.trim().is_empty() {
            existing.description.clone_from(&owner.description);
        }
        for path in owner.paths {
            if !path.is_empty() && !existing.paths.contains(&path) {
                existing.paths.push(path);
            }
        }
        for trait_definition in owner.traits {
            if let Some(existing_trait) = existing
                .traits
                .iter_mut()
                .find(|candidate| candidate.hash == trait_definition.hash)
            {
                if existing_trait.name.trim().is_empty() && !trait_definition.name.trim().is_empty()
                {
                    existing_trait.name.clone_from(&trait_definition.name);
                }
                if existing_trait.description.trim().is_empty()
                    && !trait_definition.description.trim().is_empty()
                {
                    existing_trait
                        .description
                        .clone_from(&trait_definition.description);
                }
            } else {
                existing.traits.push(trait_definition);
            }
        }
        return;
    }
    objective.owners.push(owner);
}

pub(in crate::catalog) fn scan_unlock_flag_definitions(
    manager: &PackageManager,
    root: &[u8],
) -> Result<Vec<UnlockDefinition>, String> {
    scan_unlock_definitions(
        manager,
        root,
        UNLOCK_FLAG_DEFINITION_TABLE_SLOT,
        UNLOCK_FLAG_DEFINITION_ROW_CLASS,
        "flag",
    )
}

pub(in crate::catalog) fn scan_unlock_value_definitions(
    manager: &PackageManager,
    root: &[u8],
) -> Result<Vec<UnlockDefinition>, String> {
    scan_unlock_definitions(
        manager,
        root,
        UNLOCK_VALUE_DEFINITION_TABLE_SLOT,
        UNLOCK_VALUE_DEFINITION_ROW_CLASS,
        "value",
    )
}

pub(super) fn scan_unlock_definitions(
    manager: &PackageManager,
    root: &[u8],
    table_slot: usize,
    expected_row_class: u32,
    kind: &str,
) -> Result<Vec<UnlockDefinition>, String> {
    let pointer = 8_usize
        .checked_add(
            table_slot
                .checked_mul(16)
                .ok_or_else(|| format!("Unlock {kind} table offset overflowed"))?,
        )
        .ok_or_else(|| format!("Unlock {kind} table offset overflowed"))?;
    let table = manager
        .read_tag(TagHash(u32_at(root, pointer)?))
        .map_err(|error| format!("Could not read unlock {kind} definitions: {error}"))?;
    let (count, rows, row_class) = array_at(&table, 8)?;
    if row_class != expected_row_class {
        return Err(format!(
            "The installed unlock {kind} table has unexpected row class 0x{row_class:08X}"
        ));
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(
                    index
                        .checked_mul(UNLOCK_FLAG_DEFINITION_ROW_SIZE)
                        .ok_or_else(|| format!("Unlock {kind} row offset overflowed"))?,
                )
                .ok_or_else(|| format!("Unlock {kind} row offset overflowed"))?;
            let slot = u16_at(&table, row + UNLOCK_DEFINITION_SLOT_OFFSET)?;
            Ok(UnlockDefinition {
                hash: u64::from(u32_at(&table, row)?),
                code: u16_at(&table, row + UNLOCK_DEFINITION_CODE_OFFSET)?,
                compact_slot: (slot != UNLOCK_DEFINITION_UNBANKED_SLOT).then_some(slot),
                name: None,
                description: None,
                tested_by: Vec::new(),
            })
        })
        .collect()
}

pub(in crate::catalog) fn scan_unlock_flag_displays(
    manager: &PackageManager,
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    definitions: &mut [UnlockDefinition],
) -> Result<(), String> {
    let pointer = 16_usize
        .checked_add(
            UNLOCK_FLAG_DISPLAY_TABLE_SLOT
                .checked_mul(16)
                .ok_or("Unlock flag display table offset overflowed")?,
        )
        .ok_or("Unlock flag display table offset overflowed")?;
    let table = manager
        .read_tag(TagHash(u32_at(globals, pointer)?))
        .map_err(|error| format!("Could not read unlock flag displays: {error}"))?;
    let display_blocks = unlock_flag_display_blocks(&table, definitions)?;

    for (definition, display) in definitions.iter_mut().zip(display_blocks) {
        definition.name = nonblank_localized_string(resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &table,
            display + UNLOCK_FLAG_DISPLAY_NAME_OFFSET,
        ));
        definition.description = nonblank_localized_string(resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &table,
            display + UNLOCK_FLAG_DISPLAY_DESCRIPTION_OFFSET,
        ));
    }
    Ok(())
}

pub(super) fn unlock_flag_display_blocks(
    table: &[u8],
    definitions: &[UnlockDefinition],
) -> Result<Vec<usize>, String> {
    let (count, rows, row_class) = array_at(table, 8)?;
    let (content_count, content_rows, content_row_class) = array_at(table, 0x18)?;
    if row_class != UNLOCK_FLAG_DISPLAY_ROW_CLASS {
        return Err(format!(
            "The installed unlock flag display table has unexpected row class 0x{row_class:08X}"
        ));
    }
    if count != definitions.len() {
        return Err(format!(
            "The installed unlock flag definition and display tables do not match ({} definitions, {count} displays)",
            definitions.len()
        ));
    }
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(UNLOCK_FLAG_DISPLAY_ROW_SIZE)
                .ok_or("Unlock flag display row extent overflowed")?,
        )
        .ok_or("Unlock flag display row extent overflowed")?;
    let content_header = content_rows
        .checked_sub(16)
        .ok_or("Unlock flag display content header underflowed")?;
    let trailer_start = content_header
        .checked_sub(NESTED_ARRAY_TRAILER.len())
        .ok_or("Unlock flag display trailer underflowed")?;
    let content_end = content_rows
        .checked_add(
            content_count
                .checked_mul(UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE)
                .ok_or("Unlock flag display content extent overflowed")?,
        )
        .ok_or("Unlock flag display content extent overflowed")?;
    if content_count == 0
        || content_row_class != UNLOCK_FLAG_DISPLAY_CONTENT_ROW_CLASS
        || rows_end > trailer_start
        || table[rows_end..trailer_start].iter().any(|byte| *byte != 0)
        || table.get(trailer_start..content_header) != Some(&NESTED_ARRAY_TRAILER)
        || content_end != table.len()
    {
        return Err("The installed unlock flag display content has an unexpected layout".into());
    }

    let mut blocks = Vec::with_capacity(count);
    for (index, definition) in definitions.iter().enumerate() {
        let row = rows
            .checked_add(
                index
                    .checked_mul(UNLOCK_FLAG_DISPLAY_ROW_SIZE)
                    .ok_or("Unlock flag display row offset overflowed")?,
            )
            .ok_or("Unlock flag display row offset overflowed")?;
        let display_hash = u32_at(table, row)?;
        if u64::from(display_hash) != definition.hash {
            return Err(format!(
                "Unlock flag definition and display row {index} do not match"
            ));
        }
        // Read the reserved field as part of validating the complete fixed-size row.
        let _ = u32_at(table, row + 4)?;
        let pointer = row
            .checked_add(UNLOCK_FLAG_DISPLAY_POINTER_OFFSET)
            .ok_or("Unlock flag display pointer offset overflowed")?;
        let display = relative_offset(pointer, 0, i64_at(table, pointer)?)?;
        let end = display
            .checked_add(UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE)
            .ok_or("Unlock flag display block offset overflowed")?;
        if display < content_rows
            || end > content_end
            || (display - content_rows) % UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE != 0
        {
            return Err(format!(
                "Unlock flag display row {index} points outside the table"
            ));
        }
        blocks.push(display);
    }
    Ok(blocks)
}

pub(super) fn nonblank_localized_string(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

pub(super) fn resolve_string_index_reference(
    manager: &PackageManager,
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    data: &[u8],
    offset: usize,
) -> Option<String> {
    let hash = u32_at(data, offset).ok()?;
    let table_index = u32_at(data, offset.checked_add(4)?).ok()?;
    let mut reference = [0_u8; 8];
    reference[..4].copy_from_slice(&table_index.to_le_bytes());
    reference[4..].copy_from_slice(&hash.to_le_bytes());
    resolve_string(manager, localized_tags, localized_cache, &reference, 0)
}
