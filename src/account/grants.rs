//! Authored items an install adds to the account because nothing in the game grants them: a
//! subclass has no Collections entry, and a shader stack is used up as it is applied.
use std::path::PathBuf;

/// One authored item to add, with the native bucket it occupies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoredItemGrant {
    pub item_hash: u32,
    pub bucket: u8,
    /// Rows the bucket holds. A character bucket counts equipped and stored items together.
    pub capacity: usize,
    pub target: AuthoredGrantTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthoredGrantTarget {
    /// One copy for each character of this class: 0 Titan, 1 Hunter, 2 Warlock. With `equip`,
    /// each of those characters also equips it, and its previous one goes to its inventory.
    Class { class_type: u8, equip: bool },
    /// One stack of this size in the profile inventory.
    Profile(i32),
}

/// One copy added, or left out because its bucket was full.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoredGrantOutcome {
    pub item_hash: u32,
    /// The character that received it, or `None` for a profile stack.
    pub character_index: Option<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthoredGrantReport {
    pub account_path: PathBuf,
    /// The verified copy the save took first, present when anything was added.
    pub backup_path: Option<PathBuf>,
    pub added: Vec<AuthoredGrantOutcome>,
    pub full: Vec<AuthoredGrantOutcome>,
    /// Characters that now have the item equipped.
    pub equipped: Vec<AuthoredGrantOutcome>,
}
