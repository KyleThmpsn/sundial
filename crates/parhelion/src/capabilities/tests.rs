use super::*;
use sundial::investment::{
    WeaponInvestmentStat, WeaponRarity, WeaponSocket, WeaponStatDisplayPoint,
};

use crate::recipe::WeaponDonorReference;

fn summary(slot: Option<WeaponInventorySlot>, profile: WeaponDamageProfile) -> WeaponDonorSummary {
    WeaponDonorSummary {
        hash: 0x1234_5678,
        name: "Test Donor".to_owned(),
        type_name: "Test Weapon".to_owned(),
        bucket_hash: 0,
        collection_backed: true,
        power_cap: Some(1_060),
        damage_type: match profile {
            WeaponDamageProfile::KineticEmpty => Some(WeaponDamageType::Kinetic),
            WeaponDamageProfile::ModernFixed(damage_type)
            | WeaponDamageProfile::LegacyFixed(damage_type)
            | WeaponDamageProfile::PlugOrEmptyAmbiguous(Some(damage_type)) => Some(damage_type),
            WeaponDamageProfile::PlugOrEmptyAmbiguous(None)
            | WeaponDamageProfile::Variable
            | WeaponDamageProfile::Unknown => None,
        },
        inventory_slot: slot,
        damage_profile: profile,
        rarity: WeaponRarity::Legendary,
        ammo_type: None,
        weapon_pattern_index: None,
        weapon_translation_group: Some(1),
        stat_group_index: None,
    }
}

fn explicit_profiles(capabilities: &WeaponAuthoringCapabilities) -> Vec<CombatProfile> {
    capabilities
        .combat_profiles
        .iter()
        .filter_map(|choice| match choice.action {
            CombatProfileAction::Preserve => None,
            CombatProfileAction::Set(profile) => Some(profile),
        })
        .collect()
}

#[test]
fn appearance_checks_follow_the_selected_runtime_and_fail_closed_when_unknown() {
    let mut base = summary(
        Some(WeaponInventorySlot::Energy),
        WeaponDamageProfile::ModernFixed(WeaponDamageType::Arc),
    );
    base.weapon_pattern_index = Some(10);
    let mut appearance = base.clone();
    appearance.hash += 1;
    appearance.weapon_pattern_index = Some(20);
    appearance.weapon_translation_group = Some(2);
    let donors = vec![base.clone(), appearance.clone()];
    assert_eq!(
        effective_weapon_translation_group(&base, None, &donors),
        Some(1)
    );
    assert_eq!(
        effective_weapon_translation_group(&base, Some(20), &donors),
        Some(2)
    );
    assert_eq!(
        appearance_compatibility(&appearance, &base, WeaponInventorySlot::Energy),
        AppearanceCompatibility::Blocked("Incompatible weapon animations")
    );
    base.weapon_translation_group = effective_weapon_translation_group(&base, Some(20), &donors);
    assert_eq!(
        appearance_compatibility(&appearance, &base, WeaponInventorySlot::Energy),
        AppearanceCompatibility::Compatible
    );
    base.weapon_translation_group = effective_weapon_translation_group(&base, Some(999), &donors);
    assert_eq!(
        appearance_compatibility(&appearance, &base, WeaponInventorySlot::Energy),
        AppearanceCompatibility::Unchecked
    );
    assert!(!presentation_donor_candidate_is_compatible(
        &appearance,
        &base,
        WeaponInventorySlot::Energy
    ));
}

