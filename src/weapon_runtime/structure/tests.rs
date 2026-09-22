use super::*;

#[test]
fn damage_modifier_settings_expose_typed_keys_without_consuming_adjacent_flags() {
    let mut data = vec![0; 0xF0];
    data[0xE0..0xE4].copy_from_slice(&0xDE1D_8C04_u32.to_le_bytes());
    data[0xE4..0xE8].copy_from_slice(&[1, 0xCD, 0x23, 0xFE]);
    data[0xE8] = 1;
    let original = data.clone();
    let mut registry = Registry::new().unwrap();
    let decoded = walk(
        &data,
        0,
        0x8080_3F8C,
        runtime_registry().unwrap(),
        codecs().unwrap(),
        |handle| registry.record(handle, |_| Err("Unexpected generated schema".into())),
    );
    assert!(decoded.issues.is_empty(), "{:?}", decoded.issues);
    for (offset, label, kind) in [
        (
            0xE0,
            "Required Source Property",
            WeaponRuntimeValueKind::HexIdentifier { bits: 32 },
        ),
        (
            0xE4,
            "Invert Source Property",
            WeaponRuntimeValueKind::Boolean,
        ),
        (
            0xE8,
            "Require Matching Source Owner",
            WeaponRuntimeValueKind::Boolean,
        ),
    ] {
        let field = decoded
            .fields
            .iter()
            .find(|field| field.owner_offset == offset && field.storage.is_some())
            .unwrap();
        assert_eq!(field.label, label);
        assert_eq!(field.storage, Some((kind, 0)));
    }
    assert_eq!(data, original);
    assert!(labels::native_fields(0x8080_3F8C, 0xEF).is_err());
    assert!(labels::native_fields(0x8080_3F8D, 0xF0).unwrap().is_empty());
}

#[test]
fn native_serialization_exposes_nested_projectile_values_without_changing_bits() {
    let mut data = vec![0_u8; 480];
    let bits = 0x7FC1_2345_u32;
    data[0x144..0x148].copy_from_slice(&bits.to_le_bytes());
    let mut registry = Registry::new().unwrap();
    let decoded = walk(
        &data,
        0,
        0x8080_3B73,
        runtime_registry().unwrap(),
        codecs().unwrap(),
        |handle| registry.record(handle, |_| Err("Unexpected generated schema".into())),
    );
    assert!(decoded.issues.is_empty(), "{:?}", decoded.issues);
    let value = decoded
        .fields
        .iter()
        .find(|field| field.owner_offset == 0x144 && field.representation == "Float32")
        .expect("projectile reset data nested inside its native codec");
    assert_eq!(value.schema, 0x8080_37C9);
    assert_eq!(value.value, "NaN (0x7FC12345)");
    assert_eq!(value.label, "Initial Speed");
    assert_eq!(&data[0x144..0x148], &bits.to_le_bytes());
}

fn pointer_record(handle: u32) -> Result<Record, String> {
    Ok(match handle {
        0x8080_1234 => Record {
            size: 16,
            fields: vec![(8, 3)].into(),
        },
        0x8080_000F => Record {
            size: 4,
            fields: Vec::new().into(),
        },
        _ => return Err(format!("Unknown test schema 0x{handle:08X}")),
    })
}

#[test]
fn native_structure_reports_bad_arrays_and_out_of_bounds_pointers() {
    let mut data = vec![0; 64];
    data[8..16].copy_from_slice(&24_i64.to_le_bytes());
    data[28..32].copy_from_slice(&0x8080_9FBD_u32.to_le_bytes());
    data[32..40].copy_from_slice(&100_u64.to_le_bytes());
    data[40..44].copy_from_slice(&0x8080_000F_u32.to_le_bytes());
    let inspect = |data: &[u8]| {
        walk(
            data,
            0,
            0x8080_1234,
            runtime_registry().unwrap(),
            &BTreeMap::new(),
            pointer_record,
        )
    };
    assert!(inspect(&data).issues[0].contains("invalid bounds or stride"));
    data[8..16].copy_from_slice(&i64::MIN.to_le_bytes());
    assert!(inspect(&data).issues[0].contains("overflowed"));
    data[8..16].copy_from_slice(&100_i64.to_le_bytes());
    assert!(inspect(&data).issues[0].contains("outside its payload"));
}

