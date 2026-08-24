use super::*;

pub(in crate::catalog) fn scan_progression_definitions(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    item_hashes: &[u64],
    icon_containers: &[Option<u32>],
) -> Result<Vec<ProgressionDefinition>, String> {
    let table = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + PROGRESSION_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read progression definitions: {error}"))?;
    let mut definitions = progression_definitions_from_data(&table, item_hashes)?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + PROGRESSION_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read progression strings: {error}"))?;
    let (count, rows, row_class) = array_at(&strings, 8)?;
    if row_class != PROGRESSION_STRING_ROW_CLASS {
        return Err(format!(
            "Unexpected progression string row class 0x{row_class:08X}"
        ));
    }
    if count != definitions.len() {
        return Err("Progression definition and string tables do not match".into());
    }
    for (index, definition) in definitions.iter_mut().enumerate() {
        let row = rows
            .checked_add(
                index
                    .checked_mul(PROGRESSION_STRING_ROW_SIZE)
                    .ok_or("Progression string row offset overflowed")?,
            )
            .ok_or("Progression string row offset overflowed")?;
        let string_hash = u64::from(u32_at(&strings, row)?);
        if string_hash != definition.hash {
            return Err(format!(
                "Progression definition and string row {index} do not match"
            ));
        }
        definition.name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            row + PROGRESSION_NAME_OFFSET,
        )
        .unwrap_or_default();
        definition.description = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            row + PROGRESSION_DESCRIPTION_OFFSET,
        )
        .unwrap_or_default();
        definition.source = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            row + PROGRESSION_SOURCE_OFFSET,
        )
        .unwrap_or_default();
        definition.display_units_name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            row + PROGRESSION_DISPLAY_UNITS_NAME_OFFSET,
        )
        .unwrap_or_default();
        definition.icon_container = progression_icon_container(
            u16_at(&strings, row + PROGRESSION_ICON_INDEX_OFFSET)?,
            icon_containers,
        )?;
        let step_count = usize::try_from(u64_at(&strings, row + PROGRESSION_STEP_STRINGS_OFFSET)?)
            .map_err(|_| "Progression step string count is too large")?;
        if step_count != definition.steps.len() {
            return Err(format!(
                "Progression definition and string row {index} have different step counts"
            ));
        }
        if step_count != 0 {
            let (step_count, step_rows, step_class) =
                array_at(&strings, row + PROGRESSION_STEP_STRINGS_OFFSET)?;
            if step_class != PROGRESSION_STEP_STRING_ROW_CLASS {
                return Err(format!(
                    "Unexpected progression step string row class 0x{step_class:08X}"
                ));
            }
            for (step_index, step) in definition.steps.iter_mut().enumerate() {
                let step_row = step_rows
                    .checked_add(
                        step_index
                            .checked_mul(PROGRESSION_STEP_STRING_ROW_SIZE)
                            .ok_or("Progression step string row offset overflowed")?,
                    )
                    .ok_or("Progression step string row offset overflowed")?;
                step.name = resolve_string(
                    manager,
                    localized_tags,
                    localized_cache,
                    &strings,
                    step_row + PROGRESSION_STEP_NAME_OFFSET,
                )
                .unwrap_or_default();
                step.icon_container = progression_icon_container(
                    u16_at(&strings, step_row + PROGRESSION_STEP_ICON_INDEX_OFFSET)?,
                    icon_containers,
                )?;
            }
            debug_assert_eq!(step_count, definition.steps.len());
        }
        let reward_string_count =
            usize::try_from(u64_at(&strings, row + PROGRESSION_REWARD_STRINGS_OFFSET)?)
                .map_err(|_| "Progression reward string count is too large")?;
        if reward_string_count != definition.reward_items.len() {
            return Err(format!(
                "Progression definition and string row {index} have different reward counts"
            ));
        }
        if reward_string_count != 0 {
            let (parsed_count, _, reward_string_class) =
                array_at(&strings, row + PROGRESSION_REWARD_STRINGS_OFFSET)?;
            if parsed_count != reward_string_count
                || reward_string_class != PROGRESSION_REWARD_STRING_ROW_CLASS
            {
                return Err(format!(
                    "Unexpected progression reward string row class 0x{reward_string_class:08X}"
                ));
            }
        }
    }
    apply_progression_factions(
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
        &mut definitions,
    )?;
    Ok(definitions)
}

