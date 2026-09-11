use super::*;

pub(super) fn bundled_every_end_spec() -> WeaponCloneSpec {
    crate::WeaponRecipe::every_end()
        .to_spec()
        .expect("the bundled Every End recipe should compile")
}

pub(super) fn legacy_second_sun_gl_spec() -> WeaponCloneSpec {
    // Preserve the old cross-slot GL fixture independently of the evolving shipped recipe.
    crate::WeaponRecipe::from_json_str(include_str!("legacy-second-sun.json"))
        .expect("the bundled Second Sun recipe should parse")
        .to_spec()
        .expect("the bundled Second Sun recipe should compile")
}

pub(super) fn synthetic_weapon_identity_fields(
    rarity: AuthoredWeaponRarity,
    pattern_index: u16,
) -> Vec<u8> {
    const TRANSLATION_ROOT: usize = 0xC0;
    const ART_HEADER: usize = 0x120;
    const ART_ROWS: usize = ART_HEADER + 16;

    let mut data = vec![0_u8; 0x140];
    write_relative_pointer(
        &mut data,
        ITEM_TRANSLATION_BLOCK_POINTER_OFFSET,
        TRANSLATION_ROOT,
    )
    .unwrap();
    write_u32(
        &mut data,
        TRANSLATION_ROOT - size_of::<u32>(),
        ITEM_TRANSLATION_BLOCK_CLASS,
    )
    .unwrap();
    write_u64(&mut data, TRANSLATION_ROOT, 1).unwrap();
    write_relative_pointer(&mut data, TRANSLATION_ROOT + 8, ART_HEADER).unwrap();
    write_u64(&mut data, ART_HEADER, 1).unwrap();
    write_u32(&mut data, ART_HEADER - 4, 0x8080_9FBD).unwrap();
    write_u32(&mut data, ART_HEADER + 8, TRANSLATION_ART_ROW_CLASS).unwrap();
    write_u16(&mut data, ART_ROWS + TRANSLATION_ART_VARIANT_OFFSET, 0x1234).unwrap();
    write_u16(
        &mut data,
        TRANSLATION_ROOT + TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
        pattern_index,
    )
    .unwrap();
    write_bytes(&mut data, ITEM_RARITY_OFFSET, &[rarity.package_value()]).unwrap();
    data
}

pub(super) fn synthetic_item_string_stat_group(index: i32) -> Vec<u8> {
    const RESOURCE: usize = 0xC0;
    let mut data = vec![0_u8; 0xE0];
    write_relative_pointer(&mut data, ITEM_STRING_STAT_GROUP_POINTER_OFFSET, RESOURCE).unwrap();
    write_u32(
        &mut data,
        RESOURCE - size_of::<u32>(),
        ITEM_STRING_STAT_GROUP_RESOURCE_CLASS,
    )
    .unwrap();
    write_i32(
        &mut data,
        RESOURCE + ITEM_STRING_STAT_GROUP_INDEX_OFFSET,
        index,
    )
    .unwrap();
    data
}

pub(super) fn synthetic_item_string_ammo_type(value: u16) -> Vec<u8> {
    let mut data = vec![0_u8; ITEM_STRING_AMMO_TYPE_OFFSET + size_of::<u16>()];
    write_u32(
        &mut data,
        ITEM_STRING_AMMO_CLASS_OFFSET,
        ITEM_STRING_AMMO_CLASS,
    )
    .unwrap();
    write_u16(&mut data, ITEM_STRING_AMMO_TYPE_OFFSET, value).unwrap();
    data
}

pub(super) fn changed_offsets(before: &[u8], after: &[u8]) -> Vec<usize> {
    before
        .iter()
        .zip(after)
        .enumerate()
        .filter_map(|(offset, (left, right))| (left != right).then_some(offset))
        .collect()
}

