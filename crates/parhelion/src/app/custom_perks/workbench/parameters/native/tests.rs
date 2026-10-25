use super::*;

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
