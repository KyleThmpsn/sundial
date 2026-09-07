use super::*;
use crate::weapon_runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeFieldSource, WeaponRuntimeGraph,
    WeaponRuntimeOwner, WeaponRuntimeRoot, WeaponRuntimeRootKind, WeaponRuntimeValue,
    WeaponRuntimeValueKind,
};

fn field(
    source: WeaponRuntimeFieldSource,
    value: WeaponRuntimeValue,
    kind: WeaponRuntimeValueKind,
) -> WeaponRuntimeField {
    WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            binding_hash: 1,
            resource_index: 0,
            root: WeaponRuntimeRootKind::Definition,
            root_schema: 0x8080_1234,
            path: Vec::new(),
            type_handle: 0x8080_3456,
            value_offset: 0,
            byte_size: kind.byte_size(),
        },
        owner_offset: 16,
        name: "initial_speed_scale".into(),
        path_label: "Initial Speed Scale".into(),
        kind,
        value,
        source,
        generated_kind: None,
    }
}

fn root() -> WeaponRuntimeRoot {
    WeaponRuntimeRoot {
        kind: WeaponRuntimeRootKind::Definition,
        schema: 0x8080_1234,
        owner_offset: 0,
        byte_size: 8,
        generated_schema: true,
        fields: vec![
            field(
                WeaponRuntimeFieldSource::GeneratedSchema,
                WeaponRuntimeValue::Float32Bits(0.5f32.to_bits()),
                WeaponRuntimeValueKind::Float32,
            ),
            field(
                WeaponRuntimeFieldSource::OpaqueNativeType,
                WeaponRuntimeValue::Bytes(vec![1, 2, 3, 4]),
                WeaponRuntimeValueKind::FixedBytes { size: 4 },
            ),
        ],
    }
}

#[test]
fn field_export_retains_exact_float_bits_and_opaque_bytes() {
    let special = field(
        WeaponRuntimeFieldSource::NativeMember,
        WeaponRuntimeValue::Float32Bits(0x7FC00001),
        WeaponRuntimeValueKind::Float32,
    );
    let exported = view::export_field(&special);
    assert_eq!(
        exported["value"],
        serde_json::to_value(&special.value).unwrap()
    );
    assert_eq!(exported["owner_offset"], 16);
    assert_eq!(
        exported["locator"],
        serde_json::to_value(&special.locator).unwrap()
    );
    let opaque = field(
        WeaponRuntimeFieldSource::OpaqueNativeType,
        WeaponRuntimeValue::Bytes(vec![0xAB; 96]),
        WeaponRuntimeValueKind::FixedBytes { size: 96 },
    );
    let exported = view::export_field(&opaque);
    assert_eq!(
        exported["value"],
        serde_json::to_value(&opaque.value).unwrap()
    );
    assert_eq!(exported["source"], "opaque_semantics_unknown");
    assert!(!exported["display"].as_str().unwrap().contains('…'));
}

fn pending(state: &mut RuntimeInspectionState, target: RuntimeTarget) -> mpsc::Sender<LoadResult> {
    let (sender, receiver) = mpsc::channel();
    state.pending = Some(PendingLoad {
        scope: state.scope.clone().unwrap(),
        generation: state.generation,
        target,
        receiver,
    });
    sender
}

#[test]
fn preparing_an_item_does_not_read_packages() {
    let mut state = RuntimeInspectionState::default();
    state.prepare(Path::new("missing-install"), 1, Arc::default());
    state.poll(&egui::Context::default());
    assert!(state.pending.is_none());
    assert!(state.cache.is_empty());
}

#[test]
fn navigation_discards_old_results_even_when_returning_to_same_item() {
    let mut state = RuntimeInspectionState::default();
    let install = Path::new("install");
    state.prepare(install, 1, Arc::default());
    let sender = pending(&mut state, RuntimeTarget::Perk(8));
    state.prepare(install, 2, Arc::default());
    state.prepare(install, 1, Arc::default());
    assert!(
        state.pending.is_some(),
        "one worker remains in flight across navigation"
    );
    sender.send(Err("stale result".into())).unwrap();
    state.poll(&egui::Context::default());
    assert!(state.pending.is_none());
    assert!(state.cache.is_empty());
}

#[test]
fn closing_and_reopening_same_item_cannot_accept_pre_close_results() {
    let mut state = RuntimeInspectionState::default();
    state.prepare(Path::new("install"), 1, Arc::default());
    let sender = pending(&mut state, RuntimeTarget::Perk(8));
    state.clear();
    state.prepare(Path::new("install"), 1, Arc::default());
    sender.send(Err("before close".into())).unwrap();
    state.poll(&egui::Context::default());
    assert!(state.cache.is_empty());
}

#[test]
fn changing_installs_clears_cached_values_and_filters() {
    let mut state = RuntimeInspectionState::default();
    state.prepare(Path::new("install-a"), 1, Arc::default());
    state.remember(RuntimeTarget::Weapon(1), Err("a".into()));
    state.view.show_opaque = true;
    state.view.query = "old filter".into();
    state.prepare(Path::new("install-b"), 1, Arc::default());
    assert!(state.cache.is_empty());
    assert!(state.view.query.is_empty());
    assert!(!state.view.show_opaque);
}

