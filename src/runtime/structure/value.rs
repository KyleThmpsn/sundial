//! Read stored values only after their native representation has been established.
use super::*;

pub(super) fn managed_width(code: u8) -> Option<usize> {
    match code {
        24 => Some(4),
        25 | 32 | 37 => Some(8),
        _ => None,
    }
}

pub(super) fn suffix_fields(
    code: u8,
    at: usize,
) -> Vec<(usize, WeaponRuntimeValueKind, &'static str)> {
    if code != 37 {
        return Vec::new();
    }
    vec![
        (at + 8, WeaponRuntimeValueKind::Float32, "Float32"),
        (
            at + 12,
            WeaponRuntimeValueKind::SignedInteger { bits: 8 },
            "Signed 8-Bit Integer",
        ),
    ]
}

pub(super) fn storage(code: u8, schema: u32) -> Option<(WeaponRuntimeValueKind, u8)> {
    use WeaponRuntimeValueKind::*;
    if let Some(kind) = scalar(code, schema) {
        return Some((kind, 0));
    }
    Some(match code {
        12 if schema == 0x8080_0010 => (Float64, 0),
        12 | 35 => (HexIdentifier { bits: 64 }, 0),
        44 => (SignedInteger { bits: 32 }, 44),
        45 => (Float32, 45),
        _ => return None,
    })
}

pub(super) fn read(
    data: &[u8],
    at: usize,
    end: usize,
    code: u8,
    schema: u32,
) -> Result<Option<(String, String)>, String> {
    if let Some(kind) = scalar(code, schema) {
        require_width(at, end, kind.byte_size() as usize, "Native scalar")?;
        return read_scalar(data, at, &kind).map(Some);
    }
    Ok(Some(match code {
        12 => {
            require_width(at, end, 8, "Native 64-bit value")?;
            let bits = read_u64(data, at)?;
            if schema == 0x8080_0010 {
                (
                    "Float64".into(),
                    format!("{} (0x{bits:016X})", f64::from_bits(bits)),
                )
            } else {
                ("Raw 64-Bit Value".into(), format!("0x{bits:016X}"))
            }
        }
        44 | 45 => {
            require_width(at, end, 4, "Encoded native number")?;
            encoded(data, at, code)?
        }
        24 | 25 => {
            require_width(
                at,
                end,
                if code == 25 { 8 } else { 4 },
                "Native runtime reference",
            )?;
            runtime_reference(data, at, code)?
        }
        35 => {
            require_width(at, end, 8, "Runtime-selected value")?;
            selected(data, at)?
        }
        37 => {
            require_width(at, end, 16, "Native runtime record")?;
            runtime_record(data, at)?
        }
        _ => return Ok(None),
    }))
}

fn selected(data: &[u8], at: usize) -> Result<(String, String), String> {
    // 9F6440 selects schema 80809ACF (eight bytes via 80809F7B) or 80807BF1
    // (signed 64-bit) using runtime service 50BF00. The selector is not stored
    // in this object. 9F3D50 copies all eight native bytes without conversion.
    let bits = read_u64(data, at)?;
    Ok((
        "Runtime-Selected Eight-Byte Value".into(),
        format!(
            "0x{bits:016X}. Default schema 0x80809ACF stores eight bytes. Alternate schema 0x80807BF1 reads {}. The runtime selects the interpretation.",
            bits as i64
        ),
    ))
}

fn runtime_record(data: &[u8], at: usize) -> Result<(String, String), String> {
    // 9F6780 -> 500930 skips an eight-byte native prefix and serializes the
    // suffix as 808092FF: Float32 at +0, signed byte at +4. The copy operation
    // 9F37B0 preserves the full 16-byte record, including three trailing bytes.
    let prefix = read_u64(data, at)?;
    let real = read_u32(data, at + 8)?;
    let tail = read_u32(data, at + 12)?;
    Ok((
        "Runtime Record with Float32 Suffix".into(),
        format!(
            "Native prefix 0x{prefix:016X}, Float32 {} (0x{real:08X}), signed byte {}, remaining bytes {:02X} {:02X} {:02X}",
            f32::from_bits(real),
            tail as u8 as i8,
            (tail >> 8) & 255,
            (tail >> 16) & 255,
            tail >> 24
        ),
    ))
}

fn require_width(at: usize, end: usize, width: usize, label: &str) -> Result<(), String> {
    if at.checked_add(width).is_none_or(|last| last > end) {
        return Err(format!("{label} exceeds its declaring structure"));
    }
    Ok(())
}

fn read_scalar(
    data: &[u8],
    at: usize,
    kind: &WeaponRuntimeValueKind,
) -> Result<(String, String), String> {
    let value = decode_runtime_value(data, at, kind)?;
    let representation = match kind {
        WeaponRuntimeValueKind::Boolean => "Boolean".into(),
        WeaponRuntimeValueKind::Float32 => "Float32".into(),
        WeaponRuntimeValueKind::Vector4Float32 => "Four Float32 Values".into(),
        WeaponRuntimeValueKind::SignedInteger { bits } => format!("Signed {bits}-Bit Integer"),
        WeaponRuntimeValueKind::UnsignedInteger { bits } => format!("Unsigned {bits}-Bit Integer"),
        WeaponRuntimeValueKind::HexIdentifier { bits } => format!("{bits}-Bit Identifier"),
        _ => return Err("Unsupported native scalar representation".into()),
    };
    let text = match (kind, &value) {
        (WeaponRuntimeValueKind::HexIdentifier { bits }, WeaponRuntimeValue::Unsigned(value)) => {
            format!("0x{value:0width$X}", width = usize::from(*bits / 4))
        }
        _ => scalar_text(&value),
    };
    Ok((representation, text))
}

fn encoded(data: &[u8], at: usize, code: u8) -> Result<(String, String), String> {
    let stored = read_u32(data, at)?;
    let bits = numeric::decode(code, stored).ok_or("Unknown native numeric encoding")?;
    let (kind, value) = if code == 45 {
        (
            "Encoded Float32",
            format!("{} (0x{bits:08X})", f32::from_bits(bits)),
        )
    } else {
        (
            "Encoded Signed 32-Bit Integer",
            format!("{} (0x{bits:08X})", bits as i32),
        )
    };
    Ok((kind.into(), format!("{value}, stored 0x{stored:08X}")))
}

fn runtime_reference(data: &[u8], at: usize, code: u8) -> Result<(String, String), String> {
    // 9F4F20 calls 352310. That accessor validates the handle at +4 against the
    // token at +0 in a live runtime pool. 9F4E70 consumes a four-byte handle.
    let token = read_u32(data, at)?;
    let handle = if code == 25 {
        read_u32(data, at + 4)?
    } else {
        token
    };
    let value = if handle == u32::MAX {
        "None".into()
    } else {
        format!("Handle 0x{handle:08X}, runtime resolution required")
    };
    Ok(if code == 25 {
        (
            "Guarded Runtime Reference".into(),
            format!("{value}, validation token 0x{token:08X}"),
        )
    } else {
        ("Runtime Reference".into(), value)
    })
}
