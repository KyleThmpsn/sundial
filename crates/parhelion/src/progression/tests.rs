use super::*;
use crate::tag_payload::read_u8;

#[test]
fn rarity_branches_increment_only_their_own_collection_ancestors() {
    fn array(
        data: &mut Vec<u8>,
        descriptor: usize,
        count: usize,
        class: u32,
        stride: usize,
    ) -> usize {
        let header = data.len();
        data.resize(header + 16 + count * stride, 0);
        write_u64(data, descriptor, count as u64).unwrap();
        write_relative_pointer(data, descriptor + 8, header).unwrap();
        write_u64(data, header, count as u64).unwrap();
        write_u32(data, header + 8, class).unwrap();
        header + 16
    }
    let mut nodes = vec![0; 24];
    let node_rows = array(
        &mut nodes,
        8,
        6,
        PRESENTATION_NODE_DEFINITION_ROW_CLASS,
        PRESENTATION_NODE_ROW_SIZE,
    );
    let mut objectives = vec![0; 24];
    let objective_rows = array(
        &mut objectives,
        8,
        6,
        OBJECTIVE_ROW_CLASS,
        OBJECTIVE_ROW_SIZE,
    );
    // ordinary leaf -> Weapons; Exotic leaf -> Exotics; both -> Items.
    // The final node is an unrelated collection and must not change.
    for (node, parent) in [(0, 2), (1, 3), (2, 4), (3, 4)] {
        let rows = array(
            &mut nodes,
            node_rows + node * PRESENTATION_NODE_ROW_SIZE + 0x18,
            1,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            2,
        );
        write_u16(&mut nodes, rows, parent).unwrap();
    }
    for (node, count) in [(0, 12), (1, 21)] {
        array(
            &mut nodes,
            node_rows + node * PRESENTATION_NODE_ROW_SIZE + PRESENTATION_NODE_COLLECTIBLES_OFFSET,
            count,
            PRESENTATION_NODE_COLLECTIBLE_ROW_CLASS,
            PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE,
        );
    }
    for (index, value) in [10, 20, 100, 200, 300, 400].into_iter().enumerate() {
        write_u16(
            &mut nodes,
            node_rows
                + index * PRESENTATION_NODE_ROW_SIZE
                + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
            index as u16,
        )
        .unwrap();
        write_i32(
            &mut objectives,
            objective_rows + index * OBJECTIVE_ROW_SIZE + OBJECTIVE_COMPLETION_VALUE_OFFSET,
            value,
        )
        .unwrap();
    }
    let patched = patch_project_collection_objectives(
        objectives,
        &nodes,
        &BTreeMap::from([(0, 2), (1, 1)]),
        3,
    )
    .unwrap();
    for (index, expected) in [12, 21, 102, 201, 303, 400].into_iter().enumerate() {
        assert_eq!(
            read_i32(
                &patched,
                objective_rows + index * OBJECTIVE_ROW_SIZE + OBJECTIVE_COMPLETION_VALUE_OFFSET
            )
            .unwrap(),
            expected
        );
    }
}

fn synthetic_unlock_display_table(hashes: &[u32]) -> Vec<u8> {
    let primary_header = 0x30;
    let primary_rows = primary_header + 16;
    let primary_end = primary_rows + hashes.len() * UNLOCK_FLAG_DISPLAY_ROW_SIZE;
    let content_header = primary_end + 16;
    let content_rows = content_header + 16;
    let mut data = vec![0; content_rows + UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE];

    write_u64(&mut data, 8, hashes.len() as u64).unwrap();
    write_relative_pointer(&mut data, 0x10, primary_header).unwrap();
    write_u64(&mut data, primary_header, hashes.len() as u64).unwrap();
    write_u32(&mut data, primary_header + 8, UNLOCK_FLAG_DISPLAY_ROW_CLASS).unwrap();
    data[content_header - NESTED_ARRAY_TRAILER.len()..content_header]
        .copy_from_slice(&NESTED_ARRAY_TRAILER);
    write_u64(&mut data, 0x18, 1).unwrap();
    write_relative_pointer(&mut data, 0x20, content_header).unwrap();
    write_u64(&mut data, content_header, 1).unwrap();
    write_u32(
        &mut data,
        content_header + 8,
        UNLOCK_FLAG_DISPLAY_CONTENT_ROW_CLASS,
    )
    .unwrap();
    for (index, hash) in hashes.iter().copied().enumerate() {
        let row = primary_rows + index * UNLOCK_FLAG_DISPLAY_ROW_SIZE;
        write_u32(&mut data, row, hash).unwrap();
        write_relative_pointer(&mut data, row + 8, content_rows).unwrap();
    }
    data
}