#[test]
fn fixed_and_empty_carriers_allow_independent_slot_and_damage() {
    for slot in [
        WeaponInventorySlot::Kinetic,
        WeaponInventorySlot::Energy,
        WeaponInventorySlot::Power,
    ] {
        for profile in [
            WeaponDamageProfile::KineticEmpty,
            WeaponDamageProfile::ModernFixed(WeaponDamageType::Arc),
            WeaponDamageProfile::LegacyFixed(WeaponDamageType::Solar),
        ] {
            let donor = summary(Some(slot), profile);
            let capabilities = weapon_summary_authoring_capabilities(&donor);
            assert!(capabilities.is_authorable());
            assert_eq!(capabilities.combat_profiles.len(), 12);
            for target in [
                WeaponInventorySlot::Kinetic,
                WeaponInventorySlot::Energy,
                WeaponInventorySlot::Power,
            ] {
                for damage in [
                    WeaponDamageType::Kinetic,
                    WeaponDamageType::Arc,
                    WeaponDamageType::Solar,
                    WeaponDamageType::Void,
                ] {
                    let action = if target == slot && donor.damage_type == Some(damage) {
                        CombatProfileAction::Preserve
                    } else {
                        CombatProfileAction::Set(CombatProfile {
                            inventory_slot: target,
                            damage_type: damage,
                        })
                    };
                    assert!(
                        capabilities.supports(action),
                        "{slot:?} {profile:?} -> {action:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn plug_driven_damage_keeps_its_carrier_while_slot_is_independent() {
    let capabilities = weapon_summary_authoring_capabilities(&summary(
        Some(WeaponInventorySlot::Energy),
        WeaponDamageProfile::PlugOrEmptyAmbiguous(Some(WeaponDamageType::Solar)),
    ));
    assert_eq!(capabilities.combat_profiles.len(), 9);
    assert!(
        explicit_profiles(&capabilities)
            .iter()
            .all(|p| p.damage_type != WeaponDamageType::Kinetic)
    );
    assert!(
        capabilities.supports(CombatProfileAction::Set(CombatProfile {
            inventory_slot: WeaponInventorySlot::Kinetic,
            damage_type: WeaponDamageType::Arc,
        }))
    );
}

#[test]
fn unresolved_variable_and_unknown_damage_are_preserve_only() {
    for profile in [
        WeaponDamageProfile::PlugOrEmptyAmbiguous(None),
        WeaponDamageProfile::Variable,
        WeaponDamageProfile::Unknown,
    ] {
        let capabilities = weapon_summary_authoring_capabilities(&summary(
            Some(WeaponInventorySlot::Energy),
            profile,
        ));
        assert!(capabilities.is_authorable(), "{profile:?}");
        assert_eq!(capabilities.combat_profiles.len(), 1, "{profile:?}");
        assert_eq!(
            capabilities.combat_profiles[0].action,
            CombatProfileAction::Preserve
        );
    }
}

#[test]
fn missing_slot_blocks_all_choices() {
    let capabilities = weapon_summary_authoring_capabilities(&summary(
        None,
        WeaponDamageProfile::ModernFixed(WeaponDamageType::Arc),
    ));
    assert!(!capabilities.is_authorable());
    assert!(capabilities.combat_profiles.is_empty());
    assert_eq!(
        capabilities.diagnostics[0].code,
        AuthoringDiagnosticCode::MissingInventorySlot
    );
}

#[test]
fn combat_profile_selection_writes_slot_and_element_atomically() {
    let donor = summary(
        Some(WeaponInventorySlot::Kinetic),
        WeaponDamageProfile::KineticEmpty,
    );
    let mut overrides = WeaponRecipeOverrides::default();
    assert_eq!(
        recipe_combat_profile_action(&overrides, &donor),
        Some(CombatProfileAction::Preserve)
    );

    let void_energy = CombatProfileAction::Set(CombatProfile {
        inventory_slot: WeaponInventorySlot::Energy,
        damage_type: WeaponDamageType::Void,
    });
    apply_combat_profile_action(&mut overrides, &donor, void_energy);
    assert_eq!(overrides.inventory_slot, Some(RecipeInventorySlot::Energy));
    assert_eq!(overrides.modern_damage_type, Some(RecipeDamageType::Void));
    assert_eq!(
        recipe_combat_profile_action(&overrides, &donor),
        Some(void_energy)
    );

    apply_combat_profile_action(&mut overrides, &donor, CombatProfileAction::Preserve);
    assert_eq!(overrides, WeaponRecipeOverrides::default());
}

#[test]
fn in_place_element_change_does_not_serialize_a_redundant_slot_override() {
    let donor = summary(
        Some(WeaponInventorySlot::Power),
        WeaponDamageProfile::ModernFixed(WeaponDamageType::Arc),
    );
    let mut overrides = WeaponRecipeOverrides::default();
    let solar_power = CombatProfileAction::Set(CombatProfile {
        inventory_slot: WeaponInventorySlot::Power,
        damage_type: WeaponDamageType::Solar,
    });

    apply_combat_profile_action(&mut overrides, &donor, solar_power);

    assert_eq!(overrides.inventory_slot, None);
    assert_eq!(overrides.modern_damage_type, Some(RecipeDamageType::Solar));
    assert_eq!(
        recipe_combat_profile_action(&overrides, &donor),
        Some(solar_power)
    );
}

#[test]
fn cross_slot_profiles_require_a_compatible_target_slot_presentation_donor() {
    let gameplay = summary(
        Some(WeaponInventorySlot::Kinetic),
        WeaponDamageProfile::KineticEmpty,
    );
    let mut energy = summary(
        Some(WeaponInventorySlot::Energy),
        WeaponDamageProfile::ModernFixed(WeaponDamageType::Solar),
    );
    energy.hash = 0x7405_1969;
    energy.name = "Energy Candidate".to_owned();
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.cross-slot", gameplay.hash, &gameplay.name)
            .unwrap();
    apply_combat_profile_action(
        &mut recipe.overrides,
        &gameplay,
        CombatProfileAction::Set(CombatProfile {
            inventory_slot: WeaponInventorySlot::Energy,
            damage_type: WeaponDamageType::Void,
        }),
    );

    assert_eq!(
        authored_inventory_slot(&recipe.overrides, &gameplay),
        Some(WeaponInventorySlot::Energy)
    );
    assert!(!selected_presentation_donor_is_compatible(
        &recipe,
        &gameplay,
        std::slice::from_ref(&energy)
    ));
    assert!(presentation_donor_candidate_is_compatible(
        &energy,
        &gameplay,
        WeaponInventorySlot::Energy
    ));

    recipe.presentation_donor = Some(WeaponDonorReference {
        item_hash: energy.hash.into(),
        expected_name: Some(energy.name.clone()),
    });
    assert!(selected_presentation_donor_is_compatible(
        &recipe,
        &gameplay,
        std::slice::from_ref(&energy)
    ));

    let mut wrong_slot = energy.clone();
    wrong_slot.hash = 0x1111_1111;
    wrong_slot.inventory_slot = Some(WeaponInventorySlot::Power);
    assert!(!presentation_donor_candidate_is_compatible(
        &wrong_slot,
        &gameplay,
        WeaponInventorySlot::Energy
    ));
    let mut wrong_type = energy.clone();
    wrong_type.hash = 0x2222_2222;
    wrong_type.type_name = "Grenade Launcher".to_owned();
    assert!(!presentation_donor_candidate_is_compatible(
        &wrong_type,
        &gameplay,
        WeaponInventorySlot::Energy
    ));
    let mut collectionless = energy.clone();
    collectionless.hash = 0x3333_3333;
    collectionless.collection_backed = false;
    assert!(!presentation_donor_candidate_is_compatible(
        &collectionless,
        &gameplay,
        WeaponInventorySlot::Energy
    ));
}

#[test]
fn kinetic_energy_appearance_requires_matching_known_animations() {
    for (source, target) in [
        (WeaponInventorySlot::Energy, WeaponInventorySlot::Kinetic),
        (WeaponInventorySlot::Kinetic, WeaponInventorySlot::Energy),
    ] {
        let base = summary(Some(target), WeaponDamageProfile::KineticEmpty);
        let mut appearance = summary(Some(source), WeaponDamageProfile::KineticEmpty);
        assert_eq!(
            appearance_compatibility(&appearance, &base, target),
            AppearanceCompatibility::Compatible
        );
        appearance.weapon_translation_group = Some(2);
        assert_eq!(
            appearance_compatibility(&appearance, &base, target),
            AppearanceCompatibility::Blocked("Incompatible weapon animations")
        );
        appearance.weapon_translation_group = None;
        assert_eq!(
            appearance_compatibility(&appearance, &base, target),
            AppearanceCompatibility::Unchecked
        );
        appearance.weapon_translation_group = base.weapon_translation_group;
        appearance.inventory_slot = Some(WeaponInventorySlot::Power);
        assert_eq!(
            appearance_compatibility(&appearance, &base, target),
            AppearanceCompatibility::Blocked("Different inventory slot")
        );
    }
}

#[test]
fn profile_reconciliation_clears_incompatible_or_malformed_presentation_donors() {
    let gameplay = summary(
        Some(WeaponInventorySlot::Kinetic),
        WeaponDamageProfile::KineticEmpty,
    );
    let mut energy = summary(
        Some(WeaponInventorySlot::Energy),
        WeaponDamageProfile::ModernFixed(WeaponDamageType::Arc),
    );
    energy.hash = 0xABCD_EF01;
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.reconcile", gameplay.hash, &gameplay.name)
            .unwrap();
    apply_combat_profile_action(
        &mut recipe.overrides,
        &gameplay,
        CombatProfileAction::Set(CombatProfile {
            inventory_slot: WeaponInventorySlot::Energy,
            damage_type: WeaponDamageType::Arc,
        }),
    );
    recipe.presentation_donor = Some(WeaponDonorReference {
        item_hash: energy.hash.into(),
        expected_name: Some(energy.name.clone()),
    });
    reconcile_presentation_donor(&mut recipe, &gameplay, std::slice::from_ref(&energy));
    assert!(recipe.presentation_donor.is_some());

    apply_combat_profile_action(
        &mut recipe.overrides,
        &gameplay,
        CombatProfileAction::Preserve,
    );
    reconcile_presentation_donor(&mut recipe, &gameplay, std::slice::from_ref(&energy));
    assert!(recipe.presentation_donor.is_some());

    energy.weapon_translation_group = Some(2);
    reconcile_presentation_donor(&mut recipe, &gameplay, std::slice::from_ref(&energy));
    assert!(recipe.presentation_donor.is_none());

    recipe.presentation_donor = Some(WeaponDonorReference {
        item_hash: 0_u32.into(),
        expected_name: Some("Malformed".to_owned()),
    });
    recipe
        .presentation_donor
        .as_mut()
        .unwrap()
        .item_hash
        .set_text("not-a-hash");
    assert!(!selected_presentation_donor_is_compatible(
        &recipe,
        &gameplay,
        std::slice::from_ref(&energy)
    ));
}

#[test]
fn collectionless_donor_is_blocked_even_when_profile_is_coherent() {
    let mut donor = summary(
        Some(WeaponInventorySlot::Energy),
        WeaponDamageProfile::ModernFixed(WeaponDamageType::Void),
    );
    donor.collection_backed = false;
    let capabilities = weapon_summary_authoring_capabilities(&donor);
    assert!(!capabilities.is_authorable());
    assert_eq!(
        capabilities.diagnostics[0].code,
        AuthoringDiagnosticCode::CollectionsBackingRequired
    );
    assert!(
        capabilities
            .combat_profiles
            .iter()
            .any(|choice| choice.action == CombatProfileAction::Preserve)
    );
    assert!(!capabilities.supports(CombatProfileAction::Preserve));
}

#[test]
fn conflicting_damage_metadata_blocks_preserve() {
    let mut donor = summary(
        Some(WeaponInventorySlot::Kinetic),
        WeaponDamageProfile::ModernFixed(WeaponDamageType::Void),
    );
    donor.damage_type = Some(WeaponDamageType::Arc);
    let capabilities = weapon_summary_authoring_capabilities(&donor);
    assert!(capabilities.combat_profiles.is_empty());
    assert_eq!(
        capabilities.diagnostics[0].code,
        AuthoringDiagnosticCode::IncoherentDamageProfile
    );
}

fn donor() -> WeaponDonor {
    WeaponDonor {
        summary: summary(
            Some(WeaponInventorySlot::Energy),
            WeaponDamageProfile::ModernFixed(WeaponDamageType::Arc),
        ),
        power_cap_groups: vec![7, 11],
        equipment_slot: Some(WeaponInventorySlot::Energy),
        sockets: vec![
            WeaponSocket {
                index: 0,
                socket_type: 1,
                label: "Intrinsic".to_owned(),
                native_default: Some(0xAAAA_AAAA),
                ordered_embedded_choices: vec![0xAAAA_AAAA],
                compatible_plug_count: 2,
                max_authored_choices: 1,
                reusable_plug_set_index: None,
                randomized_plug_set_index: None,
            },
            WeaponSocket {
                index: 1,
                socket_type: 3,
                label: "Trait column".to_owned(),
                native_default: Some(0x1111_1111),
                ordered_embedded_choices: vec![0x1111_1111, 0x2222_2222],
                compatible_plug_count: 3,
                max_authored_choices: MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES,
                reusable_plug_set_index: None,
                randomized_plug_set_index: None,
            },
            WeaponSocket {
                index: 2,
                socket_type: 2,
                label: "Disabled".to_owned(),
                native_default: None,
                ordered_embedded_choices: Vec::new(),
                compatible_plug_count: 0,
                max_authored_choices: 0,
                reusable_plug_set_index: None,
                randomized_plug_set_index: None,
            },
        ],
        investment_stats: vec![WeaponInvestmentStat {
            definition_index: 7,
            definition_hash: Some(0x7777_7777),
            name: "Impact".to_owned(),
            value: 50,
            minimum_value: Some(0),
            maximum_value: Some(100),
            display_as_numeric: false,
            is_linear: false,
            display_interpolation: Vec::new(),
        }],
        addable_investment_stats: vec![WeaponInvestmentStat {
            definition_index: 8,
            definition_hash: Some(0x8888_8888),
            name: "Velocity".to_owned(),
            value: 0,
            minimum_value: Some(0),
            maximum_value: Some(100),
            display_as_numeric: false,
            is_linear: false,
            display_interpolation: Vec::new(),
        }],
        base_sandbox_perks: vec![449],
        trait_indices: vec![26, 60],
        max_stack_size: Some(1),
        socket_entry_list_index: Some(0),
        plug_category_hash: None,
        roll_set_index: None,
        linked_plug_index: None,
        linked_plug_hash: None,
        art_arrangements: Vec::new(),
        render_dye_rows: std::array::from_fn(|_| Vec::new()),
    }
}

#[test]
fn equipment_slot_mismatch_is_reported_separately_from_inventory_slot_decoding() {
    let mut donor = donor();
    donor.equipment_slot = Some(WeaponInventorySlot::Kinetic);

    let capabilities = weapon_authoring_capabilities(&donor);

    assert!(capabilities.combat_profiles.is_empty());
    assert!(
        capabilities.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == AuthoringDiagnosticCode::EquipmentSlotMismatch
        })
    );
}

#[test]
fn missing_equipment_slot_does_not_erase_the_decoded_inventory_slot() {
    let mut donor = donor();
    donor.equipment_slot = None;

    let capabilities = weapon_authoring_capabilities(&donor);

    assert_eq!(
        donor.summary.inventory_slot,
        Some(WeaponInventorySlot::Energy)
    );
    assert!(capabilities.combat_profiles.is_empty());
    assert!(
        capabilities
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == AuthoringDiagnosticCode::MissingEquipmentSlot })
    );
}

#[test]
fn stat_validation_checks_definition_duplicates_and_decoded_group_bounds() {
    let diagnostics = validate_stat_overrides(&donor(), &[(7, i32::MIN), (7, i32::MAX), (9, 50)]);
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == AuthoringDiagnosticCode::DuplicateStatOverride
        })
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == AuthoringDiagnosticCode::UnsupportedStatDefinition
    }));
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == AuthoringDiagnosticCode::StatValueBelowMinimum
        })
    );
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == AuthoringDiagnosticCode::StatValueAboveMaximum
        })
    );
    assert!(validate_stat_overrides(&donor(), &[(7, 0), (8, 50)]).is_empty());
    assert!(
        validate_stat_overrides(&donor(), &[(9, 50)])
            .iter()
            .all(|diagnostic| diagnostic.code
                == AuthoringDiagnosticCode::UnsupportedStatDefinition)
    );
    assert!(validate_stat_overrides(&donor(), &[(7, 100)]).is_empty());

    let mut donor = donor();
    donor.addable_investment_stats[0].minimum_value = None;
    assert!(validate_stat_overrides(&donor, &[(8, -20)]).is_empty());
    assert!(
        validate_stat_overrides(&donor, &[(8, 101)])
            .iter()
            .any(|diagnostic| diagnostic.code == AuthoringDiagnosticCode::StatValueAboveMaximum)
    );
}

