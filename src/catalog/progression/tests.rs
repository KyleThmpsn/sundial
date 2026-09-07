use super::*;

#[test]
fn progression_definitions_preserve_native_order_scope_and_object_slot() {
    const HEADER: usize = 32;
    const ROWS: usize = HEADER + 16;
    let scopes = [0_u8, 1, 0, 2, 99];
    let scope_slots = [7_u16, 9, 3, u16::MAX, u16::MAX];
    const STEPS_HEADER: usize = ROWS + 5 * PROGRESSION_DEFINITION_ROW_SIZE;
    const STEP_ROWS: usize = STEPS_HEADER + 16;
    const REWARDS_HEADER: usize = STEP_ROWS + 2 * PROGRESSION_STEP_ROW_SIZE;
    const REWARD_ROWS: usize = REWARDS_HEADER + 16;
    let mut table = vec![0_u8; REWARD_ROWS + PROGRESSION_REWARD_ROW_SIZE];
    table[8..16].copy_from_slice(&(scopes.len() as u64).to_le_bytes());
    table[16..24].copy_from_slice(&((HEADER - 16) as i64).to_le_bytes());
    table[HEADER..HEADER + 8].copy_from_slice(&(scopes.len() as u64).to_le_bytes());
    table[HEADER + 8..HEADER + 12].copy_from_slice(&PROGRESSION_DEFINITION_ROW_CLASS.to_le_bytes());
    let hashes = [11_u32, 22, 33, 44, 55];
    for (index, (scope, hash)) in scopes.into_iter().zip(hashes).enumerate() {
        let row = ROWS + index * PROGRESSION_DEFINITION_ROW_SIZE;
        table[row + PROGRESSION_DEFINITION_HASH_OFFSET
            ..row + PROGRESSION_DEFINITION_HASH_OFFSET + 4]
            .copy_from_slice(&hash.to_le_bytes());
        table[row + PROGRESSION_DEFINITION_SCOPE_OFFSET] = scope;
        table[row + PROGRESSION_DEFINITION_SCOPE_SLOT_OFFSET
            ..row + PROGRESSION_DEFINITION_SCOPE_SLOT_OFFSET + 2]
            .copy_from_slice(&scope_slots[index].to_le_bytes());
    }
    let first_row = ROWS;
    table[first_row + PROGRESSION_DEFINITION_REPEAT_LAST_STEP_OFFSET] = 1;
    let step_descriptor = first_row + PROGRESSION_DEFINITION_STEPS_OFFSET;
    table[step_descriptor..step_descriptor + 8].copy_from_slice(&2_u64.to_le_bytes());
    table[step_descriptor + 8..step_descriptor + 16]
        .copy_from_slice(&((STEPS_HEADER - (step_descriptor + 8)) as i64).to_le_bytes());
    table[STEPS_HEADER..STEPS_HEADER + 8].copy_from_slice(&2_u64.to_le_bytes());
    table[STEPS_HEADER + 8..STEPS_HEADER + 12]
        .copy_from_slice(&PROGRESSION_STEP_ROW_CLASS.to_le_bytes());
    table[STEP_ROWS..STEP_ROWS + 4].copy_from_slice(&50_i32.to_le_bytes());
    table[STEP_ROWS + PROGRESSION_STEP_ROW_SIZE..STEP_ROWS + PROGRESSION_STEP_ROW_SIZE + 4]
        .copy_from_slice(&100_i32.to_le_bytes());
    let reward_descriptor = first_row + PROGRESSION_DEFINITION_REWARDS_OFFSET;
    table[reward_descriptor..reward_descriptor + 8].copy_from_slice(&1_u64.to_le_bytes());
    table[reward_descriptor + 8..reward_descriptor + 16]
        .copy_from_slice(&((REWARDS_HEADER - (reward_descriptor + 8)) as i64).to_le_bytes());
    table[REWARDS_HEADER..REWARDS_HEADER + 8].copy_from_slice(&1_u64.to_le_bytes());
    table[REWARDS_HEADER + 8..REWARDS_HEADER + 12]
        .copy_from_slice(&PROGRESSION_REWARD_ROW_CLASS.to_le_bytes());
    table[REWARD_ROWS + PROGRESSION_REWARD_LEVEL_OFFSET
        ..REWARD_ROWS + PROGRESSION_REWARD_LEVEL_OFFSET + 4]
        .copy_from_slice(&5_i32.to_le_bytes());
    table[REWARD_ROWS + PROGRESSION_REWARD_ITEM_INDEX_OFFSET
        ..REWARD_ROWS + PROGRESSION_REWARD_ITEM_INDEX_OFFSET + 4]
        .copy_from_slice(&1_u32.to_le_bytes());
    table[REWARD_ROWS + PROGRESSION_REWARD_QUANTITY_OFFSET
        ..REWARD_ROWS + PROGRESSION_REWARD_QUANTITY_OFFSET + 4]
        .copy_from_slice(&3_i32.to_le_bytes());

    assert_eq!(
        progression_definitions_from_data(&table, &[0xAAAA_AAAA, 0xBBBB_BBBB]).unwrap(),
        [
            ProgressionDefinition {
                definition_index: 0,
                hash: 11,
                scope: ProgressionScope::Account,
                scope_slot: Some(7),
                repeat_last_step: true,
                name: String::new(),
                description: String::new(),
                source: String::new(),
                display_units_name: String::new(),
                icon_container: None,
                factions: Vec::new(),
                steps: vec![
                    ProgressionStepDefinition {
                        progress_total: 50,
                        name: String::new(),
                        icon_container: None,
                    },
                    ProgressionStepDefinition {
                        progress_total: 100,
                        name: String::new(),
                        icon_container: None,
                    },
                ],
                reward_items: vec![ProgressionRewardDefinition {
                    rewarded_at_progression_level: 5,
                    item_hash: 0xBBBB_BBBB,
                    quantity: 3,
                }],
            },
            ProgressionDefinition {
                definition_index: 1,
                hash: 22,
                scope: ProgressionScope::Character,
                scope_slot: Some(9),
                repeat_last_step: false,
                name: String::new(),
                description: String::new(),
                source: String::new(),
                display_units_name: String::new(),
                icon_container: None,
                factions: Vec::new(),
                steps: Vec::new(),
                reward_items: Vec::new(),
            },
            ProgressionDefinition {
                definition_index: 2,
                hash: 33,
                scope: ProgressionScope::Account,
                scope_slot: Some(3),
                repeat_last_step: false,
                name: String::new(),
                description: String::new(),
                source: String::new(),
                display_units_name: String::new(),
                icon_container: None,
                factions: Vec::new(),
                steps: Vec::new(),
                reward_items: Vec::new(),
            },
            ProgressionDefinition {
                definition_index: 3,
                hash: 44,
                scope: ProgressionScope::Unreplicated,
                scope_slot: None,
                repeat_last_step: false,
                name: String::new(),
                description: String::new(),
                source: String::new(),
                display_units_name: String::new(),
                icon_container: None,
                factions: Vec::new(),
                steps: Vec::new(),
                reward_items: Vec::new(),
            },
            ProgressionDefinition {
                definition_index: 4,
                hash: 55,
                scope: ProgressionScope::Unreplicated,
                scope_slot: None,
                repeat_last_step: false,
                name: String::new(),
                description: String::new(),
                source: String::new(),
                display_units_name: String::new(),
                icon_container: None,
                factions: Vec::new(),
                steps: Vec::new(),
                reward_items: Vec::new(),
            },
        ]
    );
}