fn synthetic_unlock_table(order: &[u16]) -> Vec<u8> {
    let hashes = [30_u32, 10, 20];
    assert_eq!(order.len(), hashes.len());
    let primary_header = 0x28;
    let primary_rows = primary_header + 16;
    let secondary_header = primary_rows + hashes.len() * UNLOCK_ROW_SIZE;
    let secondary_rows = secondary_header + 16;
    let mut data = vec![0; secondary_rows + std::mem::size_of_val(order)];

    write_u64(&mut data, 8, hashes.len() as u64).unwrap();
    write_relative_pointer(&mut data, 0x10, primary_header).unwrap();
    write_u64(&mut data, primary_header, hashes.len() as u64).unwrap();
    write_u32(
        &mut data,
        primary_header + 8,
        UNLOCK_FLAG_DEFINITION_ROW_CLASS,
    )
    .unwrap();
    for (index, hash) in hashes.into_iter().enumerate() {
        let row = primary_rows + index * UNLOCK_ROW_SIZE;
        write_u32(&mut data, row, hash).unwrap();
        write_u16(&mut data, row + 4, 2).unwrap();
        write_u16(&mut data, row + 6, index as u16).unwrap();
    }

    write_u64(&mut data, 0x18, order.len() as u64).unwrap();
    write_relative_pointer(&mut data, 0x20, secondary_header).unwrap();
    write_u64(&mut data, secondary_header, order.len() as u64).unwrap();
    write_u32(
        &mut data,
        secondary_header + 8,
        UNLOCK_FLAG_SORTED_INDEX_ROW_CLASS,
    )
    .unwrap();
    for (position, index) in order.iter().copied().enumerate() {
        write_u16(
            &mut data,
            secondary_rows + position * size_of::<u16>(),
            index,
        )
        .unwrap();
    }
    data
}

#[test]
fn append_unlock_preserves_native_rows_and_rebuilds_sorted_index() {
    let data = synthetic_unlock_table(&[1, 2, 0]);
    let authored = append_unlock(data, 25, 3, 77).unwrap();
    let layout = unlock_table_layout(&authored).unwrap();

    assert_eq!(layout.count, 4);
    assert_eq!(layout.order, vec![1, 2, 3, 0]);
    assert_eq!(layout.ordered_hashes, vec![10, 20, 25, 30]);
    let row = layout.primary_rows + 3 * UNLOCK_ROW_SIZE;
    assert_eq!(read_u32(&authored, row).unwrap(), 25);
    assert_eq!(read_u16(&authored, row + 4).unwrap(), 3);
    assert_eq!(read_u16(&authored, row + 6).unwrap(), 77);
}

#[test]
fn unlock_table_rejects_incomplete_or_unsorted_secondary_indices() {
    let duplicate = synthetic_unlock_table(&[1, 1, 0]);
    assert!(unlock_table_layout(&duplicate).is_err());

    let unsorted = synthetic_unlock_table(&[2, 1, 0]);
    assert!(unlock_table_layout(&unsorted).is_err());
}

#[test]
fn append_unlock_display_preserves_the_shared_content_array() {
    let data = synthetic_unlock_display_table(&[10, 20]);
    let authored = append_unlock_display(data, 0, 30, 2).unwrap();
    let layout = unlock_display_table_layout(&authored).unwrap();

    assert_eq!(layout.count, 3);
    assert_eq!(layout.content_count, 1);
    let authored_row = layout.primary_rows + 2 * UNLOCK_FLAG_DISPLAY_ROW_SIZE;
    assert_eq!(read_u32(&authored, authored_row).unwrap(), 30);
    assert_eq!(
        relative_target(&authored, authored_row + 8).unwrap(),
        layout.content_rows
    );
}

#[test]
fn unlock_display_rejects_misaligned_content_pointers() {
    let mut data = synthetic_unlock_display_table(&[10]);
    let layout = unlock_display_table_layout(&data).unwrap();
    write_relative_pointer(&mut data, layout.primary_rows + 8, layout.content_rows + 1).unwrap();
    assert!(unlock_display_table_layout(&data).is_err());
}

fn synthetic_numeric_instruction(opcode: u8, operand: u16) -> [u8; 8] {
    let mut row = [0_u8; 8];
    row[0] = opcode;
    row[1..4].copy_from_slice(&[0xA1, 0xA2, 0xA3]);
    row[4..6].copy_from_slice(&operand.to_le_bytes());
    row[6..8].copy_from_slice(&[0xB1, 0xB2]);
    row
}

