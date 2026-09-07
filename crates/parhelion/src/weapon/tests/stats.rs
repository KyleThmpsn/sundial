use super::*;

#[test]
fn custom_plug_stats_materialize_a_null_array_without_touching_sibling_perks() {
    let mut source = synthetic_weapon_with_investment_stats();
    let resource = relative_target(&source, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    source[resource..resource + 16].fill(0);
    // The neighboring perk count must never be mistaken for the empty stat array's class.
    write_u64(
        &mut source,
        resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET,
        1,
    )
    .unwrap();
    let before = source.clone();
    apply_custom_plug_stats(&mut source, &[(13, 10)]).unwrap();
    let (count, _, rows, class) = array_at(&source, resource).unwrap();
    assert_eq!((count, class), (1, ITEM_INVESTMENT_STAT_ROW_CLASS));
    assert_eq!(read_u8(&source, rows).unwrap(), 13);
    assert_eq!(read_i32(&source, rows + 4).unwrap(), 10);
    assert_eq!(
        &source[resource + 16..before.len()],
        &before[resource + 16..]
    );
}

#[test]
fn custom_plug_stats_preserve_source_and_sibling_payloads() {
    let source = synthetic_weapon_with_investment_stats();
    let mut authored = source.clone();
    let resource = relative_target(&source, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    apply_custom_plug_stats(&mut authored, &[(16, -5), (20, 10)]).unwrap();
    let (count, _, rows, _) = array_at(&authored, resource).unwrap();
    assert_eq!(count, 3);
    assert_eq!(
        read_i32(&authored, rows + ITEM_INVESTMENT_STAT_ROW_SIZE + 4).unwrap(),
        -5
    );
    assert_eq!(
        read_i32(&authored, rows + 2 * ITEM_INVESTMENT_STAT_ROW_SIZE + 4).unwrap(),
        10
    );
    let descriptor = resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
    assert_eq!(
        &authored[descriptor..descriptor + 16],
        &source[descriptor..descriptor + 16]
    );
    assert_eq!(source, synthetic_weapon_with_investment_stats());
}

#[test]
fn custom_plug_stats_reject_silently_truncated_and_invalid_edits_atomically() {
    let source = synthetic_weapon_with_investment_stats();
    let mut authored = source.clone();
    let too_many = (0..17).map(|index| (index, 1)).collect::<Vec<_>>();
    assert!(
        apply_custom_plug_stats(&mut authored, &too_many)
            .unwrap_err()
            .to_string()
            .contains("16-contribution")
    );
    assert_eq!(authored, source);
    for invalid in [vec![(256, 10)], vec![(13, 10), (13, 20)]] {
        assert!(apply_custom_plug_stats(&mut authored, &invalid).is_err());
        assert_eq!(authored, source);
    }
    let sixteen = (0..16).map(|index| (index, 1)).collect::<Vec<_>>();
    // The inherited row 16 also counts, even if it is not edited.
    assert!(apply_custom_plug_stats(&mut authored, &sixteen).is_err());
    let fifteen = (0..15).map(|index| (index, 1)).collect::<Vec<_>>();
    apply_custom_plug_stats(&mut authored, &fifteen).unwrap();
    let resource = relative_target(&authored, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    assert_eq!(array_at(&authored, resource).unwrap().0, 16);
    let mut unrecognized = vec![0; 8];
    apply_custom_plug_stats(&mut unrecognized, &[]).unwrap();
    assert_eq!(
        unrecognized,
        vec![0; 8],
        "existing no-edit recipes must not gain new payload requirements"
    );
}

#[test]
fn stat_display_group_authoring_changes_only_the_signed_index_field() {
    const RESOURCE: usize = 0xC0;
    let mut strings = synthetic_item_string_stat_group(0x0102);
    let before = strings.clone();
    set_item_string_stat_group_index(&mut strings, 0x0304).unwrap();
    assert_eq!(item_string_stat_group_index(&strings).unwrap(), 0x0304);
    assert_eq!(
        changed_offsets(&before, &strings),
        [
            RESOURCE + ITEM_STRING_STAT_GROUP_INDEX_OFFSET,
            RESOURCE + ITEM_STRING_STAT_GROUP_INDEX_OFFSET + 1,
        ]
    );

    let mut malformed = synthetic_item_string_stat_group(-1);
    let before = malformed.clone();
    assert!(set_item_string_stat_group_index(&mut malformed, 4).is_err());
    assert_eq!(malformed, before);
}

#[test]
fn materializes_missing_investment_stats_without_shifting_the_donor_payload() {
    let mut definition = synthetic_weapon_with_investment_stats();
    let original = definition.clone();
    let original_len = definition.len();
    let resource = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let (_, _, original_rows, _) = array_at(&definition, resource).unwrap();
    let original_rows_end = original_rows + 2 * ITEM_INVESTMENT_STAT_ROW_SIZE;

    set_weapon_stats(&mut definition, &[(16, 50), (20, 100)], &[])
        .expect("a globally-known missing stat should materialize out of line");

    assert_eq!(
        &definition[original_rows..original_rows_end],
        &original[original_rows..original_rows_end],
        "the donor's original stat segment must remain byte-identical"
    );
    let (count, header, rows, class) = array_at(&definition, resource).unwrap();
    assert_eq!(count, 3);
    assert_eq!(header, original_len);
    assert_eq!(class, ITEM_INVESTMENT_STAT_ROW_CLASS);
    assert_eq!(read_u8(&definition, rows).unwrap(), 13);
    assert_eq!(read_i32(&definition, rows + 4).unwrap(), 0);
    assert_eq!(
        read_u8(&definition, rows + ITEM_INVESTMENT_STAT_ROW_SIZE).unwrap(),
        16
    );
    assert_eq!(
        read_i32(&definition, rows + ITEM_INVESTMENT_STAT_ROW_SIZE + 4).unwrap(),
        50
    );
    assert_eq!(
        definition[rows + ITEM_INVESTMENT_STAT_ROW_SIZE + 8],
        0xA5,
        "unknown bytes on preserved rows must survive materialization"
    );
    let added = rows + 2 * ITEM_INVESTMENT_STAT_ROW_SIZE;
    assert_eq!(read_u8(&definition, added).unwrap(), 20);
    assert_eq!(read_u8(&definition, added + 1).unwrap(), 0);
    assert_eq!(read_i32(&definition, added + 4).unwrap(), 100);
    assert!(
        definition[added + 8..added + ITEM_INVESTMENT_STAT_ROW_SIZE]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(
        &definition[resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET
            ..resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET + 16],
        &[0; 16],
        "the adjacent sandbox-perk descriptor must remain untouched"
    );
}

#[test]
fn investment_stat_authoring_rejects_nonzero_reserved_bytes_and_wide_ids() {
    let mut malformed = synthetic_weapon_with_investment_stats();
    let resource = relative_target(&malformed, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&malformed, resource).unwrap();
    malformed[rows + 1] = 1;
    assert!(set_weapon_stats(&mut malformed, &[(13, 10)], &[]).is_err());

    let mut definition = synthetic_weapon_with_investment_stats();
    assert!(set_weapon_stats(&mut definition, &[(256, 10)], &[]).is_err());
}

#[test]
fn removes_inherited_investment_rows_without_rewriting_retained_row_bytes() {
    let mut definition = synthetic_weapon_with_investment_stats();
    set_weapon_stats(&mut definition, &[], &[13])
        .expect("an inherited investment row should be removable");

    let resource = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let (count, _, rows, class) = array_at(&definition, resource).unwrap();
    assert_eq!(count, 1);
    assert_eq!(class, ITEM_INVESTMENT_STAT_ROW_CLASS);
    assert_eq!(read_u8(&definition, rows).unwrap(), 16);
    assert_eq!(read_i32(&definition, rows + 4).unwrap(), 44);
    assert_eq!(definition[rows + 8], 0xA5);
    assert!(set_weapon_stats(&mut definition, &[], &[13]).is_err());
}

#[test]
fn authors_distinct_native_power_cap_version_rows() {
    const QUALITY: usize = 0x100;
    const HEADER: usize = 0x180;
    let descriptor = QUALITY
        + sundial::package_authoring::investment_schema::ITEM_QUALITY_VERSION_DESCRIPTOR_OFFSET;
    let rows = HEADER + 16;
    let mut definition = vec![0_u8; rows + 2 * ITEM_VERSION_ROW_SIZE];
    write_relative_pointer(
        &mut definition,
        sundial::package_authoring::investment_schema::ITEM_QUALITY_BLOCK_POINTER_OFFSET,
        QUALITY,
    )
    .unwrap();
    write_u64(&mut definition, descriptor, 2).unwrap();
    write_relative_pointer(&mut definition, descriptor + 8, HEADER).unwrap();
    write_u64(&mut definition, HEADER, 2).unwrap();
    write_u32(
        &mut definition,
        HEADER + 8,
        sundial::package_authoring::investment_schema::ITEM_VERSION_ROW_CLASS,
    )
    .unwrap();
    write_u16(&mut definition, rows, 7).unwrap();
    write_u16(&mut definition, rows + ITEM_VERSION_ROW_SIZE, 8).unwrap();

    set_weapon_power_cap_groups(&mut definition, &[9, 11]).unwrap();

    assert_eq!(
        weapon_version_array(&definition).unwrap().unwrap().groups,
        [9, 11]
    );
    set_weapon_power_cap_groups(&mut definition, &[0, 15]).unwrap();
    assert_eq!(
        weapon_version_array(&definition).unwrap().unwrap().groups,
        [0, 15]
    );
    set_weapon_power_cap(&mut definition, 0).unwrap();
    assert_eq!(
        weapon_version_array(&definition).unwrap().unwrap().groups,
        [0, 0]
    );
    assert!(set_weapon_power_cap_groups(&mut definition, &[11]).is_err());
}
