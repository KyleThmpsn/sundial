use super::*;

fn donor() -> WeaponDonor {
    WeaponDonor {
        summary: WeaponDonorSummary {
            hash: 1,
            name: "Socket Test".into(),
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
        replace_effects: false,
        socket_index,
        choice_index: 0,
        source_plug_hash: HexHash::new(20),
        name: Some("Saved Perk".into()),
        classification_donor_hash: None,
        description: None,
        investment_stats: vec![],
        additional_sandbox_perks: vec![],
        sandbox_perks: vec![],
    }
}

#[test]
fn perk_bank_projects_defaults_alternatives_and_private_additions() {
    let mut donor = donor();
    donor.base_sandbox_perks = vec![1, 2, 3, 4, 5];
    let mut recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.bank-test",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    set_recipe_socket_column(&mut recipe, 3, 0, &[], vec![20, 21], None);
    let mut private = variant(0);
    private.additional_sandbox_perks = vec![100, 101];
    recipe.overrides.socket_plug_variants.push(private);
    let lookup = |hash| match hash {
        20 => vec![90],
        21 => vec![91, 92, 93, 94, 95],
        _ => vec![80, 81, 82, 83],
    };
    let bank = crate::weapon::perk_bank::project(&recipe, &donor, lookup);
    assert_eq!(bank.default_count, 15);
    assert_eq!(bank.maximum_count, 16);
    assert!(bank.omitted.is_empty());
    assert!(append_socket(&mut recipe, donor.sockets.len(), 92));
    set_recipe_socket_column(&mut recipe, 4, 3, &[], vec![22], None);
    let bank = crate::weapon::perk_bank::project(&recipe, &donor, lookup);
    assert_eq!(bank.default_count, 19);
    assert_eq!(bank.maximum_count, 20);
    assert_eq!(
        bank.omitted,
        [
            "Socket 4 effect 81",
            "Socket 4 effect 82",
            "Socket 4 effect 83"
        ]
    );
    remove_base_socket(&mut recipe, 4, 3);
    recipe.overrides.base_sandbox_perks = Some(vec![u16::MAX, 1]);
    let bank = crate::weapon::perk_bank::project(&recipe, &donor, lookup);
    assert_eq!(bank.default_count, 12);
    assert_eq!(bank.maximum_count, 13);
}

#[test]
fn perk_bank_applies_fixed_damage_before_projection() {
    let donor = donor();
    let mut recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.bank-damage",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    let project = |recipe: &WeaponRecipe| {
        crate::weapon::perk_bank::project(recipe, &donor, |_| vec![1, 2, 3, 4])
    };
    assert_eq!(project(&recipe).default_count, 12);
    recipe.overrides.modern_damage_type = Some(RecipeDamageType::Solar);
    assert_eq!(project(&recipe).default_count, 13);
    recipe.overrides.base_sandbox_perks = Some(vec![449, 10, 11, 12, 13]);
    assert_eq!(project(&recipe).default_count, 16);
    recipe.overrides.modern_damage_type = Some(RecipeDamageType::Kinetic);
    assert_eq!(project(&recipe).default_count, 16);
    recipe.overrides.base_sandbox_perks = Some(vec![449, 10, 11, 12]);
    assert_eq!(project(&recipe).default_count, 15);
}

#[test]
fn wave_advisory_tracks_selectable_effects_and_disabled_sockets() {
    let donor = donor();
    let mut recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.wave-advisory",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    let lookup = |hash| if hash == 21 { vec![1778] } else { vec![90] };
    let project = |recipe: &WeaponRecipe| crate::weapon::perk_bank::project(recipe, &donor, lookup);
    assert!(!project(&recipe).wave_frame);
    set_recipe_socket_column(&mut recipe, 3, 0, &[], vec![20, 21], None);
    assert!(project(&recipe).wave_frame);
    remove_base_socket(&mut recipe, 3, 0);
    assert!(!project(&recipe).wave_frame);
    set_recipe_socket_column(&mut recipe, 3, 0, &[], vec![20], None);
    set_socket_role(&mut recipe, &donor, 0, Some(92));
    let mut private = variant(0);
    private.additional_sandbox_perks = vec![1778];
    recipe.overrides.socket_plug_variants.push(private);
    assert!(project(&recipe).wave_frame);
    recipe.overrides.socket_plug_variants[0].additional_sandbox_perks = vec![91, 92, 93, 1778];
    assert!(!project(&recipe).wave_frame);
}