pub(super) fn synthetic_socket_definition() -> Vec<u8> {
    const SOCKET_DESCRIPTOR: usize = 0x100;
    const SOCKET_HEADER: usize = 0x120;
    const SOCKET_ROWS: usize = SOCKET_HEADER + 16;
    const RANDOMIZED_SELECTION_HEADER: usize = 0x220;
    const RANDOMIZED_SELECTION_ROWS: usize = RANDOMIZED_SELECTION_HEADER + 16;
    const MEMBER_HEADER: usize = 0x240;
    const MEMBER_ROWS: usize = MEMBER_HEADER + 16;

    let mut data = vec![0_u8; 0x280];
    write_relative_pointer(
        &mut data,
        ITEM_ORDINARY_SOCKET_POINTER_OFFSET,
        SOCKET_DESCRIPTOR,
    )
    .unwrap();
    write_u64(&mut data, SOCKET_DESCRIPTOR, 3).unwrap();
    write_relative_pointer(&mut data, SOCKET_DESCRIPTOR + 8, SOCKET_HEADER).unwrap();
    write_u64(&mut data, SOCKET_HEADER, 3).unwrap();
    write_u32(&mut data, SOCKET_HEADER + 8, ITEM_ORDINARY_SOCKET_ROW_CLASS).unwrap();

    for (lane, (socket_type, default)) in [(176, 10), (92, 11), (u16::MAX, u16::MAX)]
        .into_iter()
        .enumerate()
    {
        let row = SOCKET_ROWS + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        write_u16(&mut data, row, socket_type).unwrap();
        write_u16(
            &mut data,
            row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
            default,
        )
        .unwrap();
        write_u16(
            &mut data,
            row + ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET,
            if socket_type == u16::MAX { u16::MAX } else { 4 },
        )
        .unwrap();
        write_u16(
            &mut data,
            row + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET,
            if lane == 1 { 5 } else { u16::MAX },
        )
        .unwrap();
    }

    let randomized_selection = SOCKET_ROWS
        + ITEM_ORDINARY_SOCKET_ROW_SIZE
        + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET;
    write_u64(&mut data, randomized_selection, 1).unwrap();
    write_relative_pointer(
        &mut data,
        randomized_selection + 8,
        RANDOMIZED_SELECTION_HEADER,
    )
    .unwrap();
    write_u64(&mut data, RANDOMIZED_SELECTION_HEADER, 1).unwrap();
    write_u32(
        &mut data,
        RANDOMIZED_SELECTION_HEADER + 8,
        NUMERIC_PROGRAM_ROW_CLASS,
    )
    .unwrap();
    data[RANDOMIZED_SELECTION_ROWS] = 0x0B;
    write_u16(&mut data, RANDOMIZED_SELECTION_ROWS + 4, 2).unwrap();
    data[0x238..0x240].copy_from_slice(&NESTED_ARRAY_TRAILER);

    let source_descriptor =
        SOCKET_ROWS + ITEM_ORDINARY_SOCKET_ROW_SIZE + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET;
    write_u64(&mut data, source_descriptor, 1).unwrap();
    write_relative_pointer(&mut data, source_descriptor + 8, MEMBER_HEADER).unwrap();
    write_u64(&mut data, MEMBER_HEADER, 1).unwrap();
    write_u32(
        &mut data,
        MEMBER_HEADER + 8,
        ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS,
    )
    .unwrap();
    write_u16(&mut data, MEMBER_ROWS, 11).unwrap();
    write_u32(
        &mut data,
        MEMBER_ROWS + ITEM_ORDINARY_SOCKET_PLUG_MEMBER_WEIGHT_OFFSET,
        1.0_f32.to_bits(),
    )
    .unwrap();
    data[0x278..0x280].copy_from_slice(&NESTED_ARRAY_TRAILER);
    data
}

pub(super) fn assert_serialized_socket_members(data: &[u8], rows: usize, choices: &[u16]) {
    for (index, expected) in choices.iter().copied().enumerate() {
        let member = rows + index * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE;
        assert_eq!(read_u16(data, member).unwrap(), expected);
        assert!(data[member + 2..member + 8].iter().all(|byte| *byte == 0));
        assert_eq!(read_u64(data, member + 8).unwrap(), 0);
        assert_eq!(read_i64(data, member + 16).unwrap(), 0);
        assert_eq!(
            read_u32(
                data,
                member + ITEM_ORDINARY_SOCKET_PLUG_MEMBER_WEIGHT_OFFSET
            )
            .unwrap(),
            1.0_f32.to_bits()
        );
        assert!(data[member + 28..member + 32].iter().all(|byte| *byte == 0));
    }
}

pub(super) fn resolved_socket_columns(
    columns: &[Option<Vec<u16>>],
) -> Vec<Option<ResolvedSocketColumn>> {
    columns
        .iter()
        .map(|column| {
            column.as_ref().map(|choices| ResolvedSocketColumn {
                choices: choices.clone(),
                ..ResolvedSocketColumn::default()
            })
        })
        .collect()
}

