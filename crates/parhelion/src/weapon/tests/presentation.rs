use super::*;

#[test]
fn authored_watermarks_remove_only_native_version_overrides() {
    let mut data = vec![0_u8; 0xDA];
    write_u64(&mut data, 0xA8, 5).unwrap();
    write_relative_pointer(&mut data, 0xB0, 0xC0).unwrap();
    write_u64(&mut data, 0xC0, 5).unwrap();
    write_u32(&mut data, 0xC8, 0x8080_5F87).unwrap();
    for (i, value) in [0x099F, u16::MAX, 0x0994, u16::MAX, 7]
        .into_iter()
        .enumerate()
    {
        write_u16(&mut data, 0xD0 + i * 2, value).unwrap();
    }
    let before = data.clone();
    clear_item_string_watermark_overrides(&mut data).unwrap();
    assert_eq!(&data[..0xD0], &before[..0xD0]);
    assert_eq!(&data[0xD0..], &[0xFF; 10]);
    let authored = data.clone();
    clear_item_string_watermark_overrides(&mut data).unwrap();
    assert_eq!(data, authored);
    write_u32(&mut data, 0xC8, 0x8080_5F89).unwrap();
    assert!(clear_item_string_watermark_overrides(&mut data).is_err());
    assert!(clear_item_string_watermark_overrides(&mut before[..0xD9].to_vec()).is_err());
}

#[test]
fn rarity_authoring_changes_only_the_verified_root_byte() {
    let mut data = synthetic_weapon_identity_fields(AuthoredWeaponRarity::Common, 0x0123);
    let before = data.clone();
    set_weapon_rarity(&mut data, AuthoredWeaponRarity::Exotic).unwrap();
    assert_eq!(weapon_rarity(&data).unwrap(), AuthoredWeaponRarity::Exotic);
    assert_eq!(changed_offsets(&before, &data), [ITEM_RARITY_OFFSET]);

    let mut malformed = synthetic_weapon_identity_fields(AuthoredWeaponRarity::Common, 0x0123);
    malformed[ITEM_RARITY_OFFSET] = 0;
    let before = malformed.clone();
    assert!(set_weapon_rarity(&mut malformed, AuthoredWeaponRarity::Rare).is_err());
    assert_eq!(malformed, before);
}

#[test]
fn collection_material_set_matches_authored_rarity() {
    for rarity in [
        AuthoredWeaponRarity::Common,
        AuthoredWeaponRarity::Uncommon,
        AuthoredWeaponRarity::Rare,
        AuthoredWeaponRarity::Legendary,
    ] {
        assert_eq!(
            collection_material_set_for_rarity(rarity),
            COLLECTIBLE_CURATED_WEAPON_MATERIAL_SET
        );
    }
    assert_eq!(
        collection_material_set_for_rarity(AuthoredWeaponRarity::Exotic),
        COLLECTIBLE_EXOTIC_WEAPON_MATERIAL_SET
    );
}

