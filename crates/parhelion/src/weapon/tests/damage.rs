use super::*;

#[test]
fn materializes_solar_damage_for_a_kinetic_donor_moved_to_energy() {
    let mut definition =
        synthetic_weapon_definition(WeaponInventorySlot::Kinetic, WeaponDamageDescriptor::Empty);
    let mut strings = synthetic_item_strings(WeaponDamageDescriptor::Empty);
    let original_len = definition.len();
    let original_string_len = strings.len();
    let string_template = synthetic_sandbox_perk_string_template();
    let overrides = WeaponCloneOverrides {
        inventory_slot: Some(WeaponInventorySlot::Energy),
        modern_damage_type: Some(ModernDamageType::Solar),
        ..WeaponCloneOverrides::default()
    };

    apply_weapon_slot_and_damage_overrides(
        &mut definition,
        &mut strings,
        &overrides,
        Some(&ResolvedDamageCarrierSource {
            family: WeaponDamageCarrierFamily::ModernFixed,
            topology_definition: None,
        }),
        &synthetic_sandbox_perk_definition_template(),
        &string_template,
    )
    .expect("canonical Kinetic/empty donors should author Energy/Solar");

    assert_eq!(
        weapon_inventory_slot(&definition).unwrap(),
        WeaponInventorySlot::Energy
    );
    assert_eq!(
        weapon_damage_descriptor(&definition).unwrap(),
        WeaponDamageDescriptor::Elemental(ModernDamageType::Solar)
    );
    assert_eq!(definition.len(), original_len + 56);
    let investment = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let descriptor = investment + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
    let (count, header, rows, class) = array_at(&definition, descriptor).unwrap();
    assert_eq!(count, 1);
    assert_eq!(header, original_len + 16);
    assert_eq!(
        &definition[header - 8..header],
        NESTED_ARRAY_TRAILER.as_slice()
    );
    assert_eq!(class, ITEM_SANDBOX_PERK_ROW_CLASS);
    assert_eq!(
        read_u16(&definition, rows).unwrap(),
        MODERN_SOLAR_DAMAGE_PERK_INDEX
    );
    assert_eq!(
        &definition[rows + 2..rows + ITEM_SANDBOX_PERK_ROW_SIZE],
        &synthetic_sandbox_perk_definition_template()[2..]
    );
    assert_eq!(rows + ITEM_SANDBOX_PERK_ROW_SIZE, definition.len());
    assert_eq!(strings.len(), original_string_len + string_template.len());
    assert_eq!(item_string_sandbox_perk_count(&strings).unwrap(), 1);
    validate_weapon_sandbox_perk_parallelism(&definition, &strings, &string_template).unwrap();
}

#[test]
fn recognized_modern_damage_override_stays_in_place() {
    let mut definition = synthetic_weapon_definition(
        WeaponInventorySlot::Energy,
        WeaponDamageDescriptor::Elemental(ModernDamageType::Arc),
    );
    let original_len = definition.len();
    let investment = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(
        &definition,
        investment + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET,
    )
    .unwrap();
    definition[rows + 2..rows + ITEM_SANDBOX_PERK_ROW_SIZE].fill(0x5A);

    set_weapon_fixed_damage_type(
        &mut definition,
        ModernDamageType::Solar,
        WeaponDamageCarrierFamily::ModernFixed,
        &synthetic_sandbox_perk_definition_template(),
    )
    .expect("one recognized modern row should remain replaceable");

    assert_eq!(definition.len(), original_len);
    assert_eq!(
        read_u16(&definition, rows).unwrap(),
        MODERN_SOLAR_DAMAGE_PERK_INDEX
    );
    assert!(
        definition[rows + 2..rows + ITEM_SANDBOX_PERK_ROW_SIZE]
            .iter()
            .all(|byte| *byte == 0x5A)
    );
}

