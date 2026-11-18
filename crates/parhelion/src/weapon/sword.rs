//! Imported sword attacks and guards belong to the imported graph and its validated native owners.
//!
//! The importer translates a source sword's attack rows against the base's sword owner, and its
//! guards against the base's guard owners. It reports four-byte edits and appended bytes
//! relative to each owner's resource. The build applies them to the weapon's private owners once
//! each base owner still has the digest the import saw.
//!
//! A keyed attack Shadowkeep has no row for spawns entities translated from the source. Each is
//! an attachment group in the graph's folder, loaded with the weapon's other imported groups,
//! and the appended row names its root by symbol until the build has allocated it.
use crate::{
    AuthoringResult,
    error::invalid,
    item::{RuntimeResourceAppend, RuntimeResourcePatch},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use sundial::package_authoring::{PackageManager, entity::weapon_component_bindings};
use tiger_pkg::TagHash;

/// The sword controller and the guard owners, by component binding.
const SWORD: u32 = 0xCD2B_CEAC;
const GUARD: u32 = 0x5211_1D67;

#[derive(Deserialize)]
pub(crate) struct Sword {
    #[serde(flatten)]
    attacks: Owner,
    #[serde(default)]
    guards: Vec<Guard>,
    #[serde(default)]
    attachments: Vec<Attachment>,
}

/// A translated entity group of a keyed attack: its folder inside the graph's, the prepared
/// fingerprint and its root symbol.
#[derive(Deserialize)]
struct Attachment {
    directory: String,
    sha256: String,
    symbol: String,
}

#[derive(Deserialize)]
struct Guard {
    resource_index: u16,
    #[serde(flatten)]
    owner: Owner,
}

/// What one base owner takes: the owner the import read, by tag, resource offset and digest,
/// with the source values written over it and the bytes added to it.
#[derive(Deserialize)]
struct Owner {
    native_owner: u32,
    native_resource: u64,
    native_sha256: String,
    edits: Vec<Edit>,
    #[serde(default)]
    appends: Vec<Append>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    offset: u32,
    before: [u8; 4],
    after: [u8; 4],
}

/// Bytes added to an owner: the array descriptors pointed at them, as the descriptor's resource
/// offset, its header inside the bytes and its count, the words inside the bytes that hold an
/// offset from the start of the owner with their targets inside the bytes, the pointer words
/// inside the bytes whose targets stay in the base's owner, by resource offset, and the
/// unresolved resource words inside the bytes that name an imported group's node by symbol.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Append {
    bytes: String,
    arrays: Vec<(u32, usize, u64)>,
    #[serde(default)]
    references: Vec<(usize, usize)>,
    retained_pointers: Vec<(usize, u32)>,
    #[serde(default)]
    symbols: Vec<(usize, String)>,
}

pub(crate) fn imported(graph: &serde_json::Value) -> AuthoringResult<Option<Sword>> {
    let sword = &graph["firing_translation"]["sword"];
    if sword["status"] != "translated" {
        return Ok(None);
    }
    if graph["source"] != "d1_roi" || graph["kind"] != "weapon" || sword["schema"] != 1 {
        return Err(invalid(
            "Imported sword attacks have an unsupported form. Reimport this weapon",
        ));
    }
    serde_json::from_value(sword.clone())
        .map(Some)
        .map_err(|error| invalid(format!("Imported sword attacks: {error}")))
}

impl Owner {
    /// The base's owner and its resource offset, which must be what the import read.
    fn carrier(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        (binding, index): (u32, u16),
    ) -> AuthoringResult<(Vec<u8>, usize)> {
        let bindings = weapon_component_bindings(entity, binding).map_err(invalid)?;
        let binding = bindings.get(usize::from(index)).ok_or_else(|| {
            invalid("Imported sword mechanics name an owner the weapon's runtime lacks")
        })?;
        let owner = manager
            .read_tag(TagHash(binding.owner_tag))
            .map_err(|error| invalid(error.to_string()))?;
        if binding.owner_tag != self.native_owner
            || binding.resource_offset != self.native_resource
            || hex::encode(Sha256::digest(&owner)) != self.native_sha256
        {
            return Err(invalid(
                "The weapon's sword mechanics differ from its converted source mechanics. Reimport it with this base weapon.",
            ));
        }
        let resource = usize::try_from(binding.resource_offset)
            .map_err(|_| invalid("Imported sword resource offset overflow"))?;
        Ok((owner, resource))
    }

