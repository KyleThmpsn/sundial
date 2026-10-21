//! Sunrise 0.6's table merge (Sunrise f1e7c66 `table_merge`). Where several mods change one
//! table entry and describe their changes in `.merge` sidecars, the loader serves one entry with
//! every mod's rows, and shifts the row numbers each mod's other entries name. The merge reads
//! only the bytes and sidecars it is given.
pub mod describe;
mod entry;
mod layout;
mod manifest;
mod plan;
pub mod recipes;
pub mod relayout;
mod rows;
pub mod sidecar;

use sidecar::{Entry, Sidecar};

/// Tag handles start at this base, with the package id above the entry index's 13 bits.
const TAG_BASE: u32 = 0x8080_0000;
const ENTRY_BITS: u32 = 13;

/// Why a sidecar was ignored, or why an entry or the row layer fell back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Format,
    Version,
    Binding,
    Stock,
    Sites,
    Mismatch,
    Pointer,
    Order,
    Duplicate,
    Pairs,
    Limit,
    Capacity,
    Unsupported,
}

impl Status {
    /// The reason in a mod notice.
    pub fn text(self) -> &'static str {
        match self {
            Self::Format => "the merge data is malformed",
            Self::Version => "the merge data has a version Sunrise doesn't read",
            Self::Binding => "the merge data was built for another package file",
            Self::Stock => "the mods were built against different game files",
            Self::Sites => "a row reference names a row the mod didn't add",
            Self::Mismatch => "the mods describe the table differently",
            Self::Pointer => "a row pointer doesn't land on a row",
            Self::Order => "the merged keys are out of order",
            Self::Duplicate => "two mods add the same key",
            Self::Pairs => "paired tables gained different row counts",
            Self::Limit => "the merged table holds too many rows",
            Self::Capacity => "the merged entry is too large",
            Self::Unsupported => "this entry can't be merged",
        }
    }
}

/// One mod's copy of one entry.
#[derive(Clone, Copy, Debug)]
pub struct Input<'a> {
    /// Names the mod in the outcome. Input order, not this value, is load order.
    pub owner: usize,
    /// The merge data of the mod's package file, or `None` when it has none.
    pub sidecar: Option<&'a Sidecar>,
    /// The entry bytes from the mod's own package file.
    pub bytes: &'a [u8],
}

/// Every mod's copy of one entry, in load order.
#[derive(Clone, Debug)]
pub struct EntryInputs<'a> {
    pub package: u16,
    pub entry: u32,
    pub inputs: Vec<Input<'a>>,
}

/// New bytes for one entry of the served package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    pub package: u16,
    pub entry: u32,
    pub bytes: Vec<u8>,
}

/// An entry served from the mod that loads last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fallback {
    pub package: u16,
    pub entry: u32,
    pub reason: Status,
}

/// Bytes two mods changed differently. The later mod's values were kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub package: u16,
    pub entry: u32,
    pub earlier: usize,
    pub later: usize,
    pub bytes: u32,
}

/// A mod whose rows added to one table were lost to a later mod.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub owner: usize,
    pub table: u32,
}

/// Rows a mod added that the loader renumbers, since earlier mods added rows to the same table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shifted {
    pub owner: usize,
    pub table: u32,
    pub descriptor: u32,
    pub rows: u32,
}

/// A sidecar the merge didn't use, with all other merge data of its mod.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ignored {
    pub owner: usize,
    pub package: u16,
    pub reason: Status,
}

/// Everything one merge decided.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// Sorted by package, then entry.
    pub replacements: Vec<Replacement>,
    pub fallbacks: Vec<Fallback>,
    pub conflicts: Vec<Conflict>,
    pub dropped: Vec<Dropped>,
    pub ignored: Vec<Ignored>,
    /// Empty while the row layer is off, since rows then keep their numbers.
    pub shifted: Vec<Shifted>,
    /// Why every row table fell back and the table that turned the row layer off, or `None`
    /// while row tables merge.
    pub rows_off: Option<(Status, u32)>,
}

/// Merges every entry that several mods change, and shifts the row numbers mods added.
pub fn merge(entries: &[EntryInputs<'_>]) -> Outcome {
    entry::merge_all(entries)
}

/// The tag handle of `entry` in `package`.
fn tag_of(package: u16, entry: u32) -> u32 {
    TAG_BASE + (u32::from(package) << ENTRY_BITS) + entry
}

/// The entry description one input carries, or `None` without merge data.
fn entry_of<'a>(input: &Input<'a>, entry: u32) -> Option<&'a Entry> {
    input.sidecar?.entry(entry)
}
