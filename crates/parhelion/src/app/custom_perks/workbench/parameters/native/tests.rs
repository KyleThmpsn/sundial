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
            type_handle: root.schema.into(),
            byte_offset: 0,
        },
        WeaponRuntimePathElement {
            name_hash: 0x504E_5600,
            type_handle: root.schema.into(),
            byte_offset: field.locator.value_offset,
        },
    ];
    root.fields.push(field.clone());
    (loaded, field)
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
    let render = |editor: &mut PerkEditor, events| {
        ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(480.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    editor.draw_native_values(ui, &loaded, graph, owner, &[&field])
                });
            },
        )
    };
    let output = render(&mut editor, Vec::new());
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
    render(&mut editor, click(checkbox, true));
    render(&mut editor, click(checkbox, false));
    let output = render(&mut editor, Vec::new());
    assert_eq!(
        current_value(&loaded, &field, None, &editor.draft).unwrap(),
        WeaponRuntimeValue::Boolean(true)
    );
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
    render(&mut editor, click(reset, true));
    render(&mut editor, click(reset, false));
    assert!(editor.draft.is_empty());
}
