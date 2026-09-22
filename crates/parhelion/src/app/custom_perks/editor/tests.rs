use super::*;
use sundial::package_authoring::weapon_runtime::{
    WeaponRuntimeBinding, WeaponRuntimeOwner, WeaponRuntimeRoot, WeaponRuntimeRootKind,
};

pub(in crate::app) fn fixture() -> PrivatePerkRuntimeGraph {
    let roots = [
        (WeaponRuntimeRootKind::ComponentInstance, 0x8080_3B73, 0x144),
        (
            WeaponRuntimeRootKind::ComponentDefinition,
            0x8080_388F,
            0x88,
        ),
    ]
    .into_iter()
    .map(|(root, schema, offset)| {
        let field = WeaponRuntimeField {
            locator: WeaponRuntimeFieldLocator {
                graph_tag: None,
                binding_hash: 1,
                resource_index: 0,
                root,
                root_schema: schema,
                path: vec![],
                type_handle: 2,
                value_offset: offset,
                byte_size: 8,
            },
            owner_offset: offset,
            name: "Projectile".into(),
            path_label: "Projectile".into(),
            kind: WeaponRuntimeValueKind::FixedBytes { size: 8 },
            value: WeaponRuntimeValue::Bytes([1.0f32.to_le_bytes(), [0; 4]].concat()),
            source: WeaponRuntimeFieldSource::OpaqueNativeType,
            generated_kind: None,
            name_inferred: false,
        };
        WeaponRuntimeRoot {
            kind: root,
            schema,
            owner_offset: 0,
            byte_size: 512,
            generated_schema: false,
            structure: Default::default(),
            fields: vec![field],
        }
    })
    .collect();
    let graph = WeaponRuntimeGraph {
        item_hash: 1,
        pattern_global_id_hash: 0,
        entity_tag: 0x8152_82E1,
        bindings: vec![WeaponRuntimeBinding {
            binding_hash: 1,
            binding_label: "Projectile".into(),
            resource_index: 0,
            resource_count: 1,
            owner_tag: 0x8152_82E7,
            concrete_class: 0x8080_3B73,
            resource_offset: 0,
        }],
        resources: vec![],
        owners: vec![WeaponRuntimeOwner {
            owner_tag: 0x8152_82E7,
            anchor_binding_hash: 1,
            anchor_resource_index: 0,
            roots,
        }],
    };
    let mut payload = vec![0; 0x200];
    payload[..8].copy_from_slice(&0x200u64.to_le_bytes());
    payload[0x1C..0x20].copy_from_slice(&0x8080_2F16u32.to_le_bytes());
    payload[0x160..0x168].copy_from_slice(&0x60u64.to_le_bytes());
    payload[0x1BC..0x1C0].copy_from_slice(&0x8080_2F1Au32.to_le_bytes());
    payload[0x1C0..0x1C4].copy_from_slice(&0.5f32.to_le_bytes());
    PrivatePerkRuntimeGraph {
        action_tag: 0x80BC_2BBD,
        action_payload: payload,
        summary: None,
        program: None,
        graphs: vec![(graph.entity_tag, graph)],
        warnings: vec![],
        graph_errors: vec![],
        loading_issues: vec![],
        projectile_slots: vec![],
        projectile_catalog: Arc::default(),
        native_assets: Vec::new(),
    }
}

