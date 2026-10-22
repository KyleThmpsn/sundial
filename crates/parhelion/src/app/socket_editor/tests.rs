use super::*;

fn donor() -> WeaponDonor {
    WeaponDonor {
        summary: crate::test_support::donor_summary(1, "Socket Test", "Auto Rifle"),
        power_cap_groups: vec![],
        equipment_slot: None,
        sockets: (0..3)
            .map(|index| sundial::investment::WeaponSocket {
                index,
                socket_type: 92,
                label: format!("{}. Trait", index + 1),
                native_default: Some(10 + index as u32),
                ordered_embedded_choices: vec![10 + index as u32],
                max_authored_choices: authored_socket_choice_limit(92),
                compatible_plug_count: 1,
                reusable_plug_set_index: Some(index as u16),
                randomized_plug_set_index: None,
            })
            .collect(),
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

fn variant(socket_index: u16) -> WeaponSocketPlugVariantRecipe {
    WeaponSocketPlugVariantRecipe {
        projectile: None,
        offer_everywhere: false,
        replace_effects: false,
        socket_index,
        choice_index: 0,
        source_plug_hash: HexHash::new(20),
        name: Some("Saved Perk".into()),
        icon: None,
        classification_donor_hash: None,
        description: None,
        investment_stats: vec![],
        additional_sandbox_perks: vec![],
        sandbox_perks: vec![],
    }
}

#[test]
fn making_a_choice_default_moves_conditions_weights_and_private_data_together() {
    let donor = donor();
    let mut recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.choice-order",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    set_recipe_socket_column(&mut recipe, 3, 0, &[10], vec![20, 21, 22], None);
    let column = recipe.overrides.socket_columns[0].as_mut().unwrap();
    column.choice_weight_bits = vec![1.0_f32.to_bits(), 2.0_f32.to_bits(), 3.0_f32.to_bits()];
    column.choice_conditions = (0..3)
        .map(|operand| {
            vec![WeaponNumericInstructionRecipe {
                opcode: 11,
                operand,
            }]
        })
        .collect();
    recipe.overrides.socket_plug_variants = (0..3)
        .map(|index| {
            let source = WeaponRecipe::from_json_str(include_str!(
                "../../../recipes/redacted.parhelion.json"
            ))
            .unwrap();
            let mut private = source.overrides.socket_plug_variants[0].clone();
            private.choice_index = index;
            private.source_plug_hash = HexHash::new(20 + u32::from(index));
            private.name = Some(format!("Private {index}"));
            private
        })
        .collect();
    let before = recipe.clone();
    make_choice_default(&mut recipe, donor.sockets.len(), 0, &[10], 2).unwrap();
    let column = recipe.overrides.socket_columns[0].as_ref().unwrap();
    assert_eq!(column.choices, vec![22.into(), 20.into(), 21.into()]);
    assert_eq!(
        column.choice_weight_bits,
        vec![3.0_f32.to_bits(), 1.0_f32.to_bits(), 2.0_f32.to_bits()]
    );
    assert_eq!(
        column
            .choice_conditions
            .iter()
            .map(|program| program[0].operand)
            .collect::<Vec<_>>(),
        vec![2, 0, 1]
    );
    for (index, private) in recipe.overrides.socket_plug_variants.iter().enumerate() {
        let mut expected = before.overrides.socket_plug_variants[index].clone();
        expected.choice_index = if index == 2 { 0 } else { index as u16 + 1 };
        assert_eq!(*private, expected);
    }
    let reloaded = WeaponRecipe::from_json_str(&serde_json::to_string(&recipe).unwrap()).unwrap();
    assert_eq!(reloaded, recipe);
    let before_invalid = recipe.clone();
    assert!(make_choice_default(&mut recipe, 3, 0, &[10], 30).is_err());
    assert_eq!(recipe, before_invalid);
}

#[test]
fn making_a_choice_default_rejects_misaligned_metadata_without_mutation() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    set_recipe_socket_column(&mut recipe, 3, 0, &[10], vec![20, 21], None);
    recipe.overrides.socket_columns[0]
        .as_mut()
        .unwrap()
        .choice_conditions = vec![vec![]];
    let before = recipe.clone();
    assert!(make_choice_default(&mut recipe, 3, 0, &[10], 1).is_err());
    assert_eq!(recipe, before);
}

#[test]
fn added_socket_limit_and_last_removal_preserve_original_indices_and_variants() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    assert!(!append_socket(&mut recipe, 3, u16::MAX));
    for _ in 3..sundial::investment::MAX_WEAPON_SOCKETS {
        assert!(append_socket(&mut recipe, 3, 92));
    }
    recipe.overrides.socket_plug_variants = vec![variant(1), variant(10), variant(11)];
    let before = recipe.clone();
    assert!(!append_socket(&mut recipe, 3, 92));
    assert!(!remove_last_added_socket(&mut recipe, 10));
    assert_eq!(recipe, before);
    assert!(remove_last_added_socket(&mut recipe, 11));
    assert_eq!(
        recipe.overrides.socket_columns,
        before.overrides.socket_columns[..11]
    );
    assert_eq!(
        recipe.overrides.socket_plug_variants,
        vec![variant(1), variant(10)]
    );
    assert!(
        recipe.overrides.socket_columns[..3]
            .iter()
            .all(Option::is_none)
    );
}

