//! Keep armor families together, ordered by native equipment bucket.
use super::{ARMOR_SET_SIZE, Classes, GearPage};
use std::collections::{BTreeMap, VecDeque};

mod origin;
pub(crate) use origin::Origin;
#[cfg(feature = "d2-model-importer")]
pub(crate) use origin::imported;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Group {
    Native(u64),
    #[cfg(feature = "d2-model-importer")]
    Imported(String),
}

/// Head, arms, chest, legs and class item, in native Collections order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Slot(u8);

impl Slot {
    pub(crate) fn from_native_bucket(bucket: u8) -> Option<Self> {
        (3..=7).contains(&bucket).then(|| Self(bucket - 3))
    }

    pub(crate) fn from_bucket_hash(hash: u64) -> Option<Self> {
        crate::ItemKind::Armor
            .bucket_hashes()
            .iter()
            .position(|&bucket| bucket == hash)
            .map(|slot| Self(slot as u8))
    }
}

pub(crate) struct Piece {
    pub index: usize,
    pub slot: Option<Slot>,
    pub classes: Classes,
    pub group: Option<Group>,
}

pub(crate) struct Set {
    pub page: GearPage,
    /// Indices supplied by the caller, in visible tile order.
    pub members: Vec<usize>,
}

pub(crate) fn sets(pieces: &[Piece]) -> Vec<Set> {
    (0..3)
        .flat_map(|class| {
            class_sets(pieces, class)
                .into_iter()
                .enumerate()
                .map(move |(index, members)| Set {
                    page: GearPage::armor_set(class, index),
                    members,
                })
        })
        .collect()
}

fn class_sets(pieces: &[Piece], class: u8) -> Vec<Vec<usize>> {
    let mut groups: BTreeMap<&Group, Vec<&Piece>> = BTreeMap::new();
    let mut unmatched = Vec::new();
    for piece in pieces.iter().filter(|piece| piece.classes.supports(class)) {
        match (&piece.group, piece.slot) {
            (Some(group), Some(_)) => groups.entry(group).or_default().push(piece),
            _ => unmatched.push(piece),
        }
    }
    // Keep the first occurrence of each family as its page position. Recipe
    // identity order within a bucket cannot establish which family owns a piece.
    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by_key(|members| members[0].index);
    let mut rows = Vec::new();
    for members in groups {
        let mut slots: [VecDeque<usize>; ARMOR_SET_SIZE] = std::array::from_fn(|_| VecDeque::new());
        for piece in members {
            if let Some(slot) = piece.slot {
                slots[usize::from(slot.0)].push_back(piece.index);
            }
        }
        while slots.iter().filter(|slot| !slot.is_empty()).count() > 1 {
            rows.push(slots.iter_mut().filter_map(VecDeque::pop_front).collect());
        }
        let remaining = slots.into_iter().flatten().collect::<Vec<_>>();
        rows.extend(remaining.chunks(ARMOR_SET_SIZE).map(<[usize]>::to_vec));
    }
    let mut slots: [VecDeque<usize>; ARMOR_SET_SIZE] = std::array::from_fn(|_| VecDeque::new());
    let mut leftovers = Vec::new();
    for piece in unmatched {
        match piece.slot {
            Some(slot) => slots[usize::from(slot.0)].push_back(piece.index),
            None => leftovers.push(piece.index),
        }
    }
    // Only unknown families share equipment rows or the remaining packed rows.
    while slots.iter().all(|slot| !slot.is_empty()) {
        rows.push(slots.iter_mut().filter_map(VecDeque::pop_front).collect());
    }
    leftovers.extend(slots.into_iter().flatten());
    rows.extend(leftovers.chunks(ARMOR_SET_SIZE).map(<[usize]>::to_vec));
    rows
}
