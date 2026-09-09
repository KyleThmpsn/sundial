use super::*;

pub(in crate::catalog) fn scan_collectible_item_paths(
    manager: &PackageManager,
    root: &[u8],
    presentation_nodes: &[PresentationNodeDef],
) -> Result<HashMap<usize, Vec<Vec<String>>>, String> {
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + COLLECTIBLE_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read collectible definitions: {error}"))?;
    collectible_item_paths_from_definitions(&definitions, presentation_nodes)
}

pub(in crate::catalog) fn scan_collectible_condition_contexts(
    manager: &PackageManager,
    root: &[u8],
    presentation_nodes: &[PresentationNodeDef],
) -> Result<HashMap<usize, Vec<PendingProgressionContext>>, String> {
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + COLLECTIBLE_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read collectible definitions: {error}"))?;
    let (definition_count, definition_rows, row_class) = array_at(&definitions, 8)?;
    if row_class != COLLECTIBLE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed collectible table has unexpected row class 0x{row_class:08X}"
        ));
    }

    let mut contexts = vec![None::<(usize, PendingProgressionContext)>; definition_count];
    for (index, context) in contexts.iter_mut().enumerate() {
        let row = definition_rows
            .checked_add(
                index
                    .checked_mul(COLLECTIBLE_DEFINITION_ROW_SIZE)
                    .ok_or("Collectible definition row offset overflowed")?,
            )
            .ok_or("Collectible definition row offset overflowed")?;
        let item_index = usize::from(u16_at(
            &definitions,
            row + COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET,
        )?);
        if item_index == usize::from(u16::MAX) {
            continue;
        }
        let parents = definition_index_list(
            &definitions,
            row + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            presentation_nodes.len(),
            "collectible presentation-node parent",
        )?;
        let mut references = ConditionReferences::default();
        for offset in COLLECTIBLE_CONDITION_OFFSETS {
            merge_condition_references(
                &mut references,
                condition_references_at(&definitions, row + offset)?,
            );
        }
        *context = Some((
            item_index,
            PendingProgressionContext {
                hash: u64::from(u32_at(&definitions, row + COLLECTIBLE_HASH_OFFSET)?),
                kind: ProgressionContextKind::Collectible,
                references,
                paths: presentation_paths(presentation_nodes, &parents),
            },
        ));
    }

    let mut by_item = HashMap::<usize, Vec<PendingProgressionContext>>::new();
    for (item_index, context) in contexts.into_iter().flatten() {
        if !context.references.flags.is_empty() || !context.references.values.is_empty() {
            by_item.entry(item_index).or_default().push(context);
        }
    }
    Ok(by_item)
}

pub(in crate::catalog) struct ItemProgressionContext<'a> {
    pub hash: u64,
    pub name: &'a str,
    pub type_name: &'a str,
    pub paths: &'a [Vec<String>],
}

pub(in crate::catalog) fn attach_item_condition_contexts(
    item: &[u8],
    item_context: ItemProgressionContext<'_>,
    collectible_contexts: &[PendingProgressionContext],
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) {
    let direct_references = scan_condition_expressions(item);
    let direct_context = ProgressionContextDef {
        direct_references: Vec::new(),
        hash: item_context.hash,
        kind: ProgressionContextKind::InventoryItem,
        name: item_context.name.to_owned(),
        type_name: item_context.type_name.to_owned(),
        description: String::new(),
        paths: item_context.paths.to_vec(),
        condition_programs: Vec::new(),
    };
    attach_condition_context(
        flag_definitions,
        value_definitions,
        &direct_references,
        &direct_context,
    );

    for pending in collectible_contexts {
        let context = ProgressionContextDef {
            direct_references: Vec::new(),
            hash: pending.hash,
            kind: pending.kind,
            name: item_context.name.to_owned(),
            type_name: item_context.type_name.to_owned(),
            description: String::new(),
            paths: pending.paths.clone(),
            condition_programs: Vec::new(),
        };
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &pending.references,
            &context,
        );
    }
}

pub(super) fn collectible_item_paths_from_definitions(
    definitions: &[u8],
    presentation_nodes: &[PresentationNodeDef],
) -> Result<HashMap<usize, Vec<Vec<String>>>, String> {
    let (definition_count, definition_rows, row_class) = array_at(definitions, 8)?;
    if row_class != COLLECTIBLE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed collectible table has unexpected row class 0x{row_class:08X}"
        ));
    }

    let mut paths_by_item_index = HashMap::<usize, Vec<Vec<String>>>::new();
    for index in 0..definition_count {
        let definition = definition_rows
            .checked_add(
                index
                    .checked_mul(COLLECTIBLE_DEFINITION_ROW_SIZE)
                    .ok_or("Collectible definition row offset overflowed")?,
            )
            .ok_or("Collectible definition row offset overflowed")?;
        let item_index = u16_at(
            definitions,
            definition + COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET,
        )?;
        if item_index == u16::MAX {
            continue;
        }
        let parents = definition_index_list(
            definitions,
            definition + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            presentation_nodes.len(),
            "collectible presentation-node parent",
        )?;
        let paths = presentation_paths(presentation_nodes, &parents);
        if paths.is_empty() {
            continue;
        }
        let item_paths = paths_by_item_index
            .entry(usize::from(item_index))
            .or_default();
        for path in paths {
            if !item_paths.contains(&path) {
                item_paths.push(path);
            }
        }
    }
    Ok(paths_by_item_index)
}
