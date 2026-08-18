use serde_json::Value;

/// Formats a Destiny definition hash using the canonical Sunrise representation.
pub(crate) fn format_hash(hash: u64) -> String {
    format!("0x{hash:08X}")
}

/// Parses the explicit `0x`-prefixed hash syntax accepted by Sunrise settings.
pub(crate) fn parse_hash(text: &str) -> Option<u64> {
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
        .or_else(|| value.as_str().and_then(parse_hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_strict_and_canonical() {
        assert_eq!(parse_hash("0xE516CF40"), Some(0xE516_CF40));
        assert_eq!(parse_hash("0Xe516cf40"), Some(0xE516_CF40));
        assert_eq!(format_hash(0x123), "0x00000123");
        assert_eq!(parse_hash("E516CF40"), None);
        assert_eq!(parse_hash("0xnope"), None);
        assert_eq!(parse_unsigned_value(&Value::from(42)), Some(42));
        assert_eq!(
            parse_unsigned_value(&Value::String("0x0000002A".into())),
            Some(42)
        );
    }
}
