//! Synthetic label globals for tests.
//!
//! The shipped registry is a captured game asset, so it is not distributed with the
//! repository. This builds the same native layout from the handful of label hashes the
//! tests exercise, which `sandbox_perk::activation` already names as compiled constants.
//! Atom slots keep the indices the native registry uses so that recorded mask bits stay
//! comparable. Tests that need the real catalog read it through an environment variable.

use super::Mask;

const ATOM_CLASS: u32 = 0x8080_0070;
const GROUP_CLASS: u32 = 0x8080_94BE;

/// Wide enough to hold every atom index the tests reference.
const ATOM_COUNT: usize = 306;
/// Wide enough to hold every group index the tests reference, and with the atom slots
/// leaves room for every recovered name to be registered somewhere.
const GROUP_COUNT: usize = 22;

/// Atoms placed at their native indices so compiled masks keep their recorded bits.
const ATOMS: [(usize, u32); 5] = [
    (9, 0x962E_A19B),   // precision weapon kill
    (27, 0xE175_76C9),  // melee kill
    (33, 0xEC6A_8FC5),  // registered, with no recovered name
    (132, 0x5D3A_7C84), // melee kill
    (305, 0x1EE4_AC0E), // carried by the stock effect templates
];

/// Groups placed at their native indices.
///
/// Membership here is synthetic and deliberately does not mirror the real catalog. Each
/// group covers a few otherwise unremarkable atoms so that `members` resolves, and the
/// precision-kill atom at index 9 stays outside the melee group, which the conflict tests
/// rely on. The melee atoms at 27 and 132 are not members of the real melee group either.
const GROUPS: [(usize, u32, &[usize]); 2] = [
    (4, 0xC20D_D425, &[12]),     // grenade kill
    (5, 0xBF39_E12B, &[10, 11]), // melee kill
];

/// First bit position handed to filler groups. It sits in mask bytes 25 to 27, clear of every
/// reserved atom and of byte 39, which the mutation test flips.
const FILLER_GROUP_BITS: usize = 200;

/// Number of entries [`registry`] produces, atoms plus groups.
pub(crate) const ENTRY_COUNT: usize = ATOM_COUNT + GROUP_COUNT;
/// Number of group rows [`registry`] produces.
pub(crate) const GROUPS_PRODUCED: usize = GROUP_COUNT;

/// Builds a native label-globals payload the registry reader accepts.
///
/// Reserved slots keep their native index. Every other slot is filled from the recovered
/// name table so that labels carried by the stock effect templates still resolve, and any
/// remainder is filled with hashes that belong to no label.
pub(crate) fn registry() -> Vec<u8> {
    let taken = ATOMS
        .map(|(_, hash)| hash)
        .into_iter()
        .chain(GROUPS.map(|(_, hash, _)| hash))
        .collect::<Vec<_>>();
    let mut spare = super::names::hashes().filter(|hash| !taken.contains(hash));
    let atoms = (0..ATOM_COUNT)
        .map(|index| {
            ATOMS.iter().find(|(slot, _)| *slot == index).map_or_else(
                || spare.next().unwrap_or(0xF000_0000 | index as u32),
                |(_, hash)| *hash,
            )
        })
        .collect::<Vec<_>>();
    let groups = (0..GROUP_COUNT)
        .map(|index| {
            GROUPS
                .iter()
                .find(|(slot, _, _)| *slot == index)
                .map_or_else(
                    || {
                        // Give every filler group one distinct bit so the round-trip test
                        // compares real masks instead of trivially matching zeros.
                        let mut mask: Mask = [0; 40];
                        let bit = FILLER_GROUP_BITS + index;
                        mask[bit / 8] |= 1 << (bit % 8);
                        (spare.next().unwrap_or(0xE000_0000 | index as u32), mask)
                    },
                    |(_, hash, members)| {
                        let mut mask: Mask = [0; 40];
                        for member in *members {
                            mask[member / 8] |= 1 << (member % 8);
                        }
                        (*hash, mask)
                    },
                )
        })
        .collect::<Vec<(u32, Mask)>>();
    encode(&atoms, &groups)
}

/// Writes the two native arrays the registry reader expects at offsets 8 and 24.
fn encode(atoms: &[u32], groups: &[(u32, Mask)]) -> Vec<u8> {
    const ATOM_HEADER: usize = 40;
    let atom_rows = ATOM_HEADER + 16;
    let group_header = (atom_rows + atoms.len() * 4).next_multiple_of(8);
    let group_rows = group_header + 16;
    let mut bytes = vec![0; group_rows + groups.len() * 44];

    let mut write = |at: usize, value: &[u8]| bytes[at..at + value.len()].copy_from_slice(value);
    write(8, &(atoms.len() as u64).to_le_bytes());
    write(16, &(ATOM_HEADER as i64 - 16).to_le_bytes());
    write(24, &(groups.len() as u64).to_le_bytes());
    write(32, &(group_header as i64 - 32).to_le_bytes());
    write(ATOM_HEADER, &(atoms.len() as u64).to_le_bytes());
    write(ATOM_HEADER + 8, &ATOM_CLASS.to_le_bytes());
    for (index, hash) in atoms.iter().enumerate() {
        write(atom_rows + index * 4, &hash.to_le_bytes());
    }
    write(group_header, &(groups.len() as u64).to_le_bytes());
    write(group_header + 8, &GROUP_CLASS.to_le_bytes());
    for (index, (hash, mask)) in groups.iter().enumerate() {
        let at = group_rows + index * 44;
        write(at, &hash.to_le_bytes());
        write(at + 4, mask);
    }
    let length = bytes.len() as u64;
    bytes[..8].copy_from_slice(&length.to_le_bytes());
    bytes
}
