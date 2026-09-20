use super::*;

fn companion_rows(strings: &[u8]) -> usize {
    array_at(
        strings,
        item_string_sandbox_perk_descriptor(strings).unwrap(),
    )
    .unwrap()
    .2
}

fn conditional_strings() -> (Vec<u8>, usize) {
    let mut strings = synthetic_item_strings(WeaponDamageDescriptor::Empty);
    set_item_string_sandbox_perk_count(&mut strings, 1, &synthetic_sandbox_perk_string_template())
        .unwrap();
    let row = companion_rows(&strings);
    // Fallen Repurposing's native condition: flag 10209, followed by opcode 2.
    strings.extend_from_slice(&[0; 8]);
    strings.extend_from_slice(&NESTED_ARRAY_TRAILER);
    let header = strings.len();
    strings.extend_from_slice(&2_u64.to_le_bytes());
    strings.extend_from_slice(&0x8080_7D31_u32.to_le_bytes());
    strings.extend_from_slice(&0_u32.to_le_bytes());
    strings.extend_from_slice(&[1, 0, 0, 0, 0xE1, 0x27, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]);
    write_u64(&mut strings, row + 8, 2).unwrap();
    write_relative_pointer(&mut strings, row + 16, header).unwrap();
    write_u32(&mut strings, row + 24, 1).unwrap();
    (strings, header)
}

#[test]
fn conditional_companions_survive_growth_and_same_count_updates() {
    let (mut strings, condition) = conditional_strings();
    let template = synthetic_sandbox_perk_string_template();
    let original = strings.clone();
    set_item_string_sandbox_perk_count(&mut strings, 1, &template).unwrap();
    assert_eq!(strings, original);
    set_item_string_sandbox_perk_count(&mut strings, 3, &template).unwrap();
    let row = companion_rows(&strings);
    assert_eq!(relative_target(&strings, row + 16).unwrap(), condition);
    assert!(read_i64(&strings, row + 16).unwrap() < 0);
    assert_eq!(
        &strings[condition..condition + 32],
        &original[condition..condition + 32]
    );
    assert_eq!(read_u32(&strings, row + 24).unwrap(), 1);
    assert_eq!(&strings[row + 32..row + 64], &template[32..]);
    assert_eq!(&strings[row + 64..row + 96], &template[32..]);
    set_item_string_sandbox_perk_count(&mut strings, 1, &template).unwrap();
    assert_eq!(
        relative_target(&strings, companion_rows(&strings) + 16).unwrap(),
        condition
    );
}

#[test]
fn companion_rows_follow_retained_perks_when_reordered_or_replaced() {
    let (mut strings, condition) = conditional_strings();
    let template = synthetic_sandbox_perk_string_template();
    let mut definition =
        synthetic_weapon_definition(WeaponInventorySlot::Kinetic, WeaponDamageDescriptor::Empty);
    let definition_template = synthetic_sandbox_perk_definition_template();
    set_weapon_base_sandbox_perks(&mut definition, &[42], &definition_template).unwrap();
    set_weapon_base_sandbox_perks_with_strings(
        &mut definition,
        &mut strings,
        &[73, 42],
        &definition_template,
        &template,
    )
    .unwrap();
    let row = companion_rows(&strings);
    assert_eq!(&strings[row..row + 32], &template[32..]);
    assert_eq!(relative_target(&strings, row + 32 + 16).unwrap(), condition);
    set_weapon_base_sandbox_perks_with_strings(
        &mut definition,
        &mut strings,
        &[42, 73],
        &definition_template,
        &template,
    )
    .unwrap();
    assert_eq!(
        relative_target(&strings, companion_rows(&strings) + 16).unwrap(),
        condition
    );
    set_weapon_base_sandbox_perks_with_strings(
        &mut definition,
        &mut strings,
        &[91, 92],
        &definition_template,
        &template,
    )
    .unwrap();
    assert_eq!(
        &item_string_sandbox_perk_segment(&strings).unwrap().unwrap()[32..64],
        &template[32..]
    );
    assert_eq!(
        &item_string_sandbox_perk_segment(&strings).unwrap().unwrap()[64..96],
        &template[32..]
    );
}

#[test]
fn companion_localization_and_presentation_values_are_preserved() {
    let template = synthetic_sandbox_perk_string_template();
    for presentation in [0, 1, 2] {
        let (mut strings, _) = conditional_strings();
        let row = companion_rows(&strings);
        write_u16(&mut strings, row, 2385).unwrap();
        write_u32(&mut strings, row + 4, 0xDEBA_2156).unwrap();
        write_u32(&mut strings, row + 24, presentation).unwrap();
        set_item_string_sandbox_perk_count(&mut strings, 2, &template).unwrap();
        let row = companion_rows(&strings);
        assert_eq!(read_u16(&strings, row).unwrap(), 2385);
        assert_eq!(read_u32(&strings, row + 4).unwrap(), 0xDEBA_2156);
        assert_eq!(read_u32(&strings, row + 24).unwrap(), presentation);
    }
}