pub(in crate::app) fn editor(loaded: PrivatePerkRuntimeGraph) -> PerkEditor {
    PerkEditor {
        history: Default::default(),
        activation: None,
        preview: None,
        entity_source: None,
        key: PerkEditorKey {
            socket_index: 0,
            choice_index: 0,
            source_plug_hash: 1,
            source_perk_index: 1178,
        },
        plug_label: "Micro-Missile".into(),
        packages: PathBuf::new(),
        draft: vec![],
        action_draft: vec![],
        projectile_draft: vec![],
        original_projectile_draft: vec![],
        projectile_labels: BTreeMap::new(),
        item_names: BTreeMap::new(),
        projectile_query: String::new(),
        pending_movement: None,
        parameter_error: None,
        graph: Some(Arc::new(loaded)),
        error: None,
        receiver: None,
        worker: None,
        query: String::new(),
        value_text: BTreeMap::new(),
        show_all_native_values: false,
        conversion: None,
        original_draft: vec![],
        original_action_draft: vec![],
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn activity_asset_properties_allow_dependencies_that_the_build_enrolls() {
    let path = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    for tag in [0x80C107D8, 0x80BFBBAB] {
        let report = projectile::residency::inspect(&manager, tag).unwrap();
        if tag == 0x80C107D8 {
            assert!(
                !report.additions().is_empty(),
                "fixture must need enrollment"
            );
        }
        assert!(loading_issues(&manager, [("Asset", tag, false)]).is_empty());
        assert!(loading_issues(&manager, [("Projectile", tag, true)]).is_empty());
        let loaded = load_entity_parameters(&path, tag).unwrap();
        assert!(loaded.graph_errors.is_empty());
        let mut draft = editor(loaded);
        let loaded = draft.graph.as_ref().unwrap();
        let field = loaded
            .graphs
            .iter()
            .flat_map(|(_, graph)| graph.fields())
            .find(|field| {
                runtime_field_is_editable(field)
                    && validation::fields_for(loaded, &field.locator).len() == 1
                    && validation::finite(&field.value)
            })
            .unwrap();
        draft.draft.push(WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: field.value.clone(),
        });
        assert!(
            draft.validation_errors().is_empty(),
            "{:?}",
            draft.validation_errors()
        );
    }
    assert!(!loading_issues(&manager, [("Asset", 0xDEADBEEF, false)]).is_empty());
}

#[test]
fn verified_speed_updates_both_lanes_and_reset_preserves_other_bytes() {
    let loaded = fixture();
    let speed = guided::ProjectileSpeed::discover(&loaded).unwrap();
    let mut draft = vec![];
    speed.set(&loaded, &mut draft, 62.5).unwrap();
    assert_eq!(draft.len(), 2);
    assert_eq!(speed.value(&loaded, &draft).unwrap(), 62.5);
    if let WeaponRuntimeValue::Bytes(bytes) = &mut draft[0].value {
        bytes[7] = 99;
    }
    speed.set(&loaded, &mut draft, 1.0).unwrap();
    assert_eq!(draft.len(), 1);
    assert_eq!(speed.value(&loaded, &draft).unwrap(), 1.0);
    assert!(matches!(&draft[0].value, WeaponRuntimeValue::Bytes(bytes) if bytes[7] == 99));
    let before = draft.clone();
    for value in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(speed.set(&loaded, &mut draft, value).is_err());
        assert_eq!(draft, before);
    }
}

#[test]
fn speed_requires_the_known_perk_and_rejects_mismatched_lanes() {
    let mut loaded = fixture();
    let speed = guided::ProjectileSpeed::discover(&loaded).unwrap();
    let mut draft = vec![];
    speed.set(&loaded, &mut draft, 2.0).unwrap();
    draft.pop();
    assert!(speed.value(&loaded, &draft).is_err());
    loaded.action_tag = 123;
    assert!(guided::ProjectileSpeed::discover(&loaded).is_none());
}

#[test]
fn guided_profiles_require_unique_paths_matching_owners_and_native_defaults() {
    let original = fixture();
    assert_eq!(guided::ProjectileSpeed::discover_all(&original).len(), 1);
    let mut wrong_owner = original.clone();
    wrong_owner.graphs[0].1.bindings[0].owner_tag = 123;
    assert!(guided::ProjectileSpeed::discover_all(&wrong_owner).is_empty());
    let mut wrong_graph = original.clone();
    wrong_graph.graphs[0].0 = 123;
    assert!(guided::ProjectileSpeed::discover_all(&wrong_graph).is_empty());
    let mut duplicate_graph = original.clone();
    duplicate_graph.graphs.push(original.graphs[0].clone());
    assert!(guided::ProjectileSpeed::discover_all(&duplicate_graph).is_empty());
    let mut wrong_default = original.clone();
    for root in &mut wrong_default.graphs[0].1.owners[0].roots {
        if let WeaponRuntimeValue::Bytes(bytes) = &mut root.fields[0].value {
            bytes[..4].copy_from_slice(&30.0f32.to_le_bytes());
        }
    }
    assert!(guided::ProjectileSpeed::discover_all(&wrong_default).is_empty());
    let mut overflow = original;
    overflow.graphs[0].1.owners[0].roots[0].fields[0]
        .locator
        .byte_size = u32::MAX;
    assert!(guided::ProjectileSpeed::discover_all(&overflow).is_empty());
}

