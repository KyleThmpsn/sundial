//! Fallback provenance for expression-bearing root tables without a named owner decoder.
use super::*;

pub(in crate::catalog) fn scan_package_condition_contexts(
    manager: &PackageManager,
    root: &[u8],
    pool: &[Vec<crate::catalog::CollectionConditionTokenDef>],
    flags: &mut [UnlockDefinition],
    values: &mut [UnlockDefinition],
) -> Result<(), String> {
    for slot in 0..119 {
        let tag = TagHash(u32_at(root, 8 + slot * 16)?);
        if tag.0 == 0 || tag.0 == u32::MAX || manager.get_entry(tag).is_none() {
            continue;
        }
        let data = manager
            .read_tag(tag)
            .map_err(|error| format!("Could not read investment table {slot}: {error}"))?;
        let Some(last) = data.len().checked_sub(16) else {
            continue;
        };
        let mut arrays = HashSet::new();
        for descriptor in (0..=last).step_by(8) {
            let Ok((count, rows, class)) = array_at(&data, descriptor) else {
                continue;
            };
            if class != CONDITION_EXPRESSION_ROW_CLASS
                || count == 0
                || !arrays.insert((rows, count))
            {
                continue;
            }
            let mut references = condition_references_from_rows(&data, rows, count)?;
            expand_references(&mut references, pool);
            let context = ProgressionContextDef {
                hash: (u64::from(tag.0) << 32)
                    | u64::try_from(descriptor)
                        .map_err(|_| "Expression descriptor exceeds its identity range")?,
                kind: ProgressionContextKind::PackageExpression,
                name: format!("Investment Table {slot} · Expression 0x{descriptor:X}"),
                type_name: "Package Expression".into(),
                description: format!(
                    "Package tag 0x{:08X}, expression descriptor 0x{descriptor:X}. This identifies a package location rather than a content definition hash.",
                    tag.0
                ),
                paths: Vec::new(),
                condition_programs: Vec::new(),
                direct_references: Vec::new(),
            };
            attach_condition_context(flags, values, &references, &context);
        }
    }
    Ok(())
}

fn expand_references(
    references: &mut ConditionReferences,
    pool: &[Vec<crate::catalog::CollectionConditionTokenDef>],
) {
    let mut queue = references.pool_rows.clone();
    let mut visited = HashSet::new();
    while let Some(index) = queue.pop() {
        if !visited.insert(index) {
            continue;
        }
        let Some(program) = pool.get(index) else {
            continue;
        };
        for token in program {
            match token.kind {
                CONDITION_FLAG_KIND => references.flags.push(token.operand as usize),
                CONDITION_VALUE_KIND => references.values.push(token.operand as usize),
                CONDITION_POOL_KIND => queue.push(token.operand as usize),
                _ => {}
            }
        }
    }
    references.flags.sort_unstable();
    references.flags.dedup();
    references.values.sort_unstable();
    references.values.dedup();
}

pub(in crate::catalog) fn expand_shared_condition_contexts(
    pool: &[Vec<crate::catalog::CollectionConditionTokenDef>],
    flags: &mut [UnlockDefinition],
    values: &mut [UnlockDefinition],
) {
    let contexts = flags
        .iter()
        .chain(values.iter())
        .flat_map(|definition| &definition.tested_by)
        .filter(|context| {
            context
                .condition_programs
                .iter()
                .flatten()
                .any(|token| token[0] == CONDITION_POOL_KIND)
        })
        .fold(HashMap::new(), |mut contexts, context| {
            contexts
                .entry((context.kind as u8, context.hash))
                .or_insert_with(|| context.clone());
            contexts
        });
    for context in contexts.into_values() {
        let mut queue = context
            .condition_programs
            .iter()
            .flatten()
            .filter(|token| token[0] == CONDITION_POOL_KIND)
            .map(|token| token[1] as usize)
            .collect::<Vec<_>>();
        let mut visited = HashSet::new();
        while let Some(index) = queue.pop() {
            if !visited.insert(index) {
                continue;
            }
            let Some(program) = pool.get(index) else {
                continue;
            };
            for token in program {
                match token.kind {
                    CONDITION_FLAG_KIND => {
                        add_progression_context(flags, token.operand as usize, &context)
                    }
                    CONDITION_VALUE_KIND => {
                        add_progression_context(values, token.operand as usize, &context)
                    }
                    CONDITION_POOL_KIND => queue.push(token.operand as usize),
                    _ => {}
                }
            }
        }
    }
}
