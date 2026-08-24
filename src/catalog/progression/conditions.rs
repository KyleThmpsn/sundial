use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ConditionReferences {
    pub(super) flags: Vec<usize>,
    pub(super) values: Vec<usize>,
    pub(super) objectives: Vec<usize>,
    pub(super) programs: Vec<Vec<[u32; 2]>>,
}

pub(super) fn add_progression_context(
    definitions: &mut [UnlockDefinition],
    definition_index: usize,
    context: &ProgressionContextDef,
) {
    let Some(definition) = definitions.get_mut(definition_index) else {
        return;
    };
    if let Some(existing) = definition
        .tested_by
        .iter_mut()
        .find(|existing| existing.kind == context.kind && existing.hash == context.hash)
    {
        if existing.name.trim().is_empty() && !context.name.trim().is_empty() {
            existing.name.clone_from(&context.name);
        }
        if existing.type_name.trim().is_empty() && !context.type_name.trim().is_empty() {
            existing.type_name.clone_from(&context.type_name);
        }
        if existing.description.trim().is_empty() && !context.description.trim().is_empty() {
            existing.description.clone_from(&context.description);
        }
        for path in &context.paths {
            if !path.is_empty() && !existing.paths.contains(path) {
                existing.paths.push(path.clone());
            }
        }
        for program in &context.condition_programs {
            if !program.is_empty() && !existing.condition_programs.contains(program) {
                existing.condition_programs.push(program.clone());
            }
        }
        return;
    }
    definition.tested_by.push(context.clone());
}

pub(in crate::catalog) fn sort_progression_contexts(definitions: &mut [UnlockDefinition]) {
    for definition in definitions {
        for context in &mut definition.tested_by {
            context.paths.sort();
            context.paths.dedup();
        }
        definition.tested_by.sort_by(|left, right| {
            progression_context_priority(left.kind)
                .cmp(&progression_context_priority(right.kind))
                .then_with(|| {
                    left.name
                        .trim()
                        .is_empty()
                        .cmp(&right.name.trim().is_empty())
                })
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.hash.cmp(&right.hash))
        });
    }
}

const fn progression_context_priority(kind: ProgressionContextKind) -> u8 {
    match kind {
        ProgressionContextKind::InventoryItem => 0,
        ProgressionContextKind::Collectible => 1,
        ProgressionContextKind::Record => 2,
        ProgressionContextKind::Objective => 3,
        ProgressionContextKind::PresentationNode => 4,
        ProgressionContextKind::Activity => 5,
        ProgressionContextKind::Location => 6,
        ProgressionContextKind::LocationRelease => 7,
        ProgressionContextKind::ActivityAvailability => 8,
        ProgressionContextKind::ExpressionMapping => 9,
    }
}

pub(super) fn attach_condition_context(
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
    references: &ConditionReferences,
    context: &ProgressionContextDef,
) {
    let mut context = context.clone();
    for program in &references.programs {
        if !program.is_empty() && !context.condition_programs.contains(program) {
            context.condition_programs.push(program.clone());
        }
    }
    for &index in &references.flags {
        add_progression_context(flag_definitions, index, &context);
    }
    for &index in &references.values {
        add_progression_context(value_definitions, index, &context);
    }
}

pub(super) fn condition_references_at(
    data: &[u8],
    descriptor: usize,
) -> Result<ConditionReferences, String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(ConditionReferences::default());
    }
    let (count, rows, row_class) = array_at(data, descriptor)?;
    if row_class != CONDITION_EXPRESSION_ROW_CLASS {
        return Err(format!(
            "Unexpected condition-expression row class 0x{row_class:08X}"
        ));
    }
    condition_references_from_rows(data, rows, count)
}

pub(super) fn objective_condition_references_at(
    definitions: &[u8],
    definition: usize,
) -> Result<ConditionReferences, String> {
    let mut references = ConditionReferences::default();
    for offset in [
        OBJECTIVE_CONDITIONS_OFFSET,
        OBJECTIVE_SECONDARY_CONDITIONS_OFFSET,
    ] {
        let descriptor = definition
            .checked_add(offset)
            .ok_or("Objective condition descriptor offset overflowed")?;
        merge_condition_references(
            &mut references,
            condition_references_at(definitions, descriptor)?,
        );
    }
    Ok(references)
}

