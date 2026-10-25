use crate::package_payload::{array_at, relative_offset, u64_at};

use super::*;

#[test]
#[ignore = "requires SUNDIAL_INSTALL pointing to the supported Shadowkeep build"]
fn supported_shadowkeep_build_loads_collection_expression_contracts() {
    let install = PathBuf::from(std::env::var("SUNDIAL_INSTALL").unwrap());
    let temp = crate::test_support::TestDirectory::new("collection-expressions");
    let catalog =
        Catalog::load_or_scan_with_progress(&install, temp.0.join("catalog.json"), true, |_| {})
            .unwrap();

    assert!(!catalog.loaded_from_cache);
    assert!(!catalog.shared_expression_pool().is_empty());
    assert!(!catalog.collectibles().is_empty());
    assert!(catalog.collectibles().iter().any(|definition| {
        definition
            .conditions
            .iter()
            .any(|condition| condition.field == COLLECTIBLE_ACQUIRED_CONDITION_FIELD)
    }));
    assert_eq!(catalog.progression_package_error(), None);
    assert_direct_unlock_contracts(&catalog);
    let cached =
        Catalog::load_or_scan_with_progress(&install, temp.0.join("catalog.json"), false, |_| {})
            .unwrap();
    assert!(cached.loaded_from_cache);
    assert_eq!(
        cached.unlock_flag_definitions(),
        catalog.unlock_flag_definitions()
    );
    assert_eq!(
        cached.unlock_value_definitions(),
        catalog.unlock_value_definitions()
    );
}

fn assert_direct_unlock_contracts(catalog: &Catalog) {
    let rank_flags = catalog
        .progression_definitions()
        .iter()
        .flat_map(|definition| &definition.steps)
        .filter_map(|step| step.unlock_flag)
        .collect::<HashSet<_>>();
    let claim_flags = catalog
        .progression_definitions()
        .iter()
        .flat_map(|definition| &definition.reward_items)
        .filter_map(|reward| reward.claim_flag)
        .collect::<HashSet<_>>();
    assert!(!rank_flags.is_empty());
    assert!(!claim_flags.is_empty());
    assert!(rank_flags.iter().chain(&claim_flags).all(|slot| {
        !catalog
            .unlock_flag_definition(usize::from(*slot))
            .unwrap()
            .tested_by
            .is_empty()
    }));
    assert!(
        catalog
            .unlock_value_definitions()
            .iter()
            .flat_map(|definition| &definition.runtime_writers)
            .any(|writer| matches!(writer, UnlockWriter::ValueCounter { .. }))
    );

    for kind in [
        ProgressionContextKind::Achievement,
        ProgressionContextKind::Requirement,
        ProgressionContextKind::Record,
        ProgressionContextKind::Progression,
        ProgressionContextKind::Activity,
        ProgressionContextKind::PackageExpression,
    ] {
        assert!(
            catalog
                .unlock_flag_definitions()
                .iter()
                .flat_map(|definition| &definition.tested_by)
                .any(|context| context.kind == kind),
            "Missing {kind:?} references"
        );
    }
    assert!(
        catalog
            .unlock_value_definitions()
            .iter()
            .flat_map(|definition| &definition.tested_by)
            .flat_map(|context| &context.condition_programs)
            .flatten()
            .any(|token| token[0] == 11 && token[1] > u32::from(u16::MAX))
    );
}

