//! Timer extensions retain their own event predicates, independently of activation.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph, NodeKind};
use sundial::package_authoring::sandbox_perk::nodes;

pub(super) fn read(
    source: &Source,
    p: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<Action> {
    ensure!(
        p.bytes::<3>(at + 1)? == [0; 3]
            && p.u32(at + 12)? == 0
            && p.f32(at + 4)?.is_finite()
            && p.f32(at + 4)? >= 0.0
            && p.f32(at + 8)?.is_finite()
            && p.f32(at + 8)? >= 0.0,
        "Source timer extension has unsupported flags or bounds"
    );
    let children = p.array(at + 16, 8, None)?;
    ensure!(
        !children.is_empty(),
        "Source timer extension has no predicates"
    );
    let mut source_mask = 0u64;
    for child in children {
        let child = p.pointer(child)?;
        let kind = p.u8(child + 5)?;
        ensure!(
            matches!(kind, 2 | 12 | 19) && (kind == 2 || p.u8(child + 6)? == 0),
            "Source timer extension requires qualified kill, game-event or reload predicates"
        );
        source_mask |= 1u64 << kind;
    }
    ensure!(
        p.u64(at + 32)? == source_mask,
        "Source timer event mask disagrees"
    );
    // Source 6CE15F and native ECD440 test the nested event mask before their
    // ordinary condition evaluator. Neither dispatch is specific to kills.
    // Keep new predicates stateless and let their translators qualify producers,
    // identities and filters before constructing the native subscription.
    let nodes = conditions(source, p, at + 16, native)?;
    let native_mask = nodes
        .iter()
        .fold(0u64, |mask, node| mask | 1u64 << node.kind);
    let effects = p.array(0x38, 8, None)?;
    let mut extensions = Vec::new();
    for (index, &row) in effects.iter().enumerate() {
        let effect = p.pointer(row)?;
        if p.u8(effect)? == 27 {
            extensions.push((index, effect));
        }
    }
    ensure!(
        extensions.len() == 1 && extensions[0].1 == at && p.u32(0x98)? as usize == extensions[0].0,
        "Source timer extension routing differs"
    );
    // D1 6CB720 clamps even an already-over-cap timer down. Native EC2F60 skips
    // that timer. One extension and initial timers within its cap keep both routes
    // equivalent through repeated use. Do not widen this to arbitrary timers.
    for descriptor in [0x20, 0x48, 0x58] {
        for row in p.array(descriptor, 8, None)? {
            let condition = p.pointer(row)?;
            if p.u8(condition + 5)? == 1 {
                ensure!(
                    p.f32(condition + 8)? >= 0.0 && p.f32(condition + 8)? <= p.f32(at + 8)?,
                    "Source running timer exceeds the extension cap"
                );
            }
        }
    }
    let mut bytes = vec![0; 40];
    bytes[0] = 32;
    bytes[4..12].copy_from_slice(&p.bytes::<8>(at + 4)?);
    put(&mut bytes, 16, &(nodes.len() as u64).to_le_bytes())?;
    put(&mut bytes, 32, &native_mask.to_le_bytes())?;
    let mut graph = Graph {
        blocks: vec![
            Block {
                class: 0x80803E3B,
                count: None,
                bytes,
                links: BTreeMap::from([(24, 1)]),
            },
            Block {
                class: 0x808040BA,
                count: Some(nodes.len()),
                bytes: vec![0; nodes.len() * 8],
                links: BTreeMap::new(),
            },
        ],
    };
    for (i, mut node) in nodes.into_iter().enumerate() {
        node.bytes[7] = 255;
        let class = nodes::condition(node.kind)
            .context("Native timer predicate class")?
            .class;
        let child = Graph::read(&node.bytes, 0, class).map_err(anyhow::Error::msg)?;
        let target = graph.append(&child).map_err(anyhow::Error::msg)?;
        graph.blocks[1].links.insert(i * 8, target);
    }
    graph
        .validate_node(NodeKind::Effect(32))
        .map_err(anyhow::Error::msg)?;
    Ok(Action::Native {
        node: NativeNode {
            kind: 32,
            bytes: graph.emit().map_err(anyhow::Error::msg)?,
        },
    })
}
