use super::*;

#[derive(Clone, Debug)]
pub(in crate::catalog::progression) struct ActivityContext {
    pub(in crate::catalog::progression) hash: u64,
    pub(in crate::catalog::progression) definition_start: usize,
    pub(in crate::catalog::progression) name: String,
    pub(in crate::catalog::progression) description: String,
    pub(in crate::catalog::progression) gate_hashes: Vec<u32>,
}

pub(in crate::catalog) fn scan_activity_condition_contexts(
    package: &mut ProgressionPackageData<'_>,
    locations: &[LocationContext],
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<(), String> {
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + ACTIVITY_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read activity definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + ACTIVITY_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read activity strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, string_class) = array_at(&strings, 8)?;
    if definition_class != ACTIVITY_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed activity table has unexpected row class 0x{definition_class:08X}"
        ));
    }
    if string_class != ACTIVITY_STRING_ROW_CLASS {
        return Err(format!(
            "The installed activity-string table has unexpected row class 0x{string_class:08X}"
        ));
    }
    if definition_count != string_count {
        return Err("The installed activity definition and string tables do not match".into());
    }

    let mut activities = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition_row = definition_rows
            .checked_add(
                index
                    .checked_mul(ACTIVITY_INDEX_ROW_SIZE)
                    .ok_or("Activity definition row offset overflowed")?,
            )
            .ok_or("Activity definition row offset overflowed")?;
        let string_row = string_rows
            .checked_add(
                index
                    .checked_mul(ACTIVITY_INDEX_ROW_SIZE)
                    .ok_or("Activity string row offset overflowed")?,
            )
            .ok_or("Activity string row offset overflowed")?;
        let hash = u32_at(&definitions, definition_row)?;
        if u32_at(&strings, string_row)? != hash {
            return Err(format!(
                "Activity definition and string row {index} do not match"
            ));
        }
        let definition_pointer = definition_row
            .checked_add(8)
            .ok_or("Activity definition pointer offset overflowed")?;
        let definition_start =
            relative_offset(definition_row, 8, i64_at(&definitions, definition_pointer)?)?;
        if u32_at(&definitions, definition_start)? != hash {
            return Err(format!(
                "Activity definition row {index} points to another hash"
            ));
        }

        let string_pointer = string_row
            .checked_add(8)
            .ok_or("Activity string pointer offset overflowed")?;
        let string_structure = relative_offset(string_row, 8, i64_at(&strings, string_pointer)?)?;
        let display = relative_offset(string_structure, 0, i64_at(&strings, string_structure)?)?;
        let name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            display + 0x04,
        )
        .unwrap_or_default();
        let description = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            display + 0x0C,
        )
        .unwrap_or_default();

        let gate_hashes = activity_gate_hashes(&definitions, definition_start)?;
        activities.push(ActivityContext {
            hash: u64::from(hash),
            definition_start,
            name,
            description,
            gate_hashes,
        });
    }

    let mut distinct_starts = activities
        .iter()
        .map(|activity| activity.definition_start)
        .collect::<Vec<_>>();
    distinct_starts.sort_unstable();
    distinct_starts.dedup();
    let references_by_start = distinct_starts
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            let end = distinct_starts
                .get(index + 1)
                .copied()
                .unwrap_or(definitions.len());
            Ok((
                start,
                scan_condition_expressions_in(&definitions, start, end)?,
            ))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;

    for activity in &activities {
        attach_flag_writes(
            &definitions,
            activity.definition_start + 120,
            flag_definitions,
            &activity_progression_context(activity, ProgressionContextKind::Activity),
        )?;
        attach_conditional_flag_writes(
            &definitions,
            activity.definition_start + 136,
            flag_definitions,
            &activity_progression_context(activity, ProgressionContextKind::Activity),
        )?;
        attach_direct_reference(
            flag_definitions,
            u16_at(&definitions, activity.definition_start + 216)?,
            "Activity flag",
            &activity_progression_context(activity, ProgressionContextKind::Activity),
        )?;
        let Some(references) = references_by_start.get(&activity.definition_start) else {
            continue;
        };
        attach_condition_context(
            flag_definitions,
            value_definitions,
            references,
            &activity_progression_context(activity, ProgressionContextKind::Activity),
        );
    }

    let availability = activity_availability_references(manager, root)?;
    for activity in &activities {
        let mut references = ConditionReferences::default();
        for gate_hash in &activity.gate_hashes {
            if let Some(gate_references) = availability.get(gate_hash) {
                merge_condition_references(&mut references, gate_references.clone());
            }
        }
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &references,
            &activity_progression_context(activity, ProgressionContextKind::ActivityAvailability),
        );
    }
    attach_location_definition_release_contexts(
        locations,
        &activities,
        flag_definitions,
        value_definitions,
    )?;
    attach_location_release_condition_contexts(
        manager,
        root,
        locations,
        &activities,
        flag_definitions,
        value_definitions,
    )?;
    Ok(())
}