#[test]
fn native_structure_follows_checked_arrays_and_deduplicates_cycles() {
    let mut data = vec![0; 64];
    data[8..16].copy_from_slice(&24_i64.to_le_bytes());
    data[28..32].copy_from_slice(&0x8080_9FBD_u32.to_le_bytes());
    data[32..40].copy_from_slice(&2_u64.to_le_bytes());
    data[40..44].copy_from_slice(&0x8080_000F_u32.to_le_bytes());
    data[48..52].copy_from_slice(&(-0.0_f32).to_bits().to_le_bytes());
    data[52..56].copy_from_slice(&1.5_f32.to_bits().to_le_bytes());
    let inspect = |data: &[u8]| {
        walk(
            data,
            0,
            0x8080_1234,
            runtime_registry().unwrap(),
            codecs().unwrap(),
            pointer_record,
        )
    };
    let decoded = inspect(&data);
    assert!(decoded.issues.is_empty());
    assert_eq!(
        decoded
            .fields
            .iter()
            .filter(|field| field.representation == "Float32")
            .count(),
        2
    );
    assert!(
        decoded
            .fields
            .iter()
            .any(|field| field.value == "-0 (0x80000000)")
    );
    data[28..32].copy_from_slice(&0x8080_1234_u32.to_le_bytes());
    data[40..48].copy_from_slice(&(-8_i64).to_le_bytes());
    let cyclic = inspect(&data);
    assert!(cyclic.issues.is_empty());
    assert_eq!(cyclic.fields.len(), 2);
}

#[test]
fn unsupported_codec_is_visible_and_never_guessed_from_payload() {
    let data = 1.5_f32.to_bits().to_le_bytes();
    let codecs = BTreeMap::from([(
        0x8080_000F,
        codecs::Declaration {
            size: 4,
            array_len: 0,
            fields: vec![codecs::Field {
                advance: 0,
                code: 255,
                presence: false,
                child: u32::MAX,
                params: [0; 4],
            }],
        },
    )]);
    let decoded = walk(
        &data,
        0,
        0x8080_000F,
        runtime_registry().unwrap(),
        &codecs,
        pointer_record,
    );
    assert!(decoded.issues.is_empty());
    assert_eq!(decoded.fields.len(), 1);
    assert_eq!(
        decoded.fields[0].representation,
        "Unmapped Storage: Unknown Operation"
    );
    assert!(decoded.fields[0].value.contains("255"));
}

fn field(advance: usize, code: u8, child: u32) -> codecs::Field {
    codecs::Field {
        advance,
        code,
        child,
        presence: false,
        params: [0; 4],
    }
}

