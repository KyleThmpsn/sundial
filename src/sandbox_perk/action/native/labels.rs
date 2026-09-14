//! Source label sets and the masks evaluated by the client.
use super::*;
use crate::package_payload::u32_at;

pub const SOURCE_CLASS: u32 = 0x808093F3;
pub const PREDICATE_CLASS: u32 = 0x808094A8;

pub fn effective(graph: &Graph, block: usize, offset: usize) -> Result<[[u8; 40]; 4], String> {
    let owner = graph.blocks.get(block).ok_or("Missing predicate owner.")?;
    let mode = *owner
        .bytes
        .get(offset)
        .ok_or("Truncated label predicate.")?;
    let mut masks = [[0; 40]; 4];
    if mode == 255 {
        return Ok(masks);
    }
    let target = owner
        .links
        .get(&(offset + 8))
        .ok_or("A label predicate has no compiled masks.")?;
    let child = &graph.blocks[*target];
    let operations: &[usize] = match (mode, child.class) {
        (0, 0x808093F6) => &[0, 2],
        (1, 0x808093F5) => &[0, 1, 2, 3],
        _ => return Err("Invalid compiled label predicate mode or class.".into()),
    };
    for (index, &operation) in operations.iter().enumerate() {
        match child.bytes[operations.len() * 40 + index] {
            0 => masks[operation].copy_from_slice(&child.bytes[index * 40..index * 40 + 40]),
            1 => {}
            _ => return Err("Invalid compiled label empty flag.".into()),
        }
    }
    Ok(masks)
}

/// Source lists and their corresponding runtime predicates within one allocation row.
pub fn bindings(class: u32) -> Result<Vec<(usize, usize)>, String> {
    let inline = schema::inline(class)?;
    let mut sources = inline
        .iter()
        .filter(|(_, c, _)| *c == SOURCE_CLASS)
        .map(|(o, _, _)| *o)
        .collect::<Vec<_>>();
    let mut predicates = inline
        .iter()
        .filter(|(_, c, _)| *c == PREDICATE_CLASS)
        .map(|(o, _, _)| *o)
        .collect::<Vec<_>>();
    sources.sort_unstable();
    sources.dedup();
    predicates.sort_unstable();
    predicates.dedup();
    if sources.len() != predicates.len() {
        // Individual inline views do not own their source or compiled counterpart.
        return Ok(Vec::new());
    }
    Ok(sources.into_iter().zip(predicates).collect())
}

pub fn source(graph: &Graph, block: usize, offset: usize) -> Result<[Vec<u32>; 4], String> {
    let owner = graph
        .blocks
        .get(block)
        .ok_or("Missing label-filter owner.")?;
    let mut result: [Vec<u32>; 4] = std::array::from_fn(|_| Vec::new());
    for (operation, labels) in result.iter_mut().enumerate() {
        let at = offset + operation * 16;
        let count = crate::package_payload::u64_at(&owner.bytes, at)?;
        if let Some(target) = owner.links.get(&(at + 8)) {
            let entries = graph.blocks.get(*target).ok_or("Missing label entries.")?;
            if entries.class != 0x808094B3 || entries.count.map(|n| n as u64) != Some(count) {
                return Err("Invalid source label array.".into());
            }
            for row in entries.bytes.chunks_exact(24) {
                labels.push(u32_at(row, 0)?);
            }
        } else if count != 0 {
            return Err("A label array has no allocation.".into());
        }
    }
    Ok(result)
}

/// Rebuild masks from the authored source lists, including group expansion.
pub fn compile(graph: &mut Graph, registry: &[u8]) -> Result<(), String> {
    let mut changed = graph.clone();
    let mut jobs = Vec::new();
    for (index, block) in graph
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.class != 0)
    {
        let stride = schema::record(block.class)?.size;
        for row in 0..block.count.unwrap_or(1) {
            for (source, predicate) in bindings(block.class)? {
                jobs.push((index, row * stride + source, row * stride + predicate));
            }
        }
    }
    for (block, at, predicate) in jobs {
        let lists = source(graph, block, at)?;
        let masks = lists
            .iter()
            .map(|labels| crate::sandbox_perk::program::compiler::compile_labels(registry, labels))
            .collect::<Result<Vec<_>, _>>()?;
        let expected: [[u8; 40]; 4] = std::array::from_fn(|index| masks[index]);
        if effective(graph, block, predicate).is_ok_and(|stored| stored == expected) {
            continue;
        }
        let four = !lists[1].is_empty() || !lists[3].is_empty();
        let mut bytes = vec![0; if four { 164 } else { 84 }];
        let operations: &[usize] = if four { &[0, 1, 2, 3] } else { &[0, 2] };
        for (slot, &operation) in operations.iter().enumerate() {
            bytes[slot * 40..slot * 40 + 40].copy_from_slice(&masks[operation]);
            bytes[operations.len() * 40 + slot] = u8::from(lists[operation].is_empty());
        }
        let target = changed.blocks.len();
        changed.blocks.push(Block {
            class: if four { 0x808093F5 } else { 0x808093F6 },
            count: None,
            bytes,
            links: BTreeMap::new(),
        });
        changed.blocks[block].bytes[predicate] = u8::from(four);
        changed.blocks[block].links.insert(predicate + 8, target);
    }
    // Added event labels have a compiled set instead of a predicate wrapper.
    for (index, block) in graph.blocks.iter().enumerate() {
        let (source, mask) = match block.class {
            0x80803E1A => (0x98, 0xA8),
            0x8080281C => (0x68, 0x78),
            _ => continue,
        };
        let labels = if let Some(target) = block.links.get(&(source + 8)) {
            let rows = &graph.blocks[*target];
            if rows.class != 0x808094B3 {
                return Err("Invalid added-label array.".into());
            }
            rows.bytes
                .chunks_exact(24)
                .map(|row| u32_at(row, 0))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        let compiled = crate::sandbox_perk::program::compiler::compile_labels(registry, &labels)?;
        changed.blocks[index].bytes[mask..mask + 40].copy_from_slice(&compiled);
    }
    changed.validate()?;
    *graph = changed;
    Ok(())
}
