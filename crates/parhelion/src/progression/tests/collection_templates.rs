use super::*;

fn collectible_table(programs: &[&[(u8, u16)]]) -> Vec<u8> {
    let rows = 48;
    let mut data = vec![0; rows + programs.len() * COLLECTIBLE_ROW_SIZE];
    write_u64(&mut data, 8, programs.len() as u64).unwrap();
    write_relative_pointer(&mut data, 16, rows - 16).unwrap();
    write_u64(&mut data, rows - 16, programs.len() as u64).unwrap();
    write_u32(&mut data, rows - 8, COLLECTIBLE_DEFINITION_ROW_CLASS).unwrap();
    for (index, tokens) in programs.iter().enumerate() {
        if tokens.is_empty() {
            continue; // Native empty descriptor has zero count and pointer.
        }
        while data.len() % 16 != 0 {
            data.push(0);
        }
        let header = data.len();
        data.resize(header + 16, 0);
        write_u64(&mut data, header, tokens.len() as u64).unwrap();
        write_u32(&mut data, header + 8, NUMERIC_PROGRAM_ROW_CLASS).unwrap();
        for (opcode, operand) in *tokens {
            data.extend_from_slice(&synthetic_numeric_instruction(*opcode, *operand));
        }
        while (data.len() - header + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
            data.push(0);
        }
        data.extend_from_slice(&NESTED_ARRAY_TRAILER);
        let descriptor = rows + index * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_CONDITION_OFFSET;
        write_u64(&mut data, descriptor, tokens.len() as u64).unwrap();
        write_relative_pointer(&mut data, descriptor + 8, header).unwrap();
    }
    data
}

#[test]
fn alternative_acquisition_uses_placement_template_with_private_unlock() {
    for tokens in [
        vec![],
        vec![(11, 1)], // Khvostov and other always-acquired early weapons.
        vec![(11, 0)],
        vec![(NUMERIC_POOL_INSTRUCTION, 8)],
        vec![
            (NUMERIC_FLAG_INSTRUCTION, 2),
            (NUMERIC_FLAG_INSTRUCTION, 3),
            (3, u16::MAX),
        ],
        vec![
            (NUMERIC_FLAG_INSTRUCTION, 2),
            (NUMERIC_FLAG_INSTRUCTION, 3),
            (NUMERIC_FLAG_INSTRUCTION, 4),
            (3, u16::MAX),
            (3, u16::MAX),
        ],
    ] {
        let data = collectible_table(&[&tokens, &[(NUMERIC_FLAG_INSTRUCTION, 7)]]);
        let before = data.clone();
        let (template, source_unlock) = collectible_clone_template(&data, Some(0), 1, 10).unwrap();
        assert_eq!((template, source_unlock), (1, 7), "{tokens:?}");
        let (count, _, rows, _) = array_at(&data, 8).unwrap();
        let clones = collectible_nested_clones(
            &data,
            rows,
            count,
            rows + count * COLLECTIBLE_ROW_SIZE,
            rows + template * COLLECTIBLE_ROW_SIZE,
            source_unlock,
            10,
        )
        .unwrap();
        assert_eq!(
            cloned_condition_tokens(&clones, COLLECTIBLE_CONDITION_OFFSET),
            [(NUMERIC_FLAG_INSTRUCTION, 10)]
        );
        assert_eq!(data, before, "stock conditions must remain untouched");
    }
}

#[test]
fn one_distinct_acquisition_flag_preserves_gameplay_template() {
    for tokens in [
        vec![(NUMERIC_FLAG_INSTRUCTION, 2)],
        vec![(NUMERIC_FLAG_INSTRUCTION, 2), (22, 0)],
        vec![
            (NUMERIC_FLAG_INSTRUCTION, 2),
            (NUMERIC_FLAG_INSTRUCTION, 2),
            (3, u16::MAX),
        ],
    ] {
        let data = collectible_table(&[&tokens, &[(NUMERIC_FLAG_INSTRUCTION, 7)]]);
        assert_eq!(
            collectible_clone_template(&data, Some(0), 1, 10).unwrap(),
            (0, 2)
        );
    }
}

#[test]
fn collection_template_fallback_rejects_malformed_acquisition() {
    let mut data = collectible_table(&[&[(11, 1)], &[(NUMERIC_FLAG_INSTRUCTION, 7)]]);
    let (_, _, rows, _) = array_at(&data, 8).unwrap();
    let descriptor = rows + COLLECTIBLE_CONDITION_OFFSET;
    let header = relative_target(&data, descriptor + 8).unwrap();
    write_u32(&mut data, header + 8, 0).unwrap();
    assert!(collectible_clone_template(&data, Some(0), 1, 10).is_err());

    let mut data = collectible_table(&[&[], &[(NUMERIC_FLAG_INSTRUCTION, 7)]]);
    write_u64(&mut data, descriptor, 1).unwrap();
    assert!(collectible_clone_template(&data, Some(0), 1, 10).is_err());
}