fn unlock_flag_display_table(hash: u32) -> Vec<u8> {
    const HEADER: usize = 0x30;
    const ROW: usize = 0x40;
    const CONTENT_HEADER: usize = 0x60;
    const DISPLAY: usize = 0x70;

    let mut table = vec![0_u8; DISPLAY + UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE];
    table[8..16].copy_from_slice(&1_u64.to_le_bytes());
    table[16..24].copy_from_slice(&((HEADER - 16) as i64).to_le_bytes());
    table[0x18..0x20].copy_from_slice(&1_u64.to_le_bytes());
    table[0x20..0x28].copy_from_slice(&((CONTENT_HEADER - 0x20) as i64).to_le_bytes());
    table[HEADER..HEADER + 8].copy_from_slice(&1_u64.to_le_bytes());
    table[HEADER + 8..HEADER + 12].copy_from_slice(&UNLOCK_FLAG_DISPLAY_ROW_CLASS.to_le_bytes());
    table[CONTENT_HEADER - NESTED_ARRAY_TRAILER.len()..CONTENT_HEADER]
        .copy_from_slice(&NESTED_ARRAY_TRAILER);
    table[CONTENT_HEADER..CONTENT_HEADER + 8].copy_from_slice(&1_u64.to_le_bytes());
    table[CONTENT_HEADER + 8..CONTENT_HEADER + 12]
        .copy_from_slice(&UNLOCK_FLAG_DISPLAY_CONTENT_ROW_CLASS.to_le_bytes());
    table[ROW..ROW + 4].copy_from_slice(&hash.to_le_bytes());
    table[ROW + 4..ROW + 8].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
    table[ROW + UNLOCK_FLAG_DISPLAY_POINTER_OFFSET..ROW + UNLOCK_FLAG_DISPLAY_POINTER_OFFSET + 8]
        .copy_from_slice(
            &((DISPLAY - (ROW + UNLOCK_FLAG_DISPLAY_POINTER_OFFSET)) as i64).to_le_bytes(),
        );
    table
}

