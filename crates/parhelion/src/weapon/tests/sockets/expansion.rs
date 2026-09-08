use super::*;

fn eight_socket_definition() -> Vec<u8> {
    let descriptor = 0x100;
    let header = 0x120;
    let rows = header + 16;
    let mut data = vec![0; rows + 8 * ITEM_ORDINARY_SOCKET_ROW_SIZE + 16];
    write_relative_pointer(&mut data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET, descriptor).unwrap();
    write_u64(&mut data, descriptor, 8).unwrap();
    write_relative_pointer(&mut data, descriptor + 8, header).unwrap();
    write_u64(&mut data, header, 8).unwrap();
    write_u32(&mut data, header + 8, ITEM_ORDINARY_SOCKET_ROW_CLASS).unwrap();
    data[header - 8..header].copy_from_slice(&NESTED_ARRAY_TRAILER);
    let end = data.len();
    data[end - 8..end].copy_from_slice(&NESTED_ARRAY_TRAILER);
    for lane in 0..8 {
        let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        write_u16(&mut data, row, 92).unwrap();
        write_u16(
            &mut data,
            row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
            10 + lane as u16,
        )
        .unwrap();
        write_u16(
            &mut data,
            row + ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET,
            u16::MAX,
        )
        .unwrap();
        write_u16(
            &mut data,
            row + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET,
            u16::MAX,
        )
        .unwrap();
    }
    data
}

fn added_columns(count: usize) -> Vec<Option<ResolvedSocketColumn>> {
    (0..count)
        .map(|lane| {
            (lane >= 8).then(|| ResolvedSocketColumn {
                choices: vec![20 + lane as u16, 40 + lane as u16],
                socket_type: Some(92),
                ..Default::default()
            })
        })
        .collect()
}

fn verify_added_socket_metadata(
    data: &[u8],
    rows: usize,
    defaults: &[u16],
    columns: &[Option<ResolvedSocketColumn>],
) {
    for lane in 8..columns.len() {
        assert_eq!(defaults[lane], columns[lane].as_ref().unwrap().choices[0]);
        let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        assert_eq!(read_u16(data, row + 0x0C).unwrap(), u16::MAX);
        assert_eq!(read_u16(data, row + 0x20).unwrap(), u16::MAX);
        assert_eq!(read_u32(data, row + 4).unwrap(), u32::MAX);
        assert_eq!(read_u64(data, row + 0x10).unwrap(), 0);
        assert_eq!(read_i64(data, row + 0x18).unwrap(), 0);
        assert_eq!(read_u64(data, row + 0x28).unwrap(), 0);
        assert_eq!(read_i64(data, row + 0x30).unwrap(), 0);
    }
}

#[test]
fn added_sockets_grow_eight_native_rows_through_the_twelve_lane_limit() {
    for count in 9..=sundial::investment::MAX_WEAPON_SOCKETS {
        let mut data = eight_socket_definition();
        let before = data.clone();
        let columns = added_columns(count);
        let descriptor = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
        let (_, _, original_rows, _) = array_at(&data, descriptor).unwrap();

        set_weapon_socket_columns(&mut data, &columns).unwrap();

        let (actual_count, header, rows, class) = array_at(&data, descriptor).unwrap();
        assert_eq!(actual_count, count);
        assert_eq!(class, ITEM_ORDINARY_SOCKET_ROW_CLASS);
        assert_eq!(&data[header - 8..header], &NESTED_ARRAY_TRAILER);
        assert_eq!(read_u64(&data, descriptor).unwrap(), count as u64);
        assert_eq!(read_u64(&data, header).unwrap(), count as u64);
        assert_ne!(rows, original_rows);
        assert_eq!(&data[..descriptor], &before[..descriptor]);
        assert_eq!(
            &data[descriptor + 16..before.len()],
            &before[descriptor + 16..]
        );
        assert_eq!(
            &data[rows..rows + 8 * ITEM_ORDINARY_SOCKET_ROW_SIZE],
            &before[original_rows..original_rows + 8 * ITEM_ORDINARY_SOCKET_ROW_SIZE]
        );
        let defaults = weapon_default_plug_indices(&data).unwrap();
        assert_eq!(defaults.len(), count);
        assert_eq!(&defaults[..8], &[10, 11, 12, 13, 14, 15, 16, 17]);
        verify_added_socket_metadata(&data, rows, &defaults, &columns);
        validate_weapon_socket_columns(&data, &columns, &vec![92; count]).unwrap();
    }
}