#[test]
fn recognized_legacy_damage_override_preserves_legacy_family() {
    let mut definition = synthetic_weapon_definition(
        WeaponInventorySlot::Energy,
        WeaponDamageDescriptor::Elemental(ModernDamageType::Arc),
    );
    let mut strings =
        synthetic_item_strings(WeaponDamageDescriptor::Elemental(ModernDamageType::Arc));
    let investment = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(
        &definition,
        investment + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET,
    )
    .unwrap();
    write_u16(&mut definition, rows, LEGACY_ARC_DAMAGE_PERK_INDEX).unwrap();

    apply_weapon_slot_and_damage_overrides(
        &mut definition,
        &mut strings,
        &WeaponCloneOverrides {
            modern_damage_type: Some(ModernDamageType::Solar),
            ..WeaponCloneOverrides::default()
        },
        None,
        &synthetic_sandbox_perk_definition_template(),
        &synthetic_sandbox_perk_string_template(),
    )
    .expect("legacy fixed carriers should mutate in place");

    assert_eq!(
        weapon_damage_carrier(&definition).unwrap(),
        WeaponDamageCarrier::Fixed {
            family: WeaponDamageCarrierFamily::LegacyFixed,
            damage_type: ModernDamageType::Solar,
        }
    );
    assert_eq!(
        weapon_sandbox_perks(&definition).unwrap(),
        [LEGACY_SOLAR_DAMAGE_PERK_INDEX]
    );
}