pub(super) fn assert_socket_column_serialization(choices: Vec<u16>) {
    let mut data = synthetic_socket_definition();
    let resource = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, resource).unwrap();
    let first_before = data[rows..rows + ITEM_ORDINARY_SOCKET_ROW_SIZE].to_vec();
    let disabled = rows + 2 * ITEM_ORDINARY_SOCKET_ROW_SIZE;
    let disabled_before = data[disabled..disabled + ITEM_ORDINARY_SOCKET_ROW_SIZE].to_vec();
    let columns = resolved_socket_columns(&[None, Some(choices.clone()), None]);

    set_weapon_socket_columns(&mut data, &columns).unwrap();

    assert_eq!(
        &data[rows..rows + ITEM_ORDINARY_SOCKET_ROW_SIZE],
        first_before.as_slice()
    );
    assert_eq!(
        &data[disabled..disabled + ITEM_ORDINARY_SOCKET_ROW_SIZE],
        disabled_before.as_slice()
    );
    let row = rows + ITEM_ORDINARY_SOCKET_ROW_SIZE;
    assert_eq!(
        read_u16(&data, row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET).unwrap(),
        choices[0]
    );
    assert_eq!(
        read_u16(&data, row + ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET).unwrap(),
        u16::MAX
    );
    assert_eq!(
        read_u16(&data, row + ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET).unwrap(),
        u16::MAX
    );
    let randomized_selection = row + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET;
    assert_eq!(read_u64(&data, randomized_selection).unwrap(), 0);
    assert_eq!(read_i64(&data, randomized_selection + 8).unwrap(), 0);
    let descriptor = row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET;
    let (count, _, member_rows, class) = array_at(&data, descriptor).unwrap();
    assert_eq!(count, choices.len());
    assert_eq!(class, ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS);
    assert_serialized_socket_members(&data, member_rows, &choices);
    validate_socket_member_segment(&data, member_rows, count).unwrap();
    validate_weapon_socket_columns(&data, &columns, &[176, 92, u16::MAX]).unwrap();
}

pub(super) fn item_string_classification_fixture(
    slot: WeaponInventorySlot,
    type_key: u32,
) -> Vec<u8> {
    let mut payload = vec![0xA5; ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + 0x20];
    write_u32(
        &mut payload,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET,
        slot.bucket_hash(),
    )
    .unwrap();
    write_u32(
        &mut payload,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + 4,
        type_key,
    )
    .unwrap();
    write_u32(
        &mut payload,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + 8,
        type_key,
    )
    .unwrap();
    payload
}

