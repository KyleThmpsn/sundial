use super::*;

mod expansion;

#[test]
fn first_appended_socket_and_condition_have_native_relocation_markers() {
    let mut data = synthetic_socket_definition();
    // Previous payload content is not required to end with an array marker.
    data.extend_from_slice(&[0x12; 16]);
    let columns = resolved_socket_columns(&[Some(vec![20]), Some(vec![21]), None]);
    set_weapon_socket_columns(&mut data, &columns).unwrap();
    let resource = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, resource).unwrap();
    let (_, header, members, _) =
        array_at(&data, rows + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET).unwrap();
    assert_eq!(&data[header - 8..header], &NESTED_ARRAY_TRAILER);
    data[header - 4] = 0;
    assert!(validate_socket_member_segment(&data, members, 1).is_err());

    let mut program = vec![0; 32];
    write_numeric_program(
        &mut program,
        0,
        &[WeaponNumericInstruction {
            opcode: 0x0B,
            operand: 1,
        }],
    )
    .unwrap();
    let (_, header, _, _) = array_at(&program, 0).unwrap();
    assert_eq!(&program[header - 8..header], &NESTED_ARRAY_TRAILER);
}

#[test]
fn inherited_program_only_socket_columns_remain_inherited() {
    let mut data = synthetic_socket_definition();
    let resource = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, resource).unwrap();
    write_u16(
        &mut data,
        rows + ITEM_ORDINARY_SOCKET_ROW_SIZE + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET,
        u16::MAX,
    )
    .unwrap();
    let mut columns = vec![None, None, None];

    normalize_inherited_randomized_socket_columns(&data, &mut columns).unwrap();

    assert_eq!(columns, vec![None, None, None]);

    columns[1] = Some(vec![11]);
    assert!(normalize_inherited_randomized_socket_columns(&data, &mut columns).is_err());
}

#[test]
fn socket_column_validation_rejects_restored_randomized_selection_metadata() {
    let mut data = synthetic_socket_definition();
    let columns = resolved_socket_columns(&[None, Some(vec![20, 21]), None]);
    set_weapon_socket_columns(&mut data, &columns).unwrap();
    let resource = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, resource).unwrap();
    let descriptor = rows
        + ITEM_ORDINARY_SOCKET_ROW_SIZE
        + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET;
    write_i64(&mut data, descriptor + 8, 1).unwrap();

    assert!(validate_weapon_socket_columns(&data, &columns, &[176, 92, u16::MAX]).is_err());
}

#[test]
fn socket_columns_reject_empty_duplicate_and_disabled_choices() {
    for columns in [
        vec![None, Some(Vec::new()), None],
        vec![None, Some(vec![20, 20]), None],
        vec![None, Some(vec![u16::MAX]), None],
        vec![None, None, Some(vec![20])],
    ] {
        assert!(
            set_weapon_socket_columns(
                &mut synthetic_socket_definition(),
                &resolved_socket_columns(&columns),
            )
            .is_err()
        );
    }
}

#[test]
fn removed_socket_rejects_leftover_choices_and_selection_metadata() {
    let mut column = WeaponSocketColumnOverride {
        socket_type: Some(u16::MAX),
        ..Default::default()
    };
    validate_socket_column_shapes(&[Some(column.clone())], &[]).unwrap();
    column.reusable_plug_set_index = Some(3);
    assert!(validate_socket_column_shapes(&[Some(column.clone())], &[]).is_err());
    column.reusable_plug_set_index = None;
    column.choices.push(20);
    assert!(validate_socket_column_shapes(&[Some(column)], &[]).is_err());
}
