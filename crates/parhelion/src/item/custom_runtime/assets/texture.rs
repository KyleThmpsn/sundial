//! Share immutable resident texture identities within one private resource group.
use super::*;
use sha2::{Digest, Sha256};

pub(super) fn aliases<M>(nodes: &[Node<M>], manager: &PackageManager) -> Vec<usize> {
    let symbols: BTreeMap<_, _> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.symbol.as_str(), index))
        .collect();
    let mut aliases: Vec<_> = (0..nodes.len()).collect();
    let mut candidates: BTreeMap<[u8; 32], Vec<(usize, usize)>> = BTreeMap::new();
    for (index, header) in nodes.iter().enumerate() {
        let Some(entry) = manager.get_entry(TagHash(header.template)) else {
            continue;
        };
        if entry.file_type != 32
            || entry.file_subtype != 1
            || header.storage != crate::NewTagStorageMode::InheritTemplate
            || !header.patches.is_empty()
            || header.payload.len() != 40
            || header.payload[12..14] != 0xCAFEu16.to_le_bytes()
            || header.payload[24] != 0
            || header.payload[28] != 0
            || header.payload[36..40] != u32::MAX.to_le_bytes()
        {
            continue;
        }
        let Some(&data_index) = header.reference.as_deref().and_then(|s| symbols.get(s)) else {
            continue;
        };
        let data = &nodes[data_index];
        if data.reference.as_deref() != Some(header.symbol.as_str())
            || data.storage != crate::NewTagStorageMode::InheritTemplate
            || !data.patches.is_empty()
            || manager
                .get_entry(TagHash(data.template))
                .is_none_or(|entry| entry.file_type != 40 || entry.file_subtype != 1)
            || u32::from_le_bytes(header.payload[..4].try_into().unwrap()) as usize
                != data.payload.len()
        {
            continue;
        }
        let mut digest = Sha256::new();
        digest.update(header.template.to_le_bytes());
        digest.update(data.template.to_le_bytes());
        digest.update(&header.payload);
        digest.update(&data.payload);
        let matches = candidates.entry(digest.finalize().into()).or_default();
        if let Some(&(previous_header, previous_data)) = matches.iter().find(|&&(h, d)| {
            nodes[h].template == header.template
                && nodes[d].template == data.template
                && nodes[h].payload == header.payload
                && nodes[d].payload == data.payload
        }) {
            aliases[index] = previous_header;
            aliases[data_index] = previous_data;
        } else {
            matches.push((index, data_index));
        }
    }
    aliases
}
