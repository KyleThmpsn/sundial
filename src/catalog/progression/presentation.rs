use super::*;

pub(in crate::catalog) fn scan_presentation_nodes(
    package: &mut ProgressionPackageData<'_>,
    objective_count: usize,
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<Vec<PresentationNodeDef>, String> {
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
            8 + PRESENTATION_NODE_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read presentation-node definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + PRESENTATION_NODE_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read presentation-node strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_class != PRESENTATION_NODE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed presentation-node definition table has row class 0x{definition_class:08X}; expected 0x{PRESENTATION_NODE_DEFINITION_ROW_CLASS:08X}"
        ));
    }
    if definition_count != string_count {
        return Err(
            "The installed presentation-node definition and string tables do not match".into(),
        );
    }

    let mut nodes = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition = definition_rows + index * PRESENTATION_NODE_DEFINITION_ROW_SIZE;
        let string = string_rows + index * PRESENTATION_NODE_STRING_ROW_SIZE;
        let hash = u32_at(&definitions, definition + PRESENTATION_NODE_HASH_OFFSET)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Presentation-node definition and string row {index} do not match"
            ));
        }
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
        let parents = definition_index_list(
            &definitions,
            definition + PRESENTATION_NODE_PARENTS_OFFSET,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            definition_count,
            "presentation-node parent",
        )?;
        let objective_index = usize::from(u16_at(
            &definitions,
            definition + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )?);
        let objective_index = (objective_index != usize::from(u16::MAX)
            && objective_index < objective_count)
            .then_some(objective_index);
        let mut condition_references = ConditionReferences::default();
        for offset in PRESENTATION_NODE_CONDITION_OFFSETS {
            merge_condition_references(
                &mut condition_references,
                condition_references_at(&definitions, definition + offset)?,
            );
        }
        nodes.push(PresentationNodeDef {
            hash: u64::from(hash),
            name,
            parents,
            objective_index,
            condition_references,
        });
    }
    for node in &nodes {
        let context = ProgressionContextDef {
            direct_references: Vec::new(),
            hash: node.hash,
            kind: ProgressionContextKind::PresentationNode,
            name: node.name.clone(),
            type_name: String::new(),
            description: String::new(),
            paths: presentation_paths(&nodes, &node.parents),
            condition_programs: Vec::new(),
        };
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &node.condition_references,
            &context,
        );
    }
    Ok(nodes)
}

pub(in crate::catalog) fn attach_presentation_node_objective_owners(
    objectives: &mut [ObjectiveDef],
    nodes: &[PresentationNodeDef],
) {
    for node in nodes {
        let Some(objective_index) = node.objective_index else {
            continue;
        };
        add_objective_owner(
            objectives,
            objective_index,
            ObjectiveOwnerDef {
                hash: node.hash,
                kind: ObjectiveOwnerKind::PresentationNode,
                name: node.name.clone(),
                type_name: "Presentation node".into(),
                description: String::new(),
                traits: Vec::new(),
                paths: presentation_paths(nodes, &node.parents),
            },
        );
    }
}

pub(in crate::catalog) fn presentation_paths(
    nodes: &[PresentationNodeDef],
    parents: &[usize],
) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for &parent in parents {
        collect_presentation_paths(nodes, parent, &mut Vec::new(), &mut Vec::new(), &mut paths);
    }
    paths.retain(|path| !path.is_empty());
    let mut deduplicated = Vec::new();
    for path in paths {
        if !deduplicated.contains(&path) {
            deduplicated.push(path);
        }
    }
    deduplicated
}

pub(super) fn collect_presentation_paths(
    nodes: &[PresentationNodeDef],
    index: usize,
    visited: &mut Vec<usize>,
    names: &mut Vec<String>,
    paths: &mut Vec<Vec<String>>,
) {
    if visited.contains(&index) {
        if !names.is_empty() {
            paths.push(names.clone());
        }
        return;
    }
    let Some(node) = nodes.get(index) else {
        if !names.is_empty() {
            paths.push(names.clone());
        }
        return;
    };
    visited.push(index);
    let added_name = !node.name.trim().is_empty();
    if added_name {
        names.push(node.name.clone());
    }
    if node.parents.is_empty() {
        if !names.is_empty() {
            paths.push(names.clone());
        }
    } else {
        for &parent in &node.parents {
            collect_presentation_paths(nodes, parent, visited, names, paths);
        }
    }
    if added_name {
        names.pop();
    }
    visited.pop();
}

pub(in crate::catalog) fn definition_index_list(
    data: &[u8],
    descriptor: usize,
    expected_class: u32,
    definition_count: usize,
    label: &str,
) -> Result<Vec<usize>, String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, class) = array_at(data, descriptor)?;
    if class != expected_class {
        return Err(format!("Unexpected {label} row class 0x{class:08X}"));
    }
    if count > definition_count {
        return Err(format!("{label} list is larger than its definition table"));
    }
    let byte_count = count
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| format!("{label} list size overflowed"))?;
    let end = rows
        .checked_add(byte_count)
        .ok_or_else(|| format!("{label} list offset overflowed"))?;
    if end > data.len() {
        return Err(format!("{label} list extends beyond its package data"));
    }
    (0..count)
        .map(|index| {
            let definition_index = usize::from(u16_at(data, rows + index * size_of::<u16>())?);
            if definition_index >= definition_count {
                Err(format!("{label} index {definition_index} is out of range"))
            } else {
                Ok(definition_index)
            }
        })
        .collect()
}