pub(super) fn objective_intrinsic_perk_flag_indices_at(
    definitions: &[u8],
    definition: usize,
    flag_definition_count: usize,
) -> Result<Vec<u16>, String> {
    let descriptor = definition
        .checked_add(OBJECTIVE_INTRINSIC_PERK_FLAGS_OFFSET)
        .ok_or("Objective intrinsic-perk descriptor offset overflowed")?;
    if u64_at(definitions, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, row_class) = array_at(definitions, descriptor)?;
    if row_class != OBJECTIVE_INTRINSIC_PERK_FLAG_ROW_CLASS {
        return Err(format!(
            "Unexpected objective intrinsic-perk row class 0x{row_class:08X}"
        ));
    }
    let byte_count = count
        .checked_mul(OBJECTIVE_INTRINSIC_PERK_FLAG_ROW_SIZE)
        .ok_or("Objective intrinsic-perk list size overflowed")?;
    if rows
        .checked_add(byte_count)
        .is_none_or(|end| end > definitions.len())
    {
        return Err("Objective intrinsic-perk list extends beyond its package data".into());
    }

    let mut indices = Vec::with_capacity(count);
    for row_index in 0..count {
        let index = u16_at(
            definitions,
            rows + row_index * OBJECTIVE_INTRINSIC_PERK_FLAG_ROW_SIZE,
        )?;
        if usize::from(index) >= flag_definition_count {
            return Err(format!(
                "Objective intrinsic perk references unavailable unlock flag definition #{index}"
            ));
        }
        if !indices.contains(&index) {
            indices.push(index);
        }
    }
    Ok(indices)
}

pub(super) fn condition_references_from_rows(
    data: &[u8],
    rows: usize,
    count: usize,
) -> Result<ConditionReferences, String> {
    let byte_count = count
        .checked_mul(CONDITION_EXPRESSION_ROW_SIZE)
        .ok_or("Condition-expression size overflowed")?;
    let end = rows
        .checked_add(byte_count)
        .ok_or("Condition-expression offset overflowed")?;
    if end > data.len() {
        return Err("Condition expression extends beyond its package data".into());
    }

    let mut references = ConditionReferences::default();
    let mut program = Vec::with_capacity(count);
    for index in 0..count {
        let row = rows + index * CONDITION_EXPRESSION_ROW_SIZE;
        let kind = u32_at(data, row)?;
        let raw_operand = u32_at(data, row + 4)?;
        program.push([kind, raw_operand]);
        let operand = usize::try_from(raw_operand)
            .map_err(|_| "Condition-expression definition index is too large")?;
        match kind {
            CONDITION_FLAG_KIND => references.flags.push(operand),
            CONDITION_VALUE_KIND => references.values.push(operand),
            CONDITION_OBJECTIVE_KIND => references.objectives.push(operand),
            _ => {}
        }
    }
    references.flags.sort_unstable();
    references.flags.dedup();
    references.values.sort_unstable();
    references.values.dedup();
    references.objectives.sort_unstable();
    references.objectives.dedup();
    if !program.is_empty() {
        references.programs.push(program);
    }
    Ok(references)
}

pub(super) fn merge_condition_references(
    target: &mut ConditionReferences,
    source: ConditionReferences,
) {
    target.flags.extend(source.flags);
    target.values.extend(source.values);
    target.objectives.extend(source.objectives);
    for program in source.programs {
        if !program.is_empty() && !target.programs.contains(&program) {
            target.programs.push(program);
        }
    }
    target.flags.sort_unstable();
    target.flags.dedup();
    target.values.sort_unstable();
    target.values.dedup();
    target.objectives.sort_unstable();
    target.objectives.dedup();
}

pub(super) fn scan_condition_expressions(data: &[u8]) -> ConditionReferences {
    let mut references = ConditionReferences::default();
    let Some(last_descriptor) = data.len().checked_sub(16) else {
        return references;
    };
    for descriptor in (0..=last_descriptor).step_by(8) {
        let Ok((count, rows, row_class)) = array_at(data, descriptor) else {
            continue;
        };
        if row_class != CONDITION_EXPRESSION_ROW_CLASS {
            continue;
        }
        let Ok(found) = condition_references_from_rows(data, rows, count) else {
            continue;
        };
        merge_condition_references(&mut references, found);
    }
    references
}

pub(super) fn scan_condition_expressions_in(
    data: &[u8],
    owner_start: usize,
    owner_end: usize,
) -> Result<ConditionReferences, String> {
    if owner_start > owner_end || owner_end > data.len() {
        return Err("Condition-expression owner range is outside its package data".into());
    }
    let Some(last_descriptor) = owner_end.checked_sub(16) else {
        return Ok(ConditionReferences::default());
    };
    if last_descriptor < owner_start {
        return Ok(ConditionReferences::default());
    }

    let mut references = ConditionReferences::default();
    let mut arrays = HashSet::new();
    for descriptor in (owner_start..=last_descriptor).step_by(8) {
        let Ok((count, rows, row_class)) = array_at(data, descriptor) else {
            continue;
        };
        if row_class != CONDITION_EXPRESSION_ROW_CLASS || !arrays.insert((rows, count)) {
            continue;
        }
        let pointer = descriptor
            .checked_add(8)
            .ok_or("Condition-expression pointer offset overflowed")?;
        let header = relative_offset(descriptor, 8, i64_at(data, pointer)?)?;
        let rows_end = rows
            .checked_add(
                count
                    .checked_mul(CONDITION_EXPRESSION_ROW_SIZE)
                    .ok_or("Condition-expression size overflowed")?,
            )
            .ok_or("Condition-expression row offset overflowed")?;
        if header < owner_start || rows_end > owner_end {
            continue;
        }
        merge_condition_references(
            &mut references,
            condition_references_from_rows(data, rows, count)?,
        );
    }
    Ok(references)
}
