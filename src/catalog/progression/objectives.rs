use super::*;

pub(super) fn milestone_objective_indices(
    definitions: &[u8],
    row: usize,
    objective_count: usize,
) -> Result<Vec<usize>, String> {
    let phase_count = usize::try_from(u32_at(definitions, row + MILESTONE_PHASE_COUNT_OFFSET)?)
        .map_err(|_| "Milestone phase count is too large")?;
    if phase_count > MILESTONE_MAX_PHASES {
        return Err(format!(
            "Milestone has {phase_count} phases; expected at most {MILESTONE_MAX_PHASES}"
        ));
    }

    let mut indices = Vec::with_capacity(1 + phase_count * 2);
    let mut push_index = |raw: u16| -> Result<(), String> {
        if raw == u16::MAX {
            return Ok(());
        }
        let index = usize::from(raw);
        if index >= objective_count {
            return Err(format!(
                "Milestone references out-of-range objective index {index}"
            ));
        }
        if !indices.contains(&index) {
            indices.push(index);
        }
        Ok(())
    };
    push_index(u16_at(
        definitions,
        row + MILESTONE_PRIMARY_OBJECTIVE_INDEX_OFFSET,
    )?)?;
    for phase in 0..phase_count {
        let pair =
            row + MILESTONE_PHASE_OBJECTIVES_OFFSET + phase * MILESTONE_PHASE_OBJECTIVE_PAIR_SIZE;
        push_index(u16_at(definitions, pair)?)?;
        push_index(u16_at(definitions, pair + 2)?)?;
    }
    Ok(indices)
}

pub(in crate::catalog) fn scan_milestone_objective_owners(
    package: &mut ProgressionPackageData<'_>,
    objectives: &mut [ObjectiveDef],
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
            8 + MILESTONE_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read milestone definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + MILESTONE_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read milestone strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, string_class) = array_at(&strings, 8)?;
    if definition_class != MILESTONE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed milestone table has unexpected row class 0x{definition_class:08X}"
        ));
    }
    if string_class != MILESTONE_STRING_ROW_CLASS {
        return Err(format!(
            "The installed milestone-string table has unexpected row class 0x{string_class:08X}"
        ));
    }
    if definition_count != string_count {
        return Err("The installed milestone definition and string tables do not match".into());
    }

    for index in 0..definition_count {
        let definition = definition_rows + index * MILESTONE_DEFINITION_ROW_SIZE;
        let string = string_rows + index * MILESTONE_STRING_ROW_SIZE;
        let hash = u32_at(&definitions, definition)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Milestone definition and string row {index} do not match"
            ));
        }
        let name = MILESTONE_STRING_OFFSETS
            .into_iter()
            .filter_map(|offset| {
                resolve_string_index_reference(
                    manager,
                    localized_tags,
                    localized_cache,
                    &strings,
                    string + offset,
                )
            })
            .find(|value| !value.trim().is_empty())
            .unwrap_or_default();
        let owner = ObjectiveOwnerDef {
            hash: u64::from(hash),
            kind: ObjectiveOwnerKind::Milestone,
            name,
            type_name: "Milestone".into(),
            description: String::new(),
            traits: Vec::new(),
            paths: Vec::new(),
        };
        for objective_index in
            milestone_objective_indices(&definitions, definition, objectives.len())?
        {
            add_objective_owner(objectives, objective_index, owner.clone());
        }
    }
    Ok(())
}

