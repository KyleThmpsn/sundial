//! Native storage transforms, recovered from the actual executed writer paths.
//! Image SHA-256: 26554b0c48109c9752a1a015ec0c45ff1137b675666338353ca35b9e81977484.
//! These run before wire bias/quantization. Neither operation changes the stored bytes.

pub(in crate::weapon_runtime) fn decode(code: u8, stored: u32) -> Option<u32> {
    Some(match code {
        // 9F9400 through 9F958B, before the integer bias is added.
        44 => stored ^ (stored << 16) ^ 0xB230_016E,
        // 9F7410 through 9F7516, before float range/quantization handling.
        45 => stored ^ (stored >> 16) ^ 0x4062_B681,
        _ => return None,
    })
}

/// Invert the independently recovered storage transform without changing its wire codec.
pub(in crate::weapon_runtime) fn encode(code: u8, decoded: u32) -> Option<u32> {
    Some(match code {
        44 => {
            let value = decoded ^ 0xB230_016E;
            value ^ (value << 16)
        }
        45 => {
            let value = decoded ^ 0x4062_B681;
            value ^ (value >> 16)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_storage_matches_executed_native_writers() {
        // Independent outputs from the original client instructions in an offline
        // CPU emulator. Each path was also checked on all 32 basis bits and 128
        // seeded random words. No callback or value-transform instruction was stubbed.
        for (stored, integer, real) in [
            (0, 0xB230_016E, 0x4062_B681),
            (0xFFFF_FFFF, 0xB230_FE91, 0xBF9D_B681),
            (0x8000_0000, 0x3230_016E, 0xC062_3681),
            (0x3F80_0000, 0x8DB0_016E, 0x7FE2_8901),
            (0x7FC1_2345, 0xEEB4_222B, 0x3FA3_EA05),
        ] {
            assert_eq!(decode(44, stored), Some(integer));
            assert_eq!(decode(45, stored), Some(real));
            assert_eq!(encode(44, integer), Some(stored));
            assert_eq!(encode(45, real), Some(stored));
        }
        assert_eq!(decode(11, 0), None);
        assert_eq!(decode(46, 0), None);
    }
}