#[test]
fn added_sockets_rebase_inherited_lists_without_moving_nested_conditions_or_programs() {
    let mut data = eight_socket_definition();
    let mut native_columns = vec![None; 8];
    native_columns[1] = Some(ResolvedSocketColumn {
        choices: vec![11, 12],
        choice_weight_bits: vec![0.25_f32.to_bits(), 0.75_f32.to_bits()],
        choice_conditions: vec![
            vec![WeaponNumericInstruction {
                opcode: 0x0B,
                operand: 1,
            }],
            vec![],
        ],
        randomized_plug_set_index: Some(5),
        randomized_selection_program: vec![WeaponNumericInstruction {
            opcode: 0x0B,
            operand: 2,
        }],
        ..Default::default()
    });
    set_weapon_socket_columns(&mut data, &native_columns).unwrap();
    let descriptor = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, original_rows, _) = array_at(&data, descriptor).unwrap();
    let original_row = original_rows + ITEM_ORDINARY_SOCKET_ROW_SIZE;
    // Native masterwork rows also carry an auxiliary array at 0x28. Its opaque
    // payload can have further internal pointers, which stay at their original addresses.
    let auxiliary_header = data.len();
    data.resize(auxiliary_header + 32, 0);
    write_u64(&mut data, auxiliary_header, 1).unwrap();
    write_u32(&mut data, auxiliary_header + 8, 0x8080_3149).unwrap();
    write_u64(&mut data, auxiliary_header + 16, 0x0123_4567_89AB_CDEF).unwrap();
    write_u64(&mut data, original_row + 0x28, 1).unwrap();
    write_relative_pointer(&mut data, original_row + 0x30, auxiliary_header).unwrap();
    let before = data.clone();
    let mut columns = added_columns(9);
    let added = columns[8].as_mut().unwrap();
    added.choice_weight_bits = vec![0.5_f32.to_bits(), 1.0_f32.to_bits()];
    added.choice_conditions = vec![
        vec![],
        vec![WeaponNumericInstruction {
            opcode: 0x0B,
            operand: 1,
        }],
    ];

    set_weapon_socket_columns(&mut data, &columns).unwrap();

    let (_, _, rows, _) = array_at(&data, descriptor).unwrap();
    let row = rows + ITEM_ORDINARY_SOCKET_ROW_SIZE;
    let mut relocated_row = data[row..row + ITEM_ORDINARY_SOCKET_ROW_SIZE].to_vec();
    for offset in [
        ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET,
        0x28,
        ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET,
    ] {
        let before_target = array_at(&before, original_row + offset).unwrap();
        assert_eq!(array_at(&data, row + offset).unwrap(), before_target);
        assert!(read_i64(&data, row + offset + 8).unwrap() < 0);
        relocated_row[offset + 8..offset + 16]
            .copy_from_slice(&before[original_row + offset + 8..original_row + offset + 16]);
    }
    assert_eq!(
        relocated_row,
        before[original_row..original_row + ITEM_ORDINARY_SOCKET_ROW_SIZE]
    );
    let (_, _, member_rows, _) =
        array_at(&data, row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET).unwrap();
    assert_eq!(
        read_numeric_program(&data, member_rows + 8).unwrap(),
        vec![WeaponNumericInstruction {
            opcode: 0x0B,
            operand: 1
        }]
    );
    assert_eq!(
        &data[descriptor + 16..before.len()],
        &before[descriptor + 16..]
    );
    validate_weapon_socket_columns(&data, &columns, &[92; 9]).unwrap();
}

#[test]
fn added_socket_choices_support_private_default_and_alternative_plugs() {
    let mut data = eight_socket_definition();
    let mut columns = added_columns(9);
    set_weapon_socket_columns(&mut data, &columns).unwrap();

    replace_socket_choice_item_index(&mut data, 8, 0, 28, 100).unwrap();
    replace_socket_choice_item_index(&mut data, 8, 1, 48, 101).unwrap();

    columns[8].as_mut().unwrap().choices = vec![100, 101];
    validate_weapon_socket_columns(&data, &columns, &[92; 9]).unwrap();
    assert_eq!(weapon_default_plug_indices(&data).unwrap()[8], 100);
}