    fn patches(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        binding: (u32, u16),
    ) -> AuthoringResult<Vec<RuntimeResourcePatch>> {
        let (owner, resource) = self.carrier(manager, entity, binding)?;
        let mut offsets = std::collections::BTreeSet::new();
        let mut patches = Vec::new();
        for edit in &self.edits {
            let at = resource
                .checked_add(edit.offset as usize)
                .ok_or_else(|| invalid("Imported sword offset overflow"))?;
            if edit.offset % 4 != 0
                || !offsets.insert(edit.offset)
                || owner.get(at..at.saturating_add(4)) != Some(edit.before.as_slice())
            {
                return Err(invalid(
                    "Imported sword mechanics name a field the base's owner does not hold",
                ));
            }
            patches.push(RuntimeResourcePatch {
                binding_hash: binding.0,
                resource_index: binding.1,
                offset: edit.offset,
                bytes: edit.after.to_vec(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            });
        }
        Ok(patches)
    }

    fn appends(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        binding: (u32, u16),
        symbols: &BTreeMap<String, TagHash>,
    ) -> AuthoringResult<Vec<RuntimeResourceAppend>> {
        if self.appends.is_empty() {
            return Ok(Vec::new());
        }
        let (owner, resource) = self.carrier(manager, entity, binding)?;
        let inside = |offset: u32, size: usize| {
            resource
                .checked_add(offset as usize)
                .and_then(|at| at.checked_add(size))
                .is_some_and(|end| end <= owner.len())
        };
        self.appends
            .iter()
            .map(|append| {
                let mut bytes = hex::decode(&append.bytes)
                    .map_err(|error| invalid(format!("Imported sword rows: {error}")))?;
                for (word, symbol) in &append.symbols {
                    let slot = word
                        .checked_add(4)
                        .filter(|&end| bytes.get(*word..end) == Some(&[0xFF; 4][..]))
                        .map(|end| *word..end)
                        .ok_or_else(|| {
                            invalid("Imported sword rows name an entity outside an empty slot")
                        })?;
                    let tag = symbols.get(symbol).ok_or_else(|| {
                        invalid(format!("Imported sword entity {symbol} was not allocated"))
                    })?;
                    bytes[slot].copy_from_slice(&tag.0.to_le_bytes());
                }
                let within =
                    |word: usize| word.checked_add(8).is_some_and(|end| end <= bytes.len());
                let arrays = append.arrays.iter().all(|&(descriptor, header, _)| {
                    inside(descriptor, 16)
                        && header >= 4
                        && header % 16 == 0
                        && header.checked_add(16).is_some_and(|end| end <= bytes.len())
                });
                let references = append
                    .references
                    .iter()
                    .all(|&(word, target)| within(word) && target < bytes.len());
                let pointers = append
                    .retained_pointers
                    .iter()
                    .all(|&(word, target)| within(word) && inside(target, 1));
                if append.arrays.is_empty() || !arrays || !references || !pointers {
                    return Err(invalid("Imported sword rows do not fit the base's owner"));
                }
                Ok(RuntimeResourceAppend {
                    binding_hash: binding.0,
                    resource_index: binding.1,
                    bytes,
                    slots: Vec::new(),
                    arrays: append.arrays.clone(),
                    pointers: Vec::new(),
                    references: append.references.clone(),
                    retained_pointers: append.retained_pointers.clone(),
                    retained_bytes: Vec::new(),
                })
            })
            .collect()
    }
}

impl Sword {
    /// The source values written over the base's attack rows and guard owners. A keyed override
    /// plans its own copies of the attack rows, so the two cannot share an owner.
    pub(crate) fn patches(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        keyed_override: bool,
    ) -> AuthoringResult<Vec<RuntimeResourcePatch>> {
        if keyed_override {
            return Err(invalid(
                "Imported sword attacks already own the attack profiles",
            ));
        }
        let mut patches = self.attacks.patches(manager, entity, (SWORD, 0))?;
        for guard in &self.guards {
            patches.extend(
                guard
                    .owner
                    .patches(manager, entity, (GUARD, guard.resource_index))?,
            );
        }
        Ok(patches)
    }

    /// The rows the source adds, with the lists, combos and guards pointed at them.
    pub(crate) fn appends(
        &self,
        manager: &PackageManager,
        entity: &[u8],
        symbols: &BTreeMap<String, TagHash>,
    ) -> AuthoringResult<Vec<RuntimeResourceAppend>> {
        let mut appends = self.attacks.appends(manager, entity, (SWORD, 0), symbols)?;
        for guard in &self.guards {
            appends.extend(guard.owner.appends(
                manager,
                entity,
                (GUARD, guard.resource_index),
                symbols,
            )?);
        }
        Ok(appends)
    }

    /// The translated entity groups the attack rows name: each group's folder inside the
    /// graph's, its prepared fingerprint and its root symbol.
    pub(crate) fn attachments(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.attachments.iter().map(|group| {
            (
                group.directory.as_str(),
                group.sha256.as_str(),
                group.symbol.as_str(),
            )
        })
    }
}
