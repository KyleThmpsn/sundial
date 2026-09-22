//! Native value and provenance formatting shared by inspectors and property editors.
use super::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeValue, WeaponRuntimeValueKind,
};
pub fn value_text(field: &WeaponRuntimeField) -> String {
    if let Some(meaning) =
        super::modifiers::field_meaning(field.locator.type_handle, field.locator.value_offset)
    {
        let number = match field.value {
            WeaponRuntimeValue::Signed(value) => Some(value),
            WeaponRuntimeValue::Unsigned(value) => i64::try_from(value).ok(),
            _ => None,
        };
        if let Some((value, name)) = meaning.choices.iter().find(|(v, _)| Some(*v) == number) {
            return format!("{name} ({value})");
        }
    }
    match &field.value {
        WeaponRuntimeValue::Boolean(value) => value.to_string(),
        WeaponRuntimeValue::Signed(value) => value.to_string(),
        WeaponRuntimeValue::Unsigned(value) => match field.kind {
            WeaponRuntimeValueKind::HexIdentifier { bits }
            | WeaponRuntimeValueKind::BitFlags { bits } => {
                format!("0x{value:0width$X}", width = usize::from(bits) / 4)
            }
            WeaponRuntimeValueKind::Enum { .. } => format!("{value} (enum)"),
            _ => value.to_string(),
        },
        WeaponRuntimeValue::Float32Bits(bits) => float_text(*bits),
        WeaponRuntimeValue::Float64Bits(bits) => {
            format!("{} (0x{bits:016X})", f64::from_bits(*bits))
        }
        WeaponRuntimeValue::Vector4Float32Bits(bits) => format!(
            "[{}]",
            bits.iter()
                .map(|bits| float_text(*bits))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WeaponRuntimeValue::Bytes(bytes) => {
            let mut text = hex_bytes(&bytes[..bytes.len().min(32)]);
            if bytes.len() > 32 {
                text.push_str(&format!(" … ({} bytes)", bytes.len()));
            }
            text
        }
    }
}

pub fn exact_value_text(field: &WeaponRuntimeField) -> String {
    match &field.value {
        WeaponRuntimeValue::Bytes(bytes) => hex_bytes(bytes),
        WeaponRuntimeValue::Float32Bits(bits) => {
            format!("{} · bits 0x{bits:08X}", float_text(*bits))
        }
        WeaponRuntimeValue::Vector4Float32Bits(bits) => {
            format!("{} · bits {:08X?}", value_text(field), bits)
        }
        _ => value_text(field),
    }
}

fn float_text(bits: u32) -> String {
    let value = f32::from_bits(bits);
    if value.is_finite() {
        value.to_string()
    } else {
        format!("{value} (0x{bits:08X})")
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn kind_label(kind: &WeaponRuntimeValueKind) -> String {
    match kind {
        WeaponRuntimeValueKind::Boolean => "Boolean".to_owned(),
        WeaponRuntimeValueKind::SignedInteger { bits } => format!("Signed {bits}-bit integer"),
        WeaponRuntimeValueKind::UnsignedInteger { bits } => {
            format!("Unsigned {bits}-bit integer")
        }
        WeaponRuntimeValueKind::Enum { bits } => format!("{bits}-bit enum"),
        WeaponRuntimeValueKind::BitFlags { bits } => format!("{bits}-bit flags"),
        WeaponRuntimeValueKind::HexIdentifier { bits } => format!("{bits}-bit identifier"),
        WeaponRuntimeValueKind::Float32 => "32-bit float".to_owned(),
        WeaponRuntimeValueKind::Float64 => "64-bit float".to_owned(),
        WeaponRuntimeValueKind::Vector4Float32 => "Four 32-bit floats".to_owned(),
        WeaponRuntimeValueKind::FixedBytes { size } => format!("{size} exact bytes"),
    }
}

pub fn field_tooltip(field: &WeaponRuntimeField) -> String {
    let meaning =
        super::modifiers::field_meaning(field.locator.type_handle, field.locator.value_offset)
            .map(|meaning| meaning.help)
            .or_else(|| {
                super::health::field_help(field.locator.type_handle, field.locator.value_offset)
            })
            .or_else(|| {
                super::invisibility::field_help(
                    field.locator.type_handle,
                    field.locator.value_offset,
                )
            })
            .map_or(String::new(), |help| format!("\n{help}"));
    let source = match field.source {
        WeaponRuntimeFieldSource::GeneratedSchema => "generated package schema",
        WeaponRuntimeFieldSource::NativeMember => "named native member",
        WeaponRuntimeFieldSource::OpaqueNativeType => "unnamed native fixed-size type",
        WeaponRuntimeFieldSource::NativeDeclaration => "checked native storage declaration",
    };
    let path = field
        .locator
        .path
        .iter()
        .map(|element| {
            format!(
                "0x{:08X}:0x{:08X}@+0x{:X}",
                element.name_hash, element.type_handle, element.byte_offset
            )
        })
        .collect::<Vec<_>>()
        .join(" / ");
    let generated_kind = field
        .generated_kind
        .map_or_else(|| "N/A".to_owned(), |kind| format!("0x{kind:02X}"));
    let name_note = if field.name_inferred {
        "\nName: inferred from a name search. It is a recovered candidate, not a verified source name."
    } else {
        ""
    };
    format!(
        "{}{name_note}{meaning}\nSource: {source}\nValue Type: {}\nOriginal: {}\nBinding: 0x{:08X}, resource index {} (zero-based)\nRoot: {} · schema 0x{:08X}\nType: 0x{:08X} · generated kind {generated_kind}\nRoot offset: 0x{:X} · resolved owner offset: 0x{:X} · {} bytes\nReflected path: {path}",
        field.name,
        kind_label(&field.kind),
        value_text(field),
        field.locator.binding_hash,
        field.locator.resource_index,
        field.locator.root.label(),
        field.locator.root_schema,
        field.locator.type_handle,
        field.locator.value_offset,
        field.owner_offset,
        field.locator.byte_size,
    )
}

pub fn summary_value(value: &WeaponRuntimeValue) -> String {
    match value {
        WeaponRuntimeValue::Boolean(value) => if *value { "On" } else { "Off" }.into(),
        WeaponRuntimeValue::Signed(value) => value.to_string(),
        WeaponRuntimeValue::Unsigned(value) => value.to_string(),
        WeaponRuntimeValue::Float32Bits(bits) => float_text(*bits),
        WeaponRuntimeValue::Float64Bits(bits) => float64_text(*bits),
        WeaponRuntimeValue::Vector4Float32Bits(bits) => bits
            .iter()
            .map(|bits| float_text(*bits))
            .collect::<Vec<_>>()
            .join(", "),
        WeaponRuntimeValue::Bytes(bytes) => bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn float64_text(bits: u64) -> String {
    let value = f64::from_bits(bits);
    if value.is_finite() {
        value.to_string()
    } else {
        format!("{value} (0x{bits:016X})")
    }
}
