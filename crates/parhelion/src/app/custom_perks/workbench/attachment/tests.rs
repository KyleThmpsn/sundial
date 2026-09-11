use super::*;
use sundial::investment::{WeaponDamageProfile, WeaponRarity, WeaponSocket};

mod native;

fn donor() -> WeaponDonor {
    WeaponDonor {
        summary: WeaponDonorSummary {
            hash: 1,
            name: "Workbench Test".into(),
            type_name: "Auto Rifle".into(),
            bucket_hash: 0,
            collection_backed: true,
            power_cap: None,
            damage_type: None,
            inventory_slot: None,
            ammo_type: None,
            weapon_pattern_index: None,
            weapon_translation_group: None,
            stat_group_index: None,
            damage_profile: WeaponDamageProfile::Unknown,
            rarity: WeaponRarity::Legendary,
        },
        power_cap_groups: vec![],
        equipment_slot: None,
        sockets: vec![WeaponSocket {
            index: 0,
            socket_type: 92,
            label: "1. Trait".into(),
            native_default: Some(crate::perk::DEFAULT_PLUG_LAYOUT),
            ordered_embedded_choices: vec![crate::perk::DEFAULT_PLUG_LAYOUT],
            max_authored_choices: authored_socket_choice_limit(92),
            compatible_plug_count: 1,
            reusable_plug_set_index: None,
            randomized_plug_set_index: None,
        }],
        investment_stats: vec![],
        addable_investment_stats: vec![],
        base_sandbox_perks: vec![],
        trait_indices: vec![],
        max_stack_size: None,
        socket_entry_list_index: None,
        plug_category_hash: None,
        roll_set_index: None,
        linked_plug_index: None,
        linked_plug_hash: None,
        art_arrangements: vec![],
        render_dye_rows: [vec![], vec![], vec![]],
    }
}

fn weapon(donor: &WeaponDonor, count: u16) -> WeaponRecipe {
    let mut weapon = WeaponRecipe::new_weapon_for_donor(
        "parhelion.workbench-regression",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    let mut perk = PerkRecipe::new();
    perk.effects.push(PerkRecipe::effect(1178));
    weapon.overrides.socket_columns = vec![Some(crate::WeaponSocketColumnRecipe {
        socket_type: Some(92),
        choices: vec![perk.template_plug.clone(); usize::from(count)],
        choice_weight_bits: (0..count)
            .map(|choice| (f32::from(choice) + 1.0).to_bits())
            .collect(),
        ..Default::default()
    })];
    weapon.overrides.socket_plug_variants = (0..count)
        .map(|choice| {
            perk.name = format!("Private Choice {}", choice + 1);
            perk.at_socket(0, choice)
        })
        .collect();
    weapon.validate().unwrap();
    weapon
}

#[test]
fn duplicate_stock_restore_is_rejected_without_changing_either_choice() {
    let donor = donor();
    for choice in [0, 1] {
        let mut weapon = weapon(&donor, 2);
        weapon
            .overrides
            .socket_plug_variants
            .retain(|variant| usize::from(variant.choice_index) == choice);
        weapon.validate().unwrap();
        let before = weapon.clone();
        let target = Target::capture(&weapon, &donor, 0, choice).unwrap();
        let error = Change { target, perk: None }
            .apply(&mut weapon, &donor)
            .unwrap_err();
        assert!(error.contains("already contains the original perk"));
        assert_eq!(weapon, before);
        assert!(weapon.to_json_pretty().is_ok());
    }
}

#[test]
fn restoring_one_private_choice_preserves_sibling_variants_and_socket_metadata() {
    let donor = donor();
    for choice in [0, 1] {
        let mut weapon = weapon(&donor, 2);
        let before = weapon.clone();
        let target = Target::capture(&weapon, &donor, 0, choice).unwrap();
        Change { target, perk: None }
            .apply(&mut weapon, &donor)
            .unwrap();
        assert_eq!(
            weapon.overrides.socket_columns,
            before.overrides.socket_columns
        );
        assert_eq!(
            weapon.overrides.socket_plug_variants,
            vec![before.overrides.socket_plug_variants[1 - choice].clone()]
        );
        assert_eq!(
            WeaponRecipe::from_json_str(&weapon.to_json_pretty().unwrap()).unwrap(),
            weapon
        );
    }
}

#[test]
fn destinations_include_choices_after_twelve_and_the_next_available_choice() {
    let donor = donor();
    let weapon = weapon(&donor, 13);
    let existing = targets(&weapon, &donor, false);
    assert_eq!(existing.len(), 13);
    for (choice, target) in existing.iter().enumerate() {
        assert_eq!(target.choice, choice);
        target.check(&weapon, &donor).unwrap();
    }
    let destinations = targets(&weapon, &donor, true);
    assert_eq!(destinations.len(), 14);
    assert_eq!(destinations.last().unwrap().choice, 13);
    assert!(destinations.last().unwrap().variant.is_none());
}

#[test]
fn full_columns_do_not_offer_an_out_of_range_destination() {
    let donor = donor();
    let mut weapon = weapon(&donor, 1);
    let limit = authored_socket_choice_limit(92);
    weapon.overrides.socket_columns[0].as_mut().unwrap().choices =
        (1..=limit).map(|hash| HexHash::new(hash as u32)).collect();
    weapon.overrides.socket_plug_variants.clear();
    let destinations = targets(&weapon, &donor, true);
    assert_eq!(destinations.len(), limit);
    assert_eq!(destinations.last().unwrap().choice, limit - 1);
    assert!(Target::capture(&weapon, &donor, 0, limit).is_err());
}
