use super::*;

#[test]
fn matching_components_in_distinct_graphs_edit_and_reset_independently() {
    let (mut loaded, _) = fixture();
    let mut other = loaded.graphs[0].clone();
    other.0 += 1;
    other.1.entity_tag = other.0;
    loaded.graphs.push(other);
    for (_, graph) in &mut loaded.graphs {
        graph.scope_fields();
    }
    let a = loaded.graphs[0]
        .1
        .fields()
        .find(|f| f.source == WeaponRuntimeFieldSource::NativeDeclaration)
        .unwrap()
        .clone();
    let b = loaded.graphs[1]
        .1
        .fields()
        .find(|f| f.source == WeaponRuntimeFieldSource::NativeDeclaration)
        .unwrap()
        .clone();
    assert_ne!(a.locator, b.locator);
    let mut draft = Vec::new();
    write_value(
        &loaded,
        &a,
        None,
        &mut draft,
        &WeaponRuntimeValue::Float32Bits(2.0_f32.to_bits()),
    )
    .unwrap();
    write_value(
        &loaded,
        &b,
        None,
        &mut draft,
        &WeaponRuntimeValue::Float32Bits(3.0_f32.to_bits()),
    )
    .unwrap();
    let mut editor = super::super::tests::editor(loaded.clone());
    editor.draft = draft.clone();
    assert!(
        editor.validation_errors().is_empty(),
        "{:?}",
        editor.validation_errors()
    );
    write_value(&loaded, &a, None, &mut draft, &a.value).unwrap();
    assert_eq!(draft.len(), 1);
    assert_eq!(draft[0].locator, b.locator);
    assert_eq!(
        current_value(&loaded, &b, None, &draft).unwrap(),
        WeaponRuntimeValue::Float32Bits(3.0_f32.to_bits())
    );
}

#[test]
#[ignore = "requires PARHELION_PROJECTILE_TEST_PACKAGES pointing to Shadowkeep packages"]
fn native_paired_movement_edits_import_and_reset_through_the_existing_controls() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_PROJECTILE_TEST_PACKAGES").unwrap());
    let loaded = super::super::load_entity_parameters(&packages, 0x80BB_B1B9).unwrap();
    let graph = &loaded.graphs[0].1;
    let speed = projectile::parameters::discover(graph)
        .into_iter()
        .find(|parameter| parameter.kind == projectile::parameters::Kind::Speed)
        .unwrap();
    let mut draft = graph
        .fields()
        .filter(|field| {
            field.source == WeaponRuntimeFieldSource::NativeDeclaration
                && speed.targets_field(speed.owner_tag, field)
        })
        .map(|field| WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: WeaponRuntimeValue::Float32Bits(3.75_f32.to_bits()),
        })
        .collect::<Vec<_>>();
    assert_eq!(draft.len(), 2);
    assert!(draft.iter().all(|edit| speed.contains(&edit.locator)));
    assert_eq!(speed.value(&draft).unwrap(), 3.75);
    speed.set(&mut draft, 5.5).unwrap();
    assert_eq!(speed.value(&draft).unwrap(), 5.5);
    assert_eq!(draft.len(), 2);
    speed.reset(&mut draft).unwrap();
    assert!(draft.is_empty());
}

fn fixture() -> (PrivatePerkRuntimeGraph, WeaponRuntimeField) {
    let mut loaded = super::super::tests::fixture();
    let root = &mut loaded.graphs[0].1.owners[0].roots[0];
    let mut field = root.fields[0].clone();
    field.name = "Initial Speed".into();
    field.path_label = "Component Instance / Initial Speed".into();
    field.kind = WeaponRuntimeValueKind::Float32;
    field.value = WeaponRuntimeValue::Float32Bits(1.0_f32.to_bits());
    field.source = WeaponRuntimeFieldSource::NativeDeclaration;
    field.locator.byte_size = 4;
    field.locator.path = vec![
        WeaponRuntimePathElement {
            name_hash: 0x504E_5200,
            type_handle: root.schema,
            byte_offset: 0,
        },
        WeaponRuntimePathElement {
            name_hash: 0x504E_5600,
            type_handle: root.schema,
            byte_offset: field.locator.value_offset,
        },
    ];
    root.fields.push(field.clone());
    (loaded, field)
}