fn activity_progression_context(
    activity: &ActivityContext,
    kind: ProgressionContextKind,
) -> ProgressionContextDef {
    ProgressionContextDef {
        direct_references: Vec::new(),
        hash: activity.hash,
        kind,
        name: activity.name.clone(),
        type_name: String::new(),
        description: activity.description.clone(),
        paths: Vec::new(),
        condition_programs: Vec::new(),
    }
}

fn activity_gate_hashes(data: &[u8], definition_start: usize) -> Result<Vec<u32>, String> {
    let descriptor = definition_start
        .checked_add(ACTIVITY_GATE_LIST_OFFSET)
        .ok_or("Activity gate-list offset overflowed")?;
    if u64_at(data, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, row_class) = array_at(data, descriptor)?;
    if row_class != ACTIVITY_GATE_ROW_CLASS {
        return Err(format!(
            "Activity has unexpected gate-list row class 0x{row_class:08X}"
        ));
    }
    let end = rows
        .checked_add(
            count
                .checked_mul(4)
                .ok_or("Activity gate-list size overflowed")?,
        )
        .ok_or("Activity gate-list row offset overflowed")?;
    if end > data.len() {
        return Err("Activity gate list extends beyond its package data".into());
    }
    let mut hashes = (0..count)
        .map(|index| u32_at(data, rows + index * 4))
        .collect::<Result<Vec<_>, _>>()?;
    hashes.sort_unstable();
    hashes.dedup();
    Ok(hashes)
}

fn activity_availability_references(
    manager: &PackageManager,
    root: &[u8],
) -> Result<HashMap<u32, ConditionReferences>, String> {
    let data = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + ACTIVITY_AVAILABILITY_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read activity availability: {error}"))?;
    let (count, rows, row_class) = array_at(&data, ACTIVITY_AVAILABILITY_LIST_OFFSET)?;
    if row_class != ACTIVITY_AVAILABILITY_ROW_CLASS {
        return Err(format!(
            "The installed activity-availability table has unexpected row class 0x{row_class:08X}"
        ));
    }
    let mut by_gate = HashMap::new();
    for index in 0..count {
        let row = rows
            .checked_add(
                index
                    .checked_mul(ACTIVITY_AVAILABILITY_ROW_SIZE)
                    .ok_or("Activity-availability row offset overflowed")?,
            )
            .ok_or("Activity-availability row offset overflowed")?;
        let gate_hash = u32_at(&data, row + ACTIVITY_AVAILABILITY_GATE_HASH_OFFSET)?;
        let mut references = ConditionReferences::default();
        for offset in ACTIVITY_AVAILABILITY_GROUP_OFFSETS {
            let descriptor = row + offset;
            if u64_at(&data, descriptor)? == 0 {
                continue;
            }
            let (group_count, group_rows, group_class) = array_at(&data, descriptor)?;
            if group_class != ACTIVITY_AVAILABILITY_GROUP_ROW_CLASS {
                return Err(format!(
                    "Activity-availability row {index} has an unexpected condition group"
                ));
            }
            for group_index in 0..group_count {
                let group = group_rows
                    .checked_add(
                        group_index
                            .checked_mul(ACTIVITY_AVAILABILITY_GROUP_ROW_SIZE)
                            .ok_or("Activity-availability group offset overflowed")?,
                    )
                    .ok_or("Activity-availability group offset overflowed")?;
                merge_condition_references(&mut references, condition_references_at(&data, group)?);
            }
        }
        merge_condition_references(by_gate.entry(gate_hash).or_default(), references);
    }
    Ok(by_gate)
}
