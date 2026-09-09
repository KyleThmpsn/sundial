use super::*;

fn checked_rows(
    data: &[u8],
    descriptor: usize,
    expected: u32,
    stride: usize,
) -> Result<(usize, usize), String> {
    let (count, rows, class) = array_at(data, descriptor)?;
    if class != expected {
        return Err(format!(
            "Unexpected direct-reference row class 0x{class:08X}"
        ));
    }
    if count
        .checked_mul(stride)
        .and_then(|size| rows.checked_add(size))
        .is_none_or(|end| end > data.len())
    {
        return Err("Direct-reference array extends beyond its package data".into());
    }
    Ok((count, rows))
}

pub(super) fn attach_direct_reference(
    definitions: &mut [UnlockDefinition],
    slot: u16,
    field: &str,
    context: &ProgressionContextDef,
) -> Result<(), String> {
    if slot == u16::MAX {
        return Ok(());
    }
    if usize::from(slot) >= definitions.len() {
        return Err(format!(
            "{field} references unavailable unlock definition {slot}"
        ));
    }
    let mut context = context.clone();
    context.direct_references.push(format!("{field}: #{slot}"));
    add_progression_context(definitions, usize::from(slot), &context);
    Ok(())
}

pub(in crate::catalog) fn scan_context_writers(
    manager: &PackageManager,
    root: &[u8],
    flags: &mut [UnlockDefinition],
    values: &mut [UnlockDefinition],
) -> Result<(), String> {
    let data = manager
        .read_tag(TagHash(u32_at(root, 8 + 110 * 16)?))
        .map_err(|error| format!("Could not read timed unlock writers: {error}"))?;
    let (count, rows) = checked_rows(&data, 8, 0x8080_7476, 40)?;
    for index in 0..count {
        let row = rows + index * 40;
        for (definitions, slot, field) in [
            (&mut *flags, u16_at(&data, row)?, "Timed flag output"),
            (&mut *values, u16_at(&data, row + 2)?, "Timed value output"),
        ] {
            if slot == u16::MAX {
                continue;
            }
            let context = context(
                index as u64,
                ProgressionContextKind::ExpressionMapping,
                format!("Timed Unlock #{index}"),
            );
            attach_direct_reference(definitions, slot, field, &context)?;
            definitions[usize::from(slot)]
                .runtime_writers
                .push(UnlockWriter::Context {
                    source: "Timed unlock refresh".into(),
                });
        }
    }
    Ok(())
}

pub(super) fn attach_flag_writes(
    data: &[u8],
    descriptor: usize,
    flags: &mut [UnlockDefinition],
    owner: &ProgressionContextDef,
) -> Result<(), String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(());
    }
    let (count, rows) = checked_rows(data, descriptor, 0x8080_7D34, 4)?;
    for index in 0..count {
        let slot = u16_at(data, rows + index * 4)?;
        if slot == u16::MAX {
            continue;
        }
        attach_direct_reference(flags, slot, "Activity flag output", owner)?;
        let writer = UnlockWriter::Context {
            source: "Activity, destination, or bubble context".into(),
        };
        if !flags[usize::from(slot)].runtime_writers.contains(&writer) {
            flags[usize::from(slot)].runtime_writers.push(writer);
        }
    }
    Ok(())
}

pub(super) fn attach_conditional_flag_writes(
    data: &[u8],
    descriptor: usize,
    flags: &mut [UnlockDefinition],
    owner: &ProgressionContextDef,
) -> Result<(), String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(());
    }
    let (count, rows) = checked_rows(data, descriptor, 0x8080_7724, 48)?;
    for index in 0..count {
        attach_flag_writes(data, rows + index * 48 + 32, flags, owner)?;
    }
    Ok(())
}

fn context(hash: u64, kind: ProgressionContextKind, name: String) -> ProgressionContextDef {
    ProgressionContextDef {
        hash,
        kind,
        name,
        type_name: String::new(),
        description: String::new(),
        paths: Vec::new(),
        condition_programs: Vec::new(),
        direct_references: Vec::new(),
    }
}

