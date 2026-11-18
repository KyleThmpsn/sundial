use super::*;

#[test]
fn signed_64_bit_decimal_drafts_require_an_exact_in_range_integer() {
    let kind = RuntimeValueKind::SignedInteger { bits: 64 };
    for valid in [
        "0",
        "-9007199254740993",
        "-9223372036854775808",
        "9223372036854775807",
    ] {
        assert!(valid_runtime_text(&kind, valid), "{valid}");
    }
    for invalid in [
        "",
        "-",
        "0x01",
        "1.5",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        assert!(!valid_runtime_text(&kind, invalid), "{invalid}");
    }
}

#[test]
fn unsigned_64_bit_decimal_drafts_require_an_exact_in_range_integer() {
    for kind in [
        RuntimeValueKind::UnsignedInteger { bits: 64 },
        RuntimeValueKind::Enum { bits: 64 },
        RuntimeValueKind::BitFlags { bits: 64 },
    ] {
        for valid in ["0", "9007199254740993", "18446744073709551615"] {
            assert!(valid_runtime_text(&kind, valid), "{kind:?}: {valid}");
        }
        for invalid in ["", "-1", "0x01", "1.5", "18446744073709551616"] {
            assert!(!valid_runtime_text(&kind, invalid), "{kind:?}: {invalid}");
        }
    }
}

#[test]
fn exact_hex_and_finite_float_validation_keep_their_existing_rules() {
    assert!(valid_runtime_text(
        &RuntimeValueKind::Float64,
        "0x3FF0000000000001"
    ));
    assert!(!valid_runtime_text(
        &RuntimeValueKind::Float64,
        "0x7FF8000000000001"
    ));
    assert!(!valid_runtime_text(
        &RuntimeValueKind::Float64,
        "0x7FF0000000000000"
    ));
    assert!(valid_runtime_text(
        &RuntimeValueKind::HexIdentifier { bits: 32 },
        "0xFFFFFFFF"
    ));
    assert!(!valid_runtime_text(
        &RuntimeValueKind::HexIdentifier { bits: 32 },
        "0x100000000"
    ));
    for kind in [RuntimeValueKind::Float32, RuntimeValueKind::Vector4Float32] {
        assert!(valid_runtime_text(&kind, "0x3F800001"));
        assert!(!valid_runtime_text(&kind, "0x7FC01234"));
        assert!(!valid_runtime_text(&kind, "0x7F800000"));
        assert!(!valid_runtime_text(&kind, "unfinished"));
    }
    assert!(valid_runtime_text(
        &RuntimeValueKind::FixedBytes { size: 2 },
        "12 34"
    ));
    assert!(!valid_runtime_text(
        &RuntimeValueKind::FixedBytes { size: 2 },
        "12"
    ));
}
