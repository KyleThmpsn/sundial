use crate::runtime::{BindingHash, SchemaHandle};
use crate::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeFieldSource, WeaponRuntimeRootKind,
    WeaponRuntimeValue, WeaponRuntimeValueKind,
};
use crate::ui::catalog::runtime as view;
fn field(
    source: WeaponRuntimeFieldSource,
    value: WeaponRuntimeValue,
    kind: WeaponRuntimeValueKind,
) -> WeaponRuntimeField {
    WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            graph_tag: None,
            binding_hash: BindingHash::new(1),
            resource_index: 0,
            root: WeaponRuntimeRootKind::Definition,
            root_schema: SchemaHandle::new(0x8080_1234),
            path: Vec::new(),
            type_handle: SchemaHandle::new(0x8080_3456),
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
        assert_eq!(
            view::export_field(&field)["value"],
            serde_json::to_value(&value).unwrap()
        );
    }
}
