use super::*;

/// Encodes a value only when it matches the reflected field type and native width.
pub fn encode_weapon_runtime_value(
    kind: &WeaponRuntimeValueKind,
    value: &WeaponRuntimeValue,
) -> Result<Vec<u8>, String> {
    let encoded = match (kind, value) {
        (WeaponRuntimeValueKind::Boolean, WeaponRuntimeValue::Boolean(value)) => {
            vec![u8::from(*value)]
        }
        (WeaponRuntimeValueKind::SignedInteger { bits }, WeaponRuntimeValue::Signed(value)) => {
            encode_signed(*bits, *value)?
        }
        (
            WeaponRuntimeValueKind::UnsignedInteger { bits }
            | WeaponRuntimeValueKind::Enum { bits }
            | WeaponRuntimeValueKind::BitFlags { bits }
            | WeaponRuntimeValueKind::HexIdentifier { bits },
            WeaponRuntimeValue::Unsigned(value),
        ) => encode_unsigned(*bits, *value)?,
        (WeaponRuntimeValueKind::Float32, WeaponRuntimeValue::Float32Bits(bits)) => {
            bits.to_le_bytes().to_vec()
        }
        (
            WeaponRuntimeValueKind::Vector4Float32,
            WeaponRuntimeValue::Vector4Float32Bits(values),
        ) => values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
        (WeaponRuntimeValueKind::FixedBytes { size }, WeaponRuntimeValue::Bytes(bytes)) => {
            if bytes.len() != *size as usize {
                return Err(format!(
                    "Runtime byte value has {} bytes, expected {size}",
                    bytes.len()
                ));
            }
            bytes.clone()
        }
        _ => return Err("Runtime value does not match the reflected field type".into()),
    };
    if encoded.len() != kind.byte_size() as usize {
        return Err("Runtime value encoder produced the wrong native width".into());
    }
    Ok(encoded)
}

pub(super) fn decode_runtime_value(
    data: &[u8],
    offset: usize,
    kind: &WeaponRuntimeValueKind,
) -> Result<WeaponRuntimeValue, String> {
    let bytes = data
        .get(offset..offset + kind.byte_size() as usize)
        .ok_or("Runtime field value extends beyond its owner payload")?;
    match kind {
        WeaponRuntimeValueKind::Boolean => match bytes[0] {
            0 => Ok(WeaponRuntimeValue::Boolean(false)),
            1 => Ok(WeaponRuntimeValue::Boolean(true)),
            value => Err(format!(
                "Runtime boolean contains invalid native value {value}"
            )),
        },
        WeaponRuntimeValueKind::SignedInteger { bits } => {
            decode_signed(*bits, bytes).map(WeaponRuntimeValue::Signed)
        }
        WeaponRuntimeValueKind::UnsignedInteger { bits }
        | WeaponRuntimeValueKind::Enum { bits }
        | WeaponRuntimeValueKind::BitFlags { bits }
        | WeaponRuntimeValueKind::HexIdentifier { bits } => {
            decode_unsigned(*bits, bytes).map(WeaponRuntimeValue::Unsigned)
        }
        WeaponRuntimeValueKind::Float32 => Ok(WeaponRuntimeValue::Float32Bits(u32::from_le_bytes(
            bytes.try_into().expect("four-byte runtime float"),
        ))),
        WeaponRuntimeValueKind::Vector4Float32 => {
            let mut values = [0; 4];
            for (index, value) in values.iter_mut().enumerate() {
                let start = index * 4;
                *value = u32::from_le_bytes(
                    bytes[start..start + 4]
                        .try_into()
                        .expect("four-byte runtime vector component"),
                );
            }
            Ok(WeaponRuntimeValue::Vector4Float32Bits(values))
        }
        WeaponRuntimeValueKind::FixedBytes { .. } => Ok(WeaponRuntimeValue::Bytes(bytes.to_vec())),
    }
}

pub(super) fn encode_signed(bits: u8, value: i64) -> Result<Vec<u8>, String> {
    match bits {
        8 => i8::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| format!("Signed runtime value {value} does not fit i8")),
        16 => i16::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| format!("Signed runtime value {value} does not fit i16")),
        32 => i32::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| format!("Signed runtime value {value} does not fit i32")),
        64 => Ok(value.to_le_bytes().to_vec()),
        _ => Err(format!("Unsupported signed runtime width {bits}")),
    }
}

pub(super) fn encode_unsigned(bits: u8, value: u64) -> Result<Vec<u8>, String> {
    match bits {
        8 => u8::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| format!("Unsigned runtime value {value} does not fit u8")),
        16 => u16::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| format!("Unsigned runtime value {value} does not fit u16")),
        32 => u32::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| format!("Unsigned runtime value {value} does not fit u32")),
        64 => Ok(value.to_le_bytes().to_vec()),
        _ => Err(format!("Unsupported unsigned runtime width {bits}")),
    }
}

pub(super) fn decode_signed(bits: u8, bytes: &[u8]) -> Result<i64, String> {
    match bits {
        8 => Ok(i64::from(i8::from_le_bytes([bytes[0]]))),
        16 => Ok(i64::from(i16::from_le_bytes(
            bytes.try_into().expect("two-byte runtime integer"),
        ))),
        32 => Ok(i64::from(i32::from_le_bytes(
            bytes.try_into().expect("four-byte runtime integer"),
        ))),
        64 => Ok(i64::from_le_bytes(
            bytes.try_into().expect("eight-byte runtime integer"),
        )),
        _ => Err(format!("Unsupported signed runtime width {bits}")),
    }
}

pub(super) fn decode_unsigned(bits: u8, bytes: &[u8]) -> Result<u64, String> {
    match bits {
        8 => Ok(u64::from(bytes[0])),
        16 => Ok(u64::from(u16::from_le_bytes(
            bytes.try_into().expect("two-byte runtime integer"),
        ))),
        32 => Ok(u64::from(u32::from_le_bytes(
            bytes.try_into().expect("four-byte runtime integer"),
        ))),
        64 => Ok(u64::from_le_bytes(
            bytes.try_into().expect("eight-byte runtime integer"),
        )),
        _ => Err(format!("Unsupported unsigned runtime width {bits}")),
    }
}

pub(super) fn ensure_field_range(
    owner_payload: &[u8],
    root: OwnerRootDescriptor,
    root_size: usize,
    absolute: usize,
    size: u32,
) -> Result<(), String> {
    let root_end = root
        .target
        .checked_add(root_size)
        .ok_or("Runtime root range overflowed")?;
    let end = absolute
        .checked_add(size as usize)
        .ok_or("Runtime field range overflowed")?;
    if absolute < root.target || end > root_end || end > owner_payload.len() {
        return Err(format!(
            "Runtime field range 0x{absolute:X}..0x{end:X} exceeds {} root 0x{:X}..0x{root_end:X}",
            root.kind.label().to_ascii_lowercase(),
            root.target
        ));
    }
    Ok(())
}