pub(super) fn project_weapon(namespace: &str, donor_item_hash: u32) -> WeaponCloneSpec {
    WeaponCloneSpec {
        namespace: namespace.to_owned(),
        donor_item_hash,
        expected_donor_name: None,
        presentation_donor: None,
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("test namespace should allocate"),
        text: WeaponCloneText {
            name: namespace.to_owned(),
            flavor: "Project test flavor".to_owned(),
            source: "Source: project test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides::default(),
    }
}

pub(super) fn synthetic_weapon_definition(
    slot: WeaponInventorySlot,
    damage: WeaponDamageDescriptor,
) -> Vec<u8> {
    let mut data = vec![0_u8; 0x180];
    let serialized_size = data.len() as u64;
    write_u64(&mut data, 0, serialized_size).unwrap();
    data[ITEM_INVENTORY_SLOT_OFFSET] = slot.root_value();

    let equipment = 0xD0;
    write_relative_pointer(&mut data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET, equipment).unwrap();
    write_u32(&mut data, equipment - 4, ITEM_EQUIPMENT_BLOCK_CLASS).unwrap();
    write_u16(
        &mut data,
        equipment + ITEM_EQUIPMENT_SLOT_OFFSET,
        slot.equipment_value(),
    )
    .unwrap();
    write_u16(
        &mut data,
        equipment + ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET,
        u16::MAX,
    )
    .unwrap();

    let ordinary_sockets = 0x80;
    let ordinary_socket_header = 0xA0;
    write_relative_pointer(
        &mut data,
        ITEM_ORDINARY_SOCKET_POINTER_OFFSET,
        ordinary_sockets,
    )
    .unwrap();
    write_u64(&mut data, ordinary_sockets, 0).unwrap();
    write_relative_pointer(&mut data, ordinary_sockets + 8, ordinary_socket_header).unwrap();
    write_u64(&mut data, ordinary_socket_header, 0).unwrap();
    write_u32(
        &mut data,
        ordinary_socket_header + 8,
        ITEM_ORDINARY_SOCKET_ROW_CLASS,
    )
    .unwrap();

    let investment = 0x100;
    write_relative_pointer(&mut data, ITEM_INVESTMENT_STAT_POINTER_OFFSET, investment).unwrap();
    write_u32(
        &mut data,
        investment - 4,
        ITEM_INVESTMENT_STAT_RESOURCE_CLASS,
    )
    .unwrap();
    if let WeaponDamageDescriptor::Elemental(damage_type) = damage {
        let descriptor = investment + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
        let header = 0x140;
        write_u64(&mut data, descriptor, 1).unwrap();
        write_relative_pointer(&mut data, descriptor + 8, header).unwrap();
        write_u64(&mut data, header, 1).unwrap();
        write_u32(&mut data, header + 8, ITEM_SANDBOX_PERK_ROW_CLASS).unwrap();
        data[header + 16..header + 16 + ITEM_SANDBOX_PERK_ROW_SIZE]
            .copy_from_slice(&synthetic_sandbox_perk_definition_template());
        write_u16(
            &mut data,
            header + 16,
            damage_type
                .modern_sandbox_perk_index()
                .expect("elemental damage has a sandbox perk index"),
        )
        .unwrap();
    }
    data
}

pub(super) fn synthetic_sandbox_perk_definition_template() -> [u8; ITEM_SANDBOX_PERK_ROW_SIZE] {
    let mut template = [0xA5; ITEM_SANDBOX_PERK_ROW_SIZE];
    write_u16(&mut template, 0, MODERN_ARC_DAMAGE_PERK_INDEX).unwrap();
    template
}

pub(super) fn synthetic_weapon_with_investment_stats() -> Vec<u8> {
    let mut data =
        synthetic_weapon_definition(WeaponInventorySlot::Kinetic, WeaponDamageDescriptor::Empty);
    data.resize(0x1A0, 0);
    let resource = relative_target(&data, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let header = 0x140;
    set_array_count(&mut data, resource, header, 2).unwrap();
    write_relative_pointer(&mut data, resource + 8, header).unwrap();
    write_u32(&mut data, header + 8, ITEM_INVESTMENT_STAT_ROW_CLASS).unwrap();
    let rows = header + 16;
    data[rows] = 13;
    write_i32(&mut data, rows + 4, 0).unwrap();
    data[rows + ITEM_INVESTMENT_STAT_ROW_SIZE] = 16;
    write_i32(&mut data, rows + ITEM_INVESTMENT_STAT_ROW_SIZE + 4, 44).unwrap();
    data[rows + ITEM_INVESTMENT_STAT_ROW_SIZE + 8] = 0xA5;
    data
}

pub(super) fn synthetic_sandbox_perk_string_template() -> Vec<u8> {
    let mut template = vec![0_u8; 64];
    template[8..16].copy_from_slice(&NESTED_ARRAY_TRAILER);
    write_u64(&mut template, 16, 1).unwrap();
    write_u32(&mut template, 24, ITEM_STRING_SANDBOX_PERK_ROW_CLASS).unwrap();
    write_u16(&mut template, 32, u16::MAX).unwrap();
    write_u32(&mut template, 36, 0x811C_9DC5).unwrap();
    template
}

pub(super) fn synthetic_item_strings(damage: WeaponDamageDescriptor) -> Vec<u8> {
    let mut data = vec![0_u8; 0x180];
    let serialized_size = data.len() as u64;
    write_u64(&mut data, 0, serialized_size).unwrap();
    let resource = 0x100;
    write_relative_pointer(
        &mut data,
        ITEM_STRING_SANDBOX_PERK_RESOURCE_POINTER_OFFSET,
        resource,
    )
    .unwrap();
    write_u32(
        &mut data,
        resource - size_of::<u32>(),
        ITEM_STRING_SANDBOX_PERK_RESOURCE_CLASS,
    )
    .unwrap();
    if matches!(damage, WeaponDamageDescriptor::Elemental(_)) {
        set_item_string_sandbox_perk_count(&mut data, 1, &synthetic_sandbox_perk_string_template())
            .unwrap();
    }
    data
}