#[test]
fn stat_validation_uses_the_investment_axis_not_the_display_axis() {
    let mut donor = donor();
    donor.investment_stats.push(WeaponInvestmentStat {
        definition_index: 14,
        definition_hash: Some(0xFF66_4809),
        name: "Rounds Per Minute".to_owned(),
        value: 80,
        minimum_value: Some(0),
        maximum_value: Some(100),
        display_as_numeric: true,
        is_linear: false,
        display_interpolation: vec![
            WeaponStatDisplayPoint {
                investment_value: 0,
                display_value: 360,
            },
            WeaponStatDisplayPoint {
                investment_value: 100,
                display_value: 720,
            },
        ],
    });

    assert!(validate_stat_overrides(&donor, &[(14, 95)]).is_empty());
    assert!(
        validate_stat_overrides(&donor, &[(14, -1)])
            .iter()
            .any(|diagnostic| diagnostic.code == AuthoringDiagnosticCode::StatValueBelowMinimum)
    );
    assert!(
        validate_stat_overrides(&donor, &[(14, 101)])
            .iter()
            .any(|diagnostic| diagnostic.code == AuthoringDiagnosticCode::StatValueAboveMaximum)
    );
}

#[test]
fn socket_column_validation_requires_caller_supplied_compatible_sets() {
    let supported = vec![
        SupportedPlugSet {
            socket_index: 0,
            plug_hashes: vec![0xAAAA_AAAA, 0xBBBB_BBBB],
        },
        SupportedPlugSet {
            socket_index: 1,
            plug_hashes: vec![0x1111_1111, 0x2222_2222, 0x3333_3333],
        },
        SupportedPlugSet {
            socket_index: 2,
            plug_hashes: Vec::new(),
        },
    ];
    assert!(
        validate_socket_column_overrides(
            &donor(),
            &[
                Some(vec![0xBBBB_BBBB]),
                Some(vec![0x2222_2222, 0x3333_3333, 0x1111_1111]),
                None,
            ],
            &supported,
        )
        .is_empty()
    );

    let diagnostics = validate_socket_column_overrides(
        &donor(),
        &[
            Some(vec![0xAAAA_AAAA]),
            Some(vec![0x1111_1111, 0xCCCC_CCCC, 0]),
            None,
        ],
        &supported,
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == AuthoringDiagnosticCode::UnsupportedPlug)
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == AuthoringDiagnosticCode::ZeroPlugHash)
    );
    let duplicate_diagnostics = validate_socket_column_overrides(
        &donor(),
        &[
            Some(vec![0xAAAA_AAAA]),
            Some(vec![0x1111_1111, 0x2222_2222, 0x2222_2222]),
            None,
        ],
        &supported,
    );
    assert!(duplicate_diagnostics.iter().any(|diagnostic| {
        diagnostic.code == AuthoringDiagnosticCode::DuplicateSocketColumnPlug
    }));

    let diagnostics = validate_socket_column_overrides(
        &donor(),
        &[Some(vec![0xAAAA_AAAA]), Some(vec![0x1111_1111]), None],
        &supported[..1],
    );
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == AuthoringDiagnosticCode::MissingSupportedPlugSet
        })
    );
}