#[test]
fn disconnected_reader_returns_an_actionable_error() {
    let mut state = RuntimeInspectionState::default();
    state.prepare(Path::new("install"), 1, Arc::default());
    drop(pending(&mut state, RuntimeTarget::Weapon(1)));
    state.poll(&egui::Context::default());
    assert!(state.pending.is_none());
    assert!(
        state.cache[&RuntimeTarget::Weapon(1)]
            .as_ref()
            .as_ref()
            .unwrap_err()
            .contains("retry")
    );
}

#[test]
fn cache_is_bounded_and_target_kinds_cannot_collide() {
    let mut state = RuntimeInspectionState::default();
    for index in 0..8 {
        state.remember(RuntimeTarget::Perk(index), Err(index.to_string()));
    }
    assert_eq!(state.cache.len(), CACHE_LIMIT);
    assert!(!state.cache.contains_key(&RuntimeTarget::Perk(0)));
    state.remember(RuntimeTarget::Weapon(7), Err("weapon".into()));
    assert!(state.cache.contains_key(&RuntimeTarget::Perk(7)));
    assert!(state.cache.contains_key(&RuntimeTarget::Weapon(7)));
    assert_eq!(state.cache.len(), CACHE_LIMIT);
}

#[test]
fn named_fields_and_opaque_bytes_have_separate_visibility() {
    let root = root();
    assert_eq!(
        view::matching_fields(&root, "Translator", "", false).len(),
        1
    );
    assert_eq!(
        view::matching_fields(&root, "Translator", "", true).len(),
        2
    );
    assert_eq!(
        view::matching_fields(&root, "Translator", "speed", false).len(),
        1
    );
    assert_eq!(
        view::matching_fields(&root, "Translator", "translator", false).len(),
        1
    );
    assert_eq!(
        view::matching_fields(&root, "Translator", "0x80801234", false).len(),
        1
    );
    assert_eq!(
        view::matching_fields(&root, "Translator", "0x80803456", false).len(),
        1
    );
    assert!(view::matching_fields(&root, "Translator", "unmatched", true).is_empty());
}

#[test]
fn values_preserve_float_bits_and_identifiers_and_bound_byte_previews() {
    let mut field = root().fields.remove(0);
    assert_eq!(view::value_text(&field), "0.5");
    assert!(view::exact_value_text(&field).contains("3F000000"));
    field.value = WeaponRuntimeValue::Float32Bits(0x7FC0_1234);
    assert!(view::value_text(&field).contains("7FC01234"));
    field.kind = WeaponRuntimeValueKind::HexIdentifier { bits: 32 };
    field.value = WeaponRuntimeValue::Unsigned(0xABC);
    assert_eq!(view::value_text(&field), "0x00000ABC");
    field.kind = WeaponRuntimeValueKind::FixedBytes { size: 1_000 };
    field.value = WeaponRuntimeValue::Bytes(vec![0xAB; 1_000]);
    assert!(view::value_text(&field).len() < 130);
    assert_eq!(
        view::exact_value_text(&field).split_whitespace().count(),
        1_000
    );
}

#[test]
fn runtime_view_renders_named_and_technical_fields_at_supported_widths() {
    let graph = WeaponRuntimeGraph {
        item_hash: 1,
        pattern_global_id_hash: 2,
        entity_tag: 3,
        bindings: Vec::new(),
        resources: Vec::new(),
        owners: vec![WeaponRuntimeOwner {
            owner_tag: 4,
            anchor_binding_hash: 5,
            anchor_resource_index: 0,
            roots: vec![root()],
        }],
    };
    for width in [600.0, 1_000.0] {
        for show_opaque in [false, true] {
            let ctx = egui::Context::default();
            let mut options = view::RuntimeViewOptions {
                query: "speed".into(),
                show_opaque,
            };
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 720.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let right_edge = ui.max_rect().right();
                        view::draw_graph(ui, &graph, &mut options);
                        assert!(
                            ui.min_rect().right() <= right_edge + 1.0,
                            "runtime content must fit the inspector width"
                        );
                    });
                },
            );
            assert!(!output.shapes.is_empty());
            assert_eq!(graph.field_count(), 2, "rendering is read-only");
        }
    }
}

#[test]
#[ignore = "requires SUNDIAL_TEST_INSTALL pointing to a Shadowkeep install; read-only"]
fn installed_weapon_and_perk_runtime_inspection() {
    let install = std::env::var_os("SUNDIAL_TEST_INSTALL").expect("set SUNDIAL_TEST_INSTALL");
    let install = Path::new(&install);
    let weapon = loader::load(install, RuntimeTarget::Weapon(285)).unwrap();
    let LoadedDetails::Weapon(graph) = weapon else {
        panic!("expected weapon graph");
    };
    assert!(graph.field_count() > 0);
    assert!(
        graph
            .fields()
            .any(|field| field.path_label == "Initial Speed Scale")
    );
    // Stock Micro-Missile, also used by Parhelion's package-chain regression test.
    let LoadedDetails::Perk(perk) = loader::load(install, RuntimeTarget::Perk(1178)).unwrap()
    else {
        panic!("expected perk details");
    };
    assert_eq!(perk.row.index, 1178);
    let action = perk.action.unwrap();
    assert!(!action.graphs.is_empty());
    assert!(action.graphs.iter().all(|graph| graph.decoded.is_ok()));
}