pub(super) fn apply_progression_factions(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    definitions: &mut [ProgressionDefinition],
) -> Result<(), String> {
    let table = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + FACTION_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read faction definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + FACTION_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read faction strings: {error}"))?;
    let (count, rows, row_class) = array_at(&table, 8)?;
    let (string_count, string_rows, string_row_class) = array_at(&strings, 8)?;
    if row_class != FACTION_DEFINITION_ROW_CLASS {
        return Err(format!(
            "Unexpected faction definition row class 0x{row_class:08X}"
        ));
    }
    if string_row_class != FACTION_STRING_ROW_CLASS {
        return Err(format!(
            "Unexpected faction string row class 0x{string_row_class:08X}"
        ));
    }
    if count != string_count {
        return Err("Faction definition and string tables do not match".into());
    }
    for index in 0..count {
        let row = rows
            .checked_add(
                index
                    .checked_mul(FACTION_ROW_SIZE)
                    .ok_or("Faction definition row offset overflowed")?,
            )
            .ok_or("Faction definition row offset overflowed")?;
        let string_row = string_rows
            .checked_add(
                index
                    .checked_mul(FACTION_ROW_SIZE)
                    .ok_or("Faction string row offset overflowed")?,
            )
            .ok_or("Faction string row offset overflowed")?;
        let hash = u64::from(u32_at(&table, row + FACTION_HASH_OFFSET)?);
        if u64::from(u32_at(&strings, string_row + FACTION_HASH_OFFSET)?) != hash {
            return Err(format!(
                "Faction definition and string row {index} do not match"
            ));
        }
        let progression_index = u16_at(&table, row + FACTION_PROGRESSION_INDEX_OFFSET)?;
        if progression_index == u16::MAX {
            continue;
        }
        let definition = definitions
            .get_mut(usize::from(progression_index))
            .ok_or_else(|| {
                format!(
                    "Faction row {index} progression index {progression_index} is outside the progression table"
                )
            })?;
        definition.factions.push(ProgressionFactionDefinition {
            hash,
            name: resolve_string(
                manager,
                localized_tags,
                localized_cache,
                &strings,
                string_row + FACTION_NAME_OFFSET,
            )
            .unwrap_or_default(),
            description: resolve_string(
                manager,
                localized_tags,
                localized_cache,
                &strings,
                string_row + FACTION_DESCRIPTION_OFFSET,
            )
            .unwrap_or_default(),
        });
    }
    Ok(())
}

pub(super) fn progression_icon_container(
    index: u16,
    icon_containers: &[Option<u32>],
) -> Result<Option<u32>, String> {
    if index == u16::MAX {
        return Ok(None);
    }
    icon_containers
        .get(usize::from(index))
        .copied()
        .ok_or_else(|| format!("Progression icon index {index} is outside the package icon table"))
}

