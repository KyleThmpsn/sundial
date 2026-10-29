//! Which admitted mod adds each new package and which change each installed entry, so a reader
//! can say where an item it shows comes from.
use std::collections::BTreeMap;
use tiger_pkg::TagHash;

/// An admitted mod, as its manifest names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub name: String,
}

/// The mods the loader admits, in load order, with the packages each adds and the installed
/// entries each changes. A changed entry is one whose 16-byte entry record a patch replaces.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Provenance {
    mods: Vec<Source>,
    added: BTreeMap<u16, usize>,
    changed: BTreeMap<TagHash, Vec<usize>>,
}

impl Provenance {
    pub(super) fn admit(&mut self, id: &str, name: &str) -> usize {
        self.mods.push(Source {
            id: id.to_owned(),
            name: name.to_owned(),
        });
        self.mods.len() - 1
    }

    pub(super) fn add_package(&mut self, owner: usize, package: u16) {
        self.added.insert(package, owner);
    }

    pub(super) fn change(&mut self, owner: usize, tag: TagHash) {
        let owners = self.changed.entry(tag).or_default();
        if !owners.contains(&owner) {
            owners.push(owner);
        }
    }

    /// The mod whose new package holds `tag`.
    pub fn added_by(&self, tag: TagHash) -> Option<&Source> {
        self.added
            .get(&tag.pkg_id())
            .map(|&owner| &self.mods[owner])
    }

    /// The mods that change installed entry `tag`, in load order.
    pub fn changed_by(&self, tag: TagHash) -> impl Iterator<Item = &Source> {
        self.changed
            .get(&tag)
            .into_iter()
            .flatten()
            .map(|&owner| &self.mods[owner])
    }
}