/// A donor with the two lanes a behavior claims: one intrinsic, then traits.
fn behavior_donor() -> WeaponDonor {
    let mut donor = donor();
    donor.sockets = (0..3)
        .map(|index| sundial::investment::WeaponSocket {
            index,
            socket_type: if index == 0 { 176 } else { 92 },
            label: if index == 0 {
                "1. Intrinsic".to_owned()
            } else {
                format!("{}. Trait", index + 1)
            },
            native_default: Some(10 + index as u32),
            ordered_embedded_choices: vec![10 + index as u32],
            max_authored_choices: authored_socket_choice_limit(if index == 0 { 176 } else { 92 }),
            compatible_plug_count: 1,
            reusable_plug_set_index: Some(index as u16),
            randomized_plug_set_index: None,
        })
        .collect();
    donor
}

/// The saved test document with its socket rows cleared, so its lanes are `behavior_donor`'s
/// three sockets rather than the eleven the document was authored against. Without this the
/// document's own role for lane 1 is Barrel, and the trait lane these tests mean is elsewhere.
fn behavior_recipe() -> WeaponRecipe {
    let mut recipe =
        WeaponRecipe::from_json_str(include_str!("../../../recipes/redacted.parhelion.json"))
            .unwrap();
    recipe.overrides.socket_columns.clear();
    recipe.overrides.socket_plug_variants.clear();
    recipe
}