#[test]
fn plug_driven_damage_override_changes_only_the_existing_carrier() {
    let mut definition =
        synthetic_weapon_definition(WeaponInventorySlot::Energy, WeaponDamageDescriptor::Empty);
    let mut strings = synthetic_item_strings(WeaponDamageDescriptor::Empty);
    let header = definition.len();
    definition.resize(header + 16 + ITEM_ORDINARY_SOCKET_ROW_SIZE, 0);
    let descriptor = relative_target(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    write_u64(&mut definition, descriptor, 1).unwrap();
    write_relative_pointer(&mut definition, descriptor + 8, header).unwrap();
    write_u64(&mut definition, header, 1).unwrap();
    write_u32(&mut definition, header + 8, ITEM_ORDINARY_SOCKET_ROW_CLASS).unwrap();
    let row = header + 16;
    write_u16(&mut definition, row, ELEMENTAL_DAMAGE_SOCKET_TYPE).unwrap();
    write_u16(
        &mut definition,
        row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
        ARC_DAMAGE_PLUG_ITEM_INDEX,
    )
    .unwrap();

    apply_weapon_slot_and_damage_overrides(
        &mut definition,
        &mut strings,
        &WeaponCloneOverrides {
            modern_damage_type: Some(ModernDamageType::Void),
            ..WeaponCloneOverrides::default()
        },
        None,
        &synthetic_sandbox_perk_definition_template(),
        &synthetic_sandbox_perk_string_template(),
    )
    .expect("an existing type-68 carrier should change element in place");

    assert_eq!(
        weapon_damage_carrier(&definition).unwrap(),
        WeaponDamageCarrier::PlugDriven {
            damage_type: ModernDamageType::Void,
            lane: 0,
        }
    );
    assert!(weapon_sandbox_perks(&definition).unwrap().is_empty());
    assert_eq!(
        read_u16(&definition, row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET).unwrap(),
        VOID_DAMAGE_PLUG_ITEM_INDEX
    );
    assert_eq!(item_string_sandbox_perk_count(&strings).unwrap(), 0);
}

#[test]
fn modern_void_authors_energy_and_power_donors() {
    let string_template = synthetic_sandbox_perk_string_template();
    for (slot, donor_damage) in [
        (WeaponInventorySlot::Energy, ModernDamageType::Arc),
        (WeaponInventorySlot::Power, ModernDamageType::Solar),
    ] {
        let mut definition =
            synthetic_weapon_definition(slot, WeaponDamageDescriptor::Elemental(donor_damage));
        let mut strings = synthetic_item_strings(WeaponDamageDescriptor::Elemental(donor_damage));
        apply_weapon_slot_and_damage_overrides(
            &mut definition,
            &mut strings,
            &WeaponCloneOverrides {
                modern_damage_type: Some(ModernDamageType::Void),
                ..WeaponCloneOverrides::default()
            },
            None,
            &synthetic_sandbox_perk_definition_template(),
            &string_template,
        )
        .expect("modern Energy and Power donors should author package-proved Void");
        assert_eq!(
            weapon_damage_descriptor(&definition).unwrap(),
            WeaponDamageDescriptor::Elemental(ModernDamageType::Void)
        );
        assert_eq!(weapon_sandbox_perks(&definition).unwrap(), [451]);
        assert_eq!(item_string_sandbox_perk_count(&strings).unwrap(), 1);
    }
}

#[test]
fn non_damage_base_perks_are_preserved_while_authoring_damage() {
    for perks in [vec![85_u16], vec![462_u16, 463, 464]] {
        let mut definition = synthetic_weapon_definition(
            WeaponInventorySlot::Energy,
            WeaponDamageDescriptor::Elemental(ModernDamageType::Arc),
        );
        let mut strings =
            synthetic_item_strings(WeaponDamageDescriptor::Elemental(ModernDamageType::Arc));
        let mut expected = perks
            .iter()
            .copied()
            .filter(|perk| fixed_damage_perk(*perk).is_none())
            .collect::<Vec<_>>();
        expected.push(MODERN_VOID_DAMAGE_PERK_INDEX);
        apply_weapon_slot_and_damage_overrides(
            &mut definition,
            &mut strings,
            &WeaponCloneOverrides {
                base_sandbox_perks: Some(perks),
                modern_damage_type: Some(ModernDamageType::Void),
                ..WeaponCloneOverrides::default()
            },
            None,
            &synthetic_sandbox_perk_definition_template(),
            &synthetic_sandbox_perk_string_template(),
        )
        .expect("non-damage base perks should coexist with an authored modern damage marker");
        assert_eq!(weapon_sandbox_perks(&definition).unwrap(), expected);
        assert_eq!(
            weapon_damage_descriptor(&definition).unwrap(),
            WeaponDamageDescriptor::Elemental(ModernDamageType::Void)
        );
        assert_eq!(
            item_string_sandbox_perk_count(&strings).unwrap(),
            expected.len()
        );
    }
}

#[test]
fn combat_profile_creates_native_element_without_a_manual_base_perk_override() {
    for (element, perk) in [
        (ModernDamageType::Arc, 449),
        (ModernDamageType::Solar, 450),
        (ModernDamageType::Void, 451),
    ] {
        let mut definition = synthetic_weapon_definition(
            WeaponInventorySlot::Kinetic,
            WeaponDamageDescriptor::Empty,
        );
        let mut strings = synthetic_item_strings(WeaponDamageDescriptor::Empty);
        // The workbench's Combat profile selection supplies these fields. No manual
        // base-perk list or raw patch should be required to get actual elemental damage.
        let overrides = WeaponCloneOverrides {
            inventory_slot: Some(WeaponInventorySlot::Energy),
            modern_damage_type: Some(element),
            ..Default::default()
        };
        assert!(overrides.base_sandbox_perks.is_none());
        apply_weapon_slot_and_damage_overrides(
            &mut definition,
            &mut strings,
            &overrides,
            Some(&ResolvedDamageCarrierSource {
                family: WeaponDamageCarrierFamily::ModernFixed,
                topology_definition: None,
            }),
            &synthetic_sandbox_perk_definition_template(),
            &synthetic_sandbox_perk_string_template(),
        )
        .unwrap();
        assert_eq!(weapon_sandbox_perks(&definition).unwrap(), vec![perk]);
        assert_eq!(item_string_sandbox_perk_count(&strings).unwrap(), 1);
        assert_eq!(
            weapon_inventory_slot(&definition).unwrap(),
            WeaponInventorySlot::Energy
        );
        let resource = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
        let (_, header, _, _) =
            array_at(&definition, resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET).unwrap();
        assert_eq!(
            &definition[header - 8..header],
            NESTED_ARRAY_TRAILER.as_slice()
        );
    }
}

#[test]
fn appended_base_perks_have_the_native_nested_array_marker() {
    let mut definition =
        synthetic_weapon_definition(WeaponInventorySlot::Kinetic, WeaponDamageDescriptor::Empty);
    let template = synthetic_sandbox_perk_definition_template();
    for perks in [vec![MODERN_ARC_DAMAGE_PERK_INDEX], vec![449, 462, 463]] {
        set_weapon_base_sandbox_perks(&mut definition, &perks, &template).unwrap();
        let resource = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
        let descriptor = resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
        let (_, header, _, _) = array_at(&definition, descriptor).unwrap();
        assert_eq!(
            &definition[header - 8..header],
            NESTED_ARRAY_TRAILER.as_slice()
        );
        assert_eq!(weapon_sandbox_perks(&definition).unwrap(), perks);
    }
}

#[test]
fn independent_slot_and_damage_overrides_preserve_native_structure() {
    for source_slot in [
        WeaponInventorySlot::Kinetic,
        WeaponInventorySlot::Energy,
        WeaponInventorySlot::Power,
    ] {
        for source_damage in [
            WeaponDamageDescriptor::Empty,
            WeaponDamageDescriptor::Elemental(ModernDamageType::Arc),
        ] {
            for target_slot in [
                WeaponInventorySlot::Kinetic,
                WeaponInventorySlot::Energy,
                WeaponInventorySlot::Power,
            ] {
                for damage in [
                    None,
                    Some(ModernDamageType::Kinetic),
                    Some(ModernDamageType::Arc),
                    Some(ModernDamageType::Solar),
                    Some(ModernDamageType::Void),
                ] {
                    let mut definition = synthetic_weapon_definition(source_slot, source_damage);
                    let mut strings = synthetic_item_strings(source_damage);
                    let overrides = WeaponCloneOverrides {
                        inventory_slot: Some(target_slot),
                        modern_damage_type: damage,
                        ..Default::default()
                    };
                    let source = ResolvedDamageCarrierSource {
                        family: WeaponDamageCarrierFamily::ModernFixed,
                        topology_definition: None,
                    };
                    apply_weapon_slot_and_damage_overrides(
                        &mut definition,
                        &mut strings,
                        &overrides,
                        Some(&source),
                        &synthetic_sandbox_perk_definition_template(),
                        &synthetic_sandbox_perk_string_template(),
                    )
                    .unwrap();
                    assert_eq!(weapon_inventory_slot(&definition).unwrap(), target_slot);
                    assert_eq!(weapon_equipment_slot(&definition).unwrap(), target_slot);
                    let expected = match damage {
                        None => source_damage,
                        Some(ModernDamageType::Kinetic) => WeaponDamageDescriptor::Empty,
                        Some(element) => WeaponDamageDescriptor::Elemental(element),
                    };
                    assert_eq!(weapon_damage_descriptor(&definition).unwrap(), expected);
                    assert_eq!(
                        item_string_sandbox_perk_count(&strings).unwrap(),
                        weapon_sandbox_perks(&definition).unwrap().len()
                    );
                }
            }
        }
    }

    let mut mismatched =
        synthetic_weapon_definition(WeaponInventorySlot::Kinetic, WeaponDamageDescriptor::Empty);
    mismatched[ITEM_INVENTORY_SLOT_OFFSET] = WeaponInventorySlot::Energy.root_value();
    assert_eq!(
        weapon_inventory_slot(&mismatched).unwrap(),
        WeaponInventorySlot::Energy
    );
    assert_eq!(
        weapon_equipment_slot(&mismatched).unwrap(),
        WeaponInventorySlot::Kinetic
    );
    assert!(set_weapon_inventory_slot(&mut mismatched, WeaponInventorySlot::Energy).is_err());

    let mut missing_sentinel =
        synthetic_weapon_definition(WeaponInventorySlot::Kinetic, WeaponDamageDescriptor::Empty);
    let block = relative_target(&missing_sentinel, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET).unwrap();
    write_u16(
        &mut missing_sentinel,
        block + ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET,
        0,
    )
    .unwrap();
    assert_eq!(
        weapon_inventory_slot(&missing_sentinel).unwrap(),
        WeaponInventorySlot::Kinetic
    );
    assert!(weapon_equipment_slot(&missing_sentinel).is_err());
}