#[test]
fn catalog_resolves_state_slots_and_family5_indices_through_package_definitions() {
    let flag = UnlockDefinition {
        hash: 0xAAAA_AAAA,
        code: 1,
        compact_slot: Some(26),
        name: Some("Crucible Access".into()),
        description: None,
        runtime_writers: Vec::new(),
        tested_by: Vec::new(),
    };
    let value = UnlockDefinition {
        hash: 0x14D6_FB47,
        code: 0x0201,
        compact_slot: Some(58),
        name: None,
        description: None,
        runtime_writers: Vec::new(),
        tested_by: Vec::new(),
    };
    let reader_named_value = UnlockDefinition {
        hash: 0x738D_5E2D,
        code: 1,
        compact_slot: None,
        name: None,
        description: None,
        runtime_writers: Vec::new(),
        tested_by: vec![
            ProgressionContextDef {
                direct_references: Vec::new(),
                hash: 0x22EB_C08C,
                kind: ProgressionContextKind::Record,
                name: "Tradition Is Bigger Than You".into(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            }
            .into(),
        ],
    };
    let catalog = Catalog::finish(
        CatalogContents {
            objectives: vec![ObjectiveDef {
                hash: value.hash,
                name: String::new(),
                display_description: String::new(),
                progress_description: "C Arc".into(),
                description: "C Arc".into(),
                completion_value: 5_000,
                allow_overcompletion: true,
                allow_negative_value: false,
                allow_value_change_when_completed: true,
                is_counting_downward: false,
                condition_programs: Vec::new(),
                referenced_objective_indices: Vec::new(),
                intrinsic_perk_flag_definition_indices: Vec::new(),
                owners: Vec::new(),
                related_unlock_value_definition_index: Some(0),
            }],
            unlock_flag_definitions: vec![flag.clone()],
            unlock_value_definitions: vec![value.clone(), reader_named_value.clone()],
            progression_package_error: Some("Objective definitions: unavailable".to_owned()),
            ..Default::default()
        },
        PathBuf::new(),
        PathBuf::new(),
        false,
    );

    assert_eq!(catalog.unlock_flag_for_state(1, 26), Some((0, &flag)));
    assert_eq!(catalog.unlock_value_for_state(1, 58), Some((0, &value)));
    assert!(catalog.unlock_value_for_state(2, 58).is_none());
    assert_eq!(catalog.unlock_value_definition(0), Some(&value));
    assert_eq!(
        catalog.progression_package_error(),
        Some("Objective definitions: unavailable")
    );
    assert_eq!(
        catalog
            .objective_for_unlock_value(0)
            .map(|row| (row.description.as_str(), row.completion_value)),
        Some(("C Arc", 5_000))
    );
    let objective = catalog.objective_for_unlock_value(0).unwrap();
    assert_eq!(objective.maximum_value(), None);
    assert_eq!(objective.minimum_value(), None);
    assert_eq!(catalog.display_name(flag.hash), Some("Crucible Access"));
    assert_eq!(catalog.display_name(value.hash), Some("C Arc"));
    assert_eq!(
        catalog.display_name(reader_named_value.hash),
        Some("Tradition Is Bigger Than You")
    );
}

#[test]
fn shared_plug_selection_respects_each_scope_and_rejects_missing_sockets() {
    use crate::investment::plug_selection::{PlugSelectionMode, candidates_for_socket};
    let mut catalog = plug_selection_catalog();
    let item = ItemDef {
        hash: 10,
        name: "Example Weapon".into(),
        type_name: "Sidearm".into(),
        bucket_hash: 1_498_876_634,
        class_type: 3,
        default_plugs: Vec::new(),
        sockets: vec![SocketDef {
            socket_type: 100,
            pool: 1,
            ..SocketDef::default()
        }],
        abilities: AbilityOptions::default(),
    };
    // Distinct pools make accidental mode aliasing visible in this dispatch test.
    catalog.socket_type_options.insert(100, vec![2, 1]);
    catalog
        .socket_and_gear_type_options
        .insert("Sidearm".into(), HashMap::from([(100, vec![1])]));
    catalog.gear_type_options.insert("Sidearm".into(), vec![3]);
    catalog
        .gear_kind_options
        .insert(GearKind::Weapon, vec![2, 3]);
    for (mode, expected) in [
        (PlugSelectionMode::Supported, vec![3, 1, 4]),
        (PlugSelectionMode::SocketAndGearType, vec![1]),
        (PlugSelectionMode::MatchingSocketType, vec![2, 1]),
        (PlugSelectionMode::GearType, vec![3]),
        (PlugSelectionMode::GearKind, vec![2, 3]),
        (PlugSelectionMode::AnyPlug, vec![2, 3, 1, 4]),
    ] {
        assert_eq!(
            candidates_for_socket(&catalog, &item, 0, mode),
            expected,
            "{mode:?}"
        );
        assert!(
            candidates_for_socket(&catalog, &item, 1, mode).is_empty(),
            "{mode:?}"
        );
    }
    use crate::investment::plug_selection::candidates_for_socket_type;
    for mode in PlugSelectionMode::ALL {
        assert_eq!(
            candidates_for_socket_type(&catalog, &item, 0, Some(100), mode).as_ref(),
            candidates_for_socket(&catalog, &item, 0, mode),
        );
    }
    catalog.socket_type_options.insert(200, vec![2]);
    catalog
        .socket_and_gear_type_options
        .get_mut("Sidearm")
        .unwrap()
        .insert(200, vec![1]);
    for index in [0, 1] {
        for (mode, expected) in [
            (PlugSelectionMode::Supported, vec![]),
            (PlugSelectionMode::SocketAndGearType, vec![1]),
            (PlugSelectionMode::MatchingSocketType, vec![2]),
            (PlugSelectionMode::GearType, vec![3]),
            (PlugSelectionMode::GearKind, vec![2, 3]),
            (PlugSelectionMode::AnyPlug, vec![2, 3, 1, 4]),
        ] {
            assert_eq!(
                candidates_for_socket_type(&catalog, &item, index, Some(200), mode).as_ref(),
                expected
            );
        }
    }
}

fn plug_selection_catalog() -> Catalog {
    let names = HashMap::from([
        (1, "Zeta".to_owned()),
        (2, "Alpha".to_owned()),
        (3, "Beta".to_owned()),
    ]);
    Catalog::finish(
        CatalogContents {
            names,
            plug_pools: vec![Vec::new(), vec![4, 3, 1], vec![2, 3]],
            ..Default::default()
        },
        PathBuf::new(),
        PathBuf::new(),
        false,
    )
}

#[test]
fn package_offsets_reject_underflow_and_out_of_bounds_reads() {
    assert!(relative_offset(8, 0, -9).is_err());
    assert!(relative_offset(usize::MAX, 1, 0).is_err());
    assert!(u64_at(&[0; 4], usize::MAX).is_err());

    let mut descriptor = [0_u8; 32];
    descriptor[0..8].copy_from_slice(&1_u64.to_le_bytes());
    descriptor[8..16].copy_from_slice(&(-17_i64).to_le_bytes());
    assert!(array_at(&descriptor, 0).is_err());
}

#[test]
#[ignore = "requires SUNDIAL_INSTALL pointing to the supported Shadowkeep build"]
fn installed_power_cap_table_survives_catalog_cache_roundtrip() {
    let install = PathBuf::from(std::env::var("SUNDIAL_INSTALL").unwrap());
    let temp = crate::test_support::TestDirectory::new("native-power-caps");
    let path = temp.0.join("catalog.json");
    let catalog =
        Catalog::load_or_scan_with_progress(&install, path.clone(), true, |_| {}).unwrap();
    let expected = [
        999_990, 999_980, 999_970, 1010, 999_960, 999_950, 999_940, 1060, 1060, 1260, 1310, 1360,
        1410, 1610, 1660, 1710,
    ];
    assert_eq!(
        catalog
            .power_cap_definitions()
            .iter()
            .map(|row| row.power_cap)
            .collect::<Vec<_>>(),
        expected
    );
    for (index, definition) in catalog.power_cap_definitions().iter().enumerate() {
        println!(
            "cap index={index} hash={:08X} power={}",
            definition.hash, definition.power_cap
        );
        assert_eq!(
            catalog.power_cap_for_version_group(index as u16),
            Some(definition.power_cap)
        );
    }
    assert_eq!(catalog.power_cap_for_version_group(u16::MAX), None);
    let mut resolved = 0;
    for metadata in catalog.item_package_metadata.values() {
        let expected = metadata
            .power_cap_groups
            .iter()
            .map(|index| catalog.power_cap_for_version_group(*index))
            .collect::<Option<Vec<_>>>()
            .and_then(|caps| caps.into_iter().max());
        assert_eq!(metadata.power_cap, expected);
        resolved += usize::from(expected.is_some());
    }
    println!("resolved power caps for {resolved} installed definitions");
    assert!(resolved > 0);
    let restored = Catalog::load_or_scan_with_progress(&install, path, false, |_| {}).unwrap();
    assert!(restored.loaded_from_cache);
    assert_eq!(
        restored.power_cap_definitions(),
        catalog.power_cap_definitions()
    );
    for (hash, metadata) in &catalog.item_package_metadata {
        assert_eq!(
            restored.item_power_cap(*hash),
            metadata.power_cap.map(i64::from)
        );
    }
}

impl Catalog {
    pub(crate) fn for_test_plug_tooltip() -> Self {
        Self::finish(
            CatalogContents {
                names: HashMap::from([(42, "Test Barrel".into())]),
                descriptions: HashMap::from([(42, "Improves handling.".into())]),
                item_package_metadata: HashMap::from([(
                    42,
                    ItemPackageMetadata {
                        plug_category_hash: Some(0x10001),
                        investment_stats: vec![ItemInvestmentStat {
                            definition_index: 0,
                            value: 7,
                        }],
                        ..Default::default()
                    },
                )]),
                item_stat_definitions: vec![ItemStatDefinition {
                    definition_index: 0,
                    hash: 13,
                    name: "Handling".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            PathBuf::new(),
            PathBuf::new(),
            false,
        )
    }
}
