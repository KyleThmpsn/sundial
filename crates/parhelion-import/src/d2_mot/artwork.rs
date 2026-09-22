//! Assignment policy for a graph containing the complete assembled weapon.
use anyhow::{Result, ensure};

pub const EMPTY: u32 = 0x811C9DC5;

/// Retain native selector and array layouts, but display the complete graph once
/// in the body slot. Native attachment assignments would add donor geometry.
pub fn assembled(singles: &mut [u32; 2], slots: &mut [(u64, Vec<u32>)], key: u32) -> Result<()> {
    ensure!(
        ![0, u32::MAX, EMPTY].contains(&key),
        "invalid private artwork key"
    );
    if slots.is_empty() {
        *singles = [key, EMPTY];
        return Ok(());
    }
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
    *singles = [EMPTY; 2];
    for (selector, keys) in slots {
        keys.fill(EMPTY);
        if *selector == 0 {
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
        assembled(&mut singles, &mut slots, 999).unwrap();
        assert_eq!(singles, [EMPTY; 2]);
        assert_eq!(
            slots,
            vec![(1, vec![EMPTY]), (0, vec![999]), (3, vec![EMPTY; 3])]
        );
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
            assert!(assembled(&mut singles, &mut slots, 999).is_err());
            assert_eq!(slots, original);
            assert_eq!(singles, [111, 222]);
        }
    }

    #[test]
    fn direct_assignment_displays_complete_model_once() {
        let mut singles = [111, 222];
        assembled(&mut singles, &mut [], 999).unwrap();
        assert_eq!(singles, [999, EMPTY]);
        assert!(assembled(&mut singles, &mut [], EMPTY).is_err());
    }
}
