use serde_json::Value;

/// Formats a Destiny definition hash as canonical hexadecimal text.
pub(crate) fn format_hash_hex(hash: u64) -> String {
    format!("0x{hash:08X}")
}

/// Formats a Destiny definition hash as unsigned decimal text.
pub(crate) fn format_hash_decimal(hash: u64) -> String {
    hash.to_string()
}

/// Formats a Destiny definition hash with both hexadecimal and decimal text.
pub(crate) fn format_hash_hex_and_decimal(hash: u64) -> String {
    format!("{} · {}", format_hash_hex(hash), format_hash_decimal(hash))
}

/// Parses the explicit `0x`-prefixed hexadecimal syntax accepted by Sunrise settings.
pub(crate) fn parse_hash_hex(text: &str) -> Option<u64> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    if digits.is_empty()
        || digits.len() > 16
        || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

/// Reads either a JSON unsigned integer or Sunrise's explicit hexadecimal string form.
pub(crate) fn parse_unsigned_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(parse_hash_hex))
}

/// Computes the FNV-1 name hash used by Sunrise's package-backed name tables.
pub(crate) fn fnv1_name_hash(name: &str) -> u32 {
    name.bytes().fold(FNV1_EMPTY_HASH, |hash, byte| {
        hash.wrapping_mul(0x0100_0193) ^ u32::from(byte.to_ascii_lowercase())
    })
}

pub(crate) const FNV1_EMPTY_HASH: u32 = 0x811C_9DC5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_strict_and_canonical() {
        assert_eq!(parse_hash_hex("0xE516CF40"), Some(0xE516_CF40));
        assert_eq!(parse_hash_hex("0Xe516cf40"), Some(0xE516_CF40));
        assert_eq!(format_hash_hex(0x123), "0x00000123");
        assert_eq!(format_hash_decimal(0x123), "291");
        assert_eq!(format_hash_hex_and_decimal(0x123), "0x00000123 · 291");
        assert_eq!(parse_hash_hex("E516CF40"), None);
        assert_eq!(parse_hash_hex("0xnope"), None);
        assert_eq!(parse_unsigned_value(&Value::from(42)), Some(42));
        assert_eq!(
            parse_unsigned_value(&Value::String("0x0000002A".into())),
            Some(42)
        );
        assert_eq!(fnv1_name_hash("hiveship_d2"), 0xA85E_A752);
        assert_eq!(fnv1_name_hash("HiveShip_D2"), 0xA85E_A752);
    }
}