#[test]
fn appended_socket_survives_original_row_edits_and_recipe_reload() {
    let donor = donor();
    let mut recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.added-socket",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    assert!(append_socket(&mut recipe, donor.sockets.len(), 92));
    assert!(
        recipe.overrides.socket_columns[..3]
            .iter()
            .all(Option::is_none)
    );
    let pending = recipe.overrides.socket_columns[3].as_ref().unwrap();
    assert_eq!(pending.socket_type, Some(92));
    assert!(pending.choices.is_empty());
    set_recipe_socket_column(&mut recipe, 3, 3, &[], vec![20], None);
    materialize_socket_column(&mut recipe, 3, 0, &[10], false);
    set_recipe_socket_column(&mut recipe, 3, 1, &[11], vec![30], None);
    assert_eq!(recipe.overrides.socket_columns.len(), 4);
    let reloaded = WeaponRecipe::from_json_str(&serde_json::to_string(&recipe).unwrap()).unwrap();
    let expanded = socket_editor_donor(&donor, &reloaded);
    assert_eq!(&expanded.sockets[..3], &donor.sockets);
    assert_eq!(expanded.sockets[3].index, 3);
    assert_eq!(expanded.sockets[3].socket_type, 92);
    assert_eq!(expanded.sockets[3].native_default, None);
    assert_eq!(recipe_socket_choices(&reloaded, 3, &[]).unwrap(), vec![20]);
    assert_eq!(
        reloaded.overrides.socket_columns[3]
            .as_ref()
            .unwrap()
            .socket_type,
        Some(92)
    );
    assert_eq!(donor.sockets.len(), 3);
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

#[test]
fn changing_added_socket_role_retains_choices_and_explicit_role() {
    let donor = donor();
    let mut recipe = PackageAuthoringApp::default().recipe;
    append_socket(&mut recipe, donor.sockets.len(), 92);
    set_recipe_socket_column(&mut recipe, 3, 3, &[], vec![20], None);
    let expanded = socket_editor_donor(&donor, &recipe);
    set_socket_role(&mut recipe, &expanded, 3, Some(176));
    let added = recipe.overrides.socket_columns[3].as_ref().unwrap();
    assert_eq!(added.socket_type, Some(176));
    assert_eq!(added.choices, vec![HexHash::new(20)]);
}

#[test]
fn removing_base_socket_preserves_neighbor_columns_and_private_perks() {
    let donor = donor();
    let mut recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.remove-socket",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    set_recipe_socket_column(&mut recipe, 3, 0, &[10], vec![20], None);
    set_recipe_socket_column(&mut recipe, 3, 1, &[11], vec![20], None);
    set_recipe_socket_column(&mut recipe, 3, 2, &[12], vec![20], None);
    recipe.overrides.socket_plug_variants = vec![variant(0), variant(1), variant(2)];
    for variant in &mut recipe.overrides.socket_plug_variants {
        variant
            .sandbox_perks
            .push(crate::recipe::WeaponSandboxPerkRuntimeRecipe {
                program: None,
                projectiles: Vec::new(),
                source_perk_index: 1,
                activation: None,
                runtime_values: Vec::new(),
                action_float_values: Vec::new(),
            });
    }
    let before = recipe.clone();
    remove_base_socket(&mut recipe, 3, 1);
    assert_eq!(recipe.overrides.socket_columns.len(), 3);
    assert_eq!(
        recipe.overrides.socket_columns[0],
        before.overrides.socket_columns[0]
    );
    assert_eq!(
        recipe.overrides.socket_columns[2],
        before.overrides.socket_columns[2]
    );
    assert_eq!(
        recipe.overrides.socket_plug_variants,
        vec![
            before.overrides.socket_plug_variants[0].clone(),
            before.overrides.socket_plug_variants[2].clone()
        ]
    );
    let reloaded = WeaponRecipe::from_json_str(&serde_json::to_string(&recipe).unwrap()).unwrap();
    let spec = reloaded.to_spec().unwrap();
    let removed = spec.overrides.socket_columns[1].as_ref().unwrap();
    assert_eq!(removed.socket_type, Some(u16::MAX));
    assert!(removed.choices.is_empty());
}