fn synthetic_numeric_pool_rows(programs: &[Vec<(u8, u16)>]) -> Vec<u8> {
    let rows_end = programs.len() * SHARED_EXPRESSION_POOL_ROW_SIZE;
    let mut data = vec![0; (rows_end + 15) & !15];
    for (pool_index, tokens) in programs.iter().enumerate() {
        let mut segment = vec![0; 16];
        write_u64(&mut segment, 0, tokens.len() as u64).expect("count should fit");
        write_u32(&mut segment, 8, NUMERIC_PROGRAM_ROW_CLASS).expect("class should fit");
        for (opcode, operand) in tokens {
            segment.extend_from_slice(&synthetic_numeric_instruction(*opcode, *operand));
        }
        while (segment.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
            segment.push(0);
        }
        segment.extend_from_slice(&NESTED_ARRAY_TRAILER);

        let header = data.len();
        data.extend_from_slice(&segment);
        let descriptor =
            pool_index * SHARED_EXPRESSION_POOL_ROW_SIZE + SHARED_EXPRESSION_DESCRIPTOR_OFFSET;
        write_u64(&mut data, descriptor, tokens.len() as u64).expect("descriptor count should fit");
        write_relative_pointer(&mut data, descriptor + 8, header)
            .expect("descriptor pointer should fit");
    }
    data
}

#[test]
fn numeric_program_padding_survives_eight_byte_relocation() {
    let data = synthetic_numeric_pool_rows(&[vec![(NUMERIC_FLAG_INSTRUCTION, 7304)]]);
    let original = numeric_program_layout(&data, SHARED_EXPRESSION_DESCRIPTOR_OFFSET).unwrap();
    let mut shifted = vec![0; 8];
    shifted.extend_from_slice(&data);
    let relocated =
        numeric_program_layout(&shifted, SHARED_EXPRESSION_DESCRIPTOR_OFFSET + 8).unwrap();
    assert_eq!(relocated.tokens, original.tokens);
    assert_eq!(relocated.segment_end, original.segment_end + 8);
    shifted[relocated.segment_end - 1] ^= 1;
    assert!(numeric_program_layout(&shifted, SHARED_EXPRESSION_DESCRIPTOR_OFFSET + 8).is_err());
}

#[test]
fn objective_roots_select_outer_nested_acquired_count() {
    let pools = synthetic_numeric_pool_rows(&[
        vec![(NUMERIC_FLAG_INSTRUCTION, 3593)],
        vec![
            (NUMERIC_POOL_INSTRUCTION, 0),
            (NUMERIC_FLAG_INSTRUCTION, 3593),
            (NUMERIC_ADD_INSTRUCTION, u16::MAX),
        ],
        vec![(NUMERIC_POOL_INSTRUCTION, 1)],
    ]);
    assert_eq!(
        pools_for_objective_roots(&pools, 0, &BTreeSet::from([2]), 3593).unwrap(),
        BTreeSet::from([1])
    );
}

#[test]
fn objective_roots_reject_independent_unanchored_counts() {
    let pools = synthetic_numeric_pool_rows(&[
        vec![(NUMERIC_FLAG_INSTRUCTION, 3593)],
        vec![(NUMERIC_FLAG_INSTRUCTION, 3593)],
        vec![
            (NUMERIC_POOL_INSTRUCTION, 0),
            (NUMERIC_POOL_INSTRUCTION, 1),
            (NUMERIC_ADD_INSTRUCTION, u16::MAX),
        ],
    ]);
    assert!(pools_for_objective_roots(&pools, 0, &BTreeSet::from([2]), 3593).is_err());
}

fn synthetic_collectible_conditions(secondary_flag: u16, acquired_flag: u16) -> Vec<u8> {
    let mut data = vec![0; COLLECTIBLE_ROW_SIZE];
    for (field, tokens) in [
        (
            0x30,
            vec![(NUMERIC_FLAG_INSTRUCTION, secondary_flag), (2, 0)],
        ),
        (
            COLLECTIBLE_CONDITION_OFFSET,
            vec![(NUMERIC_FLAG_INSTRUCTION, acquired_flag), (22, 0)],
        ),
    ] {
        let mut segment = vec![0; 16];
        write_u64(&mut segment, 0, tokens.len() as u64).unwrap();
        write_u32(&mut segment, 8, NUMERIC_PROGRAM_ROW_CLASS).unwrap();
        for (opcode, operand) in &tokens {
            segment.extend_from_slice(&synthetic_numeric_instruction(*opcode, *operand));
        }
        while (segment.len() + NESTED_ARRAY_TRAILER.len()) % 16 != 0 {
            segment.push(0);
        }
        segment.extend_from_slice(&NESTED_ARRAY_TRAILER);
        while data.len() % 16 != 0 {
            data.push(0);
        }
        let header = data.len();
        data.extend_from_slice(&segment);
        write_u64(&mut data, field, tokens.len() as u64).unwrap();
        write_relative_pointer(&mut data, field + 8, header).unwrap();
    }
    data
}

