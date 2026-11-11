//! Private perk runtimes shared by content within one build.
//!
//! The same recipe on several plugs compiles the same private action, graphs and residency chain
//! each time. Every residency chain carries a replication record, and the client keeps one sorted
//! list of every installed replication record, so repeated copies spend a native budget as well as
//! package space. Stock maps many runtime keys to one action (14,763 keys name 6,739 actions), so a
//! perk whose output matches an earlier perk's output exactly takes the earlier action instead.
//!
//! Two outputs match when they have the same length and root position, and each entry has the same
//! template, inherited storage and bytes, with every word naming one of the output's own tags read
//! as that tag's position. A loading companion is compared by its owner and dependency closure,
//! since it encodes its dependencies as package entry bitmaps rather than tag words. Anything else
//! that differs, such as a tag written unaligned or a non-canonical companion, keeps the copy.
use super::*;
use sha2::{Digest, Sha256};
use std::ops::Range;

/// The outputs compiled so far in this build, by the digest of their relative form.
#[derive(Default)]
pub(in crate::item) struct CompiledRuntimes(BTreeMap<[u8; 32], (Range<usize>, TagHash)>);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Name {
    Own(usize),
    Other(u32),
}

#[derive(PartialEq, Eq)]
enum Entry {
    /// Bytes with each word naming an own tag cleared, and the position each such word named.
    Plain {
        template: u32,
        payload: Vec<u8>,
        own: Vec<(usize, usize)>,
    },
    Companion {
        template: u32,
        owner: Name,
        dependencies: BTreeSet<Name>,
    },
}

#[derive(PartialEq, Eq)]
struct Output {
    root: usize,
    entries: Vec<Entry>,
}

impl Output {
    fn read(
        manager: &PackageManager,
        allocator: AppendedTagAllocator,
        tags: &[NewTagSpec],
        range: Range<usize>,
        root: TagHash,
    ) -> AuthoringResult<Option<Self>> {
        let mut own = BTreeMap::new();
        for ordinal in range.clone() {
            let tag =
                allocator.assigned_tag(ordinal, "Shared private runtime", "private runtime")?;
            own.insert(tag.0, ordinal - range.start);
        }
        let name = |tag: u32| own.get(&tag).map_or(Name::Other(tag), |&at| Name::Own(at));
        let Some(&root) = own.get(&root.0) else {
            return Ok(None);
        };
        let mut entries = Vec::with_capacity(range.len());
        for (position, spec) in tags[range.clone()].iter().enumerate() {
            if spec.storage != crate::NewTagStorageMode::InheritTemplate {
                return Ok(None);
            }
            let template = spec.template_tag.0;
            let companion = manager
                .get_entry(spec.template_tag)
                .is_some_and(|entry| entry.reference == crate::format::SHARED_TAG_COMPANION_CLASS);
            if companion {
                let [owner_offset] = PRIVATE_PERK_RESIDENCY_COMPANION_OWNER_OFFSETS;
                let tag = allocator.assigned_tag(
                    range.start + position,
                    "Shared private runtime",
                    "private runtime",
                )?;
                let owner = TagHash(read_u32(&spec.payload, owner_offset)?);
                let Ok(dependencies) =
                    validate_shared_tag_companion_payload(&spec.payload, tag, owner)
                else {
                    return Ok(None);
                };
                entries.push(Entry::Companion {
                    template,
                    owner: name(owner.0),
                    dependencies: dependencies.iter().map(|&tag| name(tag)).collect(),
                });
                continue;
            }
            let mut payload = spec.payload.clone();
            let mut found = Vec::new();
            for at in (0..payload.len().saturating_sub(3)).step_by(4) {
                let word = u32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
                if let Some(&target) = own.get(&word) {
                    payload[at..at + 4].fill(0);
                    found.push((at, target));
                }
            }
            entries.push(Entry::Plain {
                template,
                payload,
                own: found,
            });
        }
        Ok(Some(Self { root, entries }))
    }

    fn digest(&self) -> [u8; 32] {
        fn name(digest: &mut Sha256, name: Name) {
            match name {
                Name::Own(at) => {
                    digest.update([0]);
                    digest.update((at as u64).to_le_bytes());
                }
                Name::Other(tag) => {
                    digest.update([1]);
                    digest.update(tag.to_le_bytes());
                }
            }
        }
        let mut digest = Sha256::new();
        digest.update((self.root as u64).to_le_bytes());
        for entry in &self.entries {
            match entry {
                Entry::Plain {
                    template,
                    payload,
                    own,
                } => {
                    digest.update([0]);
                    digest.update(template.to_le_bytes());
                    digest.update((payload.len() as u64).to_le_bytes());
                    digest.update(payload);
                    for (at, target) in own {
                        digest.update((*at as u64).to_le_bytes());
                        digest.update((*target as u64).to_le_bytes());
                    }
                }
                Entry::Companion {
                    template,
                    owner,
                    dependencies,
                } => {
                    digest.update([1]);
                    digest.update(template.to_le_bytes());
                    name(&mut digest, *owner);
                    digest.update((dependencies.len() as u64).to_le_bytes());
                    for dependency in dependencies {
                        name(&mut digest, *dependency);
                    }
                }
            }
        }
        digest.finalize().into()
    }
}

impl CompiledRuntimes {
    /// The earlier root `tags[first..]` matches, when one does. Otherwise records this output, so
    /// a later perk can share it. The caller drops a matched output and restores its records.
    pub(in crate::item) fn share(
        &mut self,
        manager: &PackageManager,
        allocator: AppendedTagAllocator,
        tags: &[NewTagSpec],
        first: usize,
        root: TagHash,
    ) -> AuthoringResult<Option<TagHash>> {
        let range = first..tags.len();
        let Some(output) = Output::read(manager, allocator, tags, range.clone(), root)? else {
            return Ok(None);
        };
        let digest = output.digest();
        if let Some((earlier, shared)) = self.0.get(&digest) {
            // A digest only finds the candidate. The earlier output is compared in full.
            if Output::read(manager, allocator, tags, earlier.clone(), *shared)?.as_ref()
                == Some(&output)
            {
                return Ok(Some(*shared));
            }
            return Ok(None);
        }
        self.0.insert(digest, (range, root));
        Ok(None)
    }
}