#[test]
fn added_socket_shapes_reject_shrinking_overflow_and_incomplete_rows_without_changes() {
    let mut invalid_columns = vec![vec![None; 7], added_columns(13)];
    for invalid in [
        None,
        Some(ResolvedSocketColumn {
            choices: vec![28],
            ..Default::default()
        }),
        Some(ResolvedSocketColumn {
            choices: vec![28],
            socket_type: Some(u16::MAX),
            ..Default::default()
        }),
        Some(ResolvedSocketColumn {
            socket_type: Some(92),
            ..Default::default()
        }),
    ] {
        let mut columns = added_columns(9);
        columns[8] = invalid;
        invalid_columns.push(columns);
    }
    for columns in invalid_columns {
        let mut data = eight_socket_definition();
        let before = data.clone();
        assert!(set_weapon_socket_columns(&mut data, &columns).is_err());
        assert_eq!(data, before);
    }
}

#[test]
fn added_socket_resolution_keeps_the_native_prefix_and_requires_typed_new_rows() {
    let data = eight_socket_definition();
    let indices = BTreeMap::from([(100, vec![28]), (101, vec![48])]);
    let mut columns = vec![None; 9];
    columns[8] = Some(WeaponSocketColumnOverride {
        choices: vec![100, 101],
        socket_type: Some(92),
        ..Default::default()
    });
    let resolved = resolve_socket_column_indices(&indices, &data, &columns).unwrap();
    assert_eq!(resolved, added_columns(9));
    assert!(resolve_socket_column_indices(&indices, &data, &columns[..7]).is_err());
    columns.resize(13, columns[8].clone());
    assert!(resolve_socket_column_indices(&indices, &data, &columns).is_err());
    columns.truncate(9);
    columns[8].as_mut().unwrap().socket_type = None;
    assert!(resolve_socket_column_indices(&indices, &data, &columns).is_err());
    columns[8] = None;
    assert!(resolve_socket_column_indices(&indices, &data, &columns).is_err());
}

#[test]
fn added_socket_resolution_normalizes_only_existing_randomized_rows() {
    let data = synthetic_socket_definition();
    let mut columns = vec![None, None, None, Some(vec![28, 48])];
    normalize_inherited_randomized_socket_columns(&data, &mut columns).unwrap();
    assert_eq!(
        columns,
        vec![None, Some(vec![11]), None, Some(vec![28, 48])]
    );
}

#[test]
fn added_sockets_preserve_empty_nested_arrays_with_native_headers() {
    let mut data = eight_socket_definition();
    let descriptor = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, descriptor).unwrap();
    let nested_descriptor = rows + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET;
    let header = data.len();
    data.resize(header + 16, 0);
    write_u32(
        &mut data,
        header + 8,
        ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS,
    )
    .unwrap();
    write_relative_pointer(&mut data, nested_descriptor + 8, header).unwrap();
    let original = array_at(&data, nested_descriptor).unwrap();

    set_weapon_socket_columns(&mut data, &added_columns(9)).unwrap();

    let (_, _, rows, _) = array_at(&data, descriptor).unwrap();
    assert_eq!(
        array_at(&data, rows + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET).unwrap(),
        original
    );
}

#[test]
fn added_sockets_reject_truncated_rows_and_broken_nested_pointers_before_relocation() {
    let data = eight_socket_definition();
    let descriptor = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, descriptor).unwrap();
    let mut truncated = data.clone();
    truncated.truncate(rows + 8 * ITEM_ORDINARY_SOCKET_ROW_SIZE - 1);
    let mut broken = data;
    write_u64(
        &mut broken,
        rows + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET,
        1,
    )
    .unwrap();
    for mut data in [truncated, broken] {
        let before = data.clone();
        assert!(set_weapon_socket_columns(&mut data, &added_columns(9)).is_err());
        assert_eq!(data, before);
    }
}