pub(in crate::catalog) fn attach_progression_references(
    progressions: &[ProgressionDefinition],
    flags: &mut [UnlockDefinition],
    values: &mut [UnlockDefinition],
) -> Result<(), String> {
    for progression in progressions {
        let context = context(
            progression.hash,
            ProgressionContextKind::Progression,
            progression.name.clone(),
        );
        for (index, step) in progression.steps.iter().enumerate() {
            if let Some(slot) = step.unlock_flag {
                attach_direct_reference(
                    flags,
                    slot,
                    &format!("Rank {} flag", index + 1),
                    &context,
                )?;
                flags[usize::from(slot)]
                    .runtime_writers
                    .push(UnlockWriter::ProgressionStep {
                        definition_index: progression.definition_index,
                        step_index: u16::try_from(index)
                            .map_err(|_| "Progression rank index exceeds its native range")?,
                    });
            }
        }
        for (index, reward) in progression.reward_items.iter().enumerate() {
            if let Some(slot) = reward.claim_flag {
                attach_direct_reference(
                    flags,
                    slot,
                    &format!("Reward {} claim flag", index + 1),
                    &context,
                )?;
            }
        }
        if let Some(slot) = progression.level_value {
            attach_direct_reference(values, slot, "Progression level output", &context)?;
            values[usize::from(slot)]
                .runtime_writers
                .push(UnlockWriter::ProgressionLevel {
                    definition_index: progression.definition_index,
                });
        }
    }
    Ok(())
}

pub(in crate::catalog) fn scan_direct_tables(
    manager: &PackageManager,
    root: &[u8],
    flags: &mut [UnlockDefinition],
    values: &mut [UnlockDefinition],
) -> Result<(), String> {
    let achievements = manager
        .read_tag(TagHash(u32_at(root, 8 + 16)?))
        .map_err(|error| format!("Could not read achievements: {error}"))?;
    let (count, rows) = checked_rows(&achievements, 8, 0x8080_7521, 20)?;
    for index in 0..count {
        let row = rows + index * 20;
        let context = context(
            u64::from(u32_at(&achievements, row)?),
            ProgressionContextKind::Achievement,
            format!("Achievement #{index}"),
        );
        attach_direct_reference(
            flags,
            u16_at(&achievements, row + 4)?,
            "Achievement flag",
            &context,
        )?;
    }

    let requirements = manager
        .read_tag(TagHash(u32_at(root, 8 + 61 * 16)?))
        .map_err(|error| format!("Could not read requirements: {error}"))?;
    let (count, rows) = checked_rows(&requirements, 8, 0x8080_74B5, 64)?;
    for index in 0..count {
        let row = rows + index * 64;
        let context = context(
            u64::from(u32_at(&requirements, row)?),
            ProgressionContextKind::Requirement,
            format!("Requirement #{index}"),
        );
        if u64_at(&requirements, row + 24)? == 0 {
            continue;
        }
        let (count, rows) = checked_rows(&requirements, row + 24, 0x8080_7D34, 4)?;
        for index in 0..count {
            let row = rows + index * 4;
            attach_direct_reference(
                flags,
                u16_at(&requirements, row)?,
                "Required flag",
                &context,
            )?;
        }
    }

    let counters = manager
        .read_tag(TagHash(u32_at(root, 8 + 115 * 16)?))
        .map_err(|error| format!("Could not read unlock value counters: {error}"))?;
    let (count, rows) = checked_rows(&counters, 8, 0x8080_747C, 32)?;
    for index in 0..count {
        let row = rows + index * 32;
        let context = context(
            u64::from(u32_at(&counters, row)?),
            ProgressionContextKind::ValueCounter,
            format!("Value Counter #{index}"),
        );
        let destination = u16_at(&counters, row + 24)?;
        let mut programs = Vec::new();
        let mut references = ConditionReferences::default();
        if u64_at(&counters, row + 8)? != 0 {
            let (count, rows) = checked_rows(&counters, row + 8, 0x8080_7D2F, 16)?;
            for index in 0..count {
                let program = condition_references_at(&counters, rows + index * 16)?;
                programs.push(program.programs.first().cloned().unwrap_or_default());
                merge_condition_references(&mut references, program);
            }
        }
        attach_condition_context(flags, values, &references, &context);
        attach_direct_reference(values, destination, "Counter output", &context)?;
        if destination != u16::MAX {
            values[usize::from(destination)]
                .runtime_writers
                .push(UnlockWriter::ValueCounter { programs });
        }
    }
    Ok(())
}