fn write_condition_array(data: &mut [u8], descriptor: usize, header: usize, rows: &[(u32, u32)]) {
    let count = u64::try_from(rows.len()).unwrap();
    let pointer = descriptor + 8;
    let relative = i64::try_from(header).unwrap() - i64::try_from(pointer).unwrap();
    data[descriptor..descriptor + 8].copy_from_slice(&count.to_le_bytes());
    data[pointer..pointer + 8].copy_from_slice(&relative.to_le_bytes());
    data[header..header + 8].copy_from_slice(&count.to_le_bytes());
    data[header + 8..header + 12].copy_from_slice(&CONDITION_EXPRESSION_ROW_CLASS.to_le_bytes());
    for (index, (kind, operand)) in rows.iter().copied().enumerate() {
        let row = header + 16 + index * CONDITION_EXPRESSION_ROW_SIZE;
        data[row..row + 4].copy_from_slice(&kind.to_le_bytes());
        data[row + 4..row + 8].copy_from_slice(&operand.to_le_bytes());
    }
}

#[test]
fn unlock_flag_displays_validate_aligned_hashes_and_relative_blocks() {
    let definition = UnlockDefinition {
        hash: 0x1234_5678,
        ..UnlockDefinition::default()
    };
    let table = unlock_flag_display_table(definition.hash as u32);

    assert_eq!(
        unlock_flag_display_blocks(&table, std::slice::from_ref(&definition)),
        Ok(vec![112])
    );

    let mut wrong_hash = table.clone();
    wrong_hash[64..68].copy_from_slice(&0x8765_4321_u32.to_le_bytes());
    assert!(unlock_flag_display_blocks(&wrong_hash, &[definition.clone()]).is_err());

    let mut wrong_class = table.clone();
    wrong_class[56..60].copy_from_slice(&0_u32.to_le_bytes());
    assert!(unlock_flag_display_blocks(&wrong_class, &[definition.clone()]).is_err());

    let mut outside = table;
    outside[72..80].copy_from_slice(&i64::MAX.to_le_bytes());
    assert!(unlock_flag_display_blocks(&outside, &[definition.clone()]).is_err());
    assert!(
        unlock_flag_display_blocks(&unlock_flag_display_table(definition.hash as u32), &[])
            .is_err()
    );
}

#[test]
fn unlock_display_text_only_filters_blank_localized_strings() {
    assert_eq!(nonblank_localized_string(None), None);
    assert_eq!(nonblank_localized_string(Some(" \t ".into())), None);
    assert_eq!(
        nonblank_localized_string(Some("  Exact package text  ".into())),
        Some("  Exact package text  ".into())
    );
}

#[test]
fn unlock_state_indices_use_the_compact_bank_and_keep_the_first_definition() {
    let definitions = vec![
        UnlockDefinition {
            hash: 0x1111_1111,
            code: 0x0201,
            compact_slot: Some(58),
            name: None,
            description: None,
            tested_by: Vec::new(),
        },
        UnlockDefinition {
            hash: 0x2222_2222,
            code: 0x0001,
            compact_slot: Some(58),
            name: None,
            description: None,
            tested_by: Vec::new(),
        },
        UnlockDefinition {
            hash: 0x3333_3333,
            code: 0x0002,
            compact_slot: Some(58),
            name: None,
            description: None,
            tested_by: Vec::new(),
        },
        UnlockDefinition {
            hash: 0x4444_4444,
            code: 0x0001,
            compact_slot: None,
            name: None,
            description: None,
            tested_by: Vec::new(),
        },
    ];

    let indices = unlock_state_indices(&definitions);

    assert_eq!(indices.get(&(1, 58)), Some(&0));
    assert_eq!(indices.get(&(2, 58)), Some(&2));
    assert!(!indices.contains_key(&(1, u16::MAX)));
}

#[test]
fn objective_limits_respect_direction_and_overcompletion() {
    let mut objective = ObjectiveDef {
        completion_value: 70,
        ..ObjectiveDef::default()
    };
    assert_eq!(objective.maximum_value(), Some(70));
    assert_eq!(objective.minimum_value(), None);

    objective.allow_overcompletion = true;
    assert_eq!(objective.maximum_value(), None);
    assert_eq!(objective.minimum_value(), None);

    objective.allow_overcompletion = false;
    objective.is_counting_downward = true;
    assert_eq!(objective.maximum_value(), None);
    assert_eq!(objective.minimum_value(), Some(70));
}

