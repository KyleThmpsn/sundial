//! Share byte-identical resources across the imported graphs of one build, as stock packages
//! keep one copy of a texture, buffer or shader that many items use.
//!
//! Each imported graph links into private entries of its own, and an entry cannot point at
//! another entry's stored bytes. A resource is therefore shared by identity: later users name
//! the first copy's entries instead of allocating their own.
//!
//! A shared resource is a header and its data, which name each other. The data's type is the
//! header's plus 8 with the same subtype: textures (32 and 40, 2D or cube), geometry buffers
//! (32 and 40, vertex or index), shader programs (33 and 41) and samplers (34 and 42). Only the
//! header names the data, and neither carries a fixup, so the pair is immutable. A streamed 2D
//! texture also has the large buffer its header names at `+0x24`, which nothing else names and no
//! loading closure lists. Templates and complete payloads decide identity, apart from that header
//! word, which names each copy's own buffer. SHA-256 selects candidates and a full comparison
//! confirms them.
//!
//! A pair an earlier graph committed is not allocated again. Its symbols resolve to the
//! committed entries, which join the later graph's loading closures. A repeat inside one graph
//! is dropped before allocation, its users renamed to the graph's first copy, and its symbols
//! resolve to that copy's tags once the graph is linked.
//!
//! Sharing stays within one importer. On Sunrise's mod loader an importer keeps or rebuilds all of
//! its packages together, so no kept package names an entry that a rebuild could move.
use super::*;
use sha2::{Digest, Sha256};
use sundial::package_authoring::mod_content::Importer;

/// The header word that names a streamed texture's large buffer.
const LARGE_WORD: usize = 36;

/// A pair an earlier graph committed.
struct Committed {
    header: TagHash,
    data: TagHash,
    large: Option<TagHash>,
}

/// A graph's pair, by node index.
#[derive(Clone, Copy)]
struct Set {
    header: usize,
    data: usize,
    large: Option<usize>,
}

/// A pair of the graph being linked that commits new entries, by symbol.
struct Fresh {
    digest: [u8; 32],
    header: String,
    data: String,
    large: Option<String>,
}

/// What [`Identities::reuse`] decided for one graph, which [`Identities::register`] completes once
/// the graph is linked.
pub(super) struct Reuse {
    pub external: linking::External,
    fresh: Vec<Fresh>,
    /// A repeat's symbol and the symbol of the graph's first copy.
    aliases: Vec<(String, String)>,
}

#[derive(Default)]
pub(super) struct Identities {
    committed: BTreeMap<(Option<Importer>, [u8; 32]), Vec<Committed>>,
    /// Pairs resolved to an earlier graph's entries or to an earlier copy in their own graph, and
    /// the payload bytes they did not repeat.
    pub reused: usize,
    pub within: usize,
    pub bytes: u64,
}

fn kind(
    manager: &sundial::package_authoring::PackageManager,
    template: TagHash,
) -> Option<(u8, u8)> {
    manager
        .get_entry(template)
        .map(|entry| (entry.file_type, entry.file_subtype))
}

/// The graph's shareable pairs: immutable headers and data that name each other, where nothing
/// but the header names the data, and headers any node may name.
fn sets(
    nodes: &[linking::Node],
    manager: &sundial::package_authoring::PackageManager,
) -> AuthoringResult<Vec<Set>> {
    let positions: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.symbol.as_str(), index))
        .collect();
    let mut uses = BTreeMap::<&str, usize>::new();
    for node in nodes {
        let patched = node.patches.iter().map(|(_, target)| target.as_str());
        for target in node.reference.as_deref().into_iter().chain(patched) {
            *uses.entry(target).or_default() += 1;
        }
    }
    let plain = |node: &linking::Node| {
        node.storage == crate::NewTagStorageMode::InheritTemplate && node.companion.is_none()
    };
    let only_named_by = |node: &linking::Node, header: &linking::Node| {
        plain(node)
            && node.patches.is_empty()
            && node.reference.as_deref() == Some(header.symbol.as_str())
            && uses.get(node.symbol.as_str()) == Some(&1)
    };
    let mut sets = Vec::new();
    for (index, header) in nodes.iter().enumerate() {
        let Some((family @ 32..=34, subtype)) = kind(manager, header.template) else {
            continue;
        };
        if !plain(header) || header.streamed {
            continue;
        }
        let Some(&data_index) = header.reference.as_deref().and_then(|s| positions.get(s)) else {
            continue;
        };
        let data = &nodes[data_index];
        if data.streamed
            || !only_named_by(data, header)
            || kind(manager, data.template) != Some((family + 8, subtype))
        {
            continue;
        }
        let large = match header.patches.as_slice() {
            [] => None,
            [(LARGE_WORD, target)] if (family, subtype) == (32, 1) => {
                let Some(&large_index) = positions.get(target.as_str()) else {
                    continue;
                };
                let large = &nodes[large_index];
                if !large.streamed
                    || !only_named_by(large, header)
                    || header.payload.len() != LARGE_WORD + 4
                    || kind(manager, large.template) != Some((48, 1))
                {
                    continue;
                }
                Some(large_index)
            }
            _ => continue,
        };
        sets.push(Set {
            header: index,
            data: data_index,
            large,
        });
    }
    Ok(sets)
}

