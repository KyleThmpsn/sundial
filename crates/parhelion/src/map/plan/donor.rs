//! The donor's records a map's world loads without using. A map copies its donor's map root,
//! bubble parent, bubble and containers with their companions, and a copied companion keeps every
//! entry the donor's listed. So the map loaded its donor's whole world: a converted Paradiso,
//! from `pvp_sabre`, loaded 4,509 of Midtown's records, among them Midtown's 347 static meshes, its
//! light collections, light occlusion bounds, lens flares and sun record. A resident record can
//! still answer a lookup: Midtown's occlusion record, loaded beside the map's own under the same
//! selector, left the map's grass black.
use super::*;
use std::collections::HashSet;

/// The class of a dependency index, whose payload names itself at `+0x08` and its owner at `+0x0C`.
const LOADING_INDEX: u32 = 0x8080_9EF9;

/// Whether an entry of `file_type` holds a record whose payload names other entries: an ordinary
/// record (8), or an owner a dependency index loads for (16), such as a map root, bubble parent,
/// bubble or container. A map root names its bubble layout at `+0x08` and its runtime name map at
/// `+0x0C`. With owners left unsearched, the trim dropped Midtown's name map `80FAA002`, which the
/// map's copied root names, and the next Paradiso faulted the same way while loading its world,
/// with the dropped tag on the faulting thread's stack. Searching owners also showed that the
/// containers and tables the map copies still name most of what the donor's lists load: on
/// Paradiso the trim then leaves 3,795 of the 4,146 donor entries the world lists.
fn record(file_type: u8) -> bool {
    matches!(file_type, 8 | 16)
}

/// Classes a donor entry of which always stays, since the client also finds them through a
/// registry rather than only by a record naming them. A runtime name map (`80809780`) maps 32-bit
/// name hashes to tags for the client's name registry.
const FOUND_BY_NAME: [u32; 1] = [0x8080_9780];

/// Drops from each companion among `tags`, at `addresses`, the entries in `donor`'s packages that
/// nothing that stays loaded names, and returns how many records left the map's lists.
/// `activity` is the map's activity package, whose companions are left as they are.
///
/// Every record that stays loaded is searched for each 32-bit tag and 64-bit tag hash of the
/// donor's listed entries: the map's own records other than its companions, the activity's, and
/// every entry either package's companions list outside the donor's packages. A data entry names
/// the entry its header names, as `references` give the map's own. Each donor entry found stays and
/// is searched the same way, and a donor companion stays with its owner. Searching only what the
/// map's own records named was not enough: the first Paradiso with the trim faulted at
/// `exe+0x438664` while loading its world, since stock materials, static meshes and texture
/// headers that its lists still loaded named 235 of Midtown's textures that they no longer did.
pub(super) fn trim(
    addresses: &[TagHash],
    tags: &mut [NewTagSpec],
    references: &[crate::NewTagReferenceOverride],
    activity: &[NewTagSpec],
    donor: &BTreeSet<u16>,
    manager: &PackageManager,
) -> AuthoringResult<usize> {
    let found = (0..tags.len())
        .filter_map(|entry| companion(addresses, entry, &tags[entry]).map(|found| (entry, found)))
        .collect::<Vec<_>>();
    let own = addresses
        .iter()
        .map(|tag| tag.pkg_id())
        .collect::<BTreeSet<_>>();
    let world = found
        .iter()
        .flat_map(|(_, (_, _, entries))| entries.iter())
        .map(|&(package, entry)| TagHash::new(package, entry))
        .collect::<BTreeSet<_>>();
    let candidates = world
        .iter()
        .filter(|tag| {
            donor.contains(&tag.pkg_id())
                && manager.get_entry(**tag).is_none_or(|entry| {
                    !(record(entry.file_type) && FOUND_BY_NAME.contains(&entry.reference))
                })
        })
        .map(|tag| tag.0)
        .collect::<HashSet<_>>();
    let mut loaded = world;
    let mut sources = Vec::new();
    let companions = found
        .iter()
        .map(|(entry, _)| *entry)
        .collect::<HashSet<_>>();
    let modes = crate::extend::resolve_reference_modes(tags.len(), references)?;
    for (index, spec) in tags.iter().enumerate() {
        let Some(template) = manager.get_entry(spec.template_tag) else {
            continue;
        };
        match modes.get(index) {
            Some(crate::NewTagReference::Tag(tag)) => sources.push(Source::Tag(tag.0)),
            Some(crate::NewTagReference::Template) if !record(template.file_type) => {
                sources.push(Source::Tag(template.reference));
            }
            _ => {}
        }
        if record(template.file_type) && !companions.contains(&index) {
            sources.push(Source::Payload(&spec.payload));
        }
    }
    for spec in activity {
        let Some(template) = manager.get_entry(spec.template_tag) else {
            continue;
        };
        if !record(template.file_type) {
            continue;
        }
        if template.file_type == 8 && template.reference == LOADING_INDEX {
            let (tag, owner) = (
                read_u32(&spec.payload, 0x08)?,
                read_u32(&spec.payload, 0x0C)?,
            );
            let listed = crate::shared_tag_dependency_index::dependencies(
                &spec.payload,
                TagHash(tag),
                TagHash(owner),
            )?;
            loaded.extend(listed.into_iter().map(TagHash));
        } else {
            sources.push(Source::Payload(&spec.payload));
        }
    }
    let mut pending = Vec::new();
    for source in sources {
        match source {
            Source::Tag(tag) => pending.push(tag),
            Source::Payload(payload) => named(payload, &candidates, manager, &mut pending),
        }
    }
    // Every other entry that stays loaded is read once.
    for tag in loaded {
        if candidates.contains(&tag.0) || own.contains(&tag.pkg_id()) {
            continue;
        }
        search(tag.0, &candidates, manager, &mut pending)?;
    }
    let reached = reachable(manager, &candidates, pending)?;
    let mut dropped = BTreeSet::new();
    for (entry, (tag, owner, entries)) in found {
        let drops = entries
            .iter()
            .map(|&(package, index)| TagHash::new(package, index))
            .filter(|listed| candidates.contains(&listed.0) && !reached.contains(&listed.0))
            .collect::<Vec<_>>();
        if drops.is_empty() {
            continue;
        }
        dropped.extend(drops.iter().map(|tag| tag.0));
        tags[entry].payload = crate::shared_tag_dependency_index::remove_dependencies(
            &tags[entry].payload,
            tag,
            owner,
            &drops,
        )?;
    }
    Ok(dropped.len())
}