pub(super) fn progression_definitions_from_data(
    table: &[u8],
    item_hashes: &[u64],
) -> Result<Vec<ProgressionDefinition>, String> {
    let (count, rows, row_class) = array_at(table, 8)?;
    if row_class != PROGRESSION_DEFINITION_ROW_CLASS {
        return Err(format!(
            "Unexpected progression definition row class 0x{row_class:08X}"
        ));
    }
    if count > usize::from(u16::MAX) + 1 {
        return Err("Progression definition count exceeds its native index".into());
    }
    if count == 0 {
        return Err("Progression definition table is empty".into());
    }
    let mut definitions = Vec::with_capacity(count);
    for definition_index in 0..count {
        let row = rows
            .checked_add(
                definition_index
                    .checked_mul(PROGRESSION_DEFINITION_ROW_SIZE)
                    .ok_or("Progression definition row offset overflowed")?,
            )
            .ok_or("Progression definition row offset overflowed")?;
        let raw_scope = *table
            .get(row + PROGRESSION_DEFINITION_SCOPE_OFFSET)
            .ok_or("Progression definition ended before its scope")?;
        let raw_scope_slot = u16_at(table, row + PROGRESSION_DEFINITION_SCOPE_SLOT_OFFSET)?;
        let (scope, scope_slot) = match raw_scope {
            0 => (ProgressionScope::Account, Some(raw_scope_slot)),
            1 => (ProgressionScope::Character, Some(raw_scope_slot)),
            _ => (ProgressionScope::Unreplicated, None),
        };
        let step_count = usize::try_from(u64_at(table, row + PROGRESSION_DEFINITION_STEPS_OFFSET)?)
            .map_err(|_| "Progression step count is too large")?;
        let mut steps = Vec::with_capacity(step_count);
        if step_count != 0 {
            let (parsed_count, step_rows, step_class) =
                array_at(table, row + PROGRESSION_DEFINITION_STEPS_OFFSET)?;
            if parsed_count != step_count || step_class != PROGRESSION_STEP_ROW_CLASS {
                return Err(format!(
                    "Unexpected progression step row class 0x{step_class:08X}"
                ));
            }
            for step_index in 0..step_count {
                let step_row = step_rows
                    .checked_add(
                        step_index
                            .checked_mul(PROGRESSION_STEP_ROW_SIZE)
                            .ok_or("Progression step row offset overflowed")?,
                    )
                    .ok_or("Progression step row offset overflowed")?;
                steps.push(ProgressionStepDefinition {
                    progress_total: i32_at(
                        table,
                        step_row + PROGRESSION_STEP_PROGRESS_TOTAL_OFFSET,
                    )?,
                    name: String::new(),
                    icon_container: None,
                });
            }
        }
        let reward_count =
            usize::try_from(u64_at(table, row + PROGRESSION_DEFINITION_REWARDS_OFFSET)?)
                .map_err(|_| "Progression reward count is too large")?;
        let mut reward_items = Vec::with_capacity(reward_count);
        if reward_count != 0 {
            let (parsed_count, reward_rows, reward_class) =
                array_at(table, row + PROGRESSION_DEFINITION_REWARDS_OFFSET)?;
            if parsed_count != reward_count || reward_class != PROGRESSION_REWARD_ROW_CLASS {
                return Err(format!(
                    "Unexpected progression reward row class 0x{reward_class:08X}"
                ));
            }
            for reward_index in 0..reward_count {
                let reward_row = reward_rows
                    .checked_add(
                        reward_index
                            .checked_mul(PROGRESSION_REWARD_ROW_SIZE)
                            .ok_or("Progression reward row offset overflowed")?,
                    )
                    .ok_or("Progression reward row offset overflowed")?;
                let item_index = usize::try_from(u32_at(
                    table,
                    reward_row + PROGRESSION_REWARD_ITEM_INDEX_OFFSET,
                )?)
                .map_err(|_| "Progression reward item index is too large")?;
                let item_hash = item_hashes.get(item_index).copied().ok_or_else(|| {
                    format!("Progression reward item index {item_index} is outside the item table")
                })?;
                reward_items.push(ProgressionRewardDefinition {
                    rewarded_at_progression_level: i32_at(
                        table,
                        reward_row + PROGRESSION_REWARD_LEVEL_OFFSET,
                    )?,
                    item_hash,
                    quantity: i32_at(table, reward_row + PROGRESSION_REWARD_QUANTITY_OFFSET)?,
                });
            }
        }
        definitions.push(ProgressionDefinition {
            definition_index: u16::try_from(definition_index)
                .map_err(|_| "Progression definition index is too large")?,
            hash: u64::from(u32_at(table, row + PROGRESSION_DEFINITION_HASH_OFFSET)?),
            scope,
            scope_slot,
            repeat_last_step: bool_at(table, row + PROGRESSION_DEFINITION_REPEAT_LAST_STEP_OFFSET)?,
            name: String::new(),
            description: String::new(),
            source: String::new(),
            display_units_name: String::new(),
            icon_container: None,
            factions: Vec::new(),
            steps,
            reward_items,
        });
    }
    Ok(definitions)
}
