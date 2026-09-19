use super::*;
use std::collections::BTreeSet;

pub(super) fn validate(graph: &Graph) -> Result<(), String> {
    let root = graph
        .blocks
        .first()
        .ok_or("The native graph has no root.")?;
    if root.class == 0 || root.count.is_some() || graph.blocks.len() > MAX_BLOCKS {
        return Err("The native graph has an invalid root or allocation count.".into());
    }
    let mut total = 0usize;
    for block in &graph.blocks {
        total = total
            .checked_add(block.bytes.len())
            .ok_or("Native graph size overflow.")?;
        if total > MAX_BYTES {
            return Err("The native graph is too large.".into());
        }
        validate_block(graph, block)?;
    }
    Ok(())
}

fn validate_block(graph: &Graph, block: &Block) -> Result<(), String> {
    if block.class == 0 {
        if block.count.is_some()
            || !block.links.is_empty()
            || block.bytes.last() != Some(&0)
            || block.bytes[..block.bytes.len().saturating_sub(1)].contains(&0)
            || std::str::from_utf8(&block.bytes).is_err()
        {
            return Err("Invalid native string allocation.".into());
        }
        return Ok(());
    }
    let record = schema::record(block.class)?;
    let count = block.count.unwrap_or(1);
    if count > MAX_ROWS || record.size.checked_mul(count) != Some(block.bytes.len()) {
        return Err(format!(
            "Native class 0x{:08X} has an invalid allocation size.",
            block.class
        ));
    }
    let fields = (0..count)
        .flat_map(|row| {
            record
                .fields
                .iter()
                .filter(|(_, code)| matches!(code, 1..=3))
                .map(move |(at, code)| (row * record.size + at, *code))
        })
        .collect::<BTreeMap<_, _>>();
    for row in 0..count {
        for (at, class, _) in schema::inline(block.class)? {
            if !schema::record(class)?.array {
                continue;
            }
            let descriptor = row * record.size + at;
            let length = crate::package_payload::u64_at(&block.bytes, descriptor)?;
            if let Some(target) = block.links.get(&(descriptor + 8)) {
                if graph
                    .blocks
                    .get(*target)
                    .and_then(|b| b.count)
                    .map(|n| n as u64)
                    != Some(length)
                {
                    return Err("A native array has the wrong allocation type or length.".into());
                }
            } else if length != 0 {
                return Err("A native array count has no allocation.".into());
            }
        }
    }
    for (&field, &code) in &fields {
        if block
            .bytes
            .get(field..field + 8)
            .is_none_or(|bytes| bytes.iter().any(|b| *b != 0))
        {
            return Err("Native pointer bytes must be owned by a relocation.".into());
        }
        if let Some(&target) = block.links.get(&field) {
            let child = graph
                .blocks
                .get(target)
                .filter(|_| target != 0)
                .ok_or("Invalid native pointer target.")?;
            if (code == 3) == (child.class == 0) {
                return Err("A native pointer targets the wrong allocation type.".into());
            }
            if code == 3 {
                let choices = schema::choices(block.class, field % record.size);
                if !choices.is_empty() && !choices.contains(&(child.class, child.count.is_some())) {
                    return Err(format!(
                        "Native reference +0x{field:X} has an incompatible target class."
                    ));
                }
            }
            if let Some(count) = child.count {
                let start = field
                    .checked_sub(8)
                    .ok_or("Invalid native array descriptor.")?;
                let stored = crate::package_payload::u64_at(&block.bytes, start)?;
                if stored != count as u64 {
                    return Err("Native array descriptor and allocation lengths disagree.".into());
                }
            }
        }
    }
    if block.links.keys().any(|field| !fields.contains_key(field)) {
        return Err("A native relocation is not declared by its class.".into());
    }
    Ok(())
}

pub(super) fn emit(graph: &Graph) -> Result<Vec<u8>, String> {
    emit_with_offsets(graph).map(|(bytes, _)| bytes)
}

pub(super) fn emit_with_offsets(
    graph: &Graph,
) -> Result<(Vec<u8>, BTreeMap<usize, usize>), String> {
    validate(graph)?;
    let mut order = Vec::new();
    let mut pending = vec![0usize];
    let mut visited = BTreeSet::new();
    while let Some(index) = pending.pop() {
        if !visited.insert(index) {
            continue;
        }
        order.push(index);
        pending.extend(graph.blocks[index].links.values().rev().copied());
    }
    let mut bytes = Vec::new();
    let mut starts = BTreeMap::new();
    let mut pointers = BTreeMap::new();
    for &index in &order {
        let block = &graph.blocks[index];
        let (start, pointer) = allocate(&mut bytes, block, index == 0)?;
        starts.insert(index, start);
        pointers.insert(index, pointer);
    }
    for &index in &order {
        let start = starts[&index];
        for (&field, target) in &graph.blocks[index].links {
            let from = start + field;
            let relative = i64::try_from(pointers[target])
                .and_then(|to| i64::try_from(from).map(|from| to - from))
                .map_err(|_| "Native relative pointer overflow.")?;
            bytes[from..from + 8].copy_from_slice(&relative.to_le_bytes());
        }
    }
    if graph.root_class() == Some(crate::sandbox_perk::action::ACTION_ROOT_CLASS) {
        let size = bytes.len() as u64;
        bytes[..8].copy_from_slice(&size.to_le_bytes());
    }
    Ok((bytes, starts))
}

fn allocate(bytes: &mut Vec<u8>, block: &Block, root: bool) -> Result<(usize, usize), String> {
    let (start, pointer) = if root || block.class == 0 {
        (bytes.len(), bytes.len())
    } else {
        let header = if block.count.is_some() { 20 } else { 4 };
        let start = bytes
            .len()
            .checked_add(header + 15)
            .ok_or("Native allocation overflow.")?
            & !15;
        bytes.resize(start, 0);
        if let Some(count) = block.count {
            bytes[start - 20..start - 16].copy_from_slice(&0x80809FBD_u32.to_le_bytes());
            bytes[start - 16..start - 8].copy_from_slice(&(count as u64).to_le_bytes());
            bytes[start - 8..start - 4].copy_from_slice(&block.class.to_le_bytes());
            (start, start - 16)
        } else {
            bytes[start - 4..start].copy_from_slice(&block.class.to_le_bytes());
            (start, start)
        }
    };
    if bytes.len().saturating_add(block.bytes.len()) > MAX_BYTES {
        return Err("The emitted native graph is too large.".into());
    }
    bytes.extend_from_slice(&block.bytes);
    Ok((start, pointer))
}
