//! Assignment policy for a graph containing the complete assembled weapon.
use anyhow::{Result, ensure};
pub mod source;

pub const EMPTY: u32 = 0x811C9DC5;

/// Translate a modern art selector using shared shipped assignment identities.
/// Source inspection retains its original values. Only native placement uses this mapping.
pub fn native_selector(modern: u64) -> Result<u64> {
    match modern {
        0..=5 | 9..=12 => Ok(modern),
        17..=22 => Ok(modern + 4),
        _ => anyhow::bail!("Modern artwork selector {modern} has no validated Shadowkeep mapping"),
    }
}

/// Retain native selector and array layouts, but display the complete graph once, in the slot
/// of the donor part that hosts it. Native attachment assignments would add donor geometry, so
/// every other slot is emptied, except slots holding a `kept` key: a private donor part that
/// draws nothing but carries the markers the engine reads from it. When the host assignment
/// sits in no slot, the import takes the body selector.
pub fn assembled(
    singles: &mut [u32; 2],
    slots: &mut [(u64, Vec<u32>)],
    key: u32,
    kept: &[u32],
) -> Result<()> {
    ensure!(
        ![0, u32::MAX, EMPTY].contains(&key),
        "invalid private artwork key"
    );
    ensure!(
        kept.iter()
            .all(|part| ![0, u32::MAX, EMPTY, key].contains(part)),
        "invalid kept part key"
    );
    if slots.is_empty() {
        *singles = [key, EMPTY];
        return Ok(());
    }
    let placed = slots
        .iter()
        .map(|(_, keys)| keys.iter().filter(|slot_key| **slot_key == key).count())
        .sum::<usize>();
    if placed == 0 {
        ensure!(
            slots.iter().filter(|(selector, _)| *selector == 0).count() == 1,
            "assembled artwork requires one native body selector"
        );
        ensure!(
            slots
                .iter()
                .any(|(selector, keys)| *selector == 0 && !keys.is_empty()),
            "native body selector has no assignment position"
        );
    } else {
        ensure!(placed == 1, "the host assignment fills more than one slot");
    }
    *singles = [EMPTY; 2];
    for (selector, keys) in slots {
        for slot_key in keys.iter_mut() {
            if *slot_key != key && !kept.contains(slot_key) {
                *slot_key = EMPTY;
            }
        }
        if placed == 0 && *selector == 0 {
            keys[0] = key;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_layout_is_rejected_without_partial_mutation() {
        for mut slots in [
            vec![(1, vec![10])],
            vec![(0, vec![])],
            vec![(0, vec![10]), (0, vec![20])],
        ] {
            let original = slots.clone();
            let mut singles = [111, 222];
            assert!(assembled(&mut singles, &mut slots, 999, &[]).is_err());
            assert_eq!(slots, original);
            assert_eq!(singles, [111, 222]);
        }
    }
}