#[test]
fn socket_column_validation_accepts_eight_ordered_embedded_choices() {
    let choices = (1..=8).map(|value| 0x5100_0000 + value).collect::<Vec<_>>();
    let supported = vec![
        SupportedPlugSet {
            socket_index: 0,
            plug_hashes: vec![0xAAAA_AAAA],
        },
        SupportedPlugSet {
            socket_index: 1,
            plug_hashes: choices.clone(),
        },
        SupportedPlugSet {
            socket_index: 2,
            plug_hashes: Vec::new(),
        },
    ];

    let diagnostics =
        validate_socket_column_overrides(&donor(), &[None, Some(choices), None], &supported);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn socket_column_validation_enforces_donor_choice_limits_and_disabled_rows() {
    let supported = vec![
        SupportedPlugSet {
            socket_index: 0,
            plug_hashes: vec![0xAAAA_AAAA, 0xBBBB_BBBB],
        },
        SupportedPlugSet {
            socket_index: 1,
            plug_hashes: vec![0x1111_1111, 0x2222_2222, 0x3333_3333],
        },
        SupportedPlugSet {
            socket_index: 2,
            plug_hashes: Vec::new(),
        },
    ];

    assert!(validate_socket_column_overrides(&donor(), &[], &supported).is_empty());
    assert!(validate_socket_column_overrides(&donor(), &[None, None, None], &supported).is_empty());

    let count_diagnostics =
        validate_socket_column_overrides(&donor(), &[Some(vec![0xAAAA_AAAA])], &supported);
    assert!(
        count_diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == AuthoringDiagnosticCode::SocketCountMismatch })
    );

    let diagnostics = validate_socket_column_overrides(
        &donor(),
        &[
            Some(vec![0xAAAA_AAAA, 0xBBBB_BBBB]),
            Some(Vec::new()),
            Some(vec![0x1111_1111]),
        ],
        &supported,
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.field == (AuthoringField::SocketColumn { socket_index: 0 })
            && diagnostic.code == AuthoringDiagnosticCode::TooManySocketColumnChoices
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.field == (AuthoringField::SocketColumn { socket_index: 1 })
            && diagnostic.code == AuthoringDiagnosticCode::EmptySocketColumn
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.field == (AuthoringField::SocketColumn { socket_index: 2 })
            && diagnostic.code == AuthoringDiagnosticCode::DisabledSocketOverride
    }));
}