#[test]
fn action_only_edits_validate_expected_bits_and_duplicate_targets() {
    let mut loaded = fixture();
    loaded.graphs.clear();
    let mut values = actions::discover(&loaded.action_payload);
    assert_eq!(values.len(), 1);
    values[0].value_bits = 2.0f32.to_bits();
    let mut editor = editor(loaded);
    editor.action_draft = values;
    assert!(editor.validation_errors().is_empty());
    editor.action_draft.push(editor.action_draft[0].clone());
    assert!(!editor.validation_errors().is_empty());
    editor.action_draft.pop();
    editor.action_draft[0].expected_bits = 1.0f32.to_bits();
    assert!(!editor.validation_errors().is_empty());
    editor.action_draft[0].expected_bits = 0.5f32.to_bits();
    editor.action_draft[0].value_bits = f32::NAN.to_bits();
    assert!(!editor.validation_errors().is_empty());
    editor.reset_all();
    assert!(editor.validation_errors().is_empty());
}

/// The notice asked users to "open asset discovery", which is not a control, and stayed
/// after discovery had built the index. The index is applied in place instead.
#[test]
fn the_index_notice_is_replaced_by_the_references_once_the_index_exists() {
    use sundial::package_authoring::tft::{Index, Reference};
    let mut loaded = fixture();
    loaded.warnings = index_warnings(None);
    assert_eq!(loaded.warnings, [UNINDEXED]);
    let graph = loaded.graphs[0].0;
    let reference = |source: u32, target: u32| Reference {
        source,
        source_class: 0,
        offset: 0,
        target,
        target_class: 0,
        path: "content/example.tft".into(),
    };
    let names = Index {
        references: vec![
            reference(loaded.action_tag, 0x1234),
            reference(0x5678, graph),
            reference(0x9999, 0x8888),
        ],
        ..Index::default()
    };
    PerkEditor::apply_asset_index(&mut loaded, &names, None);
    assert!(loaded.warnings.is_empty());
    assert_eq!(loaded.native_assets.len(), 2);
    // An index that could not read everything says so in the same place.
    let mut partial = names.clone();
    partial.errors.push("one resource".into());
    PerkEditor::apply_asset_index(&mut loaded, &partial, None);
    assert_eq!(loaded.warnings.len(), 1);
    assert!(loaded.warnings[0].starts_with("1 resources could not be read"));
    // An entity opened on its own keeps references in either direction.
    let mut entity = fixture();
    entity.action_tag = 0;
    PerkEditor::apply_asset_index(&mut entity, &names, Some(0x8888));
    assert_eq!(entity.native_assets.len(), 1);
    // Nothing to do while the notice is absent: a loaded effect is left alone.
    let mut editor = editor(fixture());
    assert!(!editor.refresh_asset_index());
}

#[test]
fn reference_index_notices_do_not_block_valid_component_edits() {
    let mut loaded = fixture();
    let field = loaded.graphs[0].1.fields().next().unwrap();
    let edit = WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value: field.value.clone(),
    };
    loaded
        .warnings
        .push("Asset references have not been indexed.".into());
    let mut editor = editor(loaded);
    editor.draft.push(edit);
    assert!(editor.validation_errors().is_empty());
    Arc::make_mut(editor.graph.as_mut().unwrap())
        .graph_errors
        .push("Graph could not be decoded.".into());
    assert!(
        editor
            .validation_errors()
            .iter()
            .any(|error| error.contains("complete graph"))
    );
}

