use super::*;
use crate::package_runtime::references::schema::{Record, Registry};

fn root(schema: u32, start: u32, size: u32) -> WeaponRuntimeRoot {
    WeaponRuntimeRoot {
        kind: WeaponRuntimeRootKind::ComponentDefinition,
        schema,
        owner_offset: start,
        byte_size: size,
        generated_schema: false,
        fields: Vec::new(),
        structure: Default::default(),
    }
}

fn array_fields(start: usize, target: usize) -> (Vec<u8>, Vec<WeaponRuntimeField>) {
    let schema = 0x8080_1234;
    let mut data = vec![0; target + 24];
    data[start + 8..start + 16]
        .copy_from_slice(&((target as i64) - (start + 8) as i64).to_le_bytes());
    data[target - 4..target].copy_from_slice(&0x8080_9FBD_u32.to_le_bytes());
    data[target..target + 8].copy_from_slice(&2_u64.to_le_bytes());
    data[target + 8..target + 12].copy_from_slice(&0x8080_000F_u32.to_le_bytes());
    data[target + 16..target + 20].copy_from_slice(&(-0.0_f32).to_le_bytes());
    data[target + 20..target + 24].copy_from_slice(&1.5_f32.to_le_bytes());
    let mut registry = Registry::new().unwrap();
    let structure = structure::test_walk(&data, start, schema, |handle| {
        if handle == schema {
            Ok(Record {
                size: 16,
                fields: vec![(8, 3)].into(),
            })
        } else {
            registry.record(handle, |_| Err("Unexpected schema".into()))
        }
    });
    assert!(structure.issues.is_empty(), "{:?}", structure.issues);
    let fields = fields(&data, &root(schema, start as u32, 16), 1, 0, &structure).unwrap();
    (data, fields)
}

#[test]
fn native_array_paths_survive_relocation_and_select_exact_elements() {
    let (data, original) = array_fields(0, 32);
    let (rebased, moved) = array_fields(24, 96);
    assert_eq!(original.len(), 2);
    assert_eq!(
        original.iter().map(|f| &f.locator).collect::<Vec<_>>(),
        moved.iter().map(|f| &f.locator).collect::<Vec<_>>()
    );
    assert_ne!(original[0].owner_offset, moved[0].owner_offset);
    for (before, after) in original.iter().zip(&moved) {
        assert!(before.locator.is_buildable());
        assert_eq!(
            encode_weapon_runtime_field_value(before, &before.value).unwrap(),
            data[before.owner_offset as usize..before.owner_offset as usize + 4]
        );
        assert_eq!(
            encode_weapon_runtime_field_value(after, &after.value).unwrap(),
            rebased[after.owner_offset as usize..after.owner_offset as usize + 4]
        );
        assert_eq!(before.value, after.value);
        let mut forged = before.locator.clone();
        forged.path[2].byte_offset += 2;
        assert!(!moved.iter().any(|f| f.locator == forged));
        forged = before.locator.clone();
        forged.path[2].type_handle ^= 1;
        assert!(!moved.iter().any(|f| f.locator == forged));
        forged = before.locator.clone();
        forged.value_offset += 4;
        assert!(!moved.iter().any(|f| f.locator == forged));
    }
    let edit = WeaponRuntimeValueOverride {
        locator: original[1].locator.clone(),
        value: WeaponRuntimeValue::Float32Bits(3.5_f32.to_bits()),
    };
    let saved = serde_json::to_string(&edit).unwrap();
    assert_eq!(
        serde_json::from_str::<WeaponRuntimeValueOverride>(&saved).unwrap(),
        edit
    );
}

#[test]
fn native_numeric_writes_preserve_all_bits_and_reject_unknown_encodings() {
    let (_, fields) = array_fields(0, 32);
    for code in [44, 45] {
        let mut field = fields[0].clone();
        field.locator.path.last_mut().unwrap().name_hash = VALUE + code;
        if code == 44 {
            field.kind = WeaponRuntimeValueKind::SignedInteger { bits: 32 };
            field.value = WeaponRuntimeValue::Signed(0);
        }
        for bits in [0, 0x8000_0000, 0x7FC1_2345, u32::MAX, 0x3F80_0000] {
            let value = if code == 44 {
                WeaponRuntimeValue::Signed(i64::from(bits as i32))
            } else {
                WeaponRuntimeValue::Float32Bits(bits)
            };
            let bytes = encode_weapon_runtime_field_value(&field, &value).unwrap();
            assert_eq!(
                decode_weapon_runtime_field_value(&field, &bytes).unwrap(),
                value
            );
            assert_eq!(
                structure::numeric::decode(
                    code as u8,
                    u32::from_le_bytes(bytes.try_into().unwrap())
                ),
                Some(bits)
            );
        }
        field.locator.path.last_mut().unwrap().name_hash = VALUE + 256 + code;
        assert!(encode_weapon_runtime_field_value(&field, &field.value).is_err());
        assert!(decode_weapon_runtime_field_value(&field, &[0; 4]).is_err());
    }
}

#[test]
fn incomplete_native_traversal_never_produces_editable_values() {
    let (_, original) = array_fields(0, 32);
    let structure = NativeStructure {
        managed_ranges: Vec::new(),
        fields: vec![NativeStructureField {
            owner_offset: original[0].owner_offset as usize,
            schema: original[0].locator.type_handle,
            schema_offset: 0,
            path: original[0].locator.path.clone(),
            label: "Partial Value".into(),
            representation: "Float32".into(),
            storage: Some((WeaponRuntimeValueKind::Float32, 0)),
            value: "0".into(),
        }],
        issues: vec!["Truncated array".into()],
    };
    assert!(
        fields(
            &[],
            &root(original[0].locator.root_schema, 0, 16),
            1,
            0,
            &structure
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn structural_metadata_wins_over_inline_numeric_aliases() {
    let (data, original) = array_fields(0, 32);
    let mut structure = NativeStructure::default();
    for field in &original {
        structure.fields.push(NativeStructureField {
            owner_offset: field.owner_offset as usize,
            schema: field.locator.type_handle,
            schema_offset: 0,
            path: field.locator.path.clone(),
            label: "Aliased Value".into(),
            representation: "Float32".into(),
            storage: Some((WeaponRuntimeValueKind::Float32, 0)),
            value: String::new(),
        });
    }
    // Cover only part of the first scalar, as can occur with a resource-reference alias.
    structure
        .managed_ranges
        .push((original[0].owner_offset as usize + 2, 2));
    let visible = fields(
        &data,
        &root(original[0].locator.root_schema, 0, 16),
        1,
        0,
        &structure,
    )
    .unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].owner_offset, original[1].owner_offset);
}
