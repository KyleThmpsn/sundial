use super::*;

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
    definition[rows + 2..rows + 8].fill(0x5A);

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
        definition[rows + 2..rows + 8]
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
    for (perks, expected) in [
        (vec![85_u16], vec![451_u16]),
        (vec![462_u16, 463, 464], vec![462_u16, 463, 464, 451]),
    ] {
        let mut definition = synthetic_weapon_definition(
            WeaponInventorySlot::Energy,
            WeaponDamageDescriptor::Elemental(ModernDamageType::Arc),
        );
        let mut strings =
            synthetic_item_strings(WeaponDamageDescriptor::Elemental(ModernDamageType::Arc));
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

#[expect(
    clippy::cognitive_complexity,
    reason = "One matrix walks every slot and damage pairing through the same override call"
)]
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
    // Stock Trust's shape. Authoring any slot makes both fields agree.
    for slot in [
        WeaponInventorySlot::Kinetic,
        WeaponInventorySlot::Energy,
        WeaponInventorySlot::Power,
    ] {
        let mut authored = mismatched.clone();
        set_weapon_inventory_slot(&mut authored, slot).unwrap();
        assert_eq!(weapon_inventory_slot(&authored).unwrap(), slot);
        assert_eq!(weapon_equipment_slot(&authored).unwrap(), slot);
    }

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
