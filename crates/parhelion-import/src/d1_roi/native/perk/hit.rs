//! Preserve the source hit's damage, ability labels and target relationship.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph, NodeKind};

pub(super) fn read(source: &Source, p: &Payload, at: usize) -> Result<NativeNode> {
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?)
            && p.u8(at + 4)? == 0xFF
            && p.u8(at + 6)? == 0
            && p.u8(at + 0x94)? & !7 == 0
            && p.u8(at + 0x95)? <= 1
            && p.bytes::<2>(at + 0x96)? == [0; 2]
            && p.f32(at + 0x98)?.is_finite()
            && p.u8(at + 0x9C)? <= 1
            && p.bytes::<3>(at + 0x9D)? == [0; 3]
            && p.bytes::<11>(at + 0x175)? == [0; 11]
            && p.bytes::<15>(at + 0x181)? == [0; 15],
        "Source hit requires state, timeout, slot or extended ownership translation"
    );
    let mut bytes = vec![0; 0xD0];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    // Original hit callbacks use the same current-ability identity helper as kill
    // events. The first three bits select grenade, super and melee respectively.
    bytes[0x98] = p.u8(at + 0x94)?;
    bytes[0x99] = p.u8(at + 0x95)?;
    put(&mut bytes, 0x9C, &0x811C9DC5u32.to_le_bytes())?;
    put(&mut bytes, 0xA0, &p.bytes::<4>(at + 0x98)?)?;
    bytes[0xA4] = p.u8(at + 0x9C)?;
    bytes[0xC1] = p.u8(at + 0x180)?;
    let mut graph = Graph {
        blocks: vec![Block {
            class: 0x80802F5F,
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    filter::damage(source, p, at + 8, at + 0xE0, &mut graph, 8)?;
    let mut labels = Graph {
        blocks: vec![Block {
            class: 0x80804D73,
            count: None,
            bytes: vec![0; 0x60],
            links: BTreeMap::new(),
        }],
    };
    labels.blocks[0].bytes[0x50] = 0xFF;
    label::translate(source, p, at + 0xA0, &mut labels, 0)?;
    let label_index = graph.blocks.len();
    for mut block in labels.blocks {
        for target in block.links.values_mut() {
            *target += label_index;
        }
        graph.blocks.push(block);
    }
    // The original native registry binds 80804D76 to the relationship-mask
    // callback. Mode zero requires both the relationship and object labels.
    let relation = graph.blocks.len();
    graph.blocks.push(Block {
        class: 0x80804D76,
        count: None,
        bytes: vec![p.u8(at + 0x174)?],
        links: BTreeMap::new(),
    });
    let predicates = graph.blocks.len();
    graph.blocks.push(Block {
        class: 0x80809316,
        count: Some(2),
        bytes: vec![0; 32],
        links: BTreeMap::from([(8, relation), (24, label_index)]),
    });
    put(&mut graph.blocks[0].bytes, 0xB0, &2u64.to_le_bytes())?;
    graph.blocks[0].links.insert(0xB8, predicates);
    graph
        .validate_node(NodeKind::Condition(4))
        .map_err(anyhow::Error::msg)?;
    Ok(NativeNode {
        kind: 4,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}