pub(in crate::catalog) fn scan_objectives(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
    unlock_flag_definitions: &mut [UnlockDefinition],
    unlock_value_definitions: &mut [UnlockDefinition],
) -> Result<Vec<ObjectiveDef>, String> {
    // Shadowkeep does not expose an explicit objective-to-unlock-value field here.
    // Keep this conservative association to definitions that share the same hash.
    let related_unlock_value_indices = unlock_value_definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.hash, index))
        .collect::<HashMap<_, _>>();
    let definition_pointer = 8_usize
        .checked_add(
            OBJECTIVE_DEFINITION_TABLE_SLOT
                .checked_mul(16)
                .ok_or("Objective definition table offset overflowed")?,
        )
        .ok_or("Objective definition table offset overflowed")?;
    let string_pointer = 16_usize
        .checked_add(
            OBJECTIVE_STRING_TABLE_SLOT
                .checked_mul(16)
                .ok_or("Objective string table offset overflowed")?,
        )
        .ok_or("Objective string table offset overflowed")?;
    let definitions = manager
        .read_tag(TagHash(u32_at(root, definition_pointer)?))
        .map_err(|error| format!("Could not read objective definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(globals, string_pointer)?))
        .map_err(|error| format!("Could not read objective strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_class != OBJECTIVE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed objective table has unexpected row class 0x{definition_class:08X}"
        ));
    }
    if definition_count != string_count {
        return Err("The installed objective definition and string tables do not match".into());
    }

    let mut objectives = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition = definition_rows
            .checked_add(
                index
                    .checked_mul(OBJECTIVE_DEFINITION_ROW_SIZE)
                    .ok_or("Objective definition row offset overflowed")?,
            )
            .ok_or("Objective definition row offset overflowed")?;
        let string = string_rows
            .checked_add(
                index
                    .checked_mul(OBJECTIVE_STRING_ROW_SIZE)
                    .ok_or("Objective string row offset overflowed")?,
            )
            .ok_or("Objective string row offset overflowed")?;
        let definition_hash = u32_at(&definitions, definition)?;
        let string_hash = u32_at(&strings, string)?;
        if definition_hash != string_hash {
            return Err(format!(
                "Objective definition and string row {index} do not match"
            ));
        }
        let mut objective_string = |offset| {
            resolve_string(
                manager,
                localized_tags,
                localized_cache,
                &strings,
                string + offset,
            )
            .unwrap_or_default()
        };
        let name = objective_string(OBJECTIVE_NAME_OFFSET);
        let display_description = objective_string(OBJECTIVE_DISPLAY_DESCRIPTION_OFFSET);
        let progress_description = objective_string(OBJECTIVE_PROGRESS_DESCRIPTION_OFFSET);
        let description = [
            progress_description.as_str(),
            name.as_str(),
            display_description.as_str(),
        ]
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default()
        .to_owned();
        let condition_references = objective_condition_references_at(&definitions, definition)?;
        let condition_programs = condition_references.programs.clone();
        let referenced_objective_indices = Vec::new();
        let intrinsic_perk_flag_definition_indices = objective_intrinsic_perk_flag_indices_at(
            &definitions,
            definition,
            unlock_flag_definitions.len(),
        )?;
        attach_condition_context(
            unlock_flag_definitions,
            unlock_value_definitions,
            &condition_references,
            &ProgressionContextDef {
                direct_references: Vec::new(),
                hash: u64::from(definition_hash),
                kind: ProgressionContextKind::Objective,
                name: description.clone(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            },
        );
        objectives.push(ObjectiveDef {
            hash: u64::from(definition_hash),
            name,
            display_description,
            progress_description,
            description,
            completion_value: i32_at(&definitions, definition + OBJECTIVE_COMPLETION_VALUE_OFFSET)?,
            allow_overcompletion: bool_at(
                &definitions,
                definition + OBJECTIVE_ALLOW_OVERCOMPLETION_OFFSET,
            )?,
            allow_negative_value: bool_at(
                &definitions,
                definition + OBJECTIVE_ALLOW_NEGATIVE_VALUE_OFFSET,
            )?,
            allow_value_change_when_completed: bool_at(
                &definitions,
                definition + OBJECTIVE_ALLOW_VALUE_CHANGE_WHEN_COMPLETED_OFFSET,
            )?,
            is_counting_downward: bool_at(
                &definitions,
                definition + OBJECTIVE_IS_COUNTING_DOWNWARD_OFFSET,
            )?,
            condition_programs,
            referenced_objective_indices,
            intrinsic_perk_flag_definition_indices,
            owners: Vec::new(),
            related_unlock_value_definition_index: related_unlock_value_indices
                .get(&u64::from(definition_hash))
                .copied()
                .and_then(|index| u16::try_from(index).ok()),
        });
    }
    Ok(objectives)
}
