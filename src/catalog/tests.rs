use super::items::{ItemSandboxPerk, SocketOptionSource, SocketOptionSourceKind};
use super::scan::{retain_progression_enrichment, retain_progression_scan};
use crate::package_payload::{array_at, relative_offset, u64_at};

use super::*;

#[test]
fn component_descriptions_survive_cache_round_trip_without_using_item_flavor() {
    let contents = CatalogContents {
        descriptions: HashMap::from([(10, "A broad item description".into())]),
        perk_descriptions: HashMap::from([(7, "Collecting a cell emits an impulse".into())]),
        ..Default::default()
    };
    let stored = serde_json::to_vec(&contents).unwrap();
    let restored = serde_json::from_slice(&stored).unwrap();
    let catalog = Catalog::finish(restored, PathBuf::new(), PathBuf::new(), true);
    assert_eq!(
        catalog.perk_description(7),
        Some("Collecting a cell emits an impulse")
    );
    assert_eq!(catalog.perk_description(10), None);
}

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

/// A failed section names itself and costs only itself: the scan helper falls back to a default
/// and the enrichment helper hands back what it was given, so neither takes the rest of the
/// catalog down with it.
#[test]
fn a_failed_progression_section_is_reported_without_discarding_the_others() {
    let mut errors = Vec::new();
    let flags = retain_progression_scan("Unlock flag definitions", Ok(vec![1, 2]), &mut errors);
    let values: Vec<u8> = retain_progression_scan(
        "Unlock value definitions",
        Err("table unavailable".into()),
        &mut errors,
    );
    let retained = retain_progression_enrichment(
        "Unlock flag displays",
        Err("table unavailable".into()),
        vec![0x1234_5678_u32],
        &mut errors,
    );

    assert_eq!(flags, vec![1, 2]);
    assert!(values.is_empty());
    assert_eq!(retained, vec![0x1234_5678]);
    assert_eq!(
        errors,
        vec![
            "Unlock value definitions: table unavailable",
            "Unlock flag displays: table unavailable"
        ]
    );
}

fn assert_profile_inventory_apis(catalog: &Catalog) {
    assert_eq!(catalog.item(30).unwrap().name, "Character item");
    assert!(catalog.item(10).is_none());
    let profile_only = catalog.inventory_definition(10).unwrap();
    assert_eq!(profile_only.name, "Alpha material");
    assert_eq!(profile_only.type_name, "Currency");
    assert!(profile_only.item.is_none());
    assert_eq!(
        catalog
            .profile_item_candidates("")
            .map(|definition| definition.hash)
            .collect::<Vec<_>>(),
        vec![10, 20]
    );
    assert_eq!(catalog.profile_item_candidates("material").count(), 2);
}

fn assert_character_inventory_apis(catalog: &Catalog) {
    assert!(catalog.browse(3_284_755_031, 0, true, false).is_empty());
    assert_eq!(
        catalog
            .browse(3_284_755_031, 0, true, true)
            .iter()
            .map(|item| item.hash)
            .collect::<Vec<_>>(),
        vec![31]
    );
    assert_eq!(
        catalog
            .search("foreign", 3_284_755_031, 0, true, true)
            .iter()
            .map(|item| item.hash)
            .collect::<Vec<_>>(),
        vec![31]
    );
    assert_eq!(
        catalog
            .browse(3_448_274_439, 0, true, true)
            .iter()
            .map(|item| item.hash)
            .collect::<Vec<_>>(),
        vec![30]
    );
    assert_eq!(
        catalog
            .character_inventory_candidates("", 0, false, false)
            .next()
            .unwrap()
            .hash,
        30
    );
    assert_eq!(
        catalog
            .character_inventory_candidates("", 0, false, true)
            .map(|definition| definition.hash)
            .collect::<Vec<_>>(),
        vec![30, 31]
    );
    assert_eq!(
        catalog
            .character_inventory_candidate_buckets(0, false)
            .iter()
            .map(|metadata| metadata.native_bucket_id)
            .collect::<Vec<_>>(),
        vec![3]
    );
    assert_eq!(
        catalog
            .character_inventory_candidate_buckets(99, false)
            .iter()
            .map(|metadata| metadata.native_bucket_id)
            .collect::<Vec<_>>(),
        vec![3]
    );
    assert_eq!(
        catalog
            .inventory_metadata(30)
            .unwrap()
            .authored_row_capacity(),
        Some(20)
    );
}

