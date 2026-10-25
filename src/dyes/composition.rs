//! Compose native item and plug dye translations in their runtime precedence.
use std::collections::BTreeMap;

/// The first dye key of the gear type a key paints. Armor, weapons, ships, Sparrows and Ghost
/// Shells each have an armor, a cloth and a suit key, starting at 0, 4, 7, 10 and 13.
pub(crate) fn gear_type(key: i8) -> Option<i8> {
    [0, 4, 7, 10, 13]
        .into_iter()
        .find(|&first| (first..first + 3).contains(&key))
}

pub(crate) fn compose(
    base: &[Vec<(i8, u16)>; 3],
    plugs: impl Iterator<Item = [Vec<(i8, u16)>; 3]>,
) -> Vec<(i8, u16)> {
    // A shader carries every gear type. The item's own rows decide which set can paint it.
    let gear_types: Vec<_> = base
        .iter()
        .flatten()
        .filter_map(|&(key, value)| (value != u16::MAX).then(|| gear_type(key)).flatten())
        .collect();
    let mut dyes = BTreeMap::new();
    let mut insert = |rows: &[(i8, u16)]| {
        for &(key, value) in rows {
            if value != u16::MAX && gear_type(key).is_some_and(|first| gear_types.contains(&first))
            {
                dyes.insert(key, value);
            }
        }
    };
    insert(&base[1]);
    insert(&base[0]);
    for rows in plugs {
        for stage in [1, 0, 2] {
            insert(&rows[stage]);
        }
    }
    insert(&base[2]);
    dyes.into_iter().collect()
}