#[test]
fn explicit_socket_type_can_activate_a_disabled_donor_row() {
    let supported = vec![SupportedPlugSet {
        socket_index: 2,
        plug_hashes: vec![0x1111_1111],
    }];
    let diagnostics = validate_socket_column_overrides_with_socket_types(
        &donor(),
        &[None, None, Some(vec![0x1111_1111])],
        &[None, None, Some(700)],
        &supported,
    );

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn added_socket_requires_its_own_type_and_choices() {
    let donor = donor();
    let columns = [None, None, None, Some(vec![0x1111_1111, 0x2222_2222])];
    let supported = [SupportedPlugSet {
        socket_index: 3,
        plug_hashes: vec![0x1111_1111, 0x2222_2222],
    }];
    let types = [None, None, None, Some(700)];
    assert!(
        validate_socket_column_overrides_with_socket_types(&donor, &columns, &types, &supported)
            .is_empty()
    );
    let missing_type =
        validate_socket_column_overrides_with_socket_types(&donor, &columns, &[], &supported);
    assert!(
        missing_type
            .iter()
            .any(|diagnostic| diagnostic.code == AuthoringDiagnosticCode::MissingAddedSocketType)
    );
    let missing_choices = validate_socket_column_overrides_with_socket_types(
        &donor,
        &[None, None, None, None],
        &types,
        &supported,
    );
    assert!(
        missing_choices
            .iter()
            .any(|diagnostic| diagnostic.code == AuthoringDiagnosticCode::EmptySocketColumn)
    );
}

#[test]
fn expanded_definition_cannot_exceed_native_socket_count() {
    let donor = donor();
    let mut columns = vec![None; donor.sockets.len()];
    let mut types = vec![None; donor.sockets.len()];
    let mut supported = Vec::new();
    for index in donor.sockets.len()..=MAX_WEAPON_SOCKETS {
        columns.push(Some(vec![0x1111_1111]));
        types.push(Some(700));
        supported.push(SupportedPlugSet {
            socket_index: index,
            plug_hashes: vec![0x1111_1111],
        });
        let diagnostics = validate_socket_column_overrides_with_socket_types(
            &donor, &columns, &types, &supported,
        );
        assert_eq!(
            diagnostics.is_empty(),
            columns.len() <= MAX_WEAPON_SOCKETS,
            "{diagnostics:#?}"
        );
        if columns.len() > MAX_WEAPON_SOCKETS {
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code
                        == AuthoringDiagnosticCode::SocketCountMismatch)
            );
        }
    }
}