#[test]
fn malformed_companion_conditions_are_rejected_before_mutation() {
    let (strings, header) = conditional_strings();
    let row = companion_rows(&strings);
    for offset in [
        row + 2,
        row + 8,
        row + 28,
        header - 8,
        header + 8,
        header + 12,
    ] {
        let mut bad = strings.clone();
        bad[offset] ^= 1;
        let before = bad.clone();
        assert!(
            set_item_string_sandbox_perk_count(
                &mut bad,
                2,
                &synthetic_sandbox_perk_string_template()
            )
            .is_err(),
            "offset {offset}"
        );
        assert_eq!(bad, before);
    }
    let mut bad = strings.clone();
    write_i64(&mut bad, row + 16, i64::MAX).unwrap();
    assert!(item_string_sandbox_perk_count(&bad).is_err());
    let mut bad = strings.clone();
    write_u64(&mut bad, row + 8, u64::MAX).unwrap();
    write_u64(&mut bad, header, u64::MAX).unwrap();
    assert!(item_string_sandbox_perk_count(&bad).is_err());
    assert!(item_string_sandbox_perk_count(&strings[..strings.len() - 1]).is_err());
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn stock_companion_layouts_preserve_conditions_when_extended() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let sources = sources::load_project_sources(&packages).unwrap();
    let template =
        canonical_item_sandbox_perk_string_template(&sources.manager, &sources.stock_item_strings)
            .unwrap();
    let mut checked = 0;
    let mut conditional = 0;
    let mut found_fallen = false;
    for index in 0..sources.stock_item_count {
        let table_row = sources.string_rows + index * ITEM_ROW_SIZE;
        let hash = read_u32(&sources.stock_item_strings, table_row).unwrap();
        let tag = TagHash(read_u32(&sources.stock_item_strings, table_row + 16).unwrap());
        let mut strings = sources.manager.read_tag(tag).unwrap();
        if item_string_sandbox_perk_descriptor(&strings).is_err() {
            continue;
        }
        let count = item_string_sandbox_perk_count(&strings)
            .unwrap_or_else(|error| panic!("{hash:08X}: {error}"));
        if count == 0 {
            continue;
        }
        let row = companion_rows(&strings);
        let original = strings.clone();
        let pointers = (0..count)
            .map(|i| {
                let pointer = row + i * 32 + 16;
                (read_i64(&strings, pointer).unwrap() != 0)
                    .then(|| relative_target(&strings, pointer).unwrap())
            })
            .collect::<Vec<_>>();
        conditional += pointers.iter().filter(|p| p.is_some()).count();
        if hash == 0x87AE_999A {
            found_fallen = true;
            assert_eq!(pointers.len(), 1);
            assert!(pointers[0].is_some());
        }
        set_item_string_sandbox_perk_count(&mut strings, count + 1, &template).unwrap();
        assert_relocated_companions(&original, &strings);
        checked += 1;
    }
    assert!(found_fallen);
    assert!(conditional > 100);
    eprintln!("Verified {checked} stock companions, including {conditional} condition arrays");
}

#[test]
fn removing_damage_marker_retains_the_other_effects_condition() {
    let (mut strings, condition) = conditional_strings();
    let template = synthetic_sandbox_perk_string_template();
    let definition_template = synthetic_sandbox_perk_definition_template();
    let mut definition = synthetic_weapon_definition(
        WeaponInventorySlot::Energy,
        WeaponDamageDescriptor::Elemental(ModernDamageType::Arc),
    );
    remap_item_string_sandbox_perks(
        &mut strings,
        &[42],
        &[MODERN_ARC_DAMAGE_PERK_INDEX, 42],
        &template,
    )
    .unwrap();
    set_weapon_base_sandbox_perks(
        &mut definition,
        &[MODERN_ARC_DAMAGE_PERK_INDEX, 42],
        &definition_template,
    )
    .unwrap();
    apply_weapon_slot_and_damage_overrides(
        &mut definition,
        &mut strings,
        &WeaponCloneOverrides {
            modern_damage_type: Some(ModernDamageType::Kinetic),
            ..Default::default()
        },
        None,
        &definition_template,
        &template,
    )
    .unwrap();
    assert_eq!(weapon_sandbox_perks(&definition).unwrap(), vec![42]);
    assert_eq!(item_string_sandbox_perk_count(&strings).unwrap(), 1);
    assert_eq!(
        relative_target(&strings, companion_rows(&strings) + 16).unwrap(),
        condition
    );
}

fn assert_relocated_companions(original: &[u8], strings: &[u8]) {
    let row = companion_rows(original);
    let moved = companion_rows(strings);
    for i in 0..item_string_sandbox_perk_count(original).unwrap() {
        let old = row + i * 32;
        let new = moved + i * 32;
        assert_eq!(&strings[new..new + 16], &original[old..old + 16]);
        assert_eq!(&strings[new + 24..new + 32], &original[old + 24..old + 32]);
        if read_i64(original, old + 16).unwrap() != 0 {
            let target = relative_target(original, old + 16).unwrap();
            assert_eq!(relative_target(strings, new + 16).unwrap(), target);
            assert_eq!(&strings[target..original.len()], &original[target..]);
        } else {
            assert_eq!(read_i64(strings, new + 16).unwrap(), 0);
        }
    }
}

