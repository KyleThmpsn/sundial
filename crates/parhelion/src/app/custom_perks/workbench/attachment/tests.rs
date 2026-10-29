use super::*;
use sundial::investment::WeaponSocket;

fn donor() -> WeaponDonor {
    WeaponDonor {
        summary: crate::test_support::donor_summary(1, "Workbench Test", "Auto Rifle"),
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
            ordered_randomized_choices: vec![],
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
fn full_columns_do_not_offer_an_out_of_range_destination() {
    let donor = donor();
    let mut weapon = weapon(&donor, 1);
    let limit = u16::MAX as usize;
    weapon.overrides.socket_columns[0].as_mut().unwrap().choices =
        (1..=limit).map(|hash| HexHash::new(hash as u32)).collect();
    weapon.overrides.socket_plug_variants.clear();
    let destinations = targets(&weapon, &donor, true);
    assert_eq!(destinations.len(), limit);
    assert_eq!(destinations.last().unwrap().choice, limit - 1);
    assert!(Target::capture(&weapon, &donor, 0, limit).is_err());
}