#[test]
fn repeated_objective_owners_merge_richer_package_metadata() {
    let mut objectives = vec![ObjectiveDef::default()];
    add_objective_owner(
        &mut objectives,
        0,
        ObjectiveOwnerDef {
            hash: 7,
            kind: ObjectiveOwnerKind::InventoryItem,
            name: String::new(),
            type_name: "Item".into(),
            description: String::new(),
            traits: Vec::new(),
            paths: vec![vec!["Items".into()]],
        },
    );
    add_objective_owner(
        &mut objectives,
        0,
        ObjectiveOwnerDef {
            hash: 7,
            kind: ObjectiveOwnerKind::InventoryItem,
            name: "Ace of Spades".into(),
            type_name: String::new(),
            description: "Hand cannon".into(),
            traits: vec![ObjectiveOwnerTraitDef {
                hash: 9,
                name: "Exotic".into(),
                description: String::new(),
            }],
            paths: vec![vec!["Collections".into()]],
        },
    );

    let owner = &objectives[0].owners[0];
    assert_eq!(objectives[0].owners.len(), 1);
    assert_eq!(owner.name, "Ace of Spades");
    assert_eq!(owner.description, "Hand cannon");
    assert_eq!(owner.paths.len(), 2);
    assert_eq!(owner.traits.len(), 1);
}

#[test]
fn condition_references_retain_the_complete_package_program() {
    let mut rows = vec![0_u8; 24];
    for (index, (kind, operand)) in [(1_u8, 3_u16), (12, 77), (10, 9)].into_iter().enumerate() {
        let row = index * CONDITION_EXPRESSION_ROW_SIZE;
        rows[row] = kind;
        rows[row + 1..row + 4].copy_from_slice(&[0xAA, 0xBB, 0xCC]);
        rows[row + 4..row + 6].copy_from_slice(&operand.to_le_bytes());
        rows[row + 6..row + 8].copy_from_slice(&[0xDD, 0xEE]);
    }

    let references = condition_references_from_rows(&rows, 0, 3).unwrap();

    assert_eq!(references.flags, vec![3]);
    assert_eq!(references.values, vec![9]);
    assert_eq!(references.pool_rows, vec![77]);
    assert_eq!(
        references.programs[0]
            .iter()
            .map(|token| (token[0], token[1]))
            .collect::<Vec<_>>(),
        vec![(1, 3), (12, 77), (10, 9)]
    );
}

#[test]
fn objective_conditions_use_row_plus_08_and_ignore_plus_10_decoy() {
    const ACTUAL_HEADER: usize = 0x60;
    const DECOY_HEADER: usize = 0x100;
    const DECOY_COUNT: usize = ACTUAL_HEADER - 0x10;
    let mut definitions =
        vec![0_u8; DECOY_HEADER + 16 + DECOY_COUNT * CONDITION_EXPRESSION_ROW_SIZE];

    definitions[OBJECTIVE_CONDITIONS_OFFSET..OBJECTIVE_CONDITIONS_OFFSET + 8]
        .copy_from_slice(&1_u64.to_le_bytes());
    definitions[OBJECTIVE_CONDITIONS_OFFSET + 8..OBJECTIVE_CONDITIONS_OFFSET + 16]
        .copy_from_slice(&i64::try_from(DECOY_COUNT).unwrap().to_le_bytes());
    definitions[0x18..0x20]
        .copy_from_slice(&i64::try_from(DECOY_HEADER - 0x18).unwrap().to_le_bytes());

    definitions[ACTUAL_HEADER..ACTUAL_HEADER + 8].copy_from_slice(&1_u64.to_le_bytes());
    definitions[ACTUAL_HEADER + 8..ACTUAL_HEADER + 12]
        .copy_from_slice(&CONDITION_EXPRESSION_ROW_CLASS.to_le_bytes());
    definitions[ACTUAL_HEADER + 16..ACTUAL_HEADER + 20]
        .copy_from_slice(&CONDITION_FLAG_KIND.to_le_bytes());
    definitions[ACTUAL_HEADER + 20..ACTUAL_HEADER + 24].copy_from_slice(&3_u32.to_le_bytes());

    definitions[DECOY_HEADER..DECOY_HEADER + 8]
        .copy_from_slice(&u64::try_from(DECOY_COUNT).unwrap().to_le_bytes());
    definitions[DECOY_HEADER + 8..DECOY_HEADER + 12]
        .copy_from_slice(&CONDITION_EXPRESSION_ROW_CLASS.to_le_bytes());
    definitions[DECOY_HEADER + 16..DECOY_HEADER + 20]
        .copy_from_slice(&CONDITION_VALUE_KIND.to_le_bytes());
    definitions[DECOY_HEADER + 20..DECOY_HEADER + 24].copy_from_slice(&9_u32.to_le_bytes());

    assert_eq!(
        objective_condition_references_at(&definitions, 0).unwrap(),
        ConditionReferences {
            flags: vec![3],
            values: Vec::new(),
            pool_rows: Vec::new(),
            programs: vec![vec![[CONDITION_FLAG_KIND, 3]]],
        }
    );
    let decoy = condition_references_at(&definitions, 0x10).unwrap();
    assert!(decoy.flags.is_empty());
    assert_eq!(decoy.values, vec![9]);
    assert_eq!(decoy.programs[0][0], [CONDITION_VALUE_KIND, 9]);
    assert_eq!(decoy.programs[0].len(), DECOY_COUNT);
}