#[test]
fn equipment_restriction_tracks_rarity_without_changing_other_fields() {
    for rarity in [
        AuthoredWeaponRarity::Common,
        AuthoredWeaponRarity::Uncommon,
        AuthoredWeaponRarity::Rare,
        AuthoredWeaponRarity::Legendary,
        AuthoredWeaponRarity::Exotic,
    ] {
        let mut data = synthetic_weapon_identity_fields(rarity, 7);
        let block = data.len() + 16;
        data.resize(block + 0x30, 0xA5);
        write_relative_pointer(&mut data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET, block).unwrap();
        write_u32(&mut data, block - 4, ITEM_EQUIPMENT_BLOCK_CLASS).unwrap();
        write_u16(&mut data, block + ITEM_EQUIPMENT_SLOT_OFFSET, 7).unwrap();
        write_u16(
            &mut data,
            block + ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET,
            u16::MAX,
        )
        .unwrap();
        for original in [(0xDAD1_1536, 0xEF7B_6AD3), (0x811C_9DC5, 0)] {
            write_u32(&mut data, block + 0x10, original.0).unwrap();
            write_u32(&mut data, block + 0x14, original.1).unwrap();
            let before = data.clone();
            sync_weapon_equipment_rarity(&mut data).unwrap();
            let expected = if rarity == AuthoredWeaponRarity::Exotic {
                (0xDAD1_1536, 0xEF7B_6AD3)
            } else {
                (0x811C_9DC5, 0)
            };
            assert_eq!(weapon_equipment_label(&data).unwrap(), expected);
            assert!(
                changed_offsets(&before, &data)
                    .iter()
                    .all(|offset| { (block + 0x10..block + 0x18).contains(offset) })
            );
            let normalized = data.clone();
            sync_weapon_equipment_rarity(&mut data).unwrap();
            assert_eq!(data, normalized);
        }
        if rarity != AuthoredWeaponRarity::Exotic {
            write_u32(&mut data, block + 0x10, 0x1234_5678).unwrap();
            let before = data.clone();
            sync_weapon_equipment_rarity(&mut data).unwrap();
            assert_eq!(
                data, before,
                "unrelated unique-equip restrictions must survive"
            );
        }
        write_u32(&mut data, block - 4, 0).unwrap();
        let before = data.clone();
        assert!(sync_weapon_equipment_rarity(&mut data).is_err());
        assert_eq!(data, before);
    }
}

#[test]
fn translation_art_and_dye_edits_preserve_the_gear_art_selector() {
    const TRANSLATION_ROOT: usize = 0xC0;
    let mut target = synthetic_weapon_identity_fields(AuthoredWeaponRarity::Legendary, 0x0123);
    let mut source = synthetic_weapon_identity_fields(AuthoredWeaponRarity::Legendary, 0x4567);
    set_weapon_art_arrangements(
        &mut source,
        &[WeaponArtArrangementOverride {
            character_class: -1,
            arrangement: 0x5678,
        }],
    )
    .unwrap();
    set_weapon_render_dye_rows(
        &mut source,
        &[
            vec![WeaponDyeReferenceOverride {
                channel_index: 3,
                dye_reference_index: 77,
            }],
            vec![WeaponDyeReferenceOverride {
                channel_index: 5,
                dye_reference_index: 99,
            }],
            Vec::new(),
        ],
    )
    .unwrap();

    let original_dyes = weapon_render_dye_rows(&target).unwrap();
    transplant_weapon_geometry(&mut target, &source).unwrap();
    assert_eq!(weapon_pattern_index(&target).unwrap(), Some(0x0123));
    assert_eq!(weapon_render_dye_rows(&target).unwrap(), original_dyes);
    assert_eq!(
        weapon_art_arrangements(&target).unwrap()[0].arrangement,
        0x5678
    );

    let preserved_geometry = weapon_art_arrangements(&target).unwrap();
    transplant_weapon_render_gear(&mut target, &source).unwrap();
    assert_eq!(weapon_pattern_index(&target).unwrap(), Some(0x0123));
    assert_eq!(
        weapon_art_arrangements(&target).unwrap(),
        preserved_geometry
    );
    assert_eq!(
        weapon_render_dye_rows(&target).unwrap(),
        weapon_render_dye_rows(&source).unwrap()
    );

    let before = target.clone();
    set_weapon_pattern_index(&mut target, 0x89AB).unwrap();
    assert_eq!(weapon_pattern_index(&target).unwrap(), Some(0x89AB));
    assert_eq!(
        changed_offsets(&before, &target),
        [
            TRANSLATION_ROOT + TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
            TRANSLATION_ROOT + TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET + 1,
        ]
    );

    let before = target.clone();
    assert!(set_weapon_pattern_index(&mut target, u16::MAX).is_err());
    assert_eq!(target, before);
}