/// A header's bytes, without the word that names a streamed texture's own large buffer.
fn header_bytes(payload: &[u8], streamed: bool) -> [&[u8]; 2] {
    if streamed {
        [&payload[..LARGE_WORD], &payload[LARGE_WORD + 4..]]
    } else {
        [payload, &[]]
    }
}

fn digest(nodes: &[linking::Node], set: Set) -> [u8; 32] {
    let header = &nodes[set.header];
    let mut digest = Sha256::new();
    digest.update(header.template.0.to_le_bytes());
    digest.update((header.payload.len() as u64).to_le_bytes());
    for part in header_bytes(&header.payload, set.large.is_some()) {
        digest.update(part);
    }
    for index in [Some(set.data), set.large].into_iter().flatten() {
        let node = &nodes[index];
        digest.update(node.template.0.to_le_bytes());
        digest.update((node.payload.len() as u64).to_le_bytes());
        digest.update(&node.payload);
    }
    digest.finalize().into()
}

/// Whether two pairs of the same graph are identical.
fn equal(nodes: &[linking::Node], a: Set, b: Set) -> bool {
    let same = |x: usize, y: usize| {
        nodes[x].template == nodes[y].template && nodes[x].payload == nodes[y].payload
    };
    let streamed = a.large.is_some();
    nodes[a.header].template == nodes[b.header].template
        && nodes[a.header].payload.len() == nodes[b.header].payload.len()
        && header_bytes(&nodes[a.header].payload, streamed)
            == header_bytes(&nodes[b.header].payload, streamed)
        && same(a.data, b.data)
        && match (a.large, b.large) {
            (None, None) => true,
            (Some(x), Some(y)) => same(x, y),
            _ => false,
        }
}

/// Whether `committed` holds exactly the graph's pair `set`.
fn committed_equal(
    nodes: &[linking::Node],
    set: Set,
    committed: &Committed,
    assets: &crate::asset_packages::AssetPackages,
) -> bool {
    let same = |index: usize, tag: TagHash| {
        assets.tag(tag).is_some_and(|spec| {
            spec.template_tag == nodes[index].template
                && spec.storage == crate::NewTagStorageMode::InheritTemplate
                && spec.payload == nodes[index].payload
        })
    };
    let Some(header) = assets.tag(committed.header) else {
        return false;
    };
    let ours = &nodes[set.header].payload;
    let streamed = set.large.is_some();
    header.template_tag == nodes[set.header].template
        && header.payload.len() == ours.len()
        && header_bytes(&header.payload, streamed) == header_bytes(ours, streamed)
        && same(set.data, committed.data)
        && match (set.large, committed.large) {
            (None, None) => true,
            (Some(index), Some(tag)) => {
                header.payload[LARGE_WORD..LARGE_WORD + 4] == tag.0.to_le_bytes()
                    && same(index, tag)
            }
            _ => false,
        }
}

