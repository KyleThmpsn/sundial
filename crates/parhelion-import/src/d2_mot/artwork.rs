//! Assignment policy for a graph containing the complete assembled weapon.
use anyhow::{Result, ensure};
pub mod source;

pub const EMPTY: u32 = 0x811C9DC5;

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
    fn assembled_model_replaces_body_and_suppresses_donor_attachments() {
        let mut singles = [111, 222];
        let mut slots = vec![(1, vec![10]), (0, vec![20]), (3, vec![30, 31, 32])];
        assembled(&mut singles, &mut slots, 999, &[]).unwrap();
        assert_eq!(singles, [EMPTY; 2]);
        assert_eq!(
            slots,
            vec![(1, vec![EMPTY]), (0, vec![999]), (3, vec![EMPTY; 3])]
        );
    }

    #[test]
    fn the_import_keeps_the_slot_of_the_part_hosting_it() {
        let mut singles = [111, 222];
        let mut slots = vec![(1, vec![999]), (0, vec![20]), (3, vec![30, 31])];
        assembled(&mut singles, &mut slots, 999, &[]).unwrap();
        assert_eq!(
            slots,
            vec![(1, vec![999]), (0, vec![EMPTY]), (3, vec![EMPTY; 2])]
        );
        let mut twice = vec![(1, vec![999]), (0, vec![999])];
        assert!(assembled(&mut singles, &mut twice, 999, &[]).is_err());
    }

    #[test]
    fn kept_parts_stay_in_their_slots() {
        let mut singles = [111, 222];
        let mut slots = vec![(0, vec![999]), (1, vec![77]), (3, vec![30, 78])];
        assembled(&mut singles, &mut slots, 999, &[77, 78]).unwrap();
        assert_eq!(
            slots,
            vec![(0, vec![999]), (1, vec![77]), (3, vec![EMPTY, 78])]
        );
        assert!(assembled(&mut singles, &mut slots, 999, &[999]).is_err());
    }

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

    #[test]
    fn direct_assignment_displays_complete_model_once() {
        let mut singles = [111, 222];
        assembled(&mut singles, &mut [], 999, &[]).unwrap();
        assert_eq!(singles, [999, EMPTY]);
        assert!(assembled(&mut singles, &mut [], EMPTY, &[]).is_err());
    }
}