#[test]
fn objective_conditions_merge_the_secondary_package_program() {
    const PRIMARY_HEADER: usize = 0x80;
    const SECONDARY_HEADER: usize = 0xA0;
    let mut definitions = vec![0_u8; 0xC0];
    write_condition_array(
        &mut definitions,
        OBJECTIVE_CONDITIONS_OFFSET,
        PRIMARY_HEADER,
        &[(CONDITION_FLAG_KIND, 4)],
    );
    write_condition_array(
        &mut definitions,
        OBJECTIVE_SECONDARY_CONDITIONS_OFFSET,
        SECONDARY_HEADER,
        &[(CONDITION_VALUE_KIND, 6)],
    );

    let references = objective_condition_references_at(&definitions, 0).unwrap();

    assert_eq!(references.flags, vec![4]);
    assert_eq!(references.values, vec![6]);
    assert_eq!(references.programs.len(), 2);
    assert_eq!(references.programs[0], vec![[CONDITION_FLAG_KIND, 4]]);
    assert_eq!(references.programs[1], vec![[CONDITION_VALUE_KIND, 6]]);
}

#[test]
fn objective_intrinsic_perks_validate_and_deduplicate_flag_indices() {
    const HEADER: usize = 0x80;
    const ROWS: usize = HEADER + 16;
    let descriptor = OBJECTIVE_INTRINSIC_PERK_FLAGS_OFFSET;
    let mut definitions = vec![0_u8; 0xA0];
    definitions[descriptor..descriptor + 8].copy_from_slice(&3_u64.to_le_bytes());
    definitions[descriptor + 8..descriptor + 16].copy_from_slice(
        &i64::try_from(HEADER - (descriptor + 8))
            .unwrap()
            .to_le_bytes(),
    );
    definitions[HEADER..HEADER + 8].copy_from_slice(&3_u64.to_le_bytes());
    definitions[HEADER + 8..HEADER + 12]
        .copy_from_slice(&OBJECTIVE_INTRINSIC_PERK_FLAG_ROW_CLASS.to_le_bytes());
    for (row, index) in [4_u16, 7, 4].into_iter().enumerate() {
        let offset = ROWS + row * OBJECTIVE_INTRINSIC_PERK_FLAG_ROW_SIZE;
        definitions[offset..offset + 2].copy_from_slice(&index.to_le_bytes());
    }

    assert_eq!(
        objective_intrinsic_perk_flag_indices_at(&definitions, 0, 8),
        Ok(vec![4, 7])
    );

    definitions[ROWS..ROWS + 2].copy_from_slice(&8_u16.to_le_bytes());
    assert!(objective_intrinsic_perk_flag_indices_at(&definitions, 0, 8).is_err());
    definitions[ROWS..ROWS + 2].copy_from_slice(&4_u16.to_le_bytes());
    definitions[HEADER + 8..HEADER + 12].copy_from_slice(&0_u32.to_le_bytes());
    assert!(objective_intrinsic_perk_flag_indices_at(&definitions, 0, 8).is_err());
}

#[test]
fn location_definition_releases_use_activity_u16_at_12_and_own_conditions() {
    const FIRST_RELEASE: usize = 0;
    const SECOND_RELEASE: usize = LOCATION_RELEASE_ROW_SIZE;
    const FIRST_HEADER: usize = 0xB0;
    const SECOND_HEADER: usize = 0xD0;
    let mut definitions = vec![0_u8; 0xF0];
    write_condition_array(
        &mut definitions,
        FIRST_RELEASE,
        FIRST_HEADER,
        &[(CONDITION_FLAG_KIND, 4)],
    );
    write_condition_array(
        &mut definitions,
        SECOND_RELEASE,
        SECOND_HEADER,
        &[(CONDITION_VALUE_KIND, 6)],
    );
    definitions[FIRST_RELEASE + 0x10..FIRST_RELEASE + 0x12]
        .copy_from_slice(&0xBEEF_u16.to_le_bytes());
    definitions[FIRST_RELEASE + LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET
        ..FIRST_RELEASE + LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET + 2]
        .copy_from_slice(&0x1234_u16.to_le_bytes());
    definitions[SECOND_RELEASE + LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET
        ..SECOND_RELEASE + LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET + 2]
        .copy_from_slice(&u16::MAX.to_le_bytes());

    let first = location_definition_release_at(&definitions, FIRST_RELEASE).unwrap();
    let second = location_definition_release_at(&definitions, SECOND_RELEASE).unwrap();

    assert_eq!(first.activity_index, Some(0x1234));
    assert_eq!(first.references.flags, vec![4]);
    assert!(first.references.values.is_empty());
    assert_eq!(second.activity_index, None);
    assert!(second.references.flags.is_empty());
    assert_eq!(second.references.values, vec![6]);
}