impl Identities {
    /// Resolves the pairs of `nodes` that an earlier graph of `origin` committed, folds repeats
    /// inside the graph into its first copy, and removes the nodes neither needs.
    pub(super) fn reuse(
        &mut self,
        origin: Option<Importer>,
        nodes: &mut Vec<linking::Node>,
        assets: &crate::asset_packages::AssetPackages,
        manager: &sundial::package_authoring::PackageManager,
    ) -> AuthoringResult<Reuse> {
        let mut external = linking::External::default();
        let mut resources = BTreeMap::new();
        let mut removed = BTreeSet::new();
        let mut fresh = Vec::new();
        let mut aliases = Vec::new();
        let mut renamed = BTreeMap::new();
        let mut local = BTreeMap::<[u8; 32], Vec<Set>>::new();
        for set in sets(nodes, manager)? {
            let digest = digest(nodes, set);
            let members = |set: Set| [Some(set.header), Some(set.data), set.large];
            let saved = [Some(set.data), set.large]
                .into_iter()
                .flatten()
                .map(|index| nodes[index].payload.len() as u64)
                .sum::<u64>();
            let committed = self
                .committed
                .get(&(origin, digest))
                .and_then(|list| list.iter().find(|c| committed_equal(nodes, set, c, assets)));
            if let Some(committed) = committed {
                let tags = [
                    Some(committed.header),
                    Some(committed.data),
                    committed.large,
                ];
                for (index, tag) in members(set).into_iter().zip(tags) {
                    if let (Some(index), Some(tag)) = (index, tag) {
                        external.symbols.insert(nodes[index].symbol.clone(), tag);
                        removed.insert(index);
                    }
                }
                // The header and its data load with every owner that names the header. A large
                // buffer streams on demand, so no closure lists it.
                resources.insert(committed.header, vec![committed.data]);
                resources.insert(committed.data, vec![committed.header]);
                self.reused += 1;
                self.bytes += saved;
                continue;
            }
            let copies = local.entry(digest).or_default();
            if let Some(&first) = copies.iter().find(|&&first| equal(nodes, set, first)) {
                for (repeat, kept) in members(set).into_iter().zip(members(first)) {
                    if let (Some(repeat), Some(kept)) = (repeat, kept) {
                        aliases.push((nodes[repeat].symbol.clone(), nodes[kept].symbol.clone()));
                        removed.insert(repeat);
                    }
                }
                renamed.insert(
                    nodes[set.header].symbol.clone(),
                    nodes[first.header].symbol.clone(),
                );
                self.within += 1;
                self.bytes += saved;
                continue;
            }
            copies.push(set);
            fresh.push(Fresh {
                digest,
                header: nodes[set.header].symbol.clone(),
                data: nodes[set.data].symbol.clone(),
                large: set.large.map(|index| nodes[index].symbol.clone()),
            });
        }
        external.resources = resources
            .into_iter()
            .map(|(tag, references)| crate::LoadingResource { tag, references })
            .collect();
        let mut index = 0;
        nodes.retain(|_| {
            index += 1;
            !removed.contains(&(index - 1))
        });
        // Only a repeat's header has users outside its pair.
        for node in nodes.iter_mut() {
            for (_, target) in &mut node.patches {
                if let Some(kept) = renamed.get(target.as_str()) {
                    target.clone_from(kept);
                }
            }
            if let Some(kept) = node.reference.as_deref().and_then(|s| renamed.get(s)) {
                node.reference = Some(kept.clone());
            }
        }
        Ok(Reuse {
            external,
            fresh,
            aliases,
        })
    }

    /// Records the pairs a graph of `origin` committed, by the tags `symbols` gave them, and gives
    /// each repeat it folded the tags of its first copy.
    pub(super) fn register(
        &mut self,
        origin: Option<Importer>,
        reuse: Reuse,
        symbols: &mut BTreeMap<String, TagHash>,
    ) -> AuthoringResult<()> {
        let tag = |symbols: &BTreeMap<String, TagHash>, symbol: &str| {
            symbols
                .get(symbol)
                .copied()
                .ok_or_else(|| invalid(format!("Shared resource {symbol} was not allocated")))
        };
        for pair in reuse.fresh {
            let committed = Committed {
                header: tag(symbols, &pair.header)?,
                data: tag(symbols, &pair.data)?,
                large: pair
                    .large
                    .as_deref()
                    .map(|symbol| tag(symbols, symbol))
                    .transpose()?,
            };
            self.committed
                .entry((origin, pair.digest))
                .or_default()
                .push(committed);
        }
        for (repeat, kept) in reuse.aliases {
            let kept = tag(symbols, &kept)?;
            if symbols.insert(repeat.clone(), kept).is_some() {
                return Err(invalid(format!(
                    "Shared resource {repeat} was also allocated"
                )));
            }
        }
        Ok(())
    }
}