#[test]
fn property_summary_resolves_values_inside_carriers_without_counting_storage_edits() {
    let (loaded, field) = fixture();
    let graph = &loaded.graphs[0].1;
    let carrier = carrier(graph, graph.owners[0].owner_tag, &field).unwrap();
    let mut draft = vec![];
    let value = WeaponRuntimeValue::Float32Bits(1.23_f32.to_bits());
    write_value(&loaded, &field, Some(carrier), &mut draft, &value).unwrap();
    assert_eq!(
        property_changes(&loaded, &draft).unwrap(),
        ["Initial Speed: 1.23"]
    );
    let direct = vec![WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value,
    }];
    assert_eq!(
        property_changes(&loaded, &draft),
        property_changes(&loaded, &direct)
    );

    // A separate byte change in the same carrier must remain visible as an advanced edit.
    let WeaponRuntimeValue::Bytes(bytes) = &mut draft[0].value else {
        panic!("expected carrier")
    };
    bytes[7] = 91;
    assert_eq!(
        property_changes(&loaded, &draft).unwrap(),
        ["Initial Speed: 1.23", "Native Bytes: +0x7=5B"]
    );
    write_value(&loaded, &field, Some(carrier), &mut draft, &field.value).unwrap();
    assert_eq!(
        property_changes(&loaded, &draft).unwrap(),
        ["Native Bytes: +0x7=5B"]
    );
    draft[0].locator.root_schema = 0;
    assert!(property_changes(&loaded, &draft).is_err());
}

#[test]
fn property_summary_preserves_small_values() {
    let (loaded, field) = fixture();
    let draft = vec![WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value: WeaponRuntimeValue::Float32Bits(0.00001_f32.to_bits()),
    }];
    assert_eq!(
        property_changes(&loaded, &draft).unwrap(),
        ["Initial Speed: 0.00001"]
    );
}

#[test]
fn paired_projectile_values_have_one_summary_and_survive_serialization() {
    use sundial::package_authoring::weapon_runtime::WeaponRuntimeResource;
    let (mut loaded, _) = fixture();
    let graph = &mut loaded.graphs[0].1;
    let mut owner = graph.owners.remove(0);
    let mut definition = owner.roots.remove(1);
    let mut instance = owner.roots.remove(0);
    instance.byte_size = 0x1E0;
    definition.byte_size = 0x5D0;
    let mut field = instance.fields[1].clone();
    field.locator.root = definition.kind;
    field.locator.root_schema = definition.schema;
    field.locator.value_offset = 0x88;
    field.owner_offset = 0x88;
    field.locator.path[0].type_handle = definition.schema;
    field.locator.path[1].byte_offset = 0x88;
    definition.fields.push(field);
    graph.resources.push(WeaponRuntimeResource {
        binding_hash: 1,
        binding_label: "Projectile Movement".into(),
        resource_index: 0,
        resource_count: 1,
        owner_tag: owner.owner_tag,
        concrete_class: 0x8080_3B73,
        alias_bindings: Vec::new(),
        instance,
        definition: Some(definition),
    });
    let parameter = projectile::parameters::discover(graph).remove(0);
    let mut draft = Vec::new();
    parameter.set(&mut draft, 1.23).unwrap();
    assert_eq!(draft.len(), 2);
    let saved = serde_json::to_string(&draft).unwrap();
    let reopened = serde_json::from_str::<Vec<WeaponRuntimeValueOverride>>(&saved).unwrap();
    assert_eq!(
        property_changes(&loaded, &reopened).unwrap(),
        ["Projectile Speed: 1.23 ×"]
    );
    parameter.reset(&mut draft).unwrap();
    assert!(property_changes(&loaded, &draft).unwrap().is_empty());
}

#[test]
fn detailed_fields_and_existing_controls_share_saved_bytes_and_reset_only_one_value() {
    let (loaded, field) = fixture();
    let graph = &loaded.graphs[0].1;
    let carrier = carrier(graph, graph.owners[0].owner_tag, &field).unwrap();
    let mut draft = vec![];
    let speed = guided::ProjectileSpeed::discover(&loaded).unwrap();
    speed.set(&loaded, &mut draft, 8.0).unwrap();
    assert_eq!(
        current_value(&loaded, &field, Some(carrier), &draft).unwrap(),
        WeaponRuntimeValue::Float32Bits(8.0_f32.to_bits())
    );
    if let WeaponRuntimeValue::Bytes(bytes) = &mut draft[0].value {
        bytes[7] = 91;
    }
    write_value(&loaded, &field, Some(carrier), &mut draft, &field.value).unwrap();
    let saved = saved(&loaded, carrier, &draft).unwrap().unwrap();
    assert!(
        matches!(&saved.value, WeaponRuntimeValue::Bytes(bytes) if bytes[7] == 91 && bytes[..4] == 1.0_f32.to_le_bytes())
    );
    assert_eq!(draft.len(), 2);
    assert!(draft.iter().all(|edit| edit.locator != field.locator));
}

