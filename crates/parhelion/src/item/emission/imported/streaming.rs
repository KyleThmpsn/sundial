//! Stream the two largest mips of large imported textures, as streamed stock textures do. The
//! native form and the rules for which textures stream are shared with private runtime groups.
use super::*;
use crate::item::custom_runtime::assets::streaming::{large_template, leading_bytes};

/// Moves each large texture's two leading mips out of its data node into a new streamed large
/// buffer node, and points its header at that buffer.
pub(super) fn split(
    nodes: &mut Vec<linking::Node>,
    manager: &sundial::package_authoring::PackageManager,
) -> AuthoringResult<()> {
    let positions: BTreeMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.symbol.clone(), index))
        .collect();
    // A data node named by more than one node keeps its whole chain, since every header that
    // names it reads the same bytes.
    let mut uses = BTreeMap::<&str, usize>::new();
    for node in nodes.iter() {
        let patched = node.patches.iter().map(|(_, target)| target.as_str());
        for target in node.reference.as_deref().into_iter().chain(patched) {
            *uses.entry(target).or_default() += 1;
        }
    }
    let shared: BTreeSet<String> = uses
        .into_iter()
        .filter(|&(_, count)| count > 1)
        .map(|(symbol, _)| symbol.to_owned())
        .collect();
    let mut template = None;
    let mut buffers = Vec::new();
    for index in 0..nodes.len() {
        let header = &nodes[index];
        if header.storage != crate::NewTagStorageMode::InheritTemplate
            || manager
                .get_entry(header.template)
                .is_none_or(|entry| entry.file_type != 32 || entry.file_subtype != 1)
        {
            continue;
        }
        let Some(&data_index) = header.reference.as_deref().and_then(|s| positions.get(s)) else {
            continue;
        };
        let data = &nodes[data_index];
        if data.storage != crate::NewTagStorageMode::InheritTemplate
            || shared.contains(&data.symbol)
            || !data.patches.is_empty()
            || manager
                .get_entry(data.template)
                .is_none_or(|entry| entry.file_type != 40 || entry.file_subtype != 1)
        {
            continue;
        }
        let Some(leading) = leading_bytes(&header.payload, data.payload.len())? else {
            continue;
        };
        let symbol = format!("{}-streamed", data.symbol);
        if positions.contains_key(&symbol) {
            return Err(invalid(format!(
                "Streamed texture symbol {symbol} is taken"
            )));
        }
        let template = match template {
            Some(tag) => tag,
            None => *template.insert(large_template(manager)?),
        };
        let header_symbol = header.symbol.clone();
        let mut buffer = linking::Node::new(
            symbol.clone(),
            template,
            nodes[data_index].payload.drain(..leading).collect(),
        );
        buffer.reference = Some(header_symbol);
        buffer.streamed = true;
        buffers.push(buffer);
        let header = &mut nodes[index];
        header.payload[24] = 2;
        header.payload[28] = 1;
        header.patch(36, symbol)?;
    }
    nodes.extend(buffers);
    Ok(())
}
