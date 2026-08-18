//! Catalyst plug identities used by Destiny 2 build 86657.20.08.23.
//!
//! Older exotic catalysts use a display-only completion plug that the client
//! maps to the actual catalyst perk. These hashes come from Bungie's public
//! manifest data and are the same legacy dummy-catalyst identities published in
//! `DestinyItemManager/d2-additional-info`'s generated
//! `output/dummy-catalyst-mapping.json`.

pub(super) const EMPTY_CATALYST_SOCKET: u64 = 1_498_917_124;

const LEGACY_COMPLETION_PLUGS: &[u64] = &[
    354_293_076,
    354_293_077,
    390_807_531,
    544_137_184,
    544_137_185,
    680_163_197,
    800_074_992,
    854_868_710,
    1_340_292_993,
    1_620_506_138,
    1_620_506_139,
    1_637_046_321,
    1_678_902_463,
    1_772_382_457,
    1_891_148_055,
    2_101_754_671,
    2_142_466_730,
    2_282_260_620,
    2_282_260_621,
    2_408_641_879,
    2_626_423_393,
    2_790_377_728,
    2_858_348_496,
    2_858_348_497,
    3_384_861_888,
    3_804_992_459,
    3_815_768_596,
    3_867_277_431,
    4_233_905_576,
    4_233_905_577,
];

pub(super) fn is_legacy_completion(hash: u64) -> bool {
    LEGACY_COMPLETION_PLUGS.binary_search(&hash).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_completion_plugs_remain_sorted_and_unique() {
        assert!(
            LEGACY_COMPLETION_PLUGS
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
    }
}
