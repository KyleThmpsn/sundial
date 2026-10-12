//! Remove carrier visuals from private secondary graphs without dropping their logic.
use super::assembly::Assembly;
use crate::{
    presentation::{Graph, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn delay_template(native: &mut Reader) -> Result<[u8; 64]> {
    let source = native.tag(0x80b825c8, Some(0x80809c36))?;
    let definition = source.pointer(24)?;
    ensure!(
        source.u32(definition - 4)? == 0x808084e9,
        "Delay envelope root differs"
    );
    for row in source.array(definition + 0x168, 24, Some(0x808093e6))? {
        let node = source.pointer(row + 16)?;
        if source.u32(node - 4)? != 0x808093cb {
            continue;
        }
        let instance = usize::try_from(source.u64(node + 8)?)?;
        ensure!(
            source.u32(instance - 4)? == 0x808093ca && source.u64(instance + 8)? == node as u64,
            "Delay envelope pair differs"
        );
        return source.bytes::<64>(instance);
    }
    anyhow::bail!("Native delay envelope is absent")
}

fn clear_tail(payload: &mut Payload, at: usize, retained: usize, previous: usize) -> Result<()> {
    let start = at
        .checked_add(retained)
        .context("Vehicle event span overflow")?;
    let end = at
        .checked_add(previous)
        .context("Vehicle event span overflow")?;
    payload
        .0
        .get_mut(start..end)
        .context("Vehicle event extends outside its owner")?
        .fill(0);
    Ok(())
}

fn remove_visuals(
    payload: &mut Payload,
    allocation: &mut Payload,
    delay: &[u8; 64],
    retained: &BTreeSet<usize>,
) -> Result<Vec<Value>> {
    let root = payload.pointer(16)?;
    let definition = usize::try_from(payload.u64(root + 8)?)?;
    if payload.u32(definition - 4)? != 0x808084e9 {
        return Ok(Vec::new());
    }
    let instances = payload.array(root + 0xb0, 48, Some(0x808093e5))?;
    let nodes = payload.array(definition + 0x168, 24, Some(0x808093e6))?;
    ensure!(
        instances.len() == nodes.len(),
        "Vehicle effect node counts differ"
    );
    let allocations = allocation
        .array(0x20, 40, Some(0x80808852))?
        .into_iter()
        .filter(|&row| allocation.u32(row).ok() == Some(0xe3cc5c98))
        .collect::<Vec<_>>();
    ensure!(
        allocations.len() == 1,
        "Vehicle event allocation is absent or ambiguous"
    );
    let events = allocations[0];
    ensure!(
        allocation.u32(events + 16)? == 0x808093e5
            && allocation.u32(events + 20)? as usize == nodes.len(),
        "Vehicle event allocation differs"
    );
    let children = allocation.array(events + 24, 40, Some(0x80808852))?;
    ensure!(
        children.len() == nodes.len(),
        "Vehicle event allocation positions differ"
    );
    let mut removed = Vec::new();
    for (index, &row) in nodes.iter().enumerate() {
        let node = payload.pointer(row + 16)?;
        if retained.contains(&node) {
            continue;
        }
        let class = payload.u32(node - 4)?;
        // Fixed footprints from the registered Shadowkeep event declarations.
        let (instance_class, instance_size, definition_size) = match class {
            0x80806f49 => (0x80806f48, 0xb0, 0x150),
            0x80806cc6 => (0x80806cc5, 0x50, 0x128),
            _ => continue,
        };
        let instance = usize::try_from(payload.u64(node + 8)?)?;
        ensure!(
            payload.u32(instance - 4)? == instance_class
                && payload.u32(node + 4)? == instance_class
                && payload.u32(instance + 4)? == class
                && payload.u64(instance + 8)? == node as u64
                && payload.pointer(instance + 16)? == instances[index]
                && payload.pointer(instances[index] + 32)? == instance,
            "Vehicle visual event pair differs"
        );
        for initializer in payload.array(48, 16, Some(0x808091a4))? {
            ensure!(
                payload.u32(initializer + 8)? != instance_class,
                "Vehicle visual event has its own initializer"
            );
        }
        let position = children[index];
        let leaves = allocation.array(position + 24, 40, Some(0x80808852))?;
        ensure!(
            allocation.u32(position)? == 0x811c9dc5
                && allocation.u64(position + 8)? == 0
                && allocation.u32(position + 16)? == u32::MAX
                && allocation.u32(position + 20)? == 0
                && leaves.len() == 1,
            "Vehicle event allocation has an unsupported extension"
        );
        let leaf = leaves[0];
        ensure!(
            allocation.u32(leaf)? == 0x3988f7d4
                && allocation.u64(leaf + 8)? == 0
                && allocation.u32(leaf + 16)? == instance_class
                && allocation.u32(leaf + 20)? == 1
                && allocation.u64(leaf + 24)? == 0,
            "Vehicle visual allocation differs"
        );
        // Retain the event row, reciprocal offsets and common timing. The loader
        // walks the complete serialized stream, so a smaller class must leave
        // zero padding through the old footprint on both sides of the pair.
        clear_tail(payload, instance, delay.len(), instance_size)?;
        clear_tail(payload, node, 0x38, definition_size)?;
        put(&mut payload.0, instance - 4, &0x808093cau32.to_le_bytes())?;
        put(&mut payload.0, instance + 4, &0x808093cbu32.to_le_bytes())?;
        put(&mut payload.0, instance + 24, &delay[24..])?;
        put(&mut payload.0, node - 4, &0x808093cbu32.to_le_bytes())?;
        put(&mut payload.0, node + 4, &0x808093cau32.to_le_bytes())?;
        put(&mut payload.0, node + 0x2c, &[0; 12])?;
        put(&mut allocation.0, leaf + 16, &0x808093cau32.to_le_bytes())?;
        removed.push(json!({"index":index,"instance":instance,"definition":node,"previous_class":format!("{class:08X}")}));
    }
    Ok(removed)
}

fn child(
    native: &mut Reader,
    graph: &mut Graph,
    tag: u32,
    delay: &[u8; 64],
) -> Result<Option<(String, Value)>> {
    let mut bundle = super::secondary::Bundle::read(native, tag)?;
    let mut removed = Vec::new();
    for (&owner, payload) in &mut bundle.owners {
        let (_, allocation) = bundle.allocations.get_mut(&owner).unwrap();
        let events = remove_visuals(payload, allocation, delay, &BTreeSet::new())?;
        if !events.is_empty() {
            removed.push(json!({"owner":format!("{owner:08X}"),"events":events}));
        }
    }
    if removed.is_empty() {
        return Ok(None);
    }
    let symbol = bundle.emit(graph, "vehicle-secondary")?;
    Ok(Some((
        symbol,
        json!({"source_entity":format!("{tag:08X}"),"flows":removed}),
    )))
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    retained: &BTreeMap<u32, BTreeSet<usize>>,
) -> Result<Value> {
    let delay = delay_template(native)?;
    let mut root_flows = Vec::new();
    let owners = assembly.payloads.keys().copied().collect::<Vec<_>>();
    for owner in owners {
        let payload = &assembly.payloads[&owner];
        let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
        if payload.u32(definition - 4)? != 0x808084e9 {
            continue;
        }
        let (tag, mut allocation) = assembly.allocation(native, graph, owner)?;
        let removed = remove_visuals(
            assembly.payloads.get_mut(&owner).unwrap(),
            &mut allocation,
            &delay,
            retained.get(&owner).unwrap_or(&BTreeSet::new()),
        )?;
        if !removed.is_empty() {
            assembly.set_allocation(graph, owner, tag, &allocation)?;
            root_flows.push(json!({"owner":format!("{owner:08X}"),"events":removed}));
        }
    }
    let owner = assembly.owner(0x80803acb)?;
    let payload = &assembly.payloads[&owner];
    let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
    ensure!(
        payload.u32(definition - 4)? == 0x80803ac9,
        "Vehicle content definition differs"
    );
    let mut properties = vec![definition + 0x80];
    properties.extend(payload.array(definition + 0x240, 0x1c0, Some(0x80803acf))?);
    let mut targets = BTreeMap::<u32, Vec<usize>>::new();
    for property in properties {
        let tag = payload.u32(property + 0x70)?;
        if ![0, u32::MAX].contains(&tag) {
            targets.entry(tag).or_default().push(property + 0x70);
        }
    }
    let mut report = Vec::new();
    for (tag, offsets) in targets {
        if let Some((symbol, evidence)) = child(native, graph, tag, &delay)? {
            for offset in offsets {
                assembly.resource(owner, offset, symbol.clone())?;
            }
            report.push(evidence);
        }
    }
    Ok(json!({"root_flows":root_flows,"secondary_entities":report}))
}
