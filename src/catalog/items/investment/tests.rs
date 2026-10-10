use super::*;

#[test]
fn unnamed_armor_stat_plugs_use_their_local_investment_values() {
    let mut item = vec![0_u8; 0x300 + ITEM_INVESTMENT_STAT_ROW_SIZE * 3];
    let count = 3_u64;
    item[INVESTMENT_STAT_DESCRIPTOR..INVESTMENT_STAT_DESCRIPTOR + 8]
        .copy_from_slice(&count.to_le_bytes());
    item[INVESTMENT_STAT_DESCRIPTOR + 8..INVESTMENT_STAT_DESCRIPTOR + 16]
        .copy_from_slice(&(0x28_i64).to_le_bytes());
    item[0x2F0..0x2F8].copy_from_slice(&count.to_le_bytes());
    item[0x2F8..0x2FC].copy_from_slice(&ITEM_INVESTMENT_STAT_ROW_CLASS.to_le_bytes());

    for (row, stat_index, value) in [(0, 5_u16, 7_u32), (1, 3, 13), (2, 4, 1)] {
        let offset = 0x300 + row * ITEM_INVESTMENT_STAT_ROW_SIZE;
        item[offset..offset + 2].copy_from_slice(&stat_index.to_le_bytes());
        item[offset + 4..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let stat_names = vec![
        String::new(),
        String::new(),
        String::new(),
        "Mobility".into(),
        "Resilience".into(),
        "Recovery".into(),
    ];

    let rows = [3, 4, 5, 6, 7, 8];

    assert_eq!(
        stat_allocation_labels(&item, &rows, &stat_names),
        Some((
            "13 Mobility / 1 Resilience / 7 Recovery".into(),
            "Top Stat Allocation"
        ))
    );

    item[0x301] = 1;
    assert_eq!(stat_allocation_labels(&item, &rows, &stat_names), None);
}

#[test]
fn item_stats_follow_the_typed_item_resource() {
    let mut item = vec![0_u8; 0x1A0];
    item[ITEM_INVESTMENT_STAT_POINTER_OFFSET..ITEM_INVESTMENT_STAT_POINTER_OFFSET + 8]
        .copy_from_slice(&(0x90_i64).to_le_bytes());
    item[0xFC..0x100].copy_from_slice(&ITEM_INVESTMENT_STAT_RESOURCE_CLASS.to_le_bytes());
    item[0x100..0x108].copy_from_slice(&2_u64.to_le_bytes());
    item[0x108..0x110].copy_from_slice(&(0x38_i64).to_le_bytes());
    item[0x140..0x148].copy_from_slice(&2_u64.to_le_bytes());
    item[0x148..0x14C].copy_from_slice(&ITEM_INVESTMENT_STAT_ROW_CLASS.to_le_bytes());
    item[0x150..0x152].copy_from_slice(&1_u16.to_le_bytes());
    item[0x154..0x158].copy_from_slice(&67_i32.to_le_bytes());
    item[0x178..0x17A].copy_from_slice(&0_u16.to_le_bytes());
    item[0x17C..0x180].copy_from_slice(&(-12_i32).to_le_bytes());
    let definitions = vec![
        ItemStatDefinition {
            definition_index: 0,
            hash: 10,
            name: "Impact".into(),
            icon_container: None,
        },
        ItemStatDefinition {
            definition_index: 1,
            hash: 20,
            name: "Range".into(),
            icon_container: None,
        },
    ];

    assert_eq!(
        item_investment_stats(&item, &definitions),
        Ok(vec![
            ItemInvestmentStat {
                definition_index: 1,
                value: 67,
            },
            ItemInvestmentStat {
                definition_index: 0,
                value: -12,
            },
        ])
    );

    item[0xFC..0x100].copy_from_slice(&0_u32.to_le_bytes());
    assert!(item_investment_stats(&item, &definitions).is_err());

    item[ITEM_INVESTMENT_STAT_POINTER_OFFSET..ITEM_INVESTMENT_STAT_POINTER_OFFSET + 8].fill(0);
    assert_eq!(item_investment_stats(&item, &definitions), Ok(Vec::new()));
}

#[test]
fn perk_only_items_can_omit_the_stat_array_without_being_malformed() {
    let mut item = vec![0_u8; 0x140];
    item[ITEM_INVESTMENT_STAT_POINTER_OFFSET..ITEM_INVESTMENT_STAT_POINTER_OFFSET + 8]
        .copy_from_slice(&0x90_i64.to_le_bytes());
    item[0xFC..0x100].copy_from_slice(&ITEM_INVESTMENT_STAT_RESOURCE_CLASS.to_le_bytes());
    for perk_count in 1_u64..=4 {
        item[0x110..0x118].copy_from_slice(&perk_count.to_le_bytes());
        assert_eq!(item_investment_stats(&item, &[]), Ok(Vec::new()));
    }
    item[0x100] = 1; // A nonempty array with a null pointer is still malformed.
    assert!(item_investment_stats(&item, &[]).is_err());
    item[0x100] = 0;
    item[0xFC..0x100].fill(0);
    assert!(item_investment_stats(&item, &[]).is_err());
}

#[test]
fn item_stat_indices_are_u8_and_require_a_zero_reserved_byte() {
    let mut item = vec![0_u8; 0x180];
    item[ITEM_INVESTMENT_STAT_POINTER_OFFSET..ITEM_INVESTMENT_STAT_POINTER_OFFSET + 8]
        .copy_from_slice(&(0x90_i64).to_le_bytes());
    item[0xFC..0x100].copy_from_slice(&ITEM_INVESTMENT_STAT_RESOURCE_CLASS.to_le_bytes());
    item[0x100..0x108].copy_from_slice(&1_u64.to_le_bytes());
    item[0x108..0x110].copy_from_slice(&(0x18_i64).to_le_bytes());
    item[0x120..0x128].copy_from_slice(&1_u64.to_le_bytes());
    item[0x128..0x12C].copy_from_slice(&ITEM_INVESTMENT_STAT_ROW_CLASS.to_le_bytes());
    item[0x130] = 1;
    item[0x131] = 0xA5;
    item[0x134..0x138].copy_from_slice(&25_i32.to_le_bytes());
    let definitions = vec![
        ItemStatDefinition::default(),
        ItemStatDefinition {
            definition_index: 1,
            ..ItemStatDefinition::default()
        },
    ];

    assert!(item_investment_stats(&item, &definitions).is_err());
    item[0x131] = 0;
    assert_eq!(
        item_investment_stats(&item, &definitions),
        Ok(vec![ItemInvestmentStat {
            definition_index: 1,
            value: 25,
        }])
    );

    item[0x130] = 9;
    assert!(item_investment_stats(&item, &definitions).is_err());
}

#[test]
fn stat_group_minimum_is_scoped_to_the_selected_stat() {
    let group = ItemStatGroup {
        hash: 0,
        maximum_value: 100,
        scaled_stats: vec![
            ItemScaledStat {
                definition_index: 14,
                display_interpolation: vec![ItemStatDisplayPoint {
                    investment_value: 10,
                    display_value: 360,
                }],
                ..ItemScaledStat::default()
            },
            ItemScaledStat {
                definition_index: 15,
                display_interpolation: vec![ItemStatDisplayPoint {
                    investment_value: -20,
                    display_value: 0,
                }],
                ..ItemScaledStat::default()
            },
        ],
    };

    assert_eq!(group.minimum_value(14), Some(10));
    assert_eq!(group.minimum_value(15), Some(-20));
    assert_eq!(group.minimum_value(16), None);
}

#[test]
fn item_stat_group_index_follows_the_typed_string_resource() {
    let mut definition = vec![0_u8; 0x130];
    definition[ITEM_STRING_STAT_GROUP_POINTER_OFFSET..ITEM_STRING_STAT_GROUP_POINTER_OFFSET + 8]
        .copy_from_slice(&(0x70_i64).to_le_bytes());
    definition[0xDC..0xE0].copy_from_slice(&ITEM_STRING_STAT_GROUP_RESOURCE_CLASS.to_le_bytes());
    definition[0xF4..0xF8].copy_from_slice(&21_i32.to_le_bytes());

    assert_eq!(item_stat_group_index(&definition), Some(21));

    definition[0xDC..0xE0].copy_from_slice(&0_u32.to_le_bytes());
    assert_eq!(item_stat_group_index(&definition), None);
}

#[test]
fn stat_groups_decode_native_numeric_and_interpolation_fields() {
    let mut table = vec![0_u8; 0xC0];
    write_array_descriptor(&mut table, 8, 1, 0x20, STAT_GROUP_CLASS);
    table[0x30..0x34].copy_from_slice(&0xEAEF_4EA1_u32.to_le_bytes());
    table[0x60..0x64].copy_from_slice(&100_i32.to_le_bytes());
    write_array_descriptor(&mut table, 0x40, 1, 0x70, SCALED_STAT_CLASS);
    table[0x80] = 14;
    table[0x81] = 1;
    table[0x83] = 0;
    write_array_descriptor(&mut table, 0x88, 2, 0xA0, STAT_INTERPOLATION_CLASS);
    table[0xB0..0xB4].copy_from_slice(&20_i32.to_le_bytes());
    table[0xB4..0xB8].copy_from_slice(&450_i32.to_le_bytes());
    table[0xB8..0xBC].copy_from_slice(&80_i32.to_le_bytes());
    table[0xBC..0xC0].copy_from_slice(&600_i32.to_le_bytes());

    assert_eq!(
        decode_stat_groups(&table).unwrap(),
        vec![ItemStatGroup {
            hash: 0xEAEF_4EA1,
            maximum_value: 100,
            scaled_stats: vec![ItemScaledStat {
                definition_index: 14,
                display_as_numeric: true,
                is_linear: false,
                display_interpolation: vec![
                    ItemStatDisplayPoint {
                        investment_value: 20,
                        display_value: 450,
                    },
                    ItemStatDisplayPoint {
                        investment_value: 80,
                        display_value: 600,
                    },
                ],
            }],
        }]
    );
}

#[test]
fn stat_groups_accept_native_null_arrays() {
    let mut table = vec![0_u8; 0x68];
    write_array_descriptor(&mut table, 8, 1, 0x20, STAT_GROUP_CLASS);
    table[0x30..0x34].copy_from_slice(&0xC3C9_6257_u32.to_le_bytes());
    table[0x60..0x64].copy_from_slice(&10_i32.to_le_bytes());

    assert_eq!(
        decode_stat_groups(&table).unwrap(),
        vec![ItemStatGroup {
            hash: 0xC3C9_6257,
            maximum_value: 10,
            scaled_stats: Vec::new(),
        }]
    );
}

#[test]
fn non_linear_stat_display_curves_clamp_to_their_decoded_endpoints() {
    let points = [
        ItemStatDisplayPoint {
            investment_value: 20,
            display_value: 450,
        },
        ItemStatDisplayPoint {
            investment_value: 80,
            display_value: 600,
        },
    ];

    assert_eq!(
        interpolate_investment_stat_display(&points, false, -100),
        Some(450)
    );
    assert_eq!(
        interpolate_investment_stat_display(&points, false, 50),
        Some(525)
    );
    assert_eq!(
        interpolate_investment_stat_display(&points, false, 100),
        Some(600)
    );
    assert_eq!(
        interpolate_investment_stat_display(&points, true, -5),
        Some(-5)
    );
}

#[test]
fn masterwork_labels_use_the_primary_local_stat_name() {
    let mut item = vec![0_u8; 0x300 + ITEM_INVESTMENT_STAT_ROW_SIZE * 2];
    let count = 2_u64;
    item[INVESTMENT_STAT_DESCRIPTOR..INVESTMENT_STAT_DESCRIPTOR + 8]
        .copy_from_slice(&count.to_le_bytes());
    item[INVESTMENT_STAT_DESCRIPTOR + 8..INVESTMENT_STAT_DESCRIPTOR + 16]
        .copy_from_slice(&(0x28_i64).to_le_bytes());
    item[0x2F0..0x2F8].copy_from_slice(&count.to_le_bytes());
    item[0x2F8..0x2FC].copy_from_slice(&ITEM_INVESTMENT_STAT_ROW_CLASS.to_le_bytes());
    item[0x300..0x302].copy_from_slice(&2_u16.to_le_bytes());
    item[0x300 + ITEM_INVESTMENT_STAT_ROW_SIZE..0x302 + ITEM_INVESTMENT_STAT_ROW_SIZE]
        .copy_from_slice(&1_u16.to_le_bytes());
    let stat_names = vec![String::new(), "Impact".into(), "Charge Time".into()];

    assert_eq!(
        masterwork_label(&item, &stat_names, "Masterwork").as_deref(),
        Some("Masterwork: Charge Time")
    );
    assert_eq!(
        masterwork_label(&item, &stat_names, "Tier 7 Weapon").as_deref(),
        Some("Tier 7 Weapon: Charge Time")
    );
    let armor_stat_names = vec![
        String::new(),
        "Heroic Resistance".into(),
        "Arc Damage Resistance".into(),
    ];
    assert_eq!(
        masterwork_label(&item, &armor_stat_names, "Tier 4 Armor").as_deref(),
        Some("Tier 4 Armor: Arc Damage Resistance")
    );
    assert_eq!(
        masterwork_label(&item, &stat_names, "Masterwork Weapon"),
        None
    );
    assert_eq!(
        masterwork_label(&item, &stat_names[..2], "Masterwork"),
        None
    );

    item[0x301] = 1;
    assert_eq!(masterwork_label(&item, &stat_names, "Masterwork"), None);
}

fn write_array_descriptor(
    data: &mut [u8],
    descriptor: usize,
    count: u64,
    header: usize,
    class: u32,
) {
    data[descriptor..descriptor + 8].copy_from_slice(&count.to_le_bytes());
    let pointer = descriptor + 8;
    data[pointer..pointer + 8]
        .copy_from_slice(&i64::try_from(header - pointer).unwrap().to_le_bytes());
    data[header..header + 8].copy_from_slice(&count.to_le_bytes());
    data[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
}