#[test]
fn runtime_validation_blocks_unfinished_stale_ambiguous_and_wrong_type_edits() {
    let loaded = fixture();
    let speed = guided::ProjectileSpeed::discover(&loaded).unwrap();
    let mut editor = editor(loaded.clone());
    speed.set(&loaded, &mut editor.draft, 2.0).unwrap();
    assert!(editor.validation_errors().is_empty());
    editor
        .value_text
        .insert((editor.draft[0].locator.clone(), 0), "bad input".into());
    assert!(!editor.validation_errors().is_empty());
    editor.value_text.clear();
    let original = editor.draft.clone();
    editor.draft[0].value = WeaponRuntimeValue::Boolean(true);
    assert!(!editor.validation_errors().is_empty());
    editor.draft = original.clone();
    editor.draft[0].locator.binding_hash = 999;
    assert!(!editor.validation_errors().is_empty());
    editor.draft = original;
    let graph = Arc::make_mut(editor.graph.as_mut().unwrap());
    graph.graphs.push(graph.graphs[0].clone());
    assert!(!editor.validation_errors().is_empty());
    editor.reset_all();
    assert!(editor.validation_errors().is_empty());
}

#[test]
fn removing_runtime_edits_keeps_private_identity_and_effects() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    let key = editor(fixture()).key;
    upsert_private_perk_runtime_values(&mut recipe, key, vec![]);
    let variant = &mut recipe.overrides.socket_plug_variants[0];
    variant.name = Some("Private Intrinsic".into());
    variant.description = Some("Authored description".into());
    variant.classification_donor_hash = Some(HexHash::new(123));
    variant.investment_stats = vec![WeaponStatOverride {
        definition_index: 13,
        value: 10,
    }];
    let before = variant.clone();
    variant.sandbox_perks[0].action_float_values = actions::discover(&fixture().action_payload);
    remove_private_perk_runtime_values(&mut recipe, key);
    assert_eq!(recipe.overrides.socket_plug_variants, vec![before]);
}

#[test]
fn removing_runtime_edits_keeps_a_stat_only_custom_plug() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    let key = editor(fixture()).key;
    upsert_private_perk_runtime_values(&mut recipe, key, vec![]);
    recipe.overrides.socket_plug_variants[0].investment_stats = vec![WeaponStatOverride {
        definition_index: 13,
        value: 10,
    }];
    let before = recipe.clone();
    remove_private_perk_runtime_values(&mut recipe, key);
    assert_eq!(recipe, before);
}

#[test]
#[ignore = "requires clean Shadowkeep packages via PARHELION_CLEAN_STOCK_PACKAGES"]
fn native_micro_missile_exposes_verified_speed() {
    use sundial::package_authoring::weapon_runtime::resolve_weapon_runtime_field;
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").expect("package path");
    let loaded =
        load_private_perk_runtime_graph(Path::new(&packages), editor(fixture()).key, &[]).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let speed = guided::ProjectileSpeed::discover(&loaded).expect("verified speed profile");
    let mut editor = editor(loaded.clone());
    speed.set(&loaded, &mut editor.draft, 62.5).unwrap();
    assert_eq!(editor.draft.len(), 2);
    assert!(
        editor.validation_errors().is_empty(),
        "{:?}",
        editor.validation_errors()
    );
    let manager = open_shadowkeep_package_manager(Path::new(&packages)).unwrap();
    let payload = manager.read_tag(TagHash(0x8152_82E1)).unwrap();
    for value in &mut editor.draft {
        // Existing recipes address this owner through a different valid binding alias.
        value.locator.binding_hash = 0xB176_70ED;
        value.locator.resource_index = 0;
        let resolved = resolve_weapon_runtime_field(
            &manager,
            &payload,
            &value.locator.for_graph(0x8152_82E1).unwrap(),
        )
        .unwrap();
        encode_weapon_runtime_value(&resolved.field.kind, &value.value).unwrap();
    }
    assert_eq!(speed.value(&loaded, &editor.draft).unwrap(), 62.5);
    assert!(
        editor.validation_errors().is_empty(),
        "{:?}",
        editor.validation_errors()
    );
    speed.set(&loaded, &mut editor.draft, 1.0).unwrap();
    assert!(editor.draft.is_empty());
}