fn chosen(recipe: &WeaponRecipe, lane: usize) -> Vec<u32> {
    // A recipe that inherits every row keeps no rows at all, which is a release, not a miss.
    recipe
        .overrides
        .socket_columns
        .get(lane)
        .and_then(Option::as_ref)
        .map(|column| {
            column
                .choices
                .iter()
                .filter_map(|hash| hash.parse_u32().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The author gave the behavior's perk a trait socket of their own. Leading the donor's trait
/// column with a second copy showed the same perk twice in game and pushed the choices they had
/// arranged down a place, which is what it did to SUROS Renaissance.
#[test]
fn a_perk_the_author_moved_to_their_own_socket_leaves_the_donors_lane_alone() {
    let donor = behavior_donor();
    let entry = crate::weapon::behavior::behavior("graviton-lance").expect("a catalogued behavior");
    let trait_plug = entry.trait_plug.expect("this behavior pins a trait plug");
    let mut recipe = behavior_recipe();
    let mut pins = BehaviorPins::default();
    for (lane, choices) in [(1, vec![0x1234_5678, 0x8765_4321]), (2, vec![trait_plug])] {
        let inherited = inherited_socket_choices(
            donor.sockets[lane].native_default,
            &donor.sockets[lane].ordered_embedded_choices,
            authored_socket_choice_limit(92),
        );
        set_recipe_socket_column(
            &mut recipe,
            donor.sockets.len(),
            lane,
            &inherited,
            choices,
            None,
        );
    }
    recipe.overrides.additional_behaviors = vec![crate::recipe::AdditionalBehaviorRecipe {
        behavior: entry.id.to_owned(),
    }];

    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);

    assert_eq!(chosen(&recipe, 1), vec![0x1234_5678, 0x8765_4321]);
    assert_eq!(chosen(&recipe, 2), vec![trait_plug]);
}

/// Turning the perks off is the author saying they want the behavior without its plugs, so the
/// sockets have to let go of them too rather than keeping what the build would have skipped.
#[test]
fn turning_off_included_perks_releases_the_sockets_again() {
    let donor = behavior_donor();
    let entry = crate::weapon::behavior::behavior("graviton-lance").expect("a catalogued behavior");
    let mut recipe = behavior_recipe();
    let mut pins = BehaviorPins::default();
    recipe.overrides.additional_behaviors = vec![crate::recipe::AdditionalBehaviorRecipe {
        behavior: entry.id.to_owned(),
    }];
    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);
    let held = chosen(&recipe, 1);
    assert_eq!(held.first().copied(), entry.trait_plug);

    recipe.overrides.skip_behavior_perks = true;
    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);
    assert_ne!(chosen(&recipe, 1).first().copied(), entry.trait_plug);
}

/// Two behaviors share the intrinsic socket. Deselecting either one takes back that one's perk,
/// wherever it sits in the lane, and leaves the other's in place.
#[test]
fn deselecting_one_of_two_intrinsic_behaviors_releases_only_its_own_perk() {
    let donor = behavior_donor();
    let mut entries = crate::weapon::behavior::CATALOG
        .iter()
        .filter(|entry| entry.intrinsic_plug.is_some());
    let first = entries.next().expect("an intrinsic-pinning behavior");
    let second = entries
        .find(|entry| entry.intrinsic_plug != first.intrinsic_plug)
        .expect("a second intrinsic-pinning behavior");
    let (first_plug, second_plug) = (
        first.intrinsic_plug.unwrap(),
        second.intrinsic_plug.unwrap(),
    );
    let mut recipe = behavior_recipe();
    let mut pins = BehaviorPins::default();
    recipe.overrides.additional_behaviors = [first, second]
        .into_iter()
        .map(|entry| crate::recipe::AdditionalBehaviorRecipe {
            behavior: entry.id.to_owned(),
        })
        .collect();
    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);
    let held = chosen(&recipe, 0);
    assert!(
        held.contains(&first_plug) && held.contains(&second_plug),
        "{held:x?}"
    );

    // Deselect the first, whose perk is not at the head of the lane.
    recipe.overrides.additional_behaviors.remove(0);
    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);
    let held = chosen(&recipe, 0);
    assert!(!held.contains(&first_plug), "{held:x?}");
    assert!(held.contains(&second_plug), "{held:x?}");
}

/// Pinning a perk ahead of the author's choices shifts their indexes. A custom perk attached to
/// one of those choices has to move with it, or the write drops it as pointing at the wrong plug.
#[test]
fn a_custom_perk_follows_its_plug_when_a_behavior_leads_the_lane() {
    let donor = behavior_donor();
    let entry = crate::weapon::behavior::behavior("graviton-lance").expect("a catalogued behavior");
    let trait_plug = entry.trait_plug.expect("this behavior pins a trait plug");
    let mut recipe = behavior_recipe();
    let mut pins = BehaviorPins::default();
    let inherited = inherited_socket_choices(
        donor.sockets[1].native_default,
        &donor.sockets[1].ordered_embedded_choices,
        authored_socket_choice_limit(92),
    );
    set_recipe_socket_column(
        &mut recipe,
        donor.sockets.len(),
        1,
        &inherited,
        vec![0x1234_5678],
        None,
    );
    let mut private = variant(1);
    private.source_plug_hash = HexHash::new(0x1234_5678);
    recipe.overrides.socket_plug_variants.push(private);
    recipe.overrides.additional_behaviors = vec![crate::recipe::AdditionalBehaviorRecipe {
        behavior: entry.id.to_owned(),
    }];

    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);

    assert_eq!(chosen(&recipe, 1), vec![trait_plug, 0x1234_5678]);
    assert_eq!(recipe.overrides.socket_plug_variants.len(), 1);
    assert_eq!(recipe.overrides.socket_plug_variants[0].choice_index, 1);

    recipe.overrides.additional_behaviors.clear();
    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);
    assert_eq!(chosen(&recipe, 1), vec![0x1234_5678]);
    assert_eq!(recipe.overrides.socket_plug_variants.len(), 1);
    assert_eq!(recipe.overrides.socket_plug_variants[0].choice_index, 0);
}

