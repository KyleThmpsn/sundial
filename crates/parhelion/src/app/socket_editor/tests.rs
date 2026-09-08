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
