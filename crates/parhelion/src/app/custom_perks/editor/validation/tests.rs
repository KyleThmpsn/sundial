use super::*;

#[test]
fn signed_64_bit_decimal_drafts_require_an_exact_in_range_integer() {
    let kind = WeaponRuntimeValueKind::SignedInteger { bits: 64 };
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
        WeaponRuntimeValueKind::UnsignedInteger { bits: 64 },
        WeaponRuntimeValueKind::Enum { bits: 64 },
        WeaponRuntimeValueKind::BitFlags { bits: 64 },
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
        &WeaponRuntimeValueKind::HexIdentifier { bits: 32 },
        "0xFFFFFFFF"
    ));
    assert!(!valid_runtime_text(
        &WeaponRuntimeValueKind::HexIdentifier { bits: 32 },
        "0x100000000"
    ));
    for kind in [
        WeaponRuntimeValueKind::Float32,
        WeaponRuntimeValueKind::Vector4Float32,
    ] {
        assert!(valid_runtime_text(&kind, "0x3F800001"));
        assert!(!valid_runtime_text(&kind, "0x7FC01234"));
        assert!(!valid_runtime_text(&kind, "0x7F800000"));
        assert!(!valid_runtime_text(&kind, "unfinished"));
    }
    assert!(valid_runtime_text(
        &WeaponRuntimeValueKind::FixedBytes { size: 2 },
        "12 34"
    ));
    assert!(!valid_runtime_text(
        &WeaponRuntimeValueKind::FixedBytes { size: 2 },
        "12"
    ));
}