/// A weapon has one frame. Choosing a behavior puts its frame in the intrinsic lane alone, in
/// place of the donor's, and dropping the behavior gives the donor's frame back.
#[test]
fn a_borrowed_frame_replaces_the_donors_frame_and_gives_it_back_when_released() {
    let donor = behavior_donor();
    let entry = crate::weapon::behavior::CATALOG
        .iter()
        .find(|entry| entry.intrinsic_plug.is_some())
        .expect("an intrinsic-pinning behavior");
    let frame = entry.intrinsic_plug.unwrap();
    let mut recipe = behavior_recipe();
    let mut pins = BehaviorPins::default();
    recipe.overrides.additional_behaviors = vec![crate::recipe::AdditionalBehaviorRecipe {
        behavior: entry.id.to_owned(),
    }];

    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);
    assert_eq!(
        chosen(&recipe, 0),
        vec![frame],
        "the donor's frame 10 is replaced"
    );

    recipe.overrides.additional_behaviors.clear();
    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);
    assert!(
        chosen(&recipe, 0).is_empty(),
        "the lane inherits the donor's frame again: {:x?}",
        chosen(&recipe, 0)
    );
}

/// An author can build their own intrinsic frame as a custom perk. Borrowing a behavior replaces
/// the donor's frame, not the author's own work, and the custom perk keeps pointing at its plug.
#[test]
fn a_custom_perk_in_the_intrinsic_lane_survives_a_borrowed_frame() {
    let donor = behavior_donor();
    let entry = crate::weapon::behavior::CATALOG
        .iter()
        .find(|entry| entry.intrinsic_plug.is_some())
        .expect("an intrinsic-pinning behavior");
    let frame = entry.intrinsic_plug.unwrap();
    let mut recipe = behavior_recipe();
    let mut pins = BehaviorPins::default();
    let inherited = inherited_socket_choices(
        donor.sockets[0].native_default,
        &donor.sockets[0].ordered_embedded_choices,
        authored_socket_choice_limit(176),
    );
    set_recipe_socket_column(
        &mut recipe,
        donor.sockets.len(),
        0,
        &inherited,
        vec![10, 20],
        None,
    );
    let mut private = variant(0);
    private.choice_index = 1;
    recipe.overrides.socket_plug_variants.push(private);
    recipe.overrides.additional_behaviors = vec![crate::recipe::AdditionalBehaviorRecipe {
        behavior: entry.id.to_owned(),
    }];

    sync_behavior_socket_pins(&mut recipe, &mut pins, &donor);

    let held = chosen(&recipe, 0);
    assert!(
        held.contains(&frame),
        "the borrowed frame leads the lane: {held:x?}"
    );
    assert!(
        held.contains(&20),
        "the author's own intrinsic perk is not deleted: {held:x?}"
    );
    let kept = recipe
        .overrides
        .socket_plug_variants
        .iter()
        .find(|variant| variant.source_plug_hash.parse_u32() == Ok(20))
        .expect("the custom perk document survives");
    assert_eq!(
        held.get(usize::from(kept.choice_index)),
        Some(&20),
        "its index still points at its own plug"
    );
}