#[test]
fn location_release_condition_rows_use_exact_fields_and_reject_bad_activity_indices() {
    const HEADER: usize = 0x40;
    let mut data = vec![0_u8; 0x58];
    data[LOCATION_RELEASE_LOCATION_INDEX_OFFSET..LOCATION_RELEASE_LOCATION_INDEX_OFFSET + 4]
        .copy_from_slice(&7_u32.to_le_bytes());
    write_condition_array(
        &mut data,
        LOCATION_RELEASE_CONDITIONS_OFFSET,
        HEADER,
        &[(CONDITION_FLAG_KIND, 11)],
    );
    data[0x18..0x1C].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
    data[LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET
        ..LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET + 2]
        .copy_from_slice(&2_u16.to_le_bytes());

    let release = location_release_condition_row_at(&data, 0).unwrap();
    assert_eq!(release.location_index, 7);
    assert_eq!(release.references.flags, vec![11]);
    assert!(release.references.values.is_empty());
    assert_eq!(release.activity_index, Some(2));

    let activities = vec![ActivityContext {
        hash: 1,
        definition_start: 0,
        name: "Only activity".into(),
        description: String::new(),
        gate_hashes: Vec::new(),
    }];
    let error = location_release_activity(&activities, release.activity_index, 5).unwrap_err();
    assert!(error.contains("row 5"));
    assert!(error.contains("activity index 2"));

    data[LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET
        ..LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET + 2]
        .copy_from_slice(&u16::MAX.to_le_bytes());
    let release = location_release_condition_row_at(&data, 0).unwrap();
    assert_eq!(release.activity_index, None);
    assert!(
        location_release_activity(&activities, release.activity_index, 5)
            .unwrap()
            .is_none()
    );
}

#[test]
fn presentation_paths_keep_immediate_parent_first_and_preserve_branches() {
    let node = |hash, name: &str, parents| PresentationNodeDef {
        hash,
        name: name.into(),
        parents,
        objective_index: None,
        condition_references: ConditionReferences::default(),
    };
    let nodes = vec![
        node(1, "Metrics", vec![]),
        node(2, "Account", vec![0]),
        node(3, "Crucible", vec![0]),
        node(4, "Account", vec![0]),
    ];

    assert_eq!(
        presentation_paths(&nodes, &[1]),
        vec![vec!["Account".to_owned(), "Metrics".to_owned()]]
    );
    assert_eq!(
        presentation_paths(&nodes, &[1, 2]),
        vec![
            vec!["Account".to_owned(), "Metrics".to_owned()],
            vec!["Crucible".to_owned(), "Metrics".to_owned()],
        ]
    );
    assert_eq!(presentation_paths(&nodes, &[1, 3]).len(), 1);
}

#[test]
fn metric_traits_use_the_authored_u16_list_without_a_row_cap() {
    const METRIC_DEFINITION: usize = 0x20;
    const HEADER: usize = 0x80;
    const ROWS: usize = HEADER + 0x10;
    let descriptor = METRIC_DEFINITION + METRIC_TRAIT_LIST_OFFSET;
    let mut definitions = vec![0_u8; ROWS + 6];
    definitions[descriptor..descriptor + 8].copy_from_slice(&3_u64.to_le_bytes());
    definitions[descriptor + 8..descriptor + 16]
        .copy_from_slice(&((HEADER - (descriptor + 8)) as i64).to_le_bytes());
    definitions[HEADER..HEADER + 8].copy_from_slice(&3_u64.to_le_bytes());
    definitions[HEADER + 8..HEADER + 12]
        .copy_from_slice(&METRIC_TRAIT_INDEX_ROW_CLASS.to_le_bytes());
    definitions[ROWS..ROWS + 2].copy_from_slice(&5_u16.to_le_bytes());
    definitions[ROWS + 2..ROWS + 4].copy_from_slice(&71_u16.to_le_bytes());
    definitions[ROWS + 4..ROWS + 6].copy_from_slice(&74_u16.to_le_bytes());

    assert_eq!(
        metric_trait_indices(&definitions, METRIC_DEFINITION, 75).unwrap(),
        vec![5, 71, 74]
    );

    definitions[HEADER + 8..HEADER + 12].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
    let error = metric_trait_indices(&definitions, METRIC_DEFINITION, 75).unwrap_err();
    assert!(error.contains("Unexpected metric trait row class 0xDEADBEEF"));
}

