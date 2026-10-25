//! Emits complete action programs without flattening groups or execution policies.
use super::*;
use crate::sandbox_perk::action::{
    self,
    native::{Graph, labels, schema},
};

pub(super) fn compile(
    manager: &PackageManager,
    program: &NativeProgram,
) -> Result<Compiled, String> {
    program.validate()?;
    // Drop detached editing allocations before validation and dependency collection.
    let mut graph = Graph::read(&program.graph.emit()?, 0, action::ACTION_ROOT_CLASS)?;
    let groups = graph.blocks[0]
        .links
        .get(&0x70)
        .and_then(|index| graph.blocks[*index].count)
        .unwrap_or(0);
    if groups != 0 || graph.blocks[0].links.contains_key(&0xB0) {
        if !graph.blocks[0].links.contains_key(&0xB0) {
            graph.create_target(0, 0xB0, 0x8080407B, true)?;
        }
        let index = graph.blocks[0].links[&0xB0];
        graph.resize_array(index, groups)?;
    }
    if graph
        .blocks
        .iter()
        .filter(|block| block.class != 0)
        .any(|block| {
            schema::inline(block.class).is_ok_and(|views| {
                views
                    .iter()
                    .any(|(_, class, _)| *class == labels::SOURCE_CLASS)
            })
        })
    {
        let registry = manager
            .read_tag(TagHash(LABEL_GLOBALS))
            .map_err(|error| error.to_string())?;
        labels::compile(&mut graph, &registry)?;
    }
    validate_native_resources(manager, &graph)?;
    let (mut payload, offsets) = graph.emit_with_offsets()?;
    let size = payload.len() as u64;
    payload[..8].copy_from_slice(&size.to_le_bytes());
    metadata::rebuild(&mut payload)?;
    let decoded = action::decode(&payload)?;
    let conditions = decoded.conditions();
    let slots = conditions.len()
        + conditions
            .iter()
            .map(|node| node.subgroups.len())
            .sum::<usize>()
        + decoded.effects().count();
    let states = u8::try_from(slots + 1)
        .map_err(|_| "The native program exceeds its state reservation limit.")?;
    payload[0xCC] = payload[0xCC].max(states);
    payload[0xCD] = payload[0xCD].max(states - 1);
    let mut asset_offsets = Vec::new();
    let mut scripts: Vec<ScriptCopy> = Vec::new();
    for (index, asset) in program.assets.iter().enumerate() {
        validate_asset(manager, &Action::attach(asset.clone()))?;
        if let Some(script) = asset.script {
            let lanes = crate::package_runtime::references::declared_fields(manager, script)
                .map_err(|error| format!("Behavior script 0x{script:08X}: {error}"))?
                .into_iter()
                .filter(|&(_, tag)| tag == asset.graph)
                .map(|(at, _)| at)
                .collect::<Vec<_>>();
            if lanes.is_empty() {
                return Err(format!(
                    "Behavior script 0x{script:08X} no longer names graph 0x{:08X}.",
                    asset.graph
                ));
            }
            match scripts.iter_mut().find(|copy| copy.tag == script) {
                Some(copy) => copy.assets.push((index, lanes)),
                None => scripts.push(ScriptCopy {
                    tag: script,
                    operands: script_operands(&graph, &offsets, &payload, script)?,
                    assets: vec![(index, lanes)],
                }),
            }
            continue;
        }
        let mut lanes = Vec::new();
        for (&block_index, &start) in &offsets {
            let block = &graph.blocks[block_index];
            if block.class == 0 {
                continue;
            }
            let record = schema::record(block.class)?;
            for row in 0..block.count.unwrap_or(1) {
                for &(field, code) in &record.fields {
                    let at = start + row * record.size + field;
                    if matches!(code, 4 | 9) && u32_at(&payload, at)? == asset.graph {
                        lanes.push(at);
                    }
                }
            }
        }
        if lanes.is_empty() {
            return Err("A component edit no longer references an asset in this program.".into());
        }
        asset_offsets.push((index, lanes));
    }
    Graph::read(&payload, 0, action::ACTION_ROOT_CLASS)?.validate_program()?;
    Ok(Compiled {
        payload,
        graph_offsets: Vec::new(),
        asset_offsets,
        scripts,
    })
}

/// The class of a Run a Game Script effect, which names its script at +0x10.
const SCRIPT_EFFECT: u32 = 0x8080_2D0A;
const SCRIPT_OPERAND: usize = 0x10;

/// Every lane where an emitted Run a Game Script effect names `script`.
fn script_operands(
    graph: &Graph,
    offsets: &std::collections::BTreeMap<usize, usize>,
    payload: &[u8],
    script: u32,
) -> Result<Vec<usize>, String> {
    let mut lanes = Vec::new();
    for (&block_index, &start) in offsets {
        let block = &graph.blocks[block_index];
        if block.class != SCRIPT_EFFECT {
            continue;
        }
        let at = start + SCRIPT_OPERAND;
        if u32_at(payload, at)? == script {
            lanes.push(at);
        }
    }
    if lanes.is_empty() {
        return Err(format!(
            "No Run a Game Script effect runs behavior script 0x{script:08X}."
        ));
    }
    Ok(lanes)
}