fn definition_rows(definition: &[u8]) -> usize {
    let resource = relative_target(definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    array_at(definition, resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET)
        .unwrap()
        .2
}

#[test]
fn gameplay_conditions_survive_growth_reordering_and_removal() {
    let mut definition =
        synthetic_weapon_definition(WeaponInventorySlot::Kinetic, WeaponDamageDescriptor::Empty);
    let template = synthetic_sandbox_perk_definition_template();
    set_weapon_base_sandbox_perks(&mut definition, &[42, 73], &template).unwrap();
    let row = definition_rows(&definition);
    definition.extend_from_slice(&[0; 8]);
    definition.extend_from_slice(&NESTED_ARRAY_TRAILER);
    let header = definition.len();
    definition.extend_from_slice(&1_u64.to_le_bytes());
    definition.extend_from_slice(&0x8080_7D31_u32.to_le_bytes());
    definition.extend_from_slice(&0_u32.to_le_bytes());
    definition.extend_from_slice(&[1, 0, 0, 0, 0xE1, 0x27, 0, 0]);
    write_u32(&mut definition, row + 4, 0x1234_5678).unwrap();
    write_u64(&mut definition, row + 8, 1).unwrap();
    write_relative_pointer(&mut definition, row + 16, header).unwrap();
    let original = definition.clone();
    for perks in [[91, 73, 42], [42, 91, 73]] {
        set_weapon_base_sandbox_perks(&mut definition, &perks, &template).unwrap();
        let index = perks.iter().position(|perk| *perk == 42).unwrap();
        let row = definition_rows(&definition) + index * ITEM_SANDBOX_PERK_ROW_SIZE;
        assert_eq!(read_u32(&definition, row + 4).unwrap(), 0x1234_5678);
        assert_eq!(relative_target(&definition, row + 16).unwrap(), header);
        assert_eq!(
            &definition[header..header + 24],
            &original[header..header + 24]
        );
        weapon_sandbox_perk_rows(&definition).unwrap();
    }
    let before = definition.clone();
    let pointer = definition_rows(&definition) + 16;
    write_i64(&mut definition, pointer, i64::MAX).unwrap();
    let malformed = definition.clone();
    assert!(set_weapon_base_sandbox_perks(&mut definition, &[42, 91, 73, 82], &template).is_err());
    assert_eq!(definition, malformed);
    definition = before;
    set_weapon_base_sandbox_perks(&mut definition, &[91, 73], &template).unwrap();
    assert!(
        weapon_sandbox_perk_rows(&definition)
            .unwrap()
            .iter()
            .all(|row| row[8..].iter().all(|byte| *byte == 0))
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn stock_gameplay_companion_conditions_survive_growth() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let sources = sources::load_project_sources(&packages).unwrap();
    let template =
        canonical_weapon_sandbox_perk_row_template(&sources.manager, &sources.stock_item_table)
            .unwrap();
    let mut checked = 0;
    let mut conditional = 0;
    for index in 0..sources.stock_item_count {
        let tag = TagHash(
            read_u32(
                &sources.stock_item_table,
                sources.item_rows + index * ITEM_ROW_SIZE + 16,
            )
            .unwrap(),
        );
        let mut definition = sources.manager.read_tag(tag).unwrap();
        let Ok(mut perks) = weapon_sandbox_perks(&definition) else {
            continue;
        };
        if perks.is_empty() {
            continue;
        }
        let original = definition.clone();
        let rows = weapon_sandbox_perk_rows(&original).unwrap();
        let start = definition_rows(&original);
        let targets = (0..rows.len())
            .map(|i| {
                let pointer = start + i * ITEM_SANDBOX_PERK_ROW_SIZE + 16;
                (read_i64(&original, pointer).unwrap() != 0)
                    .then(|| relative_target(&original, pointer).unwrap())
            })
            .collect::<Vec<_>>();
        conditional += targets.iter().filter(|target| target.is_some()).count();
        let extra = (0..u16::MAX).find(|perk| !perks.contains(perk)).unwrap();
        perks.push(extra);
        set_weapon_base_sandbox_perks(&mut definition, &perks, &template).unwrap();
        let moved = definition_rows(&definition);
        for (i, target) in targets.into_iter().enumerate() {
            let row = moved + i * ITEM_SANDBOX_PERK_ROW_SIZE;
            assert_eq!(&definition[row..row + 16], &rows[i][..16]);
            if let Some(target) = target {
                assert_eq!(relative_target(&definition, row + 16).unwrap(), target);
                assert_eq!(&definition[target..original.len()], &original[target..]);
            }
        }
        weapon_sandbox_perk_rows(&definition).unwrap();
        checked += 1;
    }
    assert!(conditional > 0);
    eprintln!("Verified {checked} gameplay perk arrays, including {conditional} condition arrays");
}