#[test]
fn native_fixed_arrays_include_first_and_last_element_with_exact_bits() {
    // Real native type: four bytes, descriptor advance 1, fixed count 4.
    let data = [17, 29, 43, 71];
    let mut registry = Registry::new().unwrap();
    let decoded = walk(
        &data,
        0,
        0x8080_0063,
        runtime_registry().unwrap(),
        codecs().unwrap(),
        |handle| registry.record(handle, |_| Err("Unexpected generated type".into())),
    );
    assert!(decoded.issues.is_empty(), "{:?}", decoded.issues);
    let bytes = decoded
        .fields
        .iter()
        .filter(|f| f.representation == "Unsigned 8-Bit Integer")
        .map(|f| (f.owner_offset, f.value.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        bytes,
        vec![
            (0, "17".into()),
            (1, "29".into()),
            (2, "43".into()),
            (3, "71".into())
        ]
    );
}

#[test]
fn network_presence_does_not_hide_stored_asset_defaults() {
    // All three flags are wire metadata. They do not prefix the package value.
    let declarations = codecs().unwrap();
    let declaration = &declarations[&0x8080_2C5E];
    assert!(declaration.fields.iter().all(|f| f.presence));
    let data = [0xFF, 1, 0xFE, 3, 0, 1];
    let mut registry = Registry::new().unwrap();
    let decoded = walk(
        &data,
        0,
        0x8080_2C5E,
        runtime_registry().unwrap(),
        declarations,
        |handle| registry.record(handle, |_| Err("Unexpected generated type".into())),
    );
    assert!(decoded.issues.is_empty(), "{:?}", decoded.issues);
    assert_eq!(decoded.fields.len(), 6);
    assert!(
        decoded
            .fields
            .iter()
            .any(|f| f.owner_offset == 0 && f.value == "-1")
    );
    assert!(
        decoded
            .fields
            .iter()
            .any(|f| f.owner_offset == 1 && f.value == "true")
    );
    assert!(
        decoded
            .fields
            .iter()
            .all(|f| !f.representation.starts_with("Unmapped"))
    );
}

#[test]
fn dynamic_native_arrays_respect_count_and_reject_capacity_overflow() {
    let parent = 0x8080_FF01;
    let array = 0x8080_FF02;
    let mut nested = field(4, 1, array);
    nested.params = [1, 0, 0, 0];
    let declarations = BTreeMap::from([
        (
            parent,
            codecs::Declaration {
                size: 16,
                array_len: 0,
                fields: vec![field(0, 5, u32::MAX), nested],
            },
        ),
        (
            array,
            codecs::Declaration {
                size: 12,
                array_len: 3,
                fields: vec![field(4, 11, u32::MAX)],
            },
        ),
    ]);
    let mut data = Vec::new();
    for value in [2, (-0.0_f32).to_bits(), 0x7FC12345, 1.0_f32.to_bits()] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    let inspect = |data: &[u8]| {
        walk(
            data,
            0,
            parent,
            runtime_registry().unwrap(),
            &declarations,
            |handle| {
                Ok(Record {
                    size: if handle == parent { 16 } else { 12 },
                    fields: Vec::new().into(),
                })
            },
        )
    };
    let decoded = inspect(&data);
    assert!(decoded.issues.is_empty(), "{:?}", decoded.issues);
    let values = decoded
        .fields
        .iter()
        .filter(|f| f.representation == "Float32")
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].value, "-0 (0x80000000)");
    assert_eq!(values[1].value, "NaN (0x7FC12345)");
    for count in [0_u32, 4, u32::MAX] {
        data[..4].copy_from_slice(&count.to_le_bytes());
        let decoded = inspect(&data);
        assert_eq!(decoded.issues.is_empty(), count == 0);
        assert!(decoded.fields.iter().all(|f| f.representation != "Float32"));
    }
}

#[test]
fn raw_64_bit_wire_operation_is_not_automatically_a_float() {
    let data = 0x7FF8123456789ABC_u64.to_le_bytes();
    let raw = 0x8080_FF03;
    let declarations = BTreeMap::from([(
        raw,
        codecs::Declaration {
            size: 8,
            array_len: 0,
            fields: vec![field(0, 12, u32::MAX)],
        },
    )]);
    let decoded = walk(
        &data,
        0,
        raw,
        runtime_registry().unwrap(),
        &declarations,
        |_| {
            Ok(Record {
                size: 8,
                fields: Vec::new().into(),
            })
        },
    );
    assert!(decoded.issues.is_empty());
    assert_eq!(decoded.fields[0].representation, "Raw 64-Bit Value");
    assert_eq!(decoded.fields[0].value, "0x7FF8123456789ABC");
}

