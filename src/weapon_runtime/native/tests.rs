use super::*;
use crate::package_runtime::references::schema::{Record, Registry};

#[test]
#[allow(clippy::cognitive_complexity)]
fn player_settings_use_checked_locators_and_preserve_untouched_storage() {
    for (schema, size, offsets) in [
        (
            0x8080_4B8A,
            0x5C8,
            &[0x54, 0x74, 0x94, 0xB4, 0xD4, 0xF4][..],
        ),
        (0x8080_4C5F, 0x50, &[0x14, 0x38, 0x3C, 0x40][..]),
        (0x8080_43E2, 0x248, &[0x10, 0x170, 0x174, 0x178][..]),
        (0x8080_43EC, 0x2F0, &[0x290, 0x294][..]),
    ] {
        for bits in [0x8000_0000_u32, 0x7FC1_2345, 1.25_f32.to_bits()] {
            // Keep native references null, but fill each value's adjacent padding.
            let mut data = vec![0; size];
            for &at in offsets {
                data[at..at + 4].copy_from_slice(&bits.to_le_bytes());
            }
            if schema == 0x8080_43E2 {
                data[0x1D8..0x1E0].copy_from_slice(&[1, 0xA5, 0x5A, 0xFF, 1, 2, 3, 4]);
            }
            let mut registry = Registry::new().unwrap();
            let decoded = structure::test_walk(&data, 0, schema, |handle| {
                registry.record(handle, |_| Err("Unexpected schema".into()))
            });
            assert!(decoded.issues.is_empty(), "{:?}", decoded.issues);
            let editable = fields(&data, &root(schema, 0, size as u32), 1, 0, &decoded).unwrap();
            for &at in offsets {
                let matches = editable
                    .iter()
                    .filter(|f| f.owner_offset as usize == at)
                    .collect::<Vec<_>>();
                assert_eq!(matches.len(), 1, "{schema:08X}+{at:X}");
                let field = matches[0];
                assert!(field.locator.is_buildable());
                assert_eq!(field.locator.type_handle, schema);
                assert_eq!(field.locator.value_offset as usize, at);
                assert_eq!(field.value, WeaponRuntimeValue::Float32Bits(bits));
                let same = encode_weapon_runtime_field_value(field, &field.value).unwrap();
                assert_eq!(same, bits.to_le_bytes());
                let changed = encode_weapon_runtime_field_value(
                    field,
                    &WeaponRuntimeValue::Float32Bits(2.5_f32.to_bits()),
                )
                .unwrap();
                let mut output = data.clone();
                output[at..at + changed.len()].copy_from_slice(&changed);
                assert_eq!(&output[..at], &data[..at]);
                assert_eq!(&output[at + 4..], &data[at + 4..]);
                assert!(
                    presentation::field_tooltip(field).contains(
                        invisibility::field_help(schema, at as u32)
                            .or_else(|| health::field_help(schema, at as u32))
                            .unwrap()
                    )
                );
            }
            if schema == 0x8080_43E2 {
                let movement = editable.iter().find(|f| f.owner_offset == 0x1D8).unwrap();
                assert_eq!(movement.value, WeaponRuntimeValue::Boolean(true));
                let encoded = encode_weapon_runtime_field_value(
                    movement,
                    &WeaponRuntimeValue::Boolean(false),
                )
                .unwrap();
                assert_eq!(encoded, [0]);
                let mut output = data.clone();
                output[0x1D8..0x1D9].copy_from_slice(&encoded);
                assert_eq!(&output[0x1D9..], &data[0x1D9..]);
            }
        }
        let incompatible = structure::test_walk(&vec![0; size], 0, schema, |_| {
            Ok(Record {
                size: size - 1,
                fields: Vec::new().into(),
            })
        });
        assert!(!incompatible.issues.is_empty());
        assert!(
            fields(
                &vec![0; size],
                &root(schema, 0, size as u32),
                1,
                0,
                &incompatible
            )
            .unwrap()
            .is_empty()
        );
    }
}

#[test]
fn player_property_modifiers_expose_exact_fields_and_preserve_adjacent_storage() {
    let schema = 0x8080_3B06;
    for bits in [3_f32.to_bits(), 0x8000_0000, 0x7FC1_2345] {
        let mut data = vec![0; 0x58];
        data[0x28..0x2C].copy_from_slice(&bits.to_le_bytes());
        data[0x2C..0x30].copy_from_slice(&[0xFE, 0xAB, 0xCD, 0xEF]);
        data[0x48..0x50].copy_from_slice(&[0xFF, 0xFF, 0x23, 0x81, 14, 0xAB, 0xCD, 0xEF]);
        let mut registry = Registry::new().unwrap();
        let structure = structure::test_walk(&data, 0, schema, |handle| {
            registry.record(handle, |_| Err("Unexpected generated schema".into()))
        });
        assert!(structure.issues.is_empty(), "{:?}", structure.issues);
        let fields = fields(&data, &root(schema, 0, 0x58), 1, 0, &structure).unwrap();
        assert_eq!(fields.len(), 5);
        for field in &fields {
            let at = field.owner_offset as usize;
            let encoded = encode_weapon_runtime_field_value(field, &field.value).unwrap();
            assert_eq!(encoded, data[at..at + encoded.len()]);
            assert_eq!(
                decode_weapon_runtime_field_value(field, &encoded).unwrap(),
                field.value
            );
        }
        let operation = fields.iter().find(|f| f.name == "Operation").unwrap();
        assert_eq!(operation.value, WeaponRuntimeValue::Unsigned(254));
        let encoded =
            encode_weapon_runtime_field_value(operation, &WeaponRuntimeValue::Unsigned(1)).unwrap();
        assert_eq!(encoded, [1]);
        let mut changed = data.clone();
        changed[0x2C..0x2D].copy_from_slice(&encoded);
        data[0x2C] = 1;
        assert_eq!(changed, data);
        assert_eq!(
            fields
                .iter()
                .find(|f| f.name == "Ability Slot")
                .unwrap()
                .value,
            WeaponRuntimeValue::Signed(-1)
        );
    }
    let bad = structure::test_walk(&[0; 0x58], 0, schema, |_| {
        Ok(Record {
            size: 0x57,
            fields: Vec::new().into(),
        })
    });
    assert!(
        bad.issues
            .iter()
            .any(|issue| issue.contains("incompatible structure size"))
    );
}

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
