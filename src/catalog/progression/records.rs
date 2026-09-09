use super::*;

pub(in crate::catalog) fn scan_record_objective_owners(
    package: &mut ProgressionPackageData<'_>,
    presentation_nodes: &[PresentationNodeDef],
    objectives: &mut [ObjectiveDef],
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
            8 + RECORD_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read record definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + RECORD_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read record strings: {error}"))?;
    let (definition_count, definition_rows, _) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_count != string_count {
        return Err("The installed record definition and string tables do not match".into());
    }

    for index in 0..definition_count {
        let definition = definition_rows + index * RECORD_DEFINITION_ROW_SIZE;
        let string = string_rows + index * RECORD_STRING_ROW_SIZE;
        let hash = u32_at(&definitions, definition + RECORD_HASH_OFFSET)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Record definition and string row {index} do not match"
            ));
        }
        let parents = if presentation_nodes.is_empty() {
            Vec::new()
        } else {
            definition_index_list(
                &definitions,
                definition + PRESENTATION_NODE_PARENTS_OFFSET,
                PRESENTATION_NODE_INDEX_ROW_CLASS,
                presentation_nodes.len(),
                "record parent",
            )?
        };
        let name = [0x08, 0x10]
            .into_iter()
            .filter_map(|offset| {
                resolve_string(
                    manager,
                    localized_tags,
                    localized_cache,
                    &strings,
                    string + offset,
                )
            })
            .find(|value| !value.trim().is_empty())
            .unwrap_or_default();
        let paths = presentation_paths(presentation_nodes, &parents);
        let mut condition_references = ConditionReferences::default();
        for offset in RECORD_CONDITION_OFFSETS {
            merge_condition_references(
                &mut condition_references,
                condition_references_at(&definitions, definition + offset)?,
            );
        }
        let context = ProgressionContextDef {
            direct_references: Vec::new(),
            hash: u64::from(hash),
            kind: ProgressionContextKind::Record,
            name: name.clone(),
            type_name: String::new(),
            description: String::new(),
            paths: paths.clone(),
            condition_programs: Vec::new(),
        };
        attach_direct_reference(
            flag_definitions,
            u16_at(&definitions, definition + 98)?,
            "Record category flag",
            &context,
        )?;
        attach_direct_reference(
            flag_definitions,
            u16_at(&definitions, definition + 100)?,
            "Record completion flag",
            &context,
        )?;
        attach_direct_reference(
            value_definitions,
            u16_at(&definitions, definition + 82)?,
            "Redeemed interval count",
            &context,
        )?;
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &condition_references,
            &context,
        );

        for objective_index in record_objective_indices(&definitions, definition, objectives.len())
            .map_err(|error| format!("Record {index}: {error}"))?
        {
            add_objective_owner(
                objectives,
                objective_index,
                ObjectiveOwnerDef {
                    hash: u64::from(hash),
                    kind: ObjectiveOwnerKind::Record,
                    name: name.clone(),
                    type_name: "Record".into(),
                    description: String::new(),
                    traits: Vec::new(),
                    paths: paths.clone(),
                },
            );
        }
    }
    Ok(())
}

pub(super) fn record_objective_indices(
    definitions: &[u8],
    definition: usize,
    objective_count: usize,
) -> Result<Vec<usize>, String> {
    let mut indices = objective_indices_from_array(
        definitions,
        definition + RECORD_OBJECTIVE_LIST_OFFSET,
        RECORD_OBJECTIVE_INDEX_ROW_CLASS,
        size_of::<u16>(),
        0,
        objective_count,
        "objective list",
    )?;
    indices.extend(objective_indices_from_array(
        definitions,
        definition + RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET,
        RECORD_INTERVAL_OBJECTIVE_ROW_CLASS,
        RECORD_INTERVAL_OBJECTIVE_ROW_SIZE,
        RECORD_INTERVAL_OBJECTIVE_INDEX_OFFSET,
        objective_count,
        "interval objective list",
    )?);
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn objective_indices_from_array(
    data: &[u8],
    descriptor: usize,
    expected_class: u32,
    row_size: usize,
    index_offset: usize,
    objective_count: usize,
    label: &str,
) -> Result<Vec<usize>, String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, class) = array_at(data, descriptor)?;
    if class != expected_class {
        return Err(format!("unexpected {label} row class 0x{class:08X}"));
    }
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(row_size)
                .ok_or_else(|| format!("{label} size overflowed"))?,
        )
        .ok_or_else(|| format!("{label} offset overflowed"))?;
    if rows_end > data.len() || index_offset + size_of::<u16>() > row_size {
        return Err(format!("{label} extends beyond its package data"));
    }

    let mut indices = Vec::with_capacity(count);
    for index in 0..count {
        let objective_index = usize::from(u16_at(data, rows + index * row_size + index_offset)?);
        if objective_index < objective_count {
            indices.push(objective_index);
        }
    }
    Ok(indices)
}

pub(in crate::catalog) fn item_objective_indices(
    item: &[u8],
    objective_count: usize,
) -> Vec<usize> {
    let mut indices = Vec::new();
    let Ok(relative) = i64_at(item, ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET) else {
        return indices;
    };
    if relative == 0 {
        return indices;
    }
    let Ok(resource) = relative_offset(ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET, 0, relative) else {
        return indices;
    };
    let Some(resource_class_offset) = resource.checked_sub(size_of::<u32>()) else {
        return indices;
    };
    if u32_at(item, resource_class_offset) != Ok(ITEM_OBJECTIVE_RESOURCE_CLASS) {
        return indices;
    }
    let Ok((count, rows, class)) = array_at(item, resource) else {
        return indices;
    };
    if class != ITEM_OBJECTIVE_INDEX_ROW_CLASS || count > objective_count {
        return indices;
    }
    let Some(byte_count) = count.checked_mul(size_of::<u16>()) else {
        return indices;
    };
    if rows
        .checked_add(byte_count)
        .is_none_or(|end| end > item.len())
    {
        return indices;
    }
    for index in 0..count {
        if let Ok(index) = u16_at(item, rows + index * size_of::<u16>()) {
            let index = usize::from(index);
            if index < objective_count {
                indices.push(index);
            }
        }
    }
    indices.sort_unstable();
    indices.dedup();
    indices
}