#[test]
fn inventory_apis_resolve_profile_only_items_and_keep_character_items_safe() {
    let character = ItemDef {
        hash: 30,
        name: "Character item".into(),
        type_name: "Helmet".into(),
        bucket_hash: 3_448_274_439,
        class_type: 3,
        default_plugs: vec![Some("0x00000028".into())],
        sockets: Vec::new(),
        abilities: AbilityOptions::default(),
    };
    let foreign_subclass = ItemDef {
        hash: 31,
        name: "Foreign subclass".into(),
        type_name: "Subclass".into(),
        bucket_hash: 3_284_755_031,
        class_type: 1,
        default_plugs: Vec::new(),
        sockets: Vec::new(),
        abilities: AbilityOptions::default(),
    };
    let foreign_helmet = ItemDef {
        hash: 32,
        name: "Foreign helmet".into(),
        type_name: "Helmet".into(),
        bucket_hash: 3_448_274_439,
        class_type: 1,
        default_plugs: Vec::new(),
        sockets: Vec::new(),
        abilities: AbilityOptions::default(),
    };
    let names = HashMap::from([
        (20, "Zeta material".into()),
        (10, "Alpha material".into()),
        (30, character.name.clone()),
        (31, foreign_subclass.name.clone()),
        (32, foreign_helmet.name.clone()),
    ]);
    let type_names = HashMap::from([
        (10, "Currency".into()),
        (20, "Material".into()),
        (30, character.type_name.clone()),
        (31, foreign_subclass.type_name.clone()),
        (32, foreign_helmet.type_name.clone()),
    ]);
    let profile = |bucket| InventoryMetadata {
        scope: InventoryScope::Profile,
        native_bucket_id: bucket,
        stackability: ItemStackability::Stackable,
        max_stack_size: Some(999),
        bucket_capacity: Some(10),
    };
    let character_inventory = |bucket| InventoryMetadata {
        scope: InventoryScope::Character,
        native_bucket_id: bucket,
        stackability: ItemStackability::Instanced,
        max_stack_size: Some(1),
        bucket_capacity: Some(20),
    };
    let inventory_metadata = HashMap::from([
        (10, profile(1)),
        (20, profile(2)),
        (30, character_inventory(3)),
        (31, character_inventory(16)),
        (32, character_inventory(3)),
    ]);
    let catalog = Catalog::finish(
        CatalogContents {
            items: vec![character, foreign_subclass, foreign_helmet],
            names,
            type_names,
            inventory_metadata,
            plug_pools: vec![Vec::new(), vec![41]],
            ..Default::default()
        },
        PathBuf::new(),
        PathBuf::new(),
        false,
    );

    assert_profile_inventory_apis(&catalog);
    assert_character_inventory_apis(&catalog);
    assert!(catalog.contains_plug(40));
    assert!(catalog.contains_plug(41));
    assert!(!catalog.contains_plug(30));
}

