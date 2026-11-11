//! Share one identity among structurally identical nodes of one private resource group.
//!
//! Two nodes are identical when their templates, storage, payload bytes and entry references agree
//! and their symbolic patches name identical nodes at the same offsets. Colour refinement over the
//! patch and entry-reference targets finds the largest such classes, including cycles such as a
//! texture header and its data naming each other. Converted vehicles repeat the same debris
//! geometry, effect records and allocations for every section that uses them, and stock shares
//! records across owners in the same way. Every alias is checked against its canonical node, byte
//! for byte and target for target, before it is used.
//!
//! A node keeps its own identity when the caller marks it unshareable, when its storage is not
//! inherited from its template, as audio media and banks are not, and when it is a type-16 loading
//! owner or a loading companion, whose identities pair with each other.
//!
//! Across groups, an entry is shared by content: a node whose final bytes match an entry another
//! group of the build recorded takes that entry's tag. Different recipes convert the same source
//! effects, shaders and records, and each would otherwise append its own copy.
use super::*;
use sha2::{Digest, Sha256};

const COMPANION_CLASS: u32 = 0x8080_9EF9;

pub(super) fn aliases<M>(
    nodes: &[Node<M>],
    manager: &PackageManager,
    shareable: impl Fn(&Node<M>) -> bool,
) -> Vec<usize> {
    let index: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(position, node)| (node.symbol.as_str(), position))
        .collect();
    let eligible: Vec<bool> = nodes
        .iter()
        .map(|node| {
            node.storage == crate::NewTagStorageMode::InheritTemplate
                && shareable(node)
                && manager
                    .get_entry(TagHash(node.template))
                    .is_some_and(|entry| {
                        entry.file_type != 16 && entry.reference != COMPANION_CLASS
                    })
        })
        .collect();
    // Each node's targets in a fixed order: its patches by offset, then its entry reference. A
    // target outside the group leaves the node unshareable.
    let mut targets: Vec<Option<Vec<(usize, usize)>>> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let mut found = Vec::with_capacity(node.patches.len() + 1);
        let mut complete = true;
        let mut patches = node.patches.clone();
        patches.sort();
        for (offset, symbol) in &patches {
            match index.get(symbol.as_str()) {
                Some(&target) => found.push((*offset, target)),
                None => complete = false,
            }
        }
        if let Some(reference) = &node.reference {
            match index.get(reference.as_str()) {
                Some(&target) => found.push((usize::MAX, target)),
                None => complete = false,
            }
        }
        targets.push(complete.then_some(found));
    }
    let base: Vec<[u8; 32]> = nodes
        .iter()
        .enumerate()
        .map(|(position, node)| {
            let mut digest = Sha256::new();
            if eligible[position] && targets[position].is_some() {
                digest.update(node.template.to_le_bytes());
                digest.update((node.payload.len() as u64).to_le_bytes());
                digest.update(&node.payload);
            } else {
                // A node that keeps its own identity is distinct from every other.
                digest.update(b"own");
                digest.update((position as u64).to_le_bytes());
            }
            digest.finalize().into()
        })
        .collect();
    let mut colour = base.clone();
    let mut classes = colour.iter().collect::<BTreeSet<_>>().len();
    loop {
        let next: Vec<[u8; 32]> = (0..nodes.len())
            .map(|position| {
                let Some(found) = &targets[position] else {
                    return base[position];
                };
                let mut digest = Sha256::new();
                digest.update(base[position]);
                for (offset, target) in found {
                    digest.update((*offset as u64).to_le_bytes());
                    digest.update(colour[*target]);
                }
                digest.finalize().into()
            })
            .collect();
        let refined = next.iter().collect::<BTreeSet<_>>().len();
        colour = next;
        if refined == classes {
            break;
        }
        classes = refined;
    }
    let mut first: BTreeMap<[u8; 32], usize> = BTreeMap::new();
    let mut aliases: Vec<usize> = (0..nodes.len()).collect();
    for position in 0..nodes.len() {
        let canonical = *first.entry(colour[position]).or_insert(position);
        if canonical != position && same(nodes, &targets, &colour, position, canonical) {
            aliases[position] = canonical;
        }
    }
    aliases
}