#[test]
fn parameter_defaults_are_a_draft_and_cancel_does_not_touch_the_recipe() {
    let loaded = fixture();
    let mut editor = editor(loaded.clone());
    let speed = guided::ProjectileSpeed::discover(&loaded).unwrap();
    speed.set(&loaded, &mut editor.draft, 62.5).unwrap();
    editor.original_draft = editor.draft.clone();
    let mut recipe = WeaponRecipe::new_weapon("parhelion.test-custom-perk").unwrap();
    let perk = upsert_private_perk_runtime_values(&mut recipe, editor.key, editor.draft.clone());
    perk.activation =
        Some(sundial::package_authoring::sandbox_perk::activation::PerkActivation::GrenadeKill);
    let before = recipe.clone();
    assert!(!editor.has_changes());
    editor.reset_all();
    assert!(editor.has_changes());
    assert_eq!(recipe, before);
    assert_eq!(editor.original_draft.len(), 2);
    drop(editor);
    assert_eq!(recipe, before);
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_PRIVATE_PERK_TEST_PLUG_HASH/INDEX"]
fn configured_private_perk_runtime_graph_decodes() {
    let (Some(packages), Some(plug_hash), Some(perk_index)) = (
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES"),
        std::env::var_os("PARHELION_PRIVATE_PERK_TEST_PLUG_HASH"),
        std::env::var_os("PARHELION_PRIVATE_PERK_TEST_INDEX"),
    ) else {
        panic!("set the package path, source plug hash and perk index for this native test");
    };
    let plug_hash_text = plug_hash.to_string_lossy();
    let plug_hash = plug_hash_text
        .strip_prefix("0x")
        .or_else(|| plug_hash_text.strip_prefix("0X"))
        .map_or_else(
            || plug_hash_text.parse::<u32>(),
            |digits| u32::from_str_radix(digits, 16),
        )
        .expect("configured plug hash should be decimal or 0x-prefixed hexadecimal");
    let perk_index = perk_index
        .to_string_lossy()
        .parse::<u16>()
        .expect("configured finished-perk index should be decimal");

    let loaded = load_private_perk_runtime_graph(
        Path::new(&packages),
        PerkEditorKey {
            socket_index: 0,
            choice_index: 0,
            source_plug_hash: plug_hash,
            source_perk_index: perk_index,
        },
        &[],
    )
    .expect("configured private-perk runtime graph should decode");

    assert_ne!(loaded.action_tag, 0);
    assert!(!loaded.graphs.is_empty());
    assert!(
        loaded
            .graphs
            .iter()
            .any(|(_, graph)| graph.fields().next().is_some())
    );
    let mut occurrences = BTreeMap::<WeaponRuntimeFieldLocator, usize>::new();
    for field in loaded.graphs.iter().flat_map(|(_, graph)| graph.fields()) {
        *occurrences.entry(field.locator.clone()).or_default() += 1;
    }
    assert!(
        occurrences.values().any(|count| *count == 1),
        "configured perk should expose at least one unambiguous editable field"
    );
}

pub(in crate::app) fn set_test_speed(
    loaded: &PrivatePerkRuntimeGraph,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
    value: f32,
) {
    guided::ProjectileSpeed::discover(loaded)
        .unwrap()
        .set(loaded, draft, value)
        .unwrap();
}
pub(in crate::app) fn test_speed(
    loaded: &PrivatePerkRuntimeGraph,
    draft: &[WeaponRuntimeValueOverride],
) -> f32 {
    guided::ProjectileSpeed::discover(loaded)
        .unwrap()
        .value(loaded, draft)
        .unwrap()
}