#[test]
fn collection_template_fallback_requires_valid_rows_and_unlocks() {
    for (gameplay, placement) in [
        (vec![(11, 1)], vec![(11, 1)]),
        (vec![(11, 1)], vec![(NUMERIC_FLAG_INSTRUCTION, 10)]),
        (
            vec![(NUMERIC_FLAG_INSTRUCTION, 10)],
            vec![(NUMERIC_FLAG_INSTRUCTION, 7)],
        ),
        (
            vec![
                (NUMERIC_FLAG_INSTRUCTION, 2),
                (NUMERIC_FLAG_INSTRUCTION, 10),
                (3, u16::MAX),
            ],
            vec![(NUMERIC_FLAG_INSTRUCTION, 7)],
        ),
    ] {
        let data = collectible_table(&[&gameplay, &placement]);
        assert!(collectible_clone_template(&data, Some(0), 1, 10).is_err());
    }
    let data = collectible_table(&[&[(11, 1)], &[(NUMERIC_FLAG_INSTRUCTION, 7)]]);
    assert!(collectible_clone_template(&data, Some(2), 1, 10).is_err());
    assert!(collectible_clone_template(&data, Some(0), 2, 10).is_err());
}

#[test]
fn missing_collectible_uses_a_validated_placement_template() {
    let data = collectible_table(&[&[(NUMERIC_FLAG_INSTRUCTION, 7)]]);
    assert_eq!(
        collectible_clone_template(&data, None, 0, 10).unwrap(),
        (0, 7)
    );
    assert!(collectible_clone_template(&data, None, 1, 10).is_err());
    assert!(collectible_clone_template(&data, None, 0, 7).is_err());
    let data = collectible_table(&[&[(11, 1)]]);
    assert!(collectible_clone_template(&data, None, 0, 10).is_err());
}

#[test]
fn appended_collectible_discards_all_secondary_programs_and_preserves_stock() {
    let programs = synthetic_collectible_conditions(3, 7);
    let mut data = collectible_table(&[&[(NUMERIC_FLAG_INSTRUCTION, 7)]]);
    let (_, _, rows, _) = array_at(&data, 8).unwrap();
    for field in COLLECTIBLE_SECONDARY_CONDITION_OFFSETS {
        let source = relative_target(&programs, 0x38).unwrap();
        let bytes = flat_collectible_nested_segment(
            &programs,
            source,
            2,
            NUMERIC_PROGRAM_ROW_CLASS,
            NUMERIC_INSTRUCTION_ROW_SIZE,
        )
        .unwrap();
        let target = data.len();
        data.extend_from_slice(&bytes);
        write_u64(&mut data, rows + field, 2).unwrap();
        write_relative_pointer(&mut data, rows + field + 8, target).unwrap();
    }
    let parent = data.len();
    data.resize(parent + 32, 0);
    write_u64(&mut data, parent, 1).unwrap();
    write_u32(&mut data, parent + 8, PRESENTATION_NODE_INDEX_ROW_CLASS).unwrap();
    write_u16(&mut data, parent + 16, 12).unwrap();
    data[parent + 28..parent + 32].copy_from_slice(&NESTED_ARRAY_MARKER);
    write_u64(
        &mut data,
        rows + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
        1,
    )
    .unwrap();
    write_relative_pointer(
        &mut data,
        rows + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET + 8,
        parent,
    )
    .unwrap();
    let before = data.clone();
    let authored = append_collectible(
        data,
        0,
        AuthoredCollectibleSpec {
            collectible_hash: 123,
            item_index: 10,
            unlock: CollectibleUnlockClone {
                source_index: 7,
                authored_index: 11,
            },
            material_set_index: COLLECTIBLE_CURATED_WEAPON_MATERIAL_SET,
            presentation_parents: &[12],
            require_donor_parent_subset: true,
        },
    )
    .unwrap();
    let row = rows + COLLECTIBLE_ROW_SIZE;
    for field in COLLECTIBLE_SECONDARY_CONDITION_OFFSETS {
        assert_eq!(&authored[row + field..row + field + 16], &[0; 16]);
        assert_eq!(
            numeric_program_layout(&authored, rows + field)
                .unwrap()
                .tokens,
            numeric_program_layout(&before, rows + field)
                .unwrap()
                .tokens,
        );
    }
    assert_eq!(
        numeric_program_layout(&authored, row + COLLECTIBLE_CONDITION_OFFSET)
            .unwrap()
            .tokens,
        [(NUMERIC_FLAG_INSTRUCTION, 11)]
    );
    // Every existing nested array remains byte-for-byte intact after pointer rebasing.
    assert_eq!(
        &authored[rows + 2 * COLLECTIBLE_ROW_SIZE..before.len() + COLLECTIBLE_ROW_SIZE],
        &before[rows + COLLECTIBLE_ROW_SIZE..]
    );
}

#[test]
fn collectible_validation_rejects_additional_account_dependencies() {
    let flag = NUMERIC_FLAG_INSTRUCTION;
    for tokens in [
        vec![],
        vec![(flag, 7)],
        vec![(flag, 11), (2, 0)],
        vec![(flag, 11), (flag, 3), (3, u16::MAX)],
        vec![(flag, 11), (NUMERIC_POOL_INSTRUCTION, 3), (3, u16::MAX)],
    ] {
        let data = collectible_table(&[&tokens]);
        assert!(
            validate_authored_collectible_nested_isolation(&data, 0, 7, 11).is_err(),
            "{tokens:?}"
        );
    }
    let data = collectible_table(&[&[(flag, 11)]]);
    validate_authored_collectible_nested_isolation(&data, 0, 7, 11).unwrap();
    let (_, _, rows, _) = array_at(&data, 8).unwrap();
    for field in COLLECTIBLE_SECONDARY_CONDITION_OFFSETS {
        let mut modified = data.clone();
        write_u64(&mut modified, rows + field, 1).unwrap();
        assert!(validate_authored_collectible_nested_isolation(&modified, 0, 7, 11).is_err());
    }
}
