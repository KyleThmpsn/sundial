#[allow(clippy::unreadable_literal)]
mod hunter;
#[allow(clippy::unreadable_literal)]
mod titan;
#[allow(clippy::unreadable_literal)]
mod warlock;

// The installed Shadowkeep investment data does not expose the API-generated
// class restriction directly. These build-specific lists are generated from the
// public manifest for installed hashes, keeping browsing class-correct without a
// runtime dependency on a manifest database.
/// Where armor item strings keep their client classification tuple, whose first word is the
/// armor's class key.
const ITEM_STRING_CLASSIFICATION_OFFSET: usize = 0xB8;

/// The class an armor item's strings name, read from the class key that leads its client
/// classification tuple. Authored armor keeps its base's tuple, so this names its class where the
/// generated lists, which hold only stock hashes, cannot.
pub fn class_from_item_strings(strings: &[u8]) -> Option<u64> {
    let key =
        strings.get(ITEM_STRING_CLASSIFICATION_OFFSET..ITEM_STRING_CLASSIFICATION_OFFSET + 4)?;
    match u32::from_le_bytes(key.try_into().ok()?) {
        0xD60E_6BA5 => Some(0),
        0xD74E_BFB3 => Some(1),
        0x82E1_5A90 => Some(2),
        _ => None,
    }
}

pub(crate) fn class_type(hash: u64) -> Option<u64> {
    let hash = u32::try_from(hash).ok()?;
    if titan::HASHES.binary_search(&hash).is_ok() {
        Some(0)
    } else if hunter::HASHES.binary_search(&hash).is_ok() {
        Some(1)
    } else if warlock::HASHES.binary_search(&hash).is_ok() {
        Some(2)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_lists_are_sorted_and_disjoint() {
        for list in [titan::HASHES, hunter::HASHES, warlock::HASHES] {
            assert!(list.windows(2).all(|pair| pair[0] < pair[1]));
        }
        assert!(
            titan::HASHES
                .iter()
                .all(|hash| hunter::HASHES.binary_search(hash).is_err())
        );
        assert!(
            titan::HASHES
                .iter()
                .all(|hash| warlock::HASHES.binary_search(hash).is_err())
        );
        assert!(
            hunter::HASHES
                .iter()
                .all(|hash| warlock::HASHES.binary_search(hash).is_err())
        );
    }
}
