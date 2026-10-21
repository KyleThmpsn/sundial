//! Private native asset groups, symbolic links and replication enrollment.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "d2-model-importer")]
pub(in crate::item) mod audio;
pub(in crate::item) mod projectile;
pub(in crate::item) mod replication;
mod texture;

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

pub(in crate::item) fn author<M>(
    nodes: &[Node<M>],
    roots: &[&str],
    entities: &BTreeSet<String>,
    manager: &PackageManager,
    packages: &mut crate::asset_packages::AssetPackages,
    bind: impl FnOnce(&BTreeMap<String, TagHash>) -> AuthoringResult<BTreeMap<String, Vec<u8>>>,
    validate: impl Fn(&Node<M>, &[u8], bool) -> AuthoringResult<()>,
) -> AuthoringResult<(BTreeMap<String, TagHash>, Vec<TagHash>)> {
    let required = replication::requirements(nodes, manager)?;
    let loading = replication::Loading::all(nodes, manager, roots, &required)?;
    let aliases = texture::aliases(nodes, manager);
    let mut bounds = nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| aliases[*index] == *index)
        .map(|(_, node)| node.payload.len())
        .collect::<Vec<_>>();
    bounds.extend(loading.iter().map(|loading| loading.bound));
    let index = packages.reserve_group(bounds)?;
    let package = &mut packages.packages[index];
    let allocator = AppendedTagAllocator::new(package.id, 0);
    let base = package.tags.len();
    let mut ordinals = vec![0; nodes.len()];
    let mut next = base;
    for (index, canonical) in aliases.iter().enumerate() {
        if index == *canonical {
            ordinals[index] = next;
            next += 1;
        }
    }
    let mut symbols = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        symbols.insert(
            node.symbol.clone(),
            allocator.assigned_tag(ordinals[aliases[index]], "Private asset group", "asset")?,
        );
    }
    let mut replacements = bind(&symbols)?;
    // Bind-time edits cannot change a texture whose identity is shared. Audio
    // replacements retain their own identities and are unaffected.
    for (index, canonical) in aliases.iter().enumerate() {
        if index != *canonical {
            for node in [&nodes[index], &nodes[*canonical]] {
                if replacements
                    .get(&node.symbol)
                    .is_some_and(|bytes| *bytes != node.payload)
                {
                    return Err(invalid("A shared resident texture changed during binding"));
                }
            }
        }
    }
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
        if aliases[index] != index {
            continue;
        }
        if let Some(target) = &node.reference {
            package.references.push(crate::NewTagReferenceOverride {
                new_tag_ordinal: package.tags.len(),
                reference: crate::NewTagReference::Appended(
                    symbols
                        .get(target)
                        .ok_or_else(|| invalid("Private asset dependency is missing"))?
                        .entry_index() as usize,
                ),
            });
        }
        package.tags.push(NewTagSpec {
            template_tag: TagHash(node.template),
            payload,
            storage: node.storage,
        });
    }
    for loading in loading {
        loading.append(nodes, &symbols, package)?;
    }
    // Native replication allocations and their root are eager. The companion
    // is reached through package enrollment rather than the global index.
    let eager = nodes
        .iter()
        .map(|node| symbols[&node.symbol])
        .chain(required)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok((symbols, eager))
}