/// The build-wide record of shareable private entries by class and final payload digest, which the
/// runtime cache also restores and depends on.
pub(in crate::item) type Shared = BTreeMap<(u32, [u8; 32]), u32>;

/// Whether `node` may share an identity with an entry of another group: a node that keeps no
/// identity of its own, has no entry reference and is no entry reference's target. Texture headers
/// and their data name each other by entry reference, so they share only within one group.
pub(super) fn portable<M>(
    nodes: &[Node<M>],
    manager: &PackageManager,
    shareable: &impl Fn(&Node<M>) -> bool,
) -> Vec<bool> {
    let referenced: BTreeSet<&str> = nodes
        .iter()
        .filter_map(|node| node.reference.as_deref())
        .collect();
    nodes
        .iter()
        .map(|node| {
            node.storage == crate::NewTagStorageMode::InheritTemplate
                && node.reference.is_none()
                && !referenced.contains(node.symbol.as_str())
                && shareable(node)
                && manager
                    .get_entry(TagHash(node.template))
                    .is_some_and(|entry| {
                        entry.file_type != 16 && entry.reference != COMPANION_CLASS
                    })
        })
        .collect()
}

/// The tag another group of the build already holds for each canonical node, when there is one.
/// A node is borrowed when its class and its payload, with every patch resolved to a borrowed
/// tag, match a recorded entry exactly, so all of its targets must be borrowed first and a node
/// on a cycle never is. Two entries with identical bytes name the same entries, so either serves
/// every referrer.
pub(super) fn borrowed<M>(
    nodes: &[Node<M>],
    manager: &PackageManager,
    aliases: &[usize],
    portable: &[bool],
    shared: &Shared,
) -> Vec<Option<TagHash>> {
    let index: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(position, node)| (node.symbol.as_str(), position))
        .collect();
    // None: not visited. Some(None): visiting or not borrowed. Some(Some): borrowed.
    let mut state: Vec<Option<Option<TagHash>>> = vec![None; nodes.len()];
    type BorrowSources<'a, M> = (
        &'a [Node<M>],
        &'a PackageManager,
        &'a [usize],
        &'a [bool],
        &'a Shared,
        &'a BTreeMap<&'a str, usize>,
    );
    fn visit<M>(
        position: usize,
        (nodes, manager, aliases, portable, shared, index): BorrowSources<'_, M>,
        state: &mut [Option<Option<TagHash>>],
    ) -> Option<TagHash> {
        if let Some(known) = state[position] {
            return known;
        }
        state[position] = Some(None);
        let context = (nodes, manager, aliases, portable, shared, index);
        let node = &nodes[position];
        if !portable[position] {
            return None;
        }
        let mut payload = node.payload.clone();
        for (offset, symbol) in &node.patches {
            let target = aliases[*index.get(symbol.as_str())?];
            let tag = visit(target, context, state)?;
            payload
                .get_mut(*offset..offset + 4)?
                .copy_from_slice(&tag.0.to_le_bytes());
        }
        let class = manager.get_entry(TagHash(node.template))?.reference;
        let tag = shared
            .get(&(class, Sha256::digest(&payload).into()))
            .copied()
            .map(TagHash);
        state[position] = Some(tag);
        tag
    }
    (0..nodes.len())
        .map(|position| {
            (aliases[position] == position)
                .then(|| {
                    visit(
                        position,
                        (nodes, manager, aliases, portable, shared, &index),
                        &mut state,
                    )
                })
                .flatten()
        })
        .collect()
}

/// Whether `node` matches `canonical` byte for byte, with every target of equal colour.
fn same<M>(
    nodes: &[Node<M>],
    targets: &[Option<Vec<(usize, usize)>>],
    colour: &[[u8; 32]],
    node: usize,
    canonical: usize,
) -> bool {
    let (a, b) = (&nodes[node], &nodes[canonical]);
    let (Some(ta), Some(tb)) = (&targets[node], &targets[canonical]) else {
        return false;
    };
    a.template == b.template
        && a.storage == b.storage
        && a.payload == b.payload
        && ta.len() == tb.len()
        && ta
            .iter()
            .zip(tb)
            .all(|((oa, xa), (ob, xb))| oa == ob && colour[*xa] == colour[*xb])
}