fn cloned_condition_tokens(clones: &[CollectibleNestedClone], field: usize) -> Vec<(u8, u16)> {
    let nested = clones
        .iter()
        .find(|nested| nested.field == field)
        .expect("requested nested condition should be cloned");
    (0..nested.count)
        .map(|index| {
            let row = 16 + index * NUMERIC_INSTRUCTION_ROW_SIZE;
            (
                read_u8(&nested.bytes, row).unwrap(),
                read_u16(&nested.bytes, row + 4).unwrap(),
            )
        })
        .collect()
}

#[test]
fn collectible_clone_drops_socket_overrides_without_changing_the_donor() {
    let mut data = synthetic_collectible_conditions(7_502, 7_502);
    let header = data.len();
    let mut segment = vec![0; 32];
    write_u64(&mut segment, 0, 1).unwrap();
    write_u32(&mut segment, 8, COLLECTIBLE_SOCKET_OVERRIDE_ROW_CLASS).unwrap();
    // Breachlight's Collections trait override: type 92 -> placeholder plug 2285.
    write_u16(&mut segment, 16, 92).unwrap();
    write_u16(&mut segment, 18, 2285).unwrap();
    write_u16(&mut segment, 20, u16::MAX).unwrap();
    write_u16(&mut segment, 22, u16::MAX).unwrap();
    segment[28..32].copy_from_slice(&NESTED_ARRAY_MARKER);
    data.extend_from_slice(&segment);
    write_u64(&mut data, COLLECTIBLE_SOCKET_OVERRIDES_OFFSET, 1).unwrap();
    write_relative_pointer(&mut data, COLLECTIBLE_SOCKET_OVERRIDES_OFFSET + 8, header).unwrap();
    let before = data.clone();
    let clones =
        collectible_nested_clones(&data, 0, 1, COLLECTIBLE_ROW_SIZE, 0, 7_502, 21_613).unwrap();
    assert!(
        !clones
            .iter()
            .any(|nested| nested.field == COLLECTIBLE_SOCKET_OVERRIDES_OFFSET)
    );
    assert_eq!(
        cloned_condition_tokens(&clones, COLLECTIBLE_CONDITION_OFFSET),
        vec![(NUMERIC_FLAG_INSTRUCTION, 21_613)]
    );
    assert_eq!(data, before);
}

#[test]
fn collectible_clone_retargets_every_matching_donor_acquired_condition() {
    let data = synthetic_collectible_conditions(7_502, 7_502);
    assert_eq!(
        collection_unlock_index(&data, 0).expect("non-bare acquisition should resolve"),
        7_502
    );
    let clones = collectible_nested_clones(&data, 0, 1, COLLECTIBLE_ROW_SIZE, 0, 7_502, 21_613)
        .expect("Mountaintop-style conditions should deep-clone");

    assert_eq!(
        cloned_condition_tokens(&clones, 0x30),
        vec![(NUMERIC_FLAG_INSTRUCTION, 21_613), (2, 0)]
    );
    assert_eq!(
        cloned_condition_tokens(&clones, COLLECTIBLE_CONDITION_OFFSET),
        vec![(NUMERIC_FLAG_INSTRUCTION, 21_613)]
    );
    let acquired = clones
        .iter()
        .find(|clone| clone.field == COLLECTIBLE_CONDITION_OFFSET)
        .unwrap();
    assert_eq!(acquired.count, 1);
    assert_eq!(read_u64(&acquired.bytes, 0).unwrap(), 1);
    assert_eq!(&acquired.bytes[17..20], &[0xA1, 0xA2, 0xA3]);
    assert_eq!(&acquired.bytes[22..24], &[0xB1, 0xB2]);
    let secondary = clones.iter().find(|clone| clone.field == 0x30).unwrap();
    assert_eq!(&secondary.bytes[17..20], &[0xA1, 0xA2, 0xA3]);
    assert_eq!(&secondary.bytes[22..24], &[0xB1, 0xB2]);
    assert_eq!(
        clones
            .iter()
            .map(|nested| nested.retargeted_source_flags)
            .sum::<usize>(),
        2
    );
}