/// Where a donor entry can be named from: a record's payload, or an entry named outright.
enum Source<'a> {
    Payload(&'a [u8]),
    Tag(u32),
}

fn read(manager: &PackageManager, tag: u32) -> AuthoringResult<Vec<u8>> {
    manager
        .read_tag(TagHash(tag))
        .map_err(|error| invalid(format!("Could not read loaded record {tag:08X}: {error}")))
}

/// Adds to `found` what the stock entry `tag` names among `candidates`: a record's payload is
/// searched, and a data entry names the entry its header names.
fn search(
    tag: u32,
    candidates: &HashSet<u32>,
    manager: &PackageManager,
    found: &mut Vec<u32>,
) -> AuthoringResult<()> {
    let Some(entry) = manager.get_entry(TagHash(tag)) else {
        return Ok(());
    };
    if record(entry.file_type) {
        named(&read(manager, tag)?, candidates, manager, found);
    } else if candidates.contains(&entry.reference) {
        found.push(entry.reference);
    }
    Ok(())
}

/// Adds to `found` each of `candidates` that `payload` names by a 32-bit tag or a 64-bit hash.
fn named(
    payload: &[u8],
    candidates: &HashSet<u32>,
    manager: &PackageManager,
    found: &mut Vec<u32>,
) {
    for at in (0..payload.len().saturating_sub(3)).step_by(4) {
        let word = u32::from_le_bytes([
            payload[at],
            payload[at + 1],
            payload[at + 2],
            payload[at + 3],
        ]);
        if candidates.contains(&word) {
            found.push(word);
        }
        if let Some(bytes) = payload.get(at..at + 8) {
            let mut hash = [0; 8];
            hash.copy_from_slice(bytes);
            if let Some(entry) = manager.lookup.tag64_entries.get(&u64::from_le_bytes(hash))
                && candidates.contains(&entry.hash32.0)
            {
                found.push(entry.hash32.0);
            }
        }
    }
}

fn reachable(
    manager: &PackageManager,
    candidates: &HashSet<u32>,
    mut pending: Vec<u32>,
) -> AuthoringResult<HashSet<u32>> {
    let mut reached = HashSet::new();
    while let Some(tag) = pending.pop() {
        if candidates.contains(&tag) && reached.insert(tag) {
            search(tag, candidates, manager, &mut pending)?;
        }
    }
    // A donor companion stays with its owner, unless the owner leaves too.
    for &tag in candidates {
        if reached.contains(&tag) {
            continue;
        }
        let Some(entry) = manager.get_entry(TagHash(tag)) else {
            continue;
        };
        if entry.file_type == 8 && entry.reference == LOADING_INDEX {
            let owner = read_u32(&read(manager, tag)?, 0x0C)?;
            if !candidates.contains(&owner) || reached.contains(&owner) {
                reached.insert(tag);
            }
        }
    }
    Ok(reached)
}