#[test]
fn item_objective_lists_follow_the_exact_resource_pointer_and_deduplicate() {
    const RESOURCE: usize = 0x80;
    const HEADER: usize = 0xA0;
    const ROWS: usize = HEADER + 0x10;
    let mut item = vec![0_u8; ROWS + 6];
    item[ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET..ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET + 8]
        .copy_from_slice(
            &((RESOURCE - ITEM_OBJECTIVE_RESOURCE_POINTER_OFFSET) as i64).to_le_bytes(),
        );
    item[RESOURCE - 4..RESOURCE].copy_from_slice(&ITEM_OBJECTIVE_RESOURCE_CLASS.to_le_bytes());
    item[RESOURCE..RESOURCE + 8].copy_from_slice(&3_u64.to_le_bytes());
    item[RESOURCE + 8..RESOURCE + 16]
        .copy_from_slice(&((HEADER - (RESOURCE + 8)) as i64).to_le_bytes());
    item[HEADER..HEADER + 8].copy_from_slice(&3_u64.to_le_bytes());
    item[HEADER + 8..HEADER + 12].copy_from_slice(&ITEM_OBJECTIVE_INDEX_ROW_CLASS.to_le_bytes());
    item[ROWS..ROWS + 2].copy_from_slice(&5_u16.to_le_bytes());
    item[ROWS + 2..ROWS + 4].copy_from_slice(&2_u16.to_le_bytes());
    item[ROWS + 4..ROWS + 6].copy_from_slice(&5_u16.to_le_bytes());

    assert_eq!(item_objective_indices(&item, 10), vec![2, 5]);

    item[RESOURCE - 4..RESOURCE].copy_from_slice(&0_u32.to_le_bytes());
    assert!(item_objective_indices(&item, 10).is_empty());
}

#[test]
fn item_objective_lists_ignore_decoy_arrays_outside_the_resource_pointer() {
    let mut item = vec![0_u8; 96];
    item[0..8].copy_from_slice(&1_u64.to_le_bytes());
    item[8..16].copy_from_slice(&8_i64.to_le_bytes());
    item[16..24].copy_from_slice(&1_u64.to_le_bytes());
    item[24..28].copy_from_slice(&ITEM_OBJECTIVE_INDEX_ROW_CLASS.to_le_bytes());
    item[32..34].copy_from_slice(&5_u16.to_le_bytes());

    assert!(item_objective_indices(&item, 10).is_empty());
}

#[test]
fn record_objective_lists_merge_direct_and_shadowkeep_interval_rows() {
    const DIRECT_HEADER: usize = 0x80;
    const DIRECT_ROWS: usize = DIRECT_HEADER + 0x10;
    const INTERVAL_HEADER: usize = 0xA0;
    const INTERVAL_ROWS: usize = INTERVAL_HEADER + 0x10;
    let mut definitions = vec![0_u8; INTERVAL_ROWS + 2 * RECORD_INTERVAL_OBJECTIVE_ROW_SIZE];

    definitions[RECORD_OBJECTIVE_LIST_OFFSET..RECORD_OBJECTIVE_LIST_OFFSET + 8]
        .copy_from_slice(&2_u64.to_le_bytes());
    definitions[RECORD_OBJECTIVE_LIST_OFFSET + 8..RECORD_OBJECTIVE_LIST_OFFSET + 16]
        .copy_from_slice(
            &((DIRECT_HEADER - (RECORD_OBJECTIVE_LIST_OFFSET + 8)) as i64).to_le_bytes(),
        );
    definitions[DIRECT_HEADER..DIRECT_HEADER + 8].copy_from_slice(&2_u64.to_le_bytes());
    definitions[DIRECT_HEADER + 8..DIRECT_HEADER + 12]
        .copy_from_slice(&RECORD_OBJECTIVE_INDEX_ROW_CLASS.to_le_bytes());
    definitions[DIRECT_ROWS..DIRECT_ROWS + 2].copy_from_slice(&5_u16.to_le_bytes());
    definitions[DIRECT_ROWS + 2..DIRECT_ROWS + 4].copy_from_slice(&7_u16.to_le_bytes());

    definitions[RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET..RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET + 8]
        .copy_from_slice(&2_u64.to_le_bytes());
    definitions
        [RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET + 8..RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET + 16]
        .copy_from_slice(
            &((INTERVAL_HEADER - (RECORD_INTERVAL_OBJECTIVE_LIST_OFFSET + 8)) as i64).to_le_bytes(),
        );
    definitions[INTERVAL_HEADER..INTERVAL_HEADER + 8].copy_from_slice(&2_u64.to_le_bytes());
    definitions[INTERVAL_HEADER + 8..INTERVAL_HEADER + 12]
        .copy_from_slice(&RECORD_INTERVAL_OBJECTIVE_ROW_CLASS.to_le_bytes());
    definitions[INTERVAL_ROWS..INTERVAL_ROWS + 2].copy_from_slice(&2_u16.to_le_bytes());
    definitions[INTERVAL_ROWS + RECORD_INTERVAL_OBJECTIVE_ROW_SIZE
        ..INTERVAL_ROWS + RECORD_INTERVAL_OBJECTIVE_ROW_SIZE + 2]
        .copy_from_slice(&5_u16.to_le_bytes());

    assert_eq!(
        record_objective_indices(&definitions, 0, 10).unwrap(),
        vec![2, 5, 7]
    );
}

