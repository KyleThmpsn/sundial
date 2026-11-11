//! Private native asset groups, symbolic links and replication enrollment.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod audio;
pub(in crate::item) mod projectile;
pub(in crate::item) mod replication;
pub(in crate::item) mod streaming;
mod structure;
pub(in crate::item) use structure::Shared;

#[derive(Clone)]
pub(in crate::item) struct Node<M = ()> {
    pub(in crate::item) symbol: String,
    pub(in crate::item) template: u32,
    pub(in crate::item) payload: Vec<u8>,
    pub(in crate::item) reference: Option<String>,
    pub(in crate::item) storage: crate::NewTagStorageMode,
    pub(in crate::item) patches: Vec<(usize, String)>,
    #[cfg_attr(not(feature = "d2-model-importer"), allow(dead_code))]
    pub(in crate::item) metadata: M,
}

/// Allocate and append one private resource group. Structurally identical nodes share one
/// identity unless `shareable` refuses one, as it must for any node `bind` rewrites by more
/// than its own content. A node whose final bytes match an entry another group recorded in
/// `shared` takes that entry instead of a copy, and the group records its own shareable entries.
/// Large textures stream, and their large buffers are left out of the eager resources.
#[allow(clippy::too_many_arguments)]
pub(in crate::item) fn author<M: Clone + Default>(
    nodes: &[Node<M>],
    roots: &[&str],
    entities: &BTreeSet<String>,
    manager: &PackageManager,
    packages: &mut crate::asset_packages::AssetPackages,
    (shareable, shared): (impl Fn(&Node<M>) -> bool, &mut structure::Shared),
    bind: impl FnOnce(&BTreeMap<String, TagHash>) -> AuthoringResult<BTreeMap<String, Vec<u8>>>,
    validate: impl Fn(&Node<M>, &[u8], bool) -> AuthoringResult<()>,
) -> AuthoringResult<(BTreeMap<String, TagHash>, Vec<TagHash>)> {
    let (nodes, streamed) = streaming::split(nodes, manager)?;
    let nodes: &[Node<M>] = &nodes;
    let required = replication::requirements(nodes, manager)?;
    let loading = replication::Loading::all(nodes, manager, roots, &required)?;
    let aliases = structure::aliases(nodes, manager, &shareable);
    let portable = structure::portable(nodes, manager, &shareable);
    let borrowed = structure::borrowed(nodes, manager, &aliases, &portable, shared);
    let allocated = |index: usize| aliases[index] == index && borrowed[index].is_none();
    let mut bounds = nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| allocated(*index))
        .map(|(_, node)| node.payload.len())
        .collect::<Vec<_>>();
    let mut ordinals = vec![0; nodes.len()];
    let mut next = 0;
    for (index, ordinal) in ordinals.iter_mut().enumerate() {
        if allocated(index) {
            *ordinal = next;
            next += 1;
        }
    }
    let loading_start = next;
    let together = loading
        .iter()
        .enumerate()
        .map(|(index, loading)| {
            let owner = nodes
                .iter()
                .position(|node| node.symbol == loading.owner)
                .ok_or_else(|| invalid("Replication loading owner is missing"))?;
            Ok((ordinals[aliases[owner]], loading_start + index))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    bounds.extend(loading.iter().map(|loading| loading.bound));
    let plan = packages.plan_graph(bounds, &together)?;
    let mut symbols = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        let canonical = aliases[index];
        let tag = match borrowed[canonical] {
            Some(tag) => tag,
            None => plan.tag(ordinals[canonical])?,
        };
        symbols.insert(node.symbol.clone(), tag);
    }
    let mut replacements = bind(&symbols)?;
    // A node whose identity is shared must bind as its canonical node does, and a node another
    // group holds must not bind at all. Audio nodes keep their own identities, and other
    // bind-time edits follow the payload alone.
    for (index, canonical) in aliases.iter().enumerate() {
        if index != *canonical
            && replacements.get(&nodes[index].symbol) != replacements.get(&nodes[*canonical].symbol)
        {
            return Err(invalid(format!(
                "Shared private asset {} bound differently from {}",
                nodes[index].symbol, nodes[*canonical].symbol
            )));
        }
        if borrowed[*canonical].is_some() && replacements.contains_key(&nodes[index].symbol) {
            return Err(invalid(format!(
                "Private asset {} matched another group's entry but binding changed it",
                nodes[index].symbol
            )));
        }
    }
    let mut tags = Vec::new();
    let mut references = Vec::new();
    let mut recorded = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.storage != crate::NewTagStorageMode::InheritTemplate
            && (node.reference.is_some()
                || manager
                    .get_entry(TagHash(node.template))
                    .is_none_or(|entry| entry.file_type != 26))
        {
            return Err(invalid(
                "Imported audio storage needs a raw audio template and its own identity",
            ));
        }
        let entity = entities.contains(&node.symbol);
        if entity
            && manager
                .get_entry(TagHash(node.template))
                .is_none_or(|entry| entry.reference != 0x80809C0F)
        {
            return Err(invalid("Private entity template is not a native entity"));
        }
        if manager.get_entry(TagHash(node.template)).is_none() {
            return Err(invalid(format!(
                "Private asset template 0x{:08X} is unavailable",
                node.template
            )));
        }
        let mut payload = replacements
            .remove(&node.symbol)
            .unwrap_or_else(|| node.payload.clone());
        for (offset, target) in &node.patches {
            let slot = payload
                .get_mut(*offset..offset + 4)
                .ok_or_else(|| invalid("Private asset patch exceeds its payload"))?;
            if slot != u32::MAX.to_le_bytes() {
                return Err(invalid("Private asset patch is not a placeholder"));
            }
            slot.copy_from_slice(
                &symbols
                    .get(target)
                    .ok_or_else(|| invalid("Private asset dependency is missing"))?
                    .0
                    .to_le_bytes(),
            );
        }
        validate(node, &payload, entity)?;
        if !allocated(index) {
            continue;
        }
        if portable[index] {
            let class = manager
                .get_entry(TagHash(node.template))
                .ok_or_else(|| invalid("Private asset template is unavailable"))?
                .reference;
            use sha2::Digest;
            recorded.push(((class, sha2::Sha256::digest(&payload).into()), tags.len()));
        }
        if let Some(target) = &node.reference {
            references.push(crate::NewTagReferenceOverride {
                new_tag_ordinal: tags.len(),
                reference: crate::NewTagReference::Tag(
                    *symbols
                        .get(target)
                        .ok_or_else(|| invalid("Private asset dependency is missing"))?,
                ),
            });
        }
        tags.push(NewTagSpec {
            template_tag: TagHash(node.template),
            payload,
            storage: node.storage,
        });
    }
    for (index, loading) in loading.iter().enumerate() {
        tags.push(loading.materialize(nodes, &symbols, plan.tag(loading_start + index)?)?);
    }
    for (key, ordinal) in recorded {
        let tag = plan.tag(ordinal)?;
        shared.entry(key).or_insert(tag.0);
    }
    packages.commit_graph(plan, tags, references)?;
    // Native replication allocations and their root are eager. The companion
    // is reached through package enrollment rather than the global index. The client streams
    // large buffers on demand, so no loading index lists them.
    let eager = nodes
        .iter()
        .filter(|node| !streamed.contains(&node.symbol))
        .map(|node| symbols[&node.symbol])
        .chain(required)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok((symbols, eager))
}