#[test]
fn ammunition_classification_is_an_independent_typed_field() {
    let mut strings = synthetic_item_string_ammo_type(1);
    let before = strings.clone();
    set_item_string_ammo_type(&mut strings, WeaponAmmoType::Special).unwrap();
    assert_eq!(
        item_string_ammo_type(&strings).unwrap(),
        Some(WeaponAmmoType::Special)
    );
    assert_eq!(
        changed_offsets(&before, &strings),
        [ITEM_STRING_AMMO_TYPE_OFFSET]
    );

    let mut malformed = synthetic_item_string_ammo_type(1);
    malformed[ITEM_STRING_AMMO_CLASS_OFFSET] ^= 1;
    let before = malformed.clone();
    assert!(set_item_string_ammo_type(&mut malformed, WeaponAmmoType::Heavy).is_err());
    assert_eq!(malformed, before);
}

#[test]
fn randomized_default_can_materialize_without_a_donor_member_template() {
    let mut data = synthetic_socket_definition();
    let resource = relative_target(&data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(&data, resource).unwrap();
    let embedded_descriptor =
        rows + ITEM_ORDINARY_SOCKET_ROW_SIZE + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET;
    write_u64(&mut data, embedded_descriptor, 0).unwrap();
    write_i64(&mut data, embedded_descriptor + 8, 0).unwrap();
    let mut columns = vec![None, None, None];

    normalize_inherited_randomized_socket_columns(&data, &mut columns).unwrap();
    let resolved = resolved_socket_columns(&columns);
    set_weapon_socket_columns(&mut data, &resolved).unwrap();

    assert_eq!(columns, vec![None, Some(vec![11]), None]);
    validate_weapon_socket_columns(&data, &resolved, &[176, 92, u16::MAX]).unwrap();
}

#[test]
fn slot_conversion_preserves_weapon_type_and_every_other_string_byte() {
    for from in [
        WeaponInventorySlot::Kinetic,
        WeaponInventorySlot::Energy,
        WeaponInventorySlot::Power,
    ] {
        for to in [
            WeaponInventorySlot::Kinetic,
            WeaponInventorySlot::Energy,
            WeaponInventorySlot::Power,
        ] {
            let mut strings = item_string_classification_fixture(from, fnv1_name_hash("sword"));
            let before = strings.clone();
            set_item_string_inventory_slot(&mut strings, from, to).unwrap();
            let tuple = item_string_client_classification(&strings, to).unwrap();
            assert_eq!(
                &tuple[4..],
                &before[ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + 4
                    ..ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + 12]
            );
            strings[ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET
                ..ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + 4]
                .copy_from_slice(
                    &before[ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET
                        ..ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + 4],
                );
            assert_eq!(strings, before);
        }
    }
    let mut malformed = item_string_classification_fixture(WeaponInventorySlot::Power, 0);
    let before = malformed.clone();
    assert!(
        set_item_string_inventory_slot(
            &mut malformed,
            WeaponInventorySlot::Power,
            WeaponInventorySlot::Energy
        )
        .is_err()
    );
    assert_eq!(malformed, before);
}

#[test]
fn item_string_classification_transplant_changes_only_the_audited_tuple() {
    let mut target = item_string_classification_fixture(WeaponInventorySlot::Kinetic, 0x6312_A690);
    let source = item_string_classification_fixture(WeaponInventorySlot::Energy, 0xC0E9_5045);
    let before = target.clone();

    transplant_item_string_client_classification(
        &mut target,
        WeaponInventorySlot::Kinetic,
        &source,
        WeaponInventorySlot::Energy,
        WeaponInventorySlot::Energy,
    )
    .expect("a package-proven target-slot tuple should transplant");

    assert_eq!(
        item_string_client_classification(&target, WeaponInventorySlot::Energy).unwrap(),
        item_string_client_classification(&source, WeaponInventorySlot::Energy).unwrap()
    );
    let mut normalized = target;
    normalized[ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET
        ..ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + ITEM_STRING_CLIENT_CLASSIFICATION_SIZE]
        .copy_from_slice(
            &before[ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET
                ..ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET
                    + ITEM_STRING_CLIENT_CLASSIFICATION_SIZE],
        );
    assert_eq!(normalized, before);
}

#[test]
fn cross_slot_classification_preserves_appearance_type_and_authored_bucket() {
    for (source_slot, target_slot) in [
        (WeaponInventorySlot::Energy, WeaponInventorySlot::Kinetic),
        (WeaponInventorySlot::Kinetic, WeaponInventorySlot::Energy),
    ] {
        let mut target = item_string_classification_fixture(target_slot, 0x6312_A690);
        let source = item_string_classification_fixture(source_slot, 0xC0E9_5045);
        let before = target.clone();
        let mut expected = item_string_client_classification(&source, source_slot).unwrap();
        expected[..4].copy_from_slice(&target_slot.bucket_hash().to_le_bytes());
        transplant_item_string_client_classification(
            &mut target,
            target_slot,
            &source,
            source_slot,
            target_slot,
        )
        .unwrap();
        assert_eq!(
            item_string_client_classification(&target, target_slot).unwrap(),
            expected
        );
        let start = ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET;
        let end = start + ITEM_STRING_CLIENT_CLASSIFICATION_SIZE;
        assert_eq!(&target[..start], &before[..start]);
        assert_eq!(&target[end..], &before[end..]);
    }
}

#[test]
fn item_string_classification_rejects_slot_and_type_key_mismatches() {
    let mut payload = item_string_classification_fixture(WeaponInventorySlot::Energy, 0xC0E9_5045);
    assert!(item_string_client_classification(&payload, WeaponInventorySlot::Kinetic).is_err());

    write_u32(
        &mut payload,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + 8,
        0x6312_A690,
    )
    .unwrap();
    assert!(item_string_client_classification(&payload, WeaponInventorySlot::Energy).is_err());
}

#[test]
fn item_string_classification_rejects_unknown_zero_and_truncated_tuples_without_mutation() {
    let mut unknown = item_string_classification_fixture(WeaponInventorySlot::Energy, 0xC0E9_5045);
    write_u32(
        &mut unknown,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET,
        0xDEAD_BEEF,
    )
    .unwrap();
    assert!(item_string_client_classification(&unknown, WeaponInventorySlot::Energy).is_err());

    let zero = item_string_classification_fixture(WeaponInventorySlot::Energy, 0);
    assert!(item_string_client_classification(&zero, WeaponInventorySlot::Energy).is_err());
    assert!(
        item_string_client_classification(
            &zero[..ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET + 8],
            WeaponInventorySlot::Energy,
        )
        .is_err()
    );

    let mut target = item_string_classification_fixture(WeaponInventorySlot::Kinetic, 0x6312_A690);
    let before = target.clone();
    assert!(
        transplant_item_string_client_classification(
            &mut target,
            WeaponInventorySlot::Kinetic,
            &unknown,
            WeaponInventorySlot::Energy,
            WeaponInventorySlot::Energy,
        )
        .is_err()
    );
    assert_eq!(target, before);
}

#[test]
fn authored_collectible_display_clears_stale_reacquire_warning() {
    let header = 0x20;
    let rows = header + 0x10;
    let template_row = rows;
    let mut displays = vec![0; rows + COLLECTIBLE_DISPLAY_ROW_SIZE];
    write_u64(&mut displays, 8, 1).unwrap();
    write_relative_pointer(&mut displays, 16, header).unwrap();
    write_u64(&mut displays, header, 1).unwrap();
    write_u32(&mut displays, header + 8, COLLECTIBLE_DISPLAY_ROW_CLASS).unwrap();

    let condition = template_row + COLLECTIBLE_DISPLAY_CONDITION_OFFSET;
    write_u64(&mut displays, condition, 0).unwrap();
    write_relative_pointer(&mut displays, condition + 8, condition + 8).unwrap();
    write_u32(&mut displays, condition + 16, 0x0000_FFFF).unwrap();
    write_localized_reference(
        &mut displays,
        template_row + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
        0x0000_0612,
        0x1BD9_4A4A,
    )
    .unwrap();

    let identity = WeaponCloneIdentity::from_namespace("parhelion.reacquire-warning-test")
        .expect("test identity should allocate");
    let authored = append_collectible_display(displays, 0, identity, 0x1234, 0x5678).unwrap();
    let (count, _, authored_rows, _) = array_at(&authored, 8).unwrap();
    let authored_row = authored_rows + COLLECTIBLE_DISPLAY_ROW_SIZE;

    assert_eq!(count, 2);
    assert_eq!(
        read_u32(
            &authored,
            template_row + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
        )
        .unwrap(),
        0x0000_0612
    );
    assert_eq!(
        read_u32(
            &authored,
            template_row + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET + 4,
        )
        .unwrap(),
        0x1BD9_4A4A
    );
    assert_eq!(
        read_u32(
            &authored,
            authored_row + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
        )
        .unwrap(),
        BLANK_LOCALIZED_REFERENCE_TABLE_INDEX
    );
    assert_eq!(
        read_u32(
            &authored,
            authored_row + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET + 4,
        )
        .unwrap(),
        BLANK_LOCALIZED_REFERENCE_HASH
    );
}

#[test]
fn authored_weapon_icon_row_preserves_stock_rows_and_selects_its_container() {
    let header = 0x20;
    let rows = header + 0x10;
    let initial_count = STOCK_ITEM_ICON_COUNT + 1;
    let mut icons = vec![0u8; rows + initial_count * ITEM_ICON_ROW_SIZE];
    write_u64(&mut icons, 8, initial_count as u64).unwrap();
    write_relative_pointer(&mut icons, 16, header).unwrap();
    write_u64(&mut icons, header, initial_count as u64).unwrap();
    write_u32(&mut icons, header + 8, ITEM_ICON_ROW_CLASS).unwrap();
    let donor_rows = icons[rows..].to_vec();
    let item_hash = 0x5355_4E44;
    let container = TagHash(0x8132_1234);

    let (authored, icon_index) =
        append_authored_weapon_icon_row(icons, 0, item_hash, container).unwrap();
    let (count, _, authored_rows, class) = array_at(&authored, 8).unwrap();
    assert_eq!(class, ITEM_ICON_ROW_CLASS);
    assert_eq!(count, initial_count + 1);
    assert_eq!(usize::from(icon_index), initial_count);
    assert_eq!(
        &authored[authored_rows..authored_rows + donor_rows.len()],
        donor_rows.as_slice()
    );

    let mut strings = vec![0u8; ITEM_STRING_ICON_INDEX_OFFSET + 2];
    write_u16(&mut strings, ITEM_STRING_ICON_INDEX_OFFSET, icon_index).unwrap();
    validate_authored_item_icon(&authored, &strings, item_hash, icon_index, container).unwrap();
}

#[test]
fn shader_unlock_preserves_base_colors_and_unrelated_render_data() {
    let mut data = synthetic_weapon_identity_fields(AuthoredWeaponRarity::Exotic, 42);
    let dye = |channel_index, dye_reference_index| WeaponDyeReferenceOverride {
        channel_index,
        dye_reference_index,
    };
    let original = [
        vec![dye(7, 11)],
        vec![dye(4, 20), dye(8, 21)],
        vec![dye(4, 30), dye(5, 31), dye(6, 32)],
    ];
    set_weapon_render_dye_rows(&mut data, &original).unwrap();
    let art = weapon_art_arrangements(&data).unwrap();
    unlock_weapon_shader_dyes(&mut data).unwrap();
    assert_eq!(
        weapon_render_dye_rows(&data).unwrap(),
        [
            vec![dye(7, 11)],
            vec![dye(8, 21), dye(4, 30), dye(5, 31), dye(6, 32)],
            vec![]
        ]
    );
    assert_eq!(weapon_art_arrangements(&data).unwrap(), art);
    assert_eq!(weapon_pattern_index(&data).unwrap(), Some(42));
    assert_eq!(weapon_rarity(&data).unwrap(), AuthoredWeaponRarity::Exotic);
    let once = data.clone();
    unlock_weapon_shader_dyes(&mut data).unwrap();
    assert_eq!(data, once);
}