#[test]
fn conflicting_native_and_byte_edits_are_reported_without_mutating_the_draft() {
    let (loaded, field) = fixture();
    let graph = &loaded.graphs[0].1;
    let carrier = carrier(graph, graph.owners[0].owner_tag, &field).unwrap();
    let mut draft = vec![
        WeaponRuntimeValueOverride {
            locator: carrier.locator.clone(),
            value: carrier.value.clone(),
        },
        WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: field.value.clone(),
        },
    ];
    let original = draft.clone();
    let mut editor = super::super::tests::editor(loaded.clone());
    editor.draft = draft.clone();
    assert!(
        editor
            .validation_errors()
            .iter()
            .any(|error| error.contains("overlaps"))
    );
    assert!(
        write_value(&loaded, &field, Some(carrier), &mut draft, &field.value)
            .unwrap_err()
            .contains("overlaps")
    );
    assert_eq!(draft, original);
}

#[test]
fn non_projectile_fields_are_searchable_and_visible_without_experimental_controls() {
    let (mut loaded, mut field) = fixture();
    field.name = "Activation Delay".into();
    field.path_label = "Effect / Activation Delay".into();
    field.locator.root_schema = 0x8080_1234;
    field.locator.path[0].type_handle = field.locator.root_schema;
    loaded.graphs[0].1.owners[0].roots[0].fields = vec![field.clone()];
    assert!(matches_query(&field, "Effect", "activation"));
    assert!(matches_query(&field, "Effect", "32-bit float"));
    for width in [480.0, 1100.0] {
        let mut editor = super::super::tests::editor(loaded.clone());
        editor.query = "activation".into();
        let ctx = egui::Context::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.draw_runtime_fields(ui, &loaded, false);
                    assert!(ui.min_rect().right() <= width);
                });
            },
        );
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(t) = &shape.shape {
                    Some(t.galley.job.text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Activation Delay"), "{text}");
        assert!(!text.contains("No supported package fields"), "{text}");
        assert!(editor.draft.is_empty());
    }
}

#[test]
fn untouched_nonfinite_donor_values_do_not_block_other_component_edits() {
    let (mut loaded, mut field) = fixture();
    field.value = WeaponRuntimeValue::Float32Bits(0x7FC0_1234);
    loaded.graphs[0].1.owners[0].roots[0].fields = vec![field.clone()];
    let mut editor = super::super::tests::editor(loaded);
    editor
        .value_text
        .insert((field.locator.clone(), 0), "0x7FC01234".into());
    assert!(editor.validation_errors().is_empty());
    editor
        .value_text
        .insert((field.locator.clone(), 0), "0x7FC01235".into());
    assert!(!editor.validation_errors().is_empty());
}

#[test]
fn component_checkbox_edits_an_isolated_draft_and_reset_removes_the_edit() {
    let (mut loaded, mut field) = fixture();
    field.name = "Enabled".into();
    field.path_label = "Effect / Enabled".into();
    field.kind = WeaponRuntimeValueKind::Boolean;
    field.value = WeaponRuntimeValue::Boolean(false);
    field.locator.byte_size = 1;
    loaded.graphs[0].1.owners[0].roots[0].fields = vec![field.clone()];
    let graph = &loaded.graphs[0].1;
    let owner = graph.owners[0].owner_tag;
    let mut editor = super::super::tests::editor(loaded.clone());
    let ctx = egui::Context::default();
    let mut render = |events| {
        ctx.run(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(480.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.draw_native_values(ui, &loaded, graph, owner, &[&field])
                });
            },
        )
    };
    let output = render(Vec::new());
    let checkbox = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if (12.0..24.0).contains(&rect.rect.width())
                    && (12.0..24.0).contains(&rect.rect.height()) =>
            {
                Some(rect.rect.center())
            }
            _ => None,
        })
        .expect("component checkbox");
    let click = |pos, pressed| {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    };
    render(click(checkbox, true));
    render(click(checkbox, false));
    let output = render(Vec::new());
    let reset = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text.galley.job.text == "Reset to Donor" || text.galley.job.text == "Reset" =>
            {
                Some(text.pos + egui::vec2(4.0, 4.0))
            }
            _ => None,
        })
        .expect("reset for modified component field");
    render(click(reset, true));
    render(click(reset, false));
    assert!(editor.draft.is_empty());
    assert_eq!(field.value, WeaponRuntimeValue::Boolean(false));
}
