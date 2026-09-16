//! Read-only native label registry and the bit sets shared by its consumers.
use crate::package_payload::{native_array_at, u32_at};
use std::collections::BTreeMap;
use tiger_pkg::{PackageManager, TagHash};

#[cfg(test)]
pub(crate) mod fixture;
mod names;
#[cfg(test)]
mod tests;

pub const TAG: u32 = 0x80C7_0CA1;
pub type Mask = [u8; 40];

pub fn name(hash: u32) -> Option<&'static str> {
    names::name(hash)
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub hash: u32,
    pub name: Option<&'static str>,
    pub group: bool,
    mask: Mask,
}

impl Entry {
    pub fn title(&self) -> String {
        self.name.map_or_else(
            || format!("Unnamed Label 0x{:08X}", self.hash),
            str::to_owned,
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct Registry {
    entries: Vec<Entry>,
    by_hash: BTreeMap<u32, usize>,
}

impl Registry {
    pub fn load(manager: &PackageManager) -> Result<Self, String> {
        let bytes = manager
            .read_tag(TagHash(TAG))
            .map_err(|error| format!("Could not read label globals: {error}"))?;
        Self::read(&bytes)
    }

    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        let (count, _, atoms, class) = native_array_at(bytes, 8)?;
        if class != 0x8080_0070 || count > 320 {
            return Err("Label globals have an unsupported layout.".into());
        }
        let (group_count, _, groups, class) = native_array_at(bytes, 24)?;
        if class != 0x8080_94BE || group_count > bytes.len() / 44 {
            return Err("Label groups have an unsupported layout.".into());
        }
        let mut result = Self::default();
        for index in 0..count {
            let hash = u32_at(bytes, atoms + index * 4)?;
            let mut mask = [0; 40];
            mask[index / 8] = 1 << (index % 8);
            result.insert(hash, false, mask)?;
        }
        for index in 0..group_count {
            let at = groups + index * 44;
            let hash = u32_at(bytes, at)?;
            let mask = bytes
                .get(at + 4..at + 44)
                .ok_or("Label group is truncated.")?
                .try_into()
                .map_err(|_| "Invalid label group width.")?;
            result.insert(hash, true, mask)?;
        }
        Ok(result)
    }

    fn insert(&mut self, hash: u32, group: bool, mask: Mask) -> Result<(), String> {
        if self.by_hash.insert(hash, self.entries.len()).is_some() {
            return Err(format!("Label 0x{hash:08X} is registered more than once."));
        }
        self.entries.push(Entry {
            hash,
            name: name(hash),
            group,
            mask,
        });
        Ok(())
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, hash: u32) -> Option<&Entry> {
        self.by_hash.get(&hash).map(|index| &self.entries[*index])
    }

    /// Expand registered groups using their original native bits, including reserved bits.
    pub fn mask(&self, hashes: &[u32]) -> Result<Mask, String> {
        let mut result = [0; 40];
        for hash in hashes {
            let entry = self
                .get(*hash)
                .ok_or_else(|| format!("Label 0x{hash:08X} is not registered."))?;
            for (out, source) in result.iter_mut().zip(entry.mask) {
                *out |= source;
            }
        }
        Ok(result)
    }

    pub fn members(&self, group: u32) -> impl Iterator<Item = &Entry> {
        let mask = self
            .get(group)
            .filter(|entry| entry.group)
            .map(|entry| entry.mask);
        self.entries.iter().filter(move |entry| {
            !entry.group && mask.is_some_and(|mask| overlaps(&mask, &entry.mask))
        })
    }

    pub fn groups(&self, hash: u32) -> impl Iterator<Item = &Entry> {
        let mask = self.get(hash).map(|entry| entry.mask);
        self.entries.iter().filter(move |entry| {
            entry.group
                && entry.hash != hash
                && mask.is_some_and(|mask| nonempty(&mask) && contains(&entry.mask, &mask))
        })
    }

    /// Only logical contradictions proven by the four native mask operations.
    /// Absence of a conflict does not imply the engine can produce every label combination.
    pub fn conflict(&self, lists: &[Vec<u32>; 4]) -> Result<Option<&'static str>, String> {
        let [any, all, excluded, not_all] = [
            self.mask(&lists[0])?,
            self.mask(&lists[1])?,
            self.mask(&lists[2])?,
            self.mask(&lists[3])?,
        ];
        if overlaps(&all, &excluded) {
            return Ok(Some(
                "A required label is also excluded. This filter cannot match.",
            ));
        }
        if nonempty(&any) && contains(&excluded, &any) {
            return Ok(Some(
                "Every matching alternative is excluded. This filter cannot match.",
            ));
        }
        if nonempty(&not_all) && contains(&all, &not_all) {
            return Ok(Some(
                "The required labels include a combination this filter excludes.",
            ));
        }
        Ok(None)
    }
}

fn overlaps(a: &Mask, b: &Mask) -> bool {
    a.iter().zip(b).any(|(a, b)| a & b != 0)
}
fn contains(a: &Mask, b: &Mask) -> bool {
    a.iter().zip(b).all(|(a, b)| a & b == *b)
}
fn nonempty(mask: &Mask) -> bool {
    mask.iter().any(|byte| *byte != 0)
}