#[test]
fn socket_column_validation_rejects_duplicate_sets_without_choosing_one() {
    let supported = vec![
        SupportedPlugSet {
            socket_index: 0,
            plug_hashes: vec![0xAAAA_AAAA],
        },
        SupportedPlugSet {
            socket_index: 0,
            plug_hashes: vec![0xBBBB_BBBB],
        },
        SupportedPlugSet {
            socket_index: 1,
            plug_hashes: vec![0x1111_1111],
        },
        SupportedPlugSet {
            socket_index: 2,
            plug_hashes: Vec::new(),
        },
    ];

    let diagnostics = validate_socket_column_overrides(
        &donor(),
        &[Some(vec![0xAAAA_AAAA]), Some(vec![0x1111_1111]), None],
        &supported,
    );

    assert_eq!(
        diagnostics,
        vec![AuthoringDiagnostic {
            field: AuthoringField::SocketColumn { socket_index: 0 },
            code: AuthoringDiagnosticCode::DuplicateSupportedPlugSet,
            message: "Compatible-plug set socket index 0 is supplied 2 times".to_owned(),
        }]
    );
}

#[test]
fn socket_column_validation_reports_structural_set_errors_in_socket_order() {
    let supported = vec![
        SupportedPlugSet {
            socket_index: 9,
            plug_hashes: vec![1],
        },
        SupportedPlugSet {
            socket_index: 3,
            plug_hashes: vec![2],
        },
        SupportedPlugSet {
            socket_index: 9,
            plug_hashes: vec![3],
        },
        SupportedPlugSet {
            socket_index: 0,
            plug_hashes: vec![0xAAAA_AAAA],
        },
        SupportedPlugSet {
            socket_index: 1,
            plug_hashes: vec![0x1111_1111],
        },
        SupportedPlugSet {
            socket_index: 2,
            plug_hashes: Vec::new(),
        },
    ];

    let diagnostics = validate_socket_column_overrides(
        &donor(),
        &[Some(vec![0xAAAA_AAAA]), Some(vec![0x1111_1111]), None],
        &supported,
    );

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.field, diagnostic.code))
            .collect::<Vec<_>>(),
        vec![
            (
                AuthoringField::SocketColumn { socket_index: 3 },
                AuthoringDiagnosticCode::SupportedPlugSetSocketIndexOutOfRange,
            ),
            (
                AuthoringField::SocketColumn { socket_index: 9 },
                AuthoringDiagnosticCode::SupportedPlugSetSocketIndexOutOfRange,
            ),
            (
                AuthoringField::SocketColumn { socket_index: 9 },
                AuthoringDiagnosticCode::DuplicateSupportedPlugSet,
            ),
        ]
    );
}