#[test]
fn collectible_item_paths_use_the_authored_u16_item_index_and_presentation_parents() {
    let nodes = vec![
        PresentationNodeDef {
            hash: 1,
            name: "Items".into(),
            parents: Vec::new(),
            objective_index: None,
            condition_references: ConditionReferences::default(),
        },
        PresentationNodeDef {
            hash: 2,
            name: "Majestic Solstice Suit".into(),
            parents: vec![0],
            objective_index: None,
            condition_references: ConditionReferences::default(),
        },
    ];
    let mut definitions = vec![0_u8; 250];
    definitions[8..16].copy_from_slice(&1_u64.to_le_bytes());
    definitions[16..24].copy_from_slice(&16_i64.to_le_bytes());
    definitions[32..40].copy_from_slice(&1_u64.to_le_bytes());
    definitions[40..44].copy_from_slice(&COLLECTIBLE_DEFINITION_ROW_CLASS.to_le_bytes());
    let row = 48;
    definitions[row + 0x18..row + 0x20].copy_from_slice(&1_u64.to_le_bytes());
    definitions[row + 0x20..row + 0x28].copy_from_slice(&152_i64.to_le_bytes());
    definitions[row + 0x2C..row + 0x30].copy_from_slice(&0xBEEF_0007_u32.to_le_bytes());
    definitions[232..240].copy_from_slice(&1_u64.to_le_bytes());
    definitions[240..244].copy_from_slice(&PRESENTATION_NODE_INDEX_ROW_CLASS.to_le_bytes());
    definitions[248..250].copy_from_slice(&1_u16.to_le_bytes());

    let paths = collectible_item_paths_from_definitions(&definitions, &nodes).unwrap();

    assert_eq!(
        paths.get(&7),
        Some(&vec![vec![
            "Majestic Solstice Suit".to_owned(),
            "Items".to_owned(),
        ]])
    );
    assert!(!paths.contains_key(&0xBEEF_0007));
}

#[test]
fn milestone_objectives_include_the_primary_and_each_authored_phase_pair() {
    let mut row = vec![0xFF_u8; MILESTONE_DEFINITION_ROW_SIZE];
    row[MILESTONE_PRIMARY_OBJECTIVE_INDEX_OFFSET..MILESTONE_PRIMARY_OBJECTIVE_INDEX_OFFSET + 2]
        .copy_from_slice(&835_u16.to_le_bytes());
    row[MILESTONE_PHASE_COUNT_OFFSET..MILESTONE_PHASE_COUNT_OFFSET + 4]
        .copy_from_slice(&3_u32.to_le_bytes());
    for (offset, objective_index) in [836_u16, 839, 837, 840, 838, 841].into_iter().enumerate() {
        let field = MILESTONE_PHASE_OBJECTIVES_OFFSET + offset * 2;
        row[field..field + 2].copy_from_slice(&objective_index.to_le_bytes());
    }

    assert_eq!(
        milestone_objective_indices(&row, 0, 1_000).unwrap(),
        vec![835, 836, 839, 837, 840, 838, 841]
    );
}

#[test]
fn milestone_objectives_reject_an_impossible_phase_count() {
    let mut row = vec![0_u8; MILESTONE_DEFINITION_ROW_SIZE];
    row[MILESTONE_PHASE_COUNT_OFFSET..MILESTONE_PHASE_COUNT_OFFSET + 4]
        .copy_from_slice(&7_u32.to_le_bytes());

    assert!(
        milestone_objective_indices(&row, 0, 1_000)
            .unwrap_err()
            .contains("expected at most 6")
    );
}

#[test]
fn collectible_item_paths_reject_an_unexpected_table_layout() {
    let mut definitions = vec![0_u8; 48];
    definitions[8..16].copy_from_slice(&0_u64.to_le_bytes());
    definitions[16..24].copy_from_slice(&16_i64.to_le_bytes());
    definitions[32..40].copy_from_slice(&0_u64.to_le_bytes());
    definitions[40..44].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());

    let error = collectible_item_paths_from_definitions(&definitions, &[]).unwrap_err();

    assert!(error.contains("unexpected row class 0xDEADBEEF"));
}