#[test]
fn native_only_curve_configuration_is_followed_and_decoded_without_wire_metadata() {
    let mut data = vec![0_u8; 0x600];
    data[0xC8..0xD0].copy_from_slice(&(0x5E0_i64 - 0xC8).to_le_bytes());
    data[0x5DC..0x5E0].copy_from_slice(&0x8080_3803_u32.to_le_bytes());
    for (at, value) in [
        (0x74, 90_f32),
        (0x88, 15.),
        (0xD8, 0.),
        (0x5E0, 2.),
        (0x5E4, 0.),
        (0x5E8, 0.),
        (0x5EC, 1.),
    ] {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut registry = Registry::new().unwrap();
    let decoded = walk(
        &data,
        0,
        0x8080_388F,
        runtime_registry().unwrap(),
        codecs().unwrap(),
        |handle| registry.record(handle, |_| Err("Unexpected generated schema".into())),
    );
    assert!(decoded.issues.is_empty(), "{:?}", decoded.issues);
    for (at, label, value) in [
        (0x88, "Initial Speed", "15 (0x41700000)"),
        (0x5E0, "Speed Curve Endpoint", "2 (0x40000000)"),
        (0x5EC, "Curve End Distance", "1 (0x3F800000)"),
    ] {
        let field = decoded
            .fields
            .iter()
            .find(|f| f.owner_offset == at && f.representation == "Float32")
            .unwrap();
        assert_eq!(field.label, label);
        assert_eq!(field.value, value);
        if at >= 0x5E0 {
            assert_eq!(field.schema, 0x8080_3803);
            assert_eq!(field.schema_offset, at - 0x5E0);
        }
    }
    assert!(labels::native_fields(0x8080_3803, 12).is_err());
    assert!(labels::native_fields(0x8080_388F, 0x5CF).is_err());
}

#[test]
fn array_scalar_cannot_read_across_its_element_boundary() {
    let array = 0x8080_FF04;
    let declarations = BTreeMap::from([(
        array,
        codecs::Declaration {
            size: 8,
            array_len: 2,
            fields: vec![field(4, 12, u32::MAX)],
        },
    )]);
    let decoded = walk(
        &[0; 8],
        0,
        array,
        runtime_registry().unwrap(),
        &declarations,
        |_| {
            Ok(Record {
                size: 8,
                fields: Vec::new().into(),
            })
        },
    );
    assert_eq!(decoded.issues.len(), 1);
    assert!(decoded.issues[0].contains("64-bit value exceeds"));
    assert!(decoded.fields.is_empty());
}

#[test]
fn encoded_native_values_display_both_decoded_and_original_bits() {
    let schema = 0x8080_FF05;
    let declarations = BTreeMap::from([(
        schema,
        codecs::Declaration {
            size: 8,
            array_len: 0,
            fields: vec![field(0, 44, u32::MAX), field(4, 45, u32::MAX)],
        },
    )]);
    let decoded = walk(
        &[0; 8],
        0,
        schema,
        runtime_registry().unwrap(),
        &declarations,
        |_| {
            Ok(Record {
                size: 8,
                fields: Vec::new().into(),
            })
        },
    );
    assert!(decoded.issues.is_empty());
    assert_eq!(decoded.fields.len(), 2);
    assert!(
        decoded.fields[0]
            .value
            .contains("(0xB230016E), stored 0x00000000")
    );
    assert!(
        decoded.fields[1]
            .value
            .contains("(0x4062B681), stored 0x00000000")
    );
}

#[test]
fn native_optional_regions_have_capacity_one_and_skip_inactive_storage() {
    // Real nested-record and scalar optionals use a zero array length and one
    // descriptor at offset zero. The parent count still limits their use.
    for schema in [0x8080_4AE6, 0x8080_4BA0, 0x8080_2DA5] {
        let mut registry = Registry::new().unwrap();
        let size = registry
            .record(schema, |_| Err("Unexpected schema".into()))
            .unwrap()
            .size;
        let mut data = vec![0; size];
        for count in [0_u32, 1, 2, u32::MAX] {
            data[..4].copy_from_slice(&count.to_le_bytes());
            let decoded = walk(
                &data,
                0,
                schema,
                runtime_registry().unwrap(),
                codecs().unwrap(),
                |handle| registry.record(handle, |_| Err("Unexpected schema".into())),
            );
            assert_eq!(
                decoded.issues.is_empty(),
                count <= 1,
                "{schema:08X}, {count}: {:?}",
                decoded.issues
            );
            if count == 0 {
                assert!(decoded.fields.iter().all(|f| f.schema == schema));
            }
            if count == 1 {
                assert!(
                    decoded
                        .fields
                        .iter()
                        .any(|f| f.owner_offset >= 4 && f.representation != "Inline Record")
                );
            }
        }
    }
}

#[test]
fn projectile_pool_curve_state_uses_the_full_element_offsets() {
    let mut data = vec![0; 0x210];
    data[0x194..0x198].copy_from_slice(&3.25_f32.to_le_bytes());
    data[0x1D1] = 1;
    let mut registry = Registry::new().unwrap();
    let decoded = walk(
        &data,
        0,
        0x8080_37BA,
        runtime_registry().unwrap(),
        codecs().unwrap(),
        |handle| registry.record(handle, |_| Err("Unexpected schema".into())),
    );
    assert!(decoded.issues.is_empty(), "{:?}", decoded.issues);
    assert!(decoded.fields.iter().any(|f| f.schema_offset == 0x194
        && f.label == "Curve Travel Distance"
        && f.value == "3.25 (0x40500000)"));
    assert!(decoded.fields.iter().any(|f| f.schema_offset == 0x1D1
        && f.label == "Distance Curve Enabled"
        && f.value == "true"));
    assert!(labels::native_fields(0x8080_37BA, 0x200).is_err());
}

#[test]
fn identifier_storage_requires_its_native_primitive_type() {
    let mut registry = Registry::new().unwrap();
    for (schema, data, expected, representation) in [
        (
            0x8080_0012,
            0x8000_0000_FEDC_BA98_u64.to_le_bytes().to_vec(),
            "0x80000000FEDCBA98",
            "64-Bit Identifier",
        ),
        (
            0x8080_0014,
            0xFEDC_BA98_u32.to_le_bytes().to_vec(),
            "0xFEDCBA98",
            "Package Reference",
        ),
    ] {
        let decoded = walk(
            &data,
            0,
            schema,
            runtime_registry().unwrap(),
            codecs().unwrap(),
            |handle| registry.record(handle, |_| Err("Unexpected schema".into())),
        );
        assert!(decoded.issues.is_empty());
        assert_eq!(decoded.fields.len(), 1);
        assert_eq!(decoded.fields[0].value, expected);
        assert_eq!(decoded.fields[0].representation, representation);
    }
    assert!(scalar(0, 0x8080_000F).is_none());
    assert!(scalar(0, 0x8080_0014).is_none());
}

#[test]
fn runtime_references_preserve_tokens_and_never_become_asset_links() {
    let schema = 0x8080_FF06;
    let declarations = BTreeMap::from([(
        schema,
        codecs::Declaration {
            size: 16,
            array_len: 0,
            fields: vec![
                field(0, 25, u32::MAX),
                field(8, 24, u32::MAX),
                field(12, 24, u32::MAX),
            ],
        },
    )]);
    let data = [0x1234_5678_u32, u32::MAX, 0x80B8_36DF, 0x80BA_A9B8]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let decoded = walk(
        &data,
        0,
        schema,
        runtime_registry().unwrap(),
        &declarations,
        |_| {
            Ok(Record {
                size: 16,
                fields: vec![(12, 4)].into(),
            })
        },
    );
    assert!(decoded.issues.is_empty());
    assert_eq!(decoded.fields.len(), 3);
    assert_eq!(
        decoded.fields[0].representation,
        "Guarded Runtime Reference"
    );
    assert_eq!(decoded.fields[0].value, "None, validation token 0x12345678");
    assert_eq!(decoded.fields[1].representation, "Runtime Reference");
    assert!(
        decoded.fields[1]
            .value
            .contains("runtime resolution required")
    );
    assert_eq!(decoded.fields[2].representation, "Package Reference");
}

#[test]
fn aliased_regions_keep_each_count_validation_and_the_valid_view() {
    let parent = 0x8080_FF07;
    let array = 0x8080_FF08;
    let mut nested = field(16, 1, array);
    nested.params = [1, 8, 0, 0];
    let declarations = BTreeMap::from([
        (
            parent,
            codecs::Declaration {
                size: 24,
                array_len: 0,
                fields: vec![nested],
            },
        ),
        (
            array,
            codecs::Declaration {
                size: 8,
                array_len: 2,
                fields: vec![field(4, 11, u32::MAX)],
            },
        ),
    ]);
    let mut data = vec![0; 24];
    data[..8].copy_from_slice(&16_i64.to_le_bytes());
    data[8..12].copy_from_slice(&3_u32.to_le_bytes());
    data[12..16].copy_from_slice(&array.to_le_bytes());
    data[16..20].copy_from_slice(&1.5_f32.to_le_bytes());
    data[20..24].copy_from_slice(&2.5_f32.to_le_bytes());
    let decoded = walk(
        &data,
        0,
        parent,
        runtime_registry().unwrap(),
        &declarations,
        |handle| {
            Ok(Record {
                size: if handle == parent { 24 } else { 8 },
                fields: if handle == parent {
                    vec![(0, 3)]
                } else {
                    vec![]
                }
                .into(),
            })
        },
    );
    assert_eq!(decoded.issues.len(), 1);
    assert!(decoded.issues[0].contains("exceeds its array capacity"));
    assert_eq!(
        decoded
            .fields
            .iter()
            .filter(|f| f.representation == "Float32")
            .count(),
        2
    );
}

#[test]
fn runtime_selected_records_keep_both_interpretations_and_all_storage_bits() {
    let selected = value::read(&0x8000_0000_0000_0001_u64.to_le_bytes(), 0, 8, 35, 0)
        .unwrap()
        .unwrap();
    assert_eq!(selected.0, "Runtime-Selected Eight-Byte Value");
    assert!(selected.1.contains("0x8000000000000001"));
    assert!(selected.1.contains("-9223372036854775807"));
    assert!(selected.1.contains("runtime selects"));
    let mut data = vec![0xA5; 16];
    data[8..12].copy_from_slice(&0x7FC1_2345_u32.to_le_bytes());
    data[12] = 0xFE;
    let value = value::read(&data, 0, 16, 37, 0).unwrap().unwrap();
    assert_eq!(value.0, "Runtime Record with Float32 Suffix");
    assert!(value.1.contains("0xA5A5A5A5A5A5A5A5"));
    assert!(value.1.contains("NaN (0x7FC12345)"));
    assert!(value.1.contains("signed byte -2"));
    assert!(value.1.contains("remaining bytes A5 A5 A5"));
    assert!(value::read(&data, 0, 7, 35, 0).is_err());
    assert!(value::read(&data, 0, 15, 37, 0).is_err());
}

#[test]
fn remaining_twelve_declarations_across_nine_schemas_have_verified_storage_views() {
    let cases = [
        (0x8080_38A3, 0x38, "Array Element Count"),
        (0x8080_3B36, 0x28, "Array Element Count"),
        (0x8080_3B36, 0x70, "Array Element Count"),
        (0x8080_3B36, 0x88, "Array Element Count"),
        (0x8080_3B73, 0x1A8, "Array Element Count"),
        (0x8080_422D, 0x20, "Array Element Count"),
        (0x8080_4BA1, 0xD0, "Array Element Count"),
        (0x8080_4DAC, 0x30, "Runtime Record with Float32 Suffix"),
        (0x8080_4DC2, 0x331, "Runtime-Selected Eight-Byte Value"),
        (0x8080_8BF1, 0x40, "Array Element Count"),
        (0x8080_8BF1, 0x50, "Array Element Count"),
        (0x8080_8BF5, 0x30, "Array Element Count"),
    ];
    let mut registry = Registry::new().unwrap();
    for (schema, offset, representation) in cases {
        let size = registry
            .record(schema, |_| Err("Unexpected schema".into()))
            .unwrap()
            .size;
        let decoded = walk(
            &vec![0; size],
            0,
            schema,
            runtime_registry().unwrap(),
            codecs().unwrap(),
            |handle| registry.record(handle, |_| Err("Unexpected schema".into())),
        );
        assert!(
            decoded.issues.is_empty(),
            "{schema:08X}: {:?}",
            decoded.issues
        );
        assert!(
            decoded
                .fields
                .iter()
                .any(|f| f.owner_offset == offset && f.representation == representation),
            "{schema:08X}+{offset:X}: {:#?}",
            decoded.fields
        );
        assert!(
            decoded
                .fields
                .iter()
                .all(|f| f.owner_offset != offset || !f.representation.starts_with("Unmapped"))
        );
    }
}
