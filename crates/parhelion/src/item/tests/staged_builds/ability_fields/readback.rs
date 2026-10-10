//! Retain a source reference route and follow the same native sites in the emitted packages.
//! Allocation changes can reorder a graph census, so a census index is not an identity.
use super::*;
use std::collections::{BTreeSet, VecDeque};
use sundial::package_authoring::{ability_spawns, entity::weapon_component_bindings};

#[derive(Clone, Copy)]
enum Site {
    Component {
        binding: u32,
        resource: usize,
        offset: u32,
    },
    Table(usize),
}

#[derive(Clone, Copy)]
pub(super) struct Step {
    site: Site,
    class: u32,
}

/// Native paired-object prefixes carry owner IDs that relocate with a private copy. Qualify
/// both prefixes and their reciprocal absolute offsets before treating a neighbor as identity.
pub(super) fn owner_identity(payload: &[u8], offset: usize, owner: u32) -> Option<usize> {
    let at = offset & !3;
    if read_u32(payload, at).ok()? != owner
        || read_u32(payload, at + 4).ok()? & 0xffff_0000 != 0x8080_0000
    {
        return None;
    }
    let twin = usize::try_from(u64::from_le_bytes(
        payload.get(at + 8..at + 16)?.try_into().ok()?,
    ))
    .ok()?;
    let back = u64::from_le_bytes(
        payload
            .get(twin.checked_add(8)?..twin.checked_add(16)?)?
            .try_into()
            .ok()?,
    );
    (twin != at
        && read_u32(payload, twin).ok()? == owner
        && read_u32(payload, twin.checked_add(4)?).ok()? & 0xffff_0000 == 0x8080_0000
        && back == at as u64)
        .then_some(at)
}

pub(super) fn route(manager: &PackageManager, root: u32, target: u32) -> Vec<Step> {
    let mut pending = VecDeque::from([(root, Vec::new())]);
    let mut seen = BTreeSet::new();
    while let Some((tag, path)) = pending.pop_front() {
        if tag == target {
            return path;
        }
        if !seen.insert(tag) {
            continue;
        }
        let mut outgoing = Vec::new();
        if ability_spawns::is_table(manager, tag) {
            for (child, offsets) in ability_spawns::table_entries(manager, tag).unwrap() {
                for offset in offsets {
                    outgoing.push((child, Site::Table(offset)));
                }
            }
        } else {
            let payload = manager.read_tag(TagHash(tag)).unwrap();
            let links = ability_spawns::links(manager, tag, &payload).unwrap();
            for link in links.spawns.into_iter().chain(links.tables) {
                outgoing.push((
                    link.graph,
                    Site::Component {
                        binding: link.binding_hash,
                        resource: usize::from(link.resource_index),
                        offset: link.offset,
                    },
                ));
            }
        }
        for (child, site) in outgoing {
            let mut path = path.clone();
            path.push(Step {
                site,
                class: manager.get_entry(child).unwrap().reference,
            });
            pending.push_back((child, path));
        }
    }
    panic!("the source root {root:08X} has no reference route to graph {target:08X}");
}

pub(super) fn copied_owner(
    manager: &PackageManager,
    root: u32,
    path: &[Step],
    binding: u32,
    resource: usize,
) -> u32 {
    let mut tag = root;
    for step in path {
        let payload = manager.read_tag(TagHash(tag)).unwrap();
        tag = match step.site {
            Site::Table(offset) => read_u32(&payload, offset).unwrap(),
            Site::Component {
                binding,
                resource,
                offset,
            } => {
                let bindings = weapon_component_bindings(&payload, binding).unwrap();
                let selected = &bindings[resource];
                let owner = manager.read_tag(TagHash(selected.owner_tag)).unwrap();
                let at = usize::try_from(selected.resource_offset).unwrap() + offset as usize;
                read_u32(&owner, at).unwrap()
            }
        };
        assert_eq!(
            manager.get_entry(tag).unwrap().reference,
            step.class,
            "the emitted reference retains its native target class"
        );
    }
    let payload = manager.read_tag(TagHash(tag)).unwrap();
    weapon_component_bindings(&payload, binding).unwrap()[resource].owner_tag
}
