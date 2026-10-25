use super::*;

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
