use super::*;
use sundial::investment::{WeaponInvestmentStat, WeaponSocket, WeaponStatDisplayPoint};

fn summary(slot: Option<WeaponInventorySlot>, profile: WeaponDamageProfile) -> WeaponDonorSummary {
    WeaponDonorSummary {
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
        weapon_translation_group: Some(1),
        ..crate::test_support::donor_summary(0x1234_5678, "Test Donor", "Test Weapon")
    }
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
                label: "Trait Column".to_owned(),
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
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.field, diagnostic.code))
            .collect::<Vec<_>>(),
        vec![(
            AuthoringField::SocketColumn { socket_index: 0 },
            AuthoringDiagnosticCode::DuplicateSupportedPlugSet
        )]
    );
}
