use super::*;
use sundial::package_authoring::runtime::{BindingHash, SchemaHandle};
use sundial::package_authoring::runtime::{
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
                binding_hash: BindingHash::new(1),
                resource_index: 0,
                root,
                root_schema: schema.into(),
                path: vec![],
                type_handle: SchemaHandle::new(2),
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
        pending_movement: None,
        parameter_error: None,
        graph: Some(Arc::new(loaded)),
        error: None,
        receiver: None,
        worker: None,
        query: String::new(),
        property_query: String::new(),
        value_text: BTreeMap::new(),
        show_all_native_values: false,
        conversion: None,
        original_draft: vec![],
        original_action_draft: vec![],
    }
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
    editor.draft[0].locator.binding_hash = BindingHash::new(999);
    assert!(!editor.validation_errors().is_empty());
    editor.draft = original;
    let graph = Arc::make_mut(editor.graph.as_mut().unwrap());
    graph.graphs.push(graph.graphs[0].clone());
    assert!(!editor.validation_errors().is_empty());
    editor.reset_all();
    assert!(editor.validation_errors().is_empty());
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
