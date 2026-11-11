//! Runtime animation parameter dictionaries (class `80808EE1`) whose rows the loader merges.
//!
//! A dictionary's root holds the group array at `+0x08`, whose 32-byte `80808EE5` rows start at
//! `+0x60`. A group row holds its names at `+0`, 8-byte `80808EE9` rows of a name hash and its
//! parent's ordinal, and its lookups at `+16`, 24-byte `80808EE8` rows of a 16-byte mask, the
//! name at `+16` and the name's ordinal at `+20`. A name's row number is its ordinal and its bit
//! in every mask. The lookups ascend by name, which the mask lookup searches.
//!
//! A build gives a profile a lookup row of its own (`parameter::alias`), a copy of its parent's
//! mask and ordinal, so lookups merge by name and name no row another mod adds. A new name
//! (`parameter::extend`) takes the next ordinal and that bit, and an earlier mod's names would
//! move its row while its masks keep the bit. The names therefore keep their stock counts, and a
//! dictionary that gains a name stays opaque. The character dictionary `80BB2C45` gains names, so
//! it isn't listed.
//!
//! Imported character emission compacts the tables it extends, which grows a group's lookups in
//! place and moves every later array. Each array after the first one that grows is listed, so the
//! relayout puts them back.
use super::{Array, Recipe, Shape};

/// Where group 0's row starts: the group array's header at `+0x50`, then its rows.
const GROUP_ROWS: usize = 0x60;
const GROUP_SIZE: usize = 0x20;
const NAMES: usize = 0;
const LOOKUPS: usize = 0x10;
/// The name a lookup row is sorted by.
const LOOKUP_NAME: usize = 16;

const NAME_ROW: Shape = super::plain(8);
const LOOKUP_ROW: Shape = super::plain(24);

const fn group(index: usize) -> usize {
    GROUP_ROWS + index * GROUP_SIZE
}

/// A group's names, held at `count`, their stock count.
const fn names(name: &'static str, index: usize, count: u32) -> Array {
    super::append(name, group(index) + NAMES, &NAME_ROW, None, count)
}

/// A group's lookups, merged by name.
const fn lookups(name: &'static str, index: usize) -> Array {
    Array {
        descriptor: group(index) + LOOKUPS,
        key_offset: Some(LOOKUP_NAME),
        ..super::sorted(name, &LOOKUP_ROW)
    }
}

/// The first-person dictionaries builds give lookup rows and no names.
pub(super) const RUNTIME_TABLES: &[Recipe] = &[
    Recipe {
        tag: 0x80BB_D647,
        name: "first-person parameters",
        arrays: &[
            names("group 0 names", 0, 123),
            lookups("group 0 lookups", 0),
            names("group 1 names", 1, 4),
            lookups("group 1 lookups", 1),
        ],
    },
    Recipe {
        tag: 0x80BC_5F52,
        name: "first-person parameters",
        arrays: &[
            names("group 0 names", 0, 123),
            lookups("group 0 lookups", 0),
            names("group 1 names", 1, 4),
            lookups("group 1 lookups", 1),
            names("group 2 names", 2, 9),
            lookups("group 2 lookups", 2),
        ],
    },
    // One group, whose lookups are the last array, so nothing moves when they grow in place.
    Recipe {
        tag: 0x80C0_E6E6,
        name: "first-person parameters",
        arrays: &[lookups("group 0 lookups", 0)],
    },
];