#[test]
fn collectible_clone_preserves_unrelated_unlock_conditions() {
    let data = synthetic_collectible_conditions(10_699, 10_999);
    let clones = collectible_nested_clones(&data, 0, 1, COLLECTIBLE_ROW_SIZE, 0, 10_999, 21_613)
        .expect("Martyr-style conditions should deep-clone selectively");

    assert_eq!(
        cloned_condition_tokens(&clones, 0x30),
        vec![(NUMERIC_FLAG_INSTRUCTION, 10_699), (2, 0)]
    );
    assert_eq!(
        cloned_condition_tokens(&clones, COLLECTIBLE_CONDITION_OFFSET),
        vec![(NUMERIC_FLAG_INSTRUCTION, 21_613)]
    );
}

#[test]
fn generic_acquired_count_discovery_selects_only_direct_additive_flag_paths() {
    let source_flag = 725;
    let programs = vec![
        vec![
            (NUMERIC_FLAG_INSTRUCTION, source_flag),
            (NUMERIC_FLAG_INSTRUCTION, 10),
            (NUMERIC_ADD_INSTRUCTION, u16::MAX),
        ],
        vec![
            (NUMERIC_FLAG_INSTRUCTION, source_flag),
            (NUMERIC_FLAG_INSTRUCTION, 11),
            (13, u16::MAX),
        ],
        vec![(NUMERIC_POOL_INSTRUCTION, source_flag)],
        vec![
            (NUMERIC_FLAG_INSTRUCTION, source_flag),
            (NUMERIC_FLAG_INSTRUCTION, source_flag),
            (NUMERIC_ADD_INSTRUCTION, u16::MAX),
        ],
        vec![
            (NUMERIC_FLAG_INSTRUCTION, source_flag),
            (NUMERIC_FLAG_INSTRUCTION, 12),
            (NUMERIC_ADD_INSTRUCTION, u16::MAX),
            (NUMERIC_FLAG_INSTRUCTION, 13),
            (NUMERIC_ADD_INSTRUCTION, u16::MAX),
        ],
        vec![
            (NUMERIC_FLAG_INSTRUCTION, 14),
            (22, 0),
            (NUMERIC_FLAG_INSTRUCTION, source_flag),
            (NUMERIC_ADD_INSTRUCTION, u16::MAX),
        ],
        vec![(NUMERIC_FLAG_INSTRUCTION, source_flag), (22, 0)],
    ];
    let data = synthetic_numeric_pool_rows(&programs);

    let links = direct_acquired_count_pool_links(&data, 0, programs.len(), source_flag)
        .expect("synthetic pools should scan");

    assert_eq!(
        links
            .iter()
            .map(|link| (link.pool_index, link.source_count))
            .collect::<Vec<_>>(),
        vec![(0, 3), (4, 5), (5, 4)]
    );
    assert!(links.iter().all(|link| link.direct_additive_flag));
}

#[test]
fn five_socket_overrides_use_the_four_byte_array_marker_without_a_zero_word() {
    let count = 5;
    let mut segment = vec![0; 16 + count * COLLECTIBLE_SOCKET_OVERRIDE_ROW_SIZE];
    write_u64(&mut segment, 0, count as u64).unwrap();
    write_u32(&mut segment, 8, COLLECTIBLE_SOCKET_OVERRIDE_ROW_CLASS).unwrap();
    for index in 0..count {
        let row = 16 + index * COLLECTIBLE_SOCKET_OVERRIDE_ROW_SIZE;
        write_u32(&mut segment, row, 0x1000 + index as u32).unwrap();
        write_u32(&mut segment, row + 4, u32::MAX).unwrap();
        write_u32(&mut segment, row + 8, index as u32).unwrap();
    }
    while (segment.len() + NESTED_ARRAY_MARKER.len()) % 16 != 0 {
        segment.push(0);
    }
    segment.extend_from_slice(&NESTED_ARRAY_MARKER);

    assert_eq!(segment.len(), 0x50);
    assert_eq!(&segment[0x4C..], &NESTED_ARRAY_MARKER);
    assert_eq!(
        flat_collectible_nested_segment(
            &segment,
            0,
            count,
            COLLECTIBLE_SOCKET_OVERRIDE_ROW_CLASS,
            COLLECTIBLE_SOCKET_OVERRIDE_ROW_SIZE,
        )
        .unwrap(),
        segment
    );
}
