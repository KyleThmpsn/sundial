use super::*;

#[test]
fn native_groups_keep_exact_masks_and_unknown_registered_labels() {
    let source = fixture::registry();
    let registry = Registry::read(&source).unwrap();
    assert_eq!(registry.entries().len(), fixture::ENTRY_COUNT);
    let (_, _, start, _) = native_array_at(&source, 24).unwrap();
    for row in source[start..start + fixture::GROUPS_PRODUCED * 44].chunks_exact(44) {
        let hash = u32::from_le_bytes(row[..4].try_into().unwrap());
        assert_eq!(registry.mask(&[hash]).unwrap().as_slice(), &row[4..]);
    }
    assert!(registry.get(0xEC6A8FC5).unwrap().name.is_none());
    assert!(registry.mask(&[0xEC6A8FC5]).is_ok());
    assert!(registry.mask(&[0x12345678]).is_err());
    let mut future = source.clone();
    future[start + 43] |= 128;
    let changed = Registry::read(&future).unwrap();
    let first = u32_at(&source, start).unwrap();
    assert_eq!(changed.mask(&[first]).unwrap()[39] & 128, 128);
}

#[test]
fn malformed_registries_do_not_produce_selectable_entries() {
    let source = fixture::registry();
    assert!(Registry::read(&source[..64]).is_err());
    let (_, _, start, _) = native_array_at(&source, 8).unwrap();
    let mut duplicate = source.clone();
    duplicate[start + 4..start + 8].copy_from_slice(&source[start..start + 4]);
    assert!(
        Registry::read(&duplicate)
            .unwrap_err()
            .contains("more than once")
    );
}

#[test]
fn conflicts_expand_groups_without_rejecting_viable_alternatives() {
    let source = fixture::registry();
    let registry = Registry::read(&source).unwrap();
    let member = registry.members(0xBF39E12B).next().unwrap().hash;
    // A melee alternative is impossible when the entire melee group is excluded.
    assert!(
        registry
            .conflict(&[vec![member], vec![], vec![0xBF39E12B], vec![]])
            .unwrap()
            .is_some()
    );
    // Other alternatives can still match, so partial overlap is not a contradiction.
    assert!(
        registry
            .conflict(&[vec![member, 0x962EA19B], vec![], vec![0xBF39E12B], vec![]])
            .unwrap()
            .is_none()
    );
    assert!(
        registry
            .conflict(&[vec![], vec![member], vec![member], vec![]])
            .unwrap()
            .is_some()
    );
    assert!(
        registry
            .conflict(&[
                vec![],
                vec![member, 0x962EA19B],
                vec![],
                vec![member, 0x962EA19B]
            ])
            .unwrap()
            .is_some()
    );
    assert!(
        registry
            .conflict(&[vec![], vec![], vec![], vec![]])
            .unwrap()
            .is_none()
    );
}
