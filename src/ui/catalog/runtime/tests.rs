use crate::ui::catalog::runtime as view;
use crate::weapon_runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeFieldSource, WeaponRuntimeRoot,
    WeaponRuntimeRootKind, WeaponRuntimeValue, WeaponRuntimeValueKind,
};
fn field(
    source: WeaponRuntimeFieldSource,
    value: WeaponRuntimeValue,
    kind: WeaponRuntimeValueKind,
) -> WeaponRuntimeField {
    WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            graph_tag: None,
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
        name_inferred: false,
    }
}

fn root() -> WeaponRuntimeRoot {
    WeaponRuntimeRoot {
        kind: WeaponRuntimeRootKind::Definition,
        schema: 0x8080_1234,
        owner_offset: 0,
        byte_size: 8,
        generated_schema: true,
        structure: Default::default(),
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
fn reading_values_preserves_wide_integers_signed_zero_and_nan_payloads() {
    for (value, kind, expected) in [
        (
            WeaponRuntimeValue::Unsigned(u64::MAX),
            WeaponRuntimeValueKind::UnsignedInteger { bits: 64 },
            "18446744073709551615",
        ),
        (
            WeaponRuntimeValue::Signed(i64::MIN),
            WeaponRuntimeValueKind::SignedInteger { bits: 64 },
            "-9223372036854775808",
        ),
        (
            WeaponRuntimeValue::Float32Bits(0x8000_0000),
            WeaponRuntimeValueKind::Float32,
            "-0",
        ),
        (
            WeaponRuntimeValue::Float32Bits(0x7FC0_1234),
            WeaponRuntimeValueKind::Float32,
            "7FC01234",
        ),
        (
            WeaponRuntimeValue::Float64Bits(0x8000_0000_0000_0000),
            WeaponRuntimeValueKind::Float64,
            "8000000000000000",
        ),
        (
            WeaponRuntimeValue::Float64Bits(0x7FF8_0000_0000_1234),
            WeaponRuntimeValueKind::Float64,
            "7FF8000000001234",
        ),
    ] {
        let field = field(WeaponRuntimeFieldSource::NativeMember, value.clone(), kind);
        assert!(view::exact_value_text(&field).contains(expected));
        let _ = view::value_text(&field);
        let _ = crate::weapon_runtime::presentation::field_tooltip(&field);
        assert_eq!(field.value, value);
        assert_eq!(
            view::export_field(&field)["value"],
            serde_json::to_value(&value).unwrap()
        );
    }
}
