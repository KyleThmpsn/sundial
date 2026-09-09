use super::items::{
    attach_item_objective_owners, infer_socket_label, infer_socket_plug_types,
    item_scan_progress_stride, socket_label_for_plug,
};
use super::scan::{retain_progression_enrichment, retain_progression_scan};
use crate::package_payload::{array_at, relative_offset, u64_at};

use super::*;

#[test]
#[ignore = "requires SUNDIAL_TEST_INSTALL pointing to the supported Shadowkeep build"]
fn supported_shadowkeep_build_loads_collection_expression_contracts() {
    let install = PathBuf::from(std::env::var("SUNDIAL_TEST_INSTALL").unwrap());
    let temp = crate::test_support::TestDirectory::new("collection-expressions");
    let catalog =
        Catalog::load_or_scan_with_progress(&install, temp.0.join("catalog.json"), true, |_| {})
            .unwrap();

    assert!(!catalog.loaded_from_cache);
    assert_eq!(
        catalog.shared_expression_pool().len(),
        6_541,
        "{}",
        catalog.progression_package_error().unwrap_or_default()
    );
    let collectible_contract = (
        catalog.collectibles().len(),
        catalog
            .collectibles()
            .iter()
            .filter(|definition| {
                definition
                    .conditions
                    .iter()
                    .any(|condition| condition.field == COLLECTIBLE_ACQUIRED_CONDITION_FIELD)
            })
            .count(),
    );
    // Later package overlays may append collectible definitions. The native
    // table contract is that exactly eleven entries lack an acquired-state
    // expression, including in both supported package variants observed so far.
    assert!(
        collectible_contract.0 >= 5_181,
        "unexpectedly short collectible table: {collectible_contract:?}"
    );
    assert_eq!(
        collectible_contract.0 - collectible_contract.1,
        11,
        "unexpected collectible acquisition-expression coverage: {collectible_contract:?}"
    );
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
    assert_eq!(rank_flags.len(), 128);
    assert_eq!(claim_flags.len(), 760);
    assert!(rank_flags.iter().chain(&claim_flags).all(|slot| {
        !catalog
            .unlock_flag_definition(usize::from(*slot))
            .unwrap()
            .tested_by
            .is_empty()
    }));
    assert_eq!(
        catalog
            .unlock_value_definitions()
            .iter()
            .flat_map(|definition| &definition.runtime_writers)
            .filter(|writer| matches!(writer, UnlockWriter::ValueCounter { .. }))
            .count(),
        62
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
fn item_scan_progress_updates_are_bounded_without_becoming_choppy() {
    assert_eq!(item_scan_progress_stride(0), 64);
    assert_eq!(item_scan_progress_stride(12_800), 64);
    assert_eq!(item_scan_progress_stride(100_000), 500);
    assert!(100_000_usize.div_ceil(item_scan_progress_stride(100_000)) <= 200);
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
        tested_by: vec![ProgressionContextDef {
            direct_references: Vec::new(),
            hash: 0x22EB_C08C,
            kind: ProgressionContextKind::Record,
            name: "Tradition Is Bigger Than You".into(),
            type_name: String::new(),
            description: String::new(),
            paths: Vec::new(),
            condition_programs: Vec::new(),
        }],
    };
    let catalog = Catalog::finish(
        CatalogContents {
            items: Vec::new(),
            names: HashMap::new(),
            type_names: HashMap::new(),
            package_item_names: HashMap::new(),
            package_item_type_names: HashMap::new(),
            descriptions: HashMap::new(),
            icon_containers: HashMap::new(),
            item_package_metadata: HashMap::new(),
            item_stat_definitions: Vec::new(),
            power_cap_definitions: Vec::new(),
            item_stat_groups: Vec::new(),
            trait_definitions: Vec::new(),
            reusable_plug_set_count: 0,
            socket_entry_list_count: 0,
            package_names: HashMap::new(),
            inventory_metadata: HashMap::new(),
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
            collectibles: Vec::new(),
            shared_expression_pool: Vec::new(),
            material_requirement_sets: Vec::new(),
            item_material_requirement_set_indices: HashMap::new(),
            progression_definitions: Vec::new(),
            progression_package_error: Some("Objective definitions: unavailable".to_owned()),
            plug_pools: Vec::new(),
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
fn progression_scan_failures_fall_back_without_discarding_other_sections() {
    let mut errors = Vec::new();
    let flags = retain_progression_scan("Unlock flag definitions", Ok(vec![1, 2]), &mut errors);
    let values: Vec<u8> = retain_progression_scan(
        "Unlock value definitions",
        Err("table unavailable".into()),
        &mut errors,
    );

    assert_eq!(flags, vec![1, 2]);
    assert!(values.is_empty());
    assert_eq!(errors, vec!["Unlock value definitions: table unavailable"]);
}

#[test]
fn optional_unlock_display_failure_keeps_core_definitions() {
    let definitions = vec![UnlockDefinition {
        hash: 0x1234_5678,
        code: 1,
        compact_slot: Some(26),
        name: None,
        description: None,
        runtime_writers: Vec::new(),
        tested_by: Vec::new(),
    }];
    let mut errors = Vec::new();

    let retained = retain_progression_enrichment(
        "Unlock flag displays",
        Err("table unavailable".into()),
        definitions.clone(),
        &mut errors,
    );

    assert_eq!(retained, definitions);
    assert_eq!(errors, vec!["Unlock flag displays: table unavailable"]);
}

#[test]
fn plug_labels_only_include_hashes_when_requested() {
    assert_eq!(format_plug_label("Rampage", 0x12AB, false), "Rampage");
    assert_eq!(
        format_plug_label("Rampage", 0x12AB, true),
        "Rampage  (0x000012AB)"
    );
}

#[test]
fn unnamed_item_objective_owners_keep_the_installed_bucket_type() {
    let mut objectives = vec![ObjectiveDef::default()];
    let metadata = InventoryMetadata {
        scope: InventoryScope::Character,
        native_bucket_id: 37,
        stackability: ItemStackability::Stackable,
        max_stack_size: Some(1),
        bucket_capacity: Some(64),
    };

    attach_item_objective_owners(
        &mut objectives,
        &[0],
        0x1234_5678,
        "",
        "",
        Some(metadata),
        &[],
    );

    assert_eq!(objectives[0].owners.len(), 1);
    assert!(objectives[0].owners[0].name.is_empty());
    assert_eq!(objectives[0].owners[0].type_name, "General inventory");
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
            package_item_names: HashMap::new(),
            package_item_type_names: HashMap::new(),
            descriptions: HashMap::new(),
            icon_containers: HashMap::new(),
            item_package_metadata: HashMap::new(),
            item_stat_definitions: Vec::new(),
            power_cap_definitions: Vec::new(),
            item_stat_groups: Vec::new(),
            trait_definitions: Vec::new(),
            reusable_plug_set_count: 0,
            socket_entry_list_count: 0,
            package_names: HashMap::new(),
            inventory_metadata,
            objectives: Vec::new(),
            unlock_flag_definitions: Vec::new(),
            unlock_value_definitions: Vec::new(),
            collectibles: Vec::new(),
            shared_expression_pool: Vec::new(),
            material_requirement_sets: Vec::new(),
            item_material_requirement_set_indices: HashMap::new(),
            progression_definitions: Vec::new(),
            progression_package_error: None,
            plug_pools: vec![Vec::new(), vec![41]],
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
            names: HashMap::new(),
            type_names: HashMap::new(),
            package_item_names: HashMap::new(),
            package_item_type_names: HashMap::new(),
            descriptions: HashMap::from([(10_042, "A description-only match".to_owned())]),
            icon_containers: HashMap::new(),
            item_package_metadata: HashMap::new(),
            item_stat_definitions: Vec::new(),
            power_cap_definitions: Vec::new(),
            item_stat_groups: Vec::new(),
            trait_definitions: Vec::new(),
            reusable_plug_set_count: 0,
            socket_entry_list_count: 0,
            package_names: HashMap::new(),
            inventory_metadata: HashMap::new(),
            objectives: Vec::new(),
            unlock_flag_definitions: Vec::new(),
            unlock_value_definitions: Vec::new(),
            collectibles: Vec::new(),
            shared_expression_pool: Vec::new(),
            material_requirement_sets: Vec::new(),
            item_material_requirement_set_indices: HashMap::new(),
            progression_definitions: Vec::new(),
            progression_package_error: None,
            plug_pools: Vec::new(),
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
fn schema_current_cache_requires_progression_and_power_sections() {
    let complete = serde_json::json!({
        "schema": CACHE_SCHEMA,
        "sundial_version": SUNDIAL_VERSION,
        "fingerprint": "test",
        "contents": {
            "items": [],
            "names": {"3365180871": "Test definition"},
            "type_names": {},
            "objectives": [],
            "unlock_flag_definitions": [],
            "unlock_value_definitions": [],
            "collectibles": [],
            "shared_expression_pool": [],
            "material_requirement_sets": [],
            "item_material_requirement_set_indices": {},
            "progression_definitions": [],
            "item_stat_groups": [],
            "power_cap_definitions": [],
            "plug_pools": [],
        },
    });
    let decoded = serde_json::from_value::<CatalogCache>(complete.clone()).unwrap();
    assert_eq!(
        decoded
            .contents
            .names
            .get(&3_365_180_871)
            .map(String::as_str),
        Some("Test definition")
    );

    for required in [
        "objectives",
        "unlock_flag_definitions",
        "unlock_value_definitions",
        "collectibles",
        "shared_expression_pool",
        "material_requirement_sets",
        "item_material_requirement_set_indices",
        "progression_definitions",
        "item_stat_groups",
        "power_cap_definitions",
    ] {
        let mut incomplete = complete.clone();
        incomplete["contents"]
            .as_object_mut()
            .unwrap()
            .remove(required);
        assert!(
            serde_json::from_value::<CatalogCache>(incomplete).is_err(),
            "a cache without {required} must be rescanned"
        );
    }
}

#[test]
fn really_unsafe_options_include_every_discovered_plug_once() {
    let catalog = plug_selection_catalog();
    assert_eq!(catalog.all_plug_options(), &[2, 3, 1, 4]);
    assert_eq!(catalog.plug_pools[1], [3, 1, 4]);
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
    catalog
        .gear_type_options
        .insert(GearKind::Weapon, vec![2, 3]);
    for (mode, expected) in [
        (PlugSelectionMode::Supported, vec![3, 1, 4]),
        (PlugSelectionMode::SocketAndGearType, vec![1]),
        (PlugSelectionMode::MatchingSocketType, vec![2, 1]),
        (PlugSelectionMode::GearType, vec![2, 3]),
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
}

fn plug_selection_catalog() -> Catalog {
    let names = HashMap::from([
        (1, "Zeta".to_owned()),
        (2, "Alpha".to_owned()),
        (3, "Beta".to_owned()),
    ]);
    Catalog::finish(
        CatalogContents {
            items: Vec::new(),
            names,
            type_names: HashMap::new(),
            package_item_names: HashMap::new(),
            package_item_type_names: HashMap::new(),
            descriptions: HashMap::new(),
            icon_containers: HashMap::new(),
            item_package_metadata: HashMap::new(),
            item_stat_definitions: Vec::new(),
            power_cap_definitions: Vec::new(),
            item_stat_groups: Vec::new(),
            trait_definitions: Vec::new(),
            reusable_plug_set_count: 0,
            socket_entry_list_count: 0,
            package_names: HashMap::new(),
            inventory_metadata: HashMap::new(),
            objectives: Vec::new(),
            unlock_flag_definitions: Vec::new(),
            unlock_value_definitions: Vec::new(),
            collectibles: Vec::new(),
            shared_expression_pool: Vec::new(),
            material_requirement_sets: Vec::new(),
            item_material_requirement_set_indices: HashMap::new(),
            progression_definitions: Vec::new(),
            progression_package_error: None,
            plug_pools: vec![Vec::new(), vec![4, 3, 1], vec![2, 3]],
        },
        PathBuf::new(),
        PathBuf::new(),
        false,
    )
}

#[test]
fn socket_labels_use_plug_semantics_and_keep_safe_fallbacks() {
    let names = HashMap::from([
        (1, "Default Shader".to_owned()),
        (2, "Celestial Nighthawk Ornament".to_owned()),
        (3, "Telesto Catalyst".to_owned()),
    ]);
    let type_names = HashMap::from([
        (1, "Restore Defaults".to_owned()),
        (2, "Hunter Universal Ornament".to_owned()),
    ]);

    assert_eq!(
        infer_socket_label(180, Some(1), &[1], &names, &type_names),
        "Shader"
    );
    assert_eq!(
        infer_socket_label(384, None, &[2], &names, &type_names),
        "Ornament"
    );
    assert_eq!(
        infer_socket_label(443, Some(3), &[3], &names, &type_names),
        "Catalyst"
    );
    assert_eq!(
        infer_socket_label(65535, None, &[], &names, &type_names),
        ""
    );
    assert_eq!(
        infer_socket_label(
            62,
            None,
            &[4],
            &names,
            &HashMap::from([(4, "Ghost Module".into())])
        ),
        "Sparrow Perk"
    );
    assert_eq!(
        infer_socket_label(29, None, &[], &names, &type_names),
        "Armor Masterwork"
    );
    assert_eq!(
        infer_socket_label(51, None, &[], &names, &type_names),
        "Ghost Perk"
    );
    assert_eq!(
        infer_socket_label(520, None, &[], &names, &type_names),
        "Armor Tier"
    );
    assert_eq!(
        infer_socket_label(676, None, &[], &names, &type_names),
        "Stat Allocation"
    );
    assert_eq!(
        infer_socket_label(678, None, &[], &names, &type_names),
        "Armor Energy Upgrade"
    );
    assert_eq!(
        infer_socket_label(760, None, &[], &names, &type_names),
        "Top Stat Allocation"
    );
    assert_eq!(
        infer_socket_label(763, None, &[], &names, &type_names),
        "Bottom Stat Allocation"
    );
    assert_eq!(
        socket_label_for_plug(
            4,
            &HashMap::from([(4, "Upgrade Armor".into())]),
            &HashMap::new()
        )
        .as_deref(),
        Some("Armor Energy Upgrade")
    );
}

#[test]
fn armor_socket_types_fill_only_missing_plug_types() {
    let items = vec![ItemDef {
        hash: 10,
        name: "Test armor".into(),
        type_name: "Helmet".into(),
        bucket_hash: 3_448_274_439,
        class_type: 3,
        default_plugs: vec![Some("0x00000001".into())],
        sockets: vec![SocketDef {
            socket_type: 520,
            allowed: vec![2],
            ..SocketDef::default()
        }],
        abilities: AbilityOptions::default(),
    }];
    let names = HashMap::from([(3, "Empty Mod Socket".into())]);
    let mut type_names = HashMap::from([(2, "Specific local type".into())]);

    infer_socket_plug_types(&items, &names, &mut type_names);

    assert_eq!(type_names[&1], "Armor Tier");
    assert_eq!(type_names[&2], "Specific local type");
    assert_eq!(type_names[&3], "Armor Mod");
}

#[test]
fn ghost_perk_socket_replaces_the_generic_intrinsic_type() {
    let items = vec![ItemDef {
        hash: 10,
        name: "Test Ghost".into(),
        type_name: "Ghost Shell".into(),
        bucket_hash: 4_023_194_814,
        class_type: 3,
        default_plugs: vec![Some("0x00000001".into())],
        sockets: vec![SocketDef {
            socket_type: 51,
            allowed: vec![2],
            ..SocketDef::default()
        }],
        abilities: AbilityOptions::default(),
    }];
    let mut type_names = HashMap::from([(1, "Intrinsic".to_owned()), (2, "Intrinsic".to_owned())]);

    infer_socket_plug_types(&items, &HashMap::new(), &mut type_names);

    assert_eq!(type_names[&1], "Ghost Perk");
    assert_eq!(type_names[&2], "Ghost Perk");
}

#[test]
fn socket_display_labels_preserve_the_native_position() {
    let named = SocketDef {
        label: "Barrel".into(),
        ..SocketDef::default()
    };
    let unnamed = SocketDef::default();

    assert_eq!(named.display_label(1), "2. Barrel");
    assert_eq!(unnamed.display_label(1), "Socket 2");
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
#[ignore = "requires SUNDIAL_TEST_INSTALL pointing to the supported Shadowkeep build"]
fn supported_shadowkeep_build_loads_v13_equipment() {
    let install = PathBuf::from(std::env::var("SUNDIAL_TEST_INSTALL").unwrap());
    let temp = crate::test_support::TestDirectory::new("v13-equipment");
    let catalog =
        Catalog::load_or_scan_with_progress(&install, temp.0.join("catalog.json"), true, |_| {})
            .unwrap();
    let wheel = catalog
        .get_for_bucket(
            crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH,
            crate::account_contract::EMOTE_BUCKET_HASH,
        )
        .expect("emote collection must be an editable equipment definition");
    assert_eq!(wheel.sockets.len(), 4);
    assert_eq!(wheel.default_plugs.len(), 4);
    assert!(catalog.get_for_bucket(0x613A3DA6, 0x59CA_1EA2).is_some());
}

#[test]
#[ignore = "requires SUNDIAL_TEST_INSTALL pointing to the supported Shadowkeep build"]
fn installed_power_cap_table_survives_catalog_cache_roundtrip() {
    let install = PathBuf::from(std::env::var("SUNDIAL_TEST_INSTALL").unwrap());
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
    assert!(resolved > 1000);
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
