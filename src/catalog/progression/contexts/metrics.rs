use super::*;

pub(in crate::catalog::progression) fn metric_trait_indices(
    definitions: &[u8],
    metric_definition: usize,
    trait_count: usize,
) -> Result<Vec<usize>, String> {
    definition_index_list(
        definitions,
        metric_definition + METRIC_TRAIT_LIST_OFFSET,
        METRIC_TRAIT_INDEX_ROW_CLASS,
        trait_count,
        "metric trait",
    )
}

pub(in crate::catalog) fn scan_metric_objective_owners(
    package: &mut ProgressionPackageData<'_>,
    presentation_nodes: &[PresentationNodeDef],
    objectives: &mut [ObjectiveDef],
) -> Result<(), String> {
    let metric_traits = scan_metric_traits(package)?;
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
            8 + METRIC_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read metric definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + METRIC_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read metric strings: {error}"))?;
    let (definition_count, definition_rows, _) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_count != string_count {
        return Err("The installed metric definition and string tables do not match".into());
    }

    for index in 0..definition_count {
        let definition = definition_rows + index * METRIC_DEFINITION_ROW_SIZE;
        let string = string_rows + index * METRIC_STRING_ROW_SIZE;
        let hash = u32_at(&definitions, definition + METRIC_HASH_OFFSET)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Metric definition and string row {index} do not match"
            ));
        }
        let objective_index = usize::from(u16_at(
            &definitions,
            definition + METRIC_OBJECTIVE_INDEX_OFFSET,
        )?);
        if objective_index == usize::from(u16::MAX) || objective_index >= objectives.len() {
            continue;
        }
        let parents = if presentation_nodes.is_empty() {
            Vec::new()
        } else {
            definition_index_list(
                &definitions,
                definition + PRESENTATION_NODE_PARENTS_OFFSET,
                PRESENTATION_NODE_INDEX_ROW_CLASS,
                presentation_nodes.len(),
                "metric parent",
            )?
        };
        let traits = metric_trait_indices(&definitions, definition, metric_traits.len())?
            .into_iter()
            .map(|trait_index| metric_traits[trait_index].clone())
            .collect();
        let description = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            string + 0x10,
        )
        .unwrap_or_default();
        let mut name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            string + 0x08,
        )
        .unwrap_or_default();
        if name.trim().is_empty() {
            name.clone_from(&description);
        }
        add_objective_owner(
            objectives,
            objective_index,
            ObjectiveOwnerDef {
                hash: u64::from(hash),
                kind: ObjectiveOwnerKind::Metric,
                name,
                type_name: "Metric".into(),
                description,
                traits,
                paths: presentation_paths(presentation_nodes, &parents),
            },
        );
    }
    Ok(())
}

fn scan_metric_traits(
    package: &mut ProgressionPackageData<'_>,
) -> Result<Vec<ObjectiveOwnerTraitDef>, String> {
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
    let definitions = manager
        .read_tag(TagHash(u32_at(root, 8 + TRAIT_DEFINITION_TABLE_SLOT * 16)?))
        .map_err(|error| format!("Could not read trait definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(globals, 16 + TRAIT_STRING_TABLE_SLOT * 16)?))
        .map_err(|error| format!("Could not read trait strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, string_class) = array_at(&strings, 8)?;
    if definition_class != TRAIT_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed trait table has unexpected row class 0x{definition_class:08X}"
        ));
    }
    if string_class != TRAIT_STRING_ROW_CLASS {
        return Err(format!(
            "The installed trait-string table has unexpected row class 0x{string_class:08X}"
        ));
    }
    if definition_count != string_count {
        return Err("The installed trait definition and string tables do not match".into());
    }

    let mut traits = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition = definition_rows
            .checked_add(
                index
                    .checked_mul(TRAIT_DEFINITION_ROW_SIZE)
                    .ok_or("Trait definition row offset overflowed")?,
            )
            .ok_or("Trait definition row offset overflowed")?;
        let string = string_rows
            .checked_add(
                index
                    .checked_mul(TRAIT_STRING_ROW_SIZE)
                    .ok_or("Trait string row offset overflowed")?,
            )
            .ok_or("Trait string row offset overflowed")?;
        let hash = u32_at(&definitions, definition)?;
        let _ = u32_at(&definitions, definition + 4)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Trait definition and string row {index} do not match"
            ));
        }
        let name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            string + 0x08,
        )
        .unwrap_or_default();
        let description = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &strings,
            string + 0x10,
        )
        .unwrap_or_default();
        traits.push(ObjectiveOwnerTraitDef {
            hash: u64::from(hash),
            name,
            description,
        });
    }
    Ok(traits)
}