// Native activity writer RVA 0x548372 applies destination +8 and bubble +8 lists.
pub(in crate::catalog) fn scan_destination_writers(
    manager: &PackageManager,
    root: &[u8],
    flags: &mut [UnlockDefinition],
) -> Result<(), String> {
    let data = manager
        .read_tag(TagHash(u32_at(root, 8 + 25 * 16)?))
        .map_err(|error| format!("Could not read destination writers: {error}"))?;
    let (count, rows) = checked_rows(&data, 8, 0x8080_776C, 56)?;
    for index in 0..count {
        let row = rows + index * 56;
        let owner = context(
            u64::from(u32_at(&data, row)?),
            ProgressionContextKind::Location,
            format!("Destination #{index}"),
        );
        attach_flag_writes(&data, row + 8, flags, &owner)?;
        if u64_at(&data, row + 32)? == 0 {
            continue;
        }
        let (count, rows) = checked_rows(&data, row + 32, 0x8080_7772, 32)?;
        for index in 0..count {
            attach_flag_writes(&data, rows + index * 32 + 8, flags, &owner)?;
        }
    }
    Ok(())
}

pub(in crate::catalog) fn scan_progression_context_outputs(
    manager: &PackageManager,
    root: &[u8],
    values: &mut [UnlockDefinition],
) -> Result<(), String> {
    let data = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + PROGRESSION_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read progression outputs: {error}"))?;
    let (count, rows) = checked_rows(
        &data,
        8,
        PROGRESSION_DEFINITION_ROW_CLASS,
        PROGRESSION_DEFINITION_ROW_SIZE,
    )?;
    for index in 0..count {
        let row = rows + index * PROGRESSION_DEFINITION_ROW_SIZE;
        let owner = context(
            u64::from(u32_at(&data, row)?),
            ProgressionContextKind::Progression,
            String::new(),
        );
        // Native writers 0x540E30 and 0x548A40. Their modifier and fractional-rank
        // inputs are distinct from the integral level output at +8.
        for (offset, field) in [(6, "Rank modifier output"), (12, "Fractional rank output")] {
            if offset == 6 && u64_at(&data, row + 40)? == 0 {
                continue;
            }
            let slot = u16_at(&data, row + offset)?;
            if slot == u16::MAX {
                continue;
            }
            attach_direct_reference(values, slot, field, &owner)?;
            values[usize::from(slot)]
                .runtime_writers
                .push(UnlockWriter::Context {
                    source: field.into(),
                });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_references_skip_native_sentinel_and_reject_missing_definitions() {
        let mut flags = vec![UnlockDefinition::default()];
        let owner = context(1, ProgressionContextKind::Record, "Record".into());
        attach_direct_reference(&mut flags, u16::MAX, "Completion Flag", &owner).unwrap();
        assert!(flags[0].tested_by.is_empty());
        assert!(attach_direct_reference(&mut flags, 1, "Completion Flag", &owner).is_err());
        attach_direct_reference(&mut flags, 0, "Completion Flag", &owner).unwrap();
        attach_direct_reference(&mut flags, 0, "Category Flag", &owner).unwrap();
        attach_direct_reference(&mut flags, 0, "Completion Flag", &owner).unwrap();
        assert_eq!(flags[0].tested_by.len(), 1);
        assert_eq!(flags[0].tested_by[0].direct_references.len(), 2);
    }

    #[test]
    fn direct_array_extent_is_validated_before_iteration() {
        let mut data = vec![0_u8; 36];
        data[..8].copy_from_slice(&1_u64.to_le_bytes());
        data[8..16].copy_from_slice(&8_i64.to_le_bytes());
        data[16..24].copy_from_slice(&1_u64.to_le_bytes());
        data[24..28].copy_from_slice(&0x8080_7D34_u32.to_le_bytes());
        assert_eq!(checked_rows(&data, 0, 0x8080_7D34, 4).unwrap(), (1, 32));
        assert!(checked_rows(&data[..35], 0, 0x8080_7D34, 4).is_err());
        assert!(checked_rows(&data, 0, 0x8080_7D34, usize::MAX).is_err());
    }
}