#[test]
fn equipment_browse_and_search_return_every_compatible_item() {
    let bucket = 1_498_876_634;
    let items = (0_u64..620)
        .rev()
        .map(|index| ItemDef {
            hash: 10_000 + index,
            name: format!("Matching item {index:04}"),
            type_name: "Test weapon".into(),
            bucket_hash: bucket,
            class_type: 3,
            default_plugs: Vec::new(),
            sockets: Vec::new(),
            abilities: AbilityOptions::default(),
        })
        .chain(std::iter::once(ItemDef {
            hash: 99_999,
            name: "Matching incompatible item".into(),
            type_name: "Test weapon".into(),
            bucket_hash: 0,
            class_type: 3,
            default_plugs: Vec::new(),
            sockets: Vec::new(),
            abilities: AbilityOptions::default(),
        }))
        .collect();
    let catalog = Catalog::finish(
        CatalogContents {
            items,
            descriptions: HashMap::from([(10_042, "A description-only match".to_owned())]),
            ..Default::default()
        },
        PathBuf::new(),
        PathBuf::new(),
        false,
    );

    let browsed = catalog.browse(bucket, 0, true, false);
    assert_eq!(browsed.len(), 620);
    assert_eq!(browsed.first().unwrap().name, "Matching item 0000");
    assert_eq!(browsed.last().unwrap().name, "Matching item 0619");
    let bucket_items = catalog.items_for_bucket(bucket).collect::<Vec<_>>();
    assert_eq!(bucket_items.len(), 620);
    assert_eq!(bucket_items.first().unwrap().name, "Matching item 0000");
    assert_eq!(bucket_items.last().unwrap().name, "Matching item 0619");
    assert_eq!(catalog.items_for_bucket(0).count(), 0);

    let searched = catalog.search("matching", bucket, 0, true, false);
    assert_eq!(searched.len(), 620);
    assert_eq!(searched.first().unwrap().name, "Matching item 0000");
    assert_eq!(searched.last().unwrap().name, "Matching item 0619");
    assert_eq!(
        catalog
            .search("description-only", bucket, 0, true, false)
            .len(),
        1
    );
    assert_eq!(
        catalog
            .search("matching description-only", bucket, 0, true, false)
            .iter()
            .map(|item| item.hash)
            .collect::<Vec<_>>(),
        vec![10_042]
    );
    assert!(
        catalog
            .search("description-only absent", bucket, 0, true, false)
            .is_empty()
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
fn supported_shadowkeep_build_loads_v13_equipment() {
    let install = PathBuf::from(std::env::var("SUNDIAL_INSTALL").unwrap());
    let temp = crate::test_support::TestDirectory::new("v13-equipment");
    let catalog =
        Catalog::load_or_scan_with_progress(&install, temp.0.join("catalog.json"), true, |_| {})
            .unwrap();
    let wheel = catalog
        .get_for_bucket(
            crate::account::contract::EMOTE_COLLECTION_DEFINITION_HASH,
            crate::account::contract::EMOTE_BUCKET_HASH,
        )
        .expect("emote collection must be an editable equipment definition");
    assert_eq!(wheel.sockets.len(), 4);
    assert_eq!(wheel.default_plugs.len(), 4);
    assert!(catalog.get_for_bucket(0x613A3DA6, 0x59CA_1EA2).is_some());
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

#[test]
fn overridden_cosmetic_sockets_restore_only_their_socket_pool() {
    let item = ItemDef {
        hash: 10,
        name: "Example Weapon".into(),
        type_name: "Sidearm".into(),
        bucket_hash: 1_498_876_634,
        class_type: 3,
        default_plugs: vec![],
        sockets: vec![
            SocketDef {
                socket_type: 746,
                allowed: vec![1],
                ..Default::default()
            },
            SocketDef {
                socket_type: 100,
                allowed: vec![2],
                ..Default::default()
            },
        ],
        abilities: AbilityOptions::default(),
    };
    let catalog = Catalog::for_test(vec![item.clone()], HashMap::new());
    assert_eq!(catalog.gear_type_options_for_type(&item, 100), vec![2]);
    let cosmetic = catalog.gear_type_options_for_type(&item, 746);
    assert_eq!(cosmetic.len(), 2);
    assert!(cosmetic.contains(&1));
    assert!(cosmetic.contains(&2));
    assert_eq!(catalog.gear_type_options_for_type(&item, 999), vec![2]);
}
fn test_item(hash: u64, name: &str, type_name: &str) -> ItemDef {
    ItemDef {
        hash,
        name: name.into(),
        type_name: type_name.into(),
        bucket_hash: 1_498_876_634,
        class_type: 3,
        default_plugs: Vec::new(),
        sockets: Vec::new(),
        abilities: AbilityOptions::default(),
    }
}

fn test_node(hash: u64, name: &str, parents: Vec<u64>) -> PresentationNode {
    PresentationNode {
        hash,
        name: name.into(),
        parents,
    }
}

fn test_collectible(
    hash: u64,
    item_hash: u64,
    name: &str,
    parent_nodes: Vec<u64>,
) -> CollectibleDef {
    CollectibleDef {
        index: 0,
        hash,
        item_definition_index: 0,
        item_hash,
        material_requirement_set_index: None,
        material_requirement_set_hash: 0,
        material_requirements: Vec::new(),
        name: name.into(),
        type_name: "Hand Cannon".into(),
        paths: Vec::new(),
        conditions: Vec::new(),
        parent_nodes,
    }
}

#[test]
#[expect(
    clippy::cognitive_complexity,
    reason = "One fixture tree is checked path by path and node by node in one place"
)]
fn presentation_paths_follow_first_parents_and_children_list_every_parent() {
    let catalog = Catalog::for_test(Vec::new(), HashMap::new())
        .with_test_presentation_nodes(vec![
            test_node(1, "Collections", vec![]),
            test_node(2, "Weapons", vec![1]),
            test_node(3, "Kinetic", vec![2, 4]),
            test_node(4, "Legacy", vec![1]),
            test_node(5, "Loop A", vec![6]),
            test_node(6, "Loop B", vec![5]),
        ])
        .with_test_collectibles(vec![test_collectible(50, 500, "Ace of Spades", vec![3])])
        .with_test_records(vec![RecordDefinition {
            hash: 60,
            name: "Gunsmith".into(),
            parent_nodes: vec![2],
            ..Default::default()
        }]);
    let path = |hash| {
        catalog
            .presentation_path(hash)
            .iter()
            .map(|node| node.hash)
            .collect::<Vec<_>>()
    };
    let child_nodes = |hash| {
        catalog
            .presentation_node_children(hash)
            .nodes
            .iter()
            .map(|node| node.hash)
            .collect::<Vec<_>>()
    };

    assert_eq!(path(3), [1, 2]);
    assert_eq!(path(50), [1, 2, 3]);
    assert_eq!(path(60), [1, 2]);
    assert!(path(1).is_empty());
    assert!(path(999).is_empty());
    assert_eq!(path(5), [6]);
    assert_eq!(child_nodes(1), [2, 4]);
    assert_eq!(child_nodes(2), [3]);
    assert_eq!(child_nodes(4), [3]);
    assert!(child_nodes(999).is_empty());
    let weapons = catalog.presentation_node_children(2);
    assert!(weapons.collectibles.is_empty());
    assert_eq!(
        weapons
            .records
            .iter()
            .map(|record| record.hash)
            .collect::<Vec<_>>(),
        [60]
    );
    assert_eq!(
        catalog
            .presentation_node_children(3)
            .collectibles
            .iter()
            .map(|collectible| collectible.hash)
            .collect::<Vec<_>>(),
        [50]
    );
    assert_eq!(
        catalog.presentation_node_hashes().to_vec(),
        [1, 2, 3, 4, 5, 6]
    );
    assert_eq!(catalog.display_name(4), Some("Legacy"));
}

#[test]
fn plug_offers_list_each_socket_once_and_flag_defaults() {
    let source = |kind, valid, members: Vec<u64>| SocketOptionSource {
        kind,
        pool: 0,
        valid,
        ordered_members: members.clone(),
        allowed: members,
    };
    let socket = |sources: Vec<SocketOptionSource>| SocketDef {
        socket_type: 1,
        allowed: sources
            .iter()
            .flat_map(|source| source.allowed.iter().copied())
            .collect(),
        sources,
        ..SocketDef::default()
    };
    let mut alpha = test_item(10, "Alpha Rifle", "Auto Rifle");
    alpha.default_plugs = vec![Some(format_hash_hex(101)), None];
    alpha.sockets = vec![
        socket(vec![
            source(SocketOptionSourceKind::Embedded, true, vec![100, 101]),
            source(
                SocketOptionSourceKind::ReusableSet { index: 3 },
                true,
                vec![101, 102],
            ),
        ]),
        socket(vec![source(
            SocketOptionSourceKind::ReusableSet { index: 4 },
            false,
            vec![100],
        )]),
    ];
    let mut beta = test_item(11, "Beta Rifle", "Auto Rifle");
    beta.default_plugs = vec![None];
    beta.sockets = vec![socket(vec![source(
        SocketOptionSourceKind::RandomizedSet { index: 5 },
        true,
        vec![100],
    )])];
    let mut aardvark = test_item(12, "Aardvark", "Emblem");
    aardvark.default_plugs = vec![Some(format_hash_hex(100))];
    aardvark.sockets = vec![socket(Vec::new())];
    let catalog = Catalog::for_test(vec![beta, alpha, aardvark], HashMap::new());
    let offer = |item_hash, socket_index, is_default| PlugOffer {
        item_hash,
        socket_index,
        is_default,
    };

    assert_eq!(
        catalog.plug_offers(100),
        [offer(12, 0, true), offer(10, 0, false), offer(11, 0, false)]
    );
    assert_eq!(catalog.plug_offers(101), [offer(10, 0, true)]);
    assert_eq!(catalog.plug_offers(102), [offer(10, 0, false)]);
    assert!(catalog.plug_offers(999).is_empty());
}

#[test]
fn records_rewarding_an_item_include_completion_and_interval_rewards() {
    let metadata = |definition_index| ItemPackageMetadata {
        definition_index,
        ..Default::default()
    };
    let record = |hash, name: &str, rewards, interval_items| RecordDefinition {
        hash,
        name: name.into(),
        runtime: Some(RecordRuntime {
            rewards,
            interval_items,
            ..Default::default()
        }),
        ..Default::default()
    };
    let catalog = Catalog::for_test(
        Vec::new(),
        HashMap::from([(500, metadata(7)), (501, metadata(8))]),
    )
    .with_test_records(vec![
        record(60, "Zeta Triumph", vec![(7, 3)], vec![None, Some(7)]),
        record(61, "Alpha Triumph", vec![(8, 1), (7, 2)], Vec::new()),
        RecordDefinition {
            hash: 62,
            name: "No Runtime".into(),
            ..Default::default()
        },
    ]);
    let reward = |record_index, interval, quantity| RecordRewardUse {
        record_index,
        interval,
        quantity,
    };

    assert_eq!(
        catalog.records_rewarding_item(500),
        [
            reward(1, None, 2),
            reward(0, None, 3),
            reward(0, Some(1), 1)
        ]
    );
    assert_eq!(catalog.records_rewarding_item(501), [reward(1, None, 1)]);
    assert!(catalog.records_rewarding_item(999).is_empty());
}

#[test]
fn metadata_reverse_lookups_sort_items_by_name() {
    let metadata =
        |perks: &[u16], trait_indices: Vec<u16>, plug_category_hash| ItemPackageMetadata {
            sandbox_perks: perks
                .iter()
                .map(|&perk_index| ItemSandboxPerk {
                    perk_index,
                    active: true,
                })
                .collect(),
            trait_indices,
            plug_category_hash,
            ..Default::default()
        };
    let stat_group = |stats: &[u16]| {
        let mut group = ItemStatGroup::default();
        group
            .scaled_stats
            .resize_with(stats.len(), Default::default);
        for (stat, &definition_index) in group.scaled_stats.iter_mut().zip(stats) {
            stat.definition_index = definition_index;
        }
        group
    };
    let catalog = Catalog::finish(
        CatalogContents {
            names: HashMap::from([
                (10, "Zeta".to_owned()),
                (11, "Alpha".to_owned()),
                (12, String::new()),
            ]),
            item_package_metadata: HashMap::from([
                (10, metadata(&[7], vec![1], Some(900))),
                (11, metadata(&[7, 7], vec![1, 2], Some(900))),
                (12, metadata(&[], vec![1], None)),
            ]),
            item_material_requirement_set_indices: HashMap::from([
                (
                    10,
                    ItemMaterialRequirementSetIndices {
                        insertion: Some(3),
                        enabled: Some(3),
                    },
                ),
                (
                    11,
                    ItemMaterialRequirementSetIndices {
                        insertion: None,
                        enabled: Some(3),
                    },
                ),
            ]),
            item_stat_groups: vec![stat_group(&[4, 4]), stat_group(&[5]), stat_group(&[4])],
            trait_definitions: vec![
                ObjectiveOwnerTraitDef {
                    hash: 0x10,
                    name: "First".into(),
                    description: String::new(),
                },
                ObjectiveOwnerTraitDef {
                    hash: 0x20,
                    name: "Second".into(),
                    description: String::new(),
                },
            ],
            ..Default::default()
        },
        PathBuf::new(),
        PathBuf::new(),
        false,
    );

    assert_eq!(catalog.items_with_sandbox_perk(7), [11, 10]);
    assert_eq!(catalog.items_with_trait(1), [11, 10, 12]);
    assert_eq!(catalog.items_with_trait(2), [11]);
    assert_eq!(catalog.plugs_in_category(900), [11, 10]);
    assert_eq!(
        catalog.items_using_material_requirement_set(3),
        [
            (11, MaterialSetUse::Enabled),
            (10, MaterialSetUse::Insertion),
            (10, MaterialSetUse::Enabled),
        ]
    );
    assert_eq!(catalog.stat_groups_with_stat(4), [0, 2]);
    assert!(catalog.stat_groups_with_stat(6).is_empty());
    assert_eq!(
        catalog
            .item_trait(0x20)
            .map(|(index, definition)| (index, definition.name.as_str())),
        Some((1, "Second"))
    );
    assert!(catalog.item_trait(0x30).is_none());
}

#[test]
fn definition_search_ranks_exact_prefix_word_prefix_then_substring() {
    let catalog = Catalog::for_test(
        vec![
            test_item(1, "Ace of Spades", "Hand Cannon"),
            test_item(2, "Ace", "Emblem"),
            test_item(3, "Grace Period", "Auto Rifle"),
            test_item(4, "The Ace Card", "Emblem"),
        ],
        HashMap::new(),
    )
    .with_test_presentation_nodes(vec![
        test_node(30, "Weapons", vec![]),
        test_node(31, "Kinetic", vec![30]),
    ])
    .with_test_collectibles(vec![test_collectible(20, 1, "Ace of Spades", vec![31])]);
    let hashes = |query: &str, limit| {
        catalog
            .search_definitions(query, limit)
            .iter()
            .map(|hit| hit.hash)
            .collect::<Vec<_>>()
    };

    assert_eq!(hashes("ace", 10), [2, 1, 20, 4, 3]);
    assert_eq!(hashes("ace", 2), [2, 1]);
    assert_eq!(hashes("ACE  spades", 10), [1, 20]);
    assert_eq!(hashes("spades ace", 10), [1, 20]);
    assert_eq!(hashes("ace of", 10), [1, 20]);
    assert!(hashes("ace missing", 10).is_empty());
    assert!(hashes("   ", 10).is_empty());
    assert!(hashes("ace", 0).is_empty());

    let hits = catalog.search_definitions("spades", 10);
    assert_eq!(
        hits[0],
        DefinitionSearchHit {
            hash: 1,
            name: "Ace of Spades".into(),
            kind: "Weapon",
            detail: "Hand Cannon".into(),
            icon: Some(1),
        }
    );
    assert_eq!(
        (hits[1].kind, hits[1].detail.as_str()),
        ("Collectible", "Weapons \u{203A} Kinetic")
    );
    let nodes = catalog.search_definitions("kinetic", 10);
    assert_eq!(nodes.len(), 1);
    assert_eq!(
        (nodes[0].kind, nodes[0].detail.as_str()),
        ("Presentation Node", "Weapons")
    );
}
