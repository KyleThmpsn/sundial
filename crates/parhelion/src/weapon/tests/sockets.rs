use super::*;

mod expansion;

#[test]
fn socket_shape_distinguishes_private_variants_from_their_stock_source() {
    let mut spec = crate::WeaponRecipe::from_json_str(include_str!(
        "../../../recipes/redacted.parhelion.json"
    ))
    .unwrap()
    .to_spec()
    .unwrap();
    let source = spec.overrides.socket_plug_variants[0].source_plug_hash;
    spec.overrides.socket_columns[0].as_mut().unwrap().choices = vec![source, source];
    validate_socket_column_shapes(
        &spec.overrides.socket_columns,
        &spec.overrides.socket_plug_variants,
    )
    .unwrap();
    let mut alternative = spec.overrides.socket_plug_variants[0].clone();
    alternative.choice_index = 1;
    spec.overrides.socket_plug_variants.push(alternative);
    assert!(
        validate_socket_column_shapes(
            &spec.overrides.socket_columns,
            &spec.overrides.socket_plug_variants
        )
        .is_err()
    );
    spec.overrides.socket_plug_variants[1].name = Some("Different Frame".into());
    validate_socket_column_shapes(
        &spec.overrides.socket_columns,
        &spec.overrides.socket_plug_variants,
    )
    .unwrap();
    spec.overrides.socket_plug_variants.clear();
    assert!(validate_socket_column_shapes(&spec.overrides.socket_columns, &[]).is_err());
}

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
fn private_intrinsic_replaces_default_and_embedded_choice_without_changing_socket_type() {
    let mut data = synthetic_socket_definition();
    let mut columns = resolved_socket_columns(&[Some(vec![20, 10, 21]), None, None]);
    set_weapon_socket_columns(&mut data, &columns).unwrap();
    let before = data.clone();

    replace_socket_choice_item_index(&mut data, 0, 0, 20, 100).unwrap();
    columns[0].as_mut().unwrap().choices[0] = 100;
    validate_weapon_socket_columns(&data, &columns, &[176, 92, u16::MAX]).unwrap();

    // Restore only the chosen plug index. Every other byte, including the
    // intrinsic socket type and both remaining choices, must be unchanged.
    replace_socket_choice_item_index(&mut data, 0, 0, 100, 20).unwrap();
    assert_eq!(data, before);
}

#[test]
fn socket_columns_serialize_variable_length_ordered_members_without_touching_other_rows() {
    for choices in [
        vec![20],
        vec![20, 21, 22],
        vec![20, 21, 22, 23, 24, 25, 26, 27],
    ] {
        assert_socket_column_serialization(choices);
    }
}

#[test]
fn inherited_randomized_socket_columns_are_normalized_to_curated_defaults() {
    let data = synthetic_socket_definition();
    let mut columns = vec![None, None, None];

    normalize_inherited_randomized_socket_columns(&data, &mut columns).unwrap();

    assert_eq!(columns, vec![None, Some(vec![11]), None]);
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
fn direct_socket_authoring_preserves_an_inherited_randomized_lane() {
    let mut data = synthetic_socket_definition();
    let resource = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, resource).unwrap();
    let inherited_row = rows + ITEM_ORDINARY_SOCKET_ROW_SIZE;
    let before = data[inherited_row..inherited_row + ITEM_ORDINARY_SOCKET_ROW_SIZE].to_vec();

    set_weapon_socket_columns(
        &mut data,
        &resolved_socket_columns(&[Some(vec![10]), None, None]),
    )
    .unwrap();

    assert_eq!(
        &data[inherited_row..inherited_row + ITEM_ORDINARY_SOCKET_ROW_SIZE],
        before.as_slice()
    );
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
fn active_socket_types_are_not_artificially_limited_to_one_embedded_choice() {
    let mut data = synthetic_socket_definition();
    let columns =
        resolved_socket_columns(&[Some(vec![20, 21, 22, 23, 24, 25, 26, 27]), None, None]);

    set_weapon_socket_columns(&mut data, &columns).unwrap();

    validate_weapon_socket_columns(&data, &columns, &[176, 92, u16::MAX]).unwrap();
}

#[test]
fn all_inherited_socket_columns_leave_the_payload_byte_identical() {
    let mut data = synthetic_socket_definition();
    let before = data.clone();
    set_weapon_socket_columns(&mut data, &resolved_socket_columns(&[None, None, None])).unwrap();
    assert_eq!(data, before);
}

#[test]
fn socket_columns_replace_inherited_randomized_selection_programs() {
    let mut data = synthetic_socket_definition();
    let resource = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, resource).unwrap();
    let descriptor = rows
        + ITEM_ORDINARY_SOCKET_ROW_SIZE
        + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET;
    assert!(!read_numeric_program(&data, descriptor).unwrap().is_empty());

    set_weapon_socket_columns(
        &mut data,
        &resolved_socket_columns(&[None, Some(vec![20, 21]), None]),
    )
    .unwrap();

    assert!(read_numeric_program(&data, descriptor).unwrap().is_empty());
}

#[test]
fn removing_socket_clears_native_choices_and_preserves_neighbor_rows() {
    let mut data = synthetic_socket_definition();
    let resource = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, resource).unwrap();
    let before = data.clone();
    let columns = vec![
        None,
        Some(ResolvedSocketColumn {
            socket_type: Some(u16::MAX),
            ..Default::default()
        }),
        None,
    ];
    set_weapon_socket_columns(&mut data, &columns).unwrap();
    validate_weapon_socket_columns(&data, &columns, &[176, u16::MAX, u16::MAX]).unwrap();
    let (_, _, after_rows, _) = array_at(&data, resource).unwrap();
    assert_eq!(rows, after_rows);
    for lane in [0, 2] {
        let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        assert_eq!(
            &data[row..row + ITEM_ORDINARY_SOCKET_ROW_SIZE],
            &before[row..row + ITEM_ORDINARY_SOCKET_ROW_SIZE]
        );
    }
    let removed = rows + ITEM_ORDINARY_SOCKET_ROW_SIZE;
    assert_eq!(
        read_u16(&data, removed + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET).unwrap(),
        u16::MAX
    );
    assert_eq!(
        read_u16(
            &data,
            removed + ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET
        )
        .unwrap(),
        u16::MAX
    );
    assert_eq!(
        read_u16(
            &data,
            removed + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET
        )
        .unwrap(),
        u16::MAX
    );
    assert_eq!(
        array_at(&data, removed + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET)
            .unwrap()
            .0,
        0
    );
    assert!(
        read_numeric_program(
            &data,
            removed + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET
        )
        .unwrap()
        .is_empty()
    );
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
