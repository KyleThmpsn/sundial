//! Timer extensions retain their own event predicates, independently of activation.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph, NodeKind};

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
    let nodes = conditions(source, p, at + 16, native)?;
    ensure!(
        !nodes.is_empty() && nodes.iter().all(|n| n.kind == 2),
        "Source timer extension requires checked kill predicates"
    );
    ensure!(
        p.u64(at + 32)? == 1 << 2,
        "Source timer event mask disagrees"
    );
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
    put(&mut bytes, 32, &(1u64 << 2).to_le_bytes())?;
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
        let child = Graph::read(&node.bytes, 0, 0x80803DE7).map_err(anyhow::Error::msg)?;
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
