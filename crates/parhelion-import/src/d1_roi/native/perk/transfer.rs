//! D1 reserve transfers retain the owning and three inventory-slot contributions.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph, NodeKind};

pub(super) fn read(p: &Payload, at: usize) -> Result<Action> {
    ensure!(
        p.u8(at + 1)? == 0
            && p.u8(at + 2)? <= 1
            && p.u8(at + 3)? <= 1
            && p.u8(at + 4)? == 0
            && p.u8(at + 5)? <= 1
            && p.bytes::<2>(at + 6)? == [0; 2],
        "Source reserve transfer has an unsupported lifecycle, selector or action-value scale"
    );
    let mut amounts = vec![0u32];
    for offset in [8, 12, 16, 20] {
        let value = p.f32(at + offset)?;
        ensure!(
            value.is_finite() && value >= 0.0,
            "Source reserve transfer requires a finite nonnegative amount"
        );
        amounts.push(value.to_bits());
    }
    let mut bytes = vec![0; 0x188];
    bytes[0] = 16;
    put(&mut bytes, 0x50, &0x80C70CA1u32.to_le_bytes())?;
    bytes[0x58] = 255;
    bytes[0x68] = p.u8(at + 2)?;
    bytes[0x69] = p.u8(at + 3)?;
    bytes[0x6A] = p.u8(at + 5)?;
    bytes[0x6B] = 255;
    let mut graph = Graph {
        blocks: vec![
            Block {
                class: 0x808029EC,
                count: None,
                bytes,
                links: BTreeMap::from([(0x48, 1)]),
            },
            Block {
                class: 0,
                count: None,
                bytes: b"content/common/native/sandbox/label_globals.label_globals.tft\0".to_vec(),
                links: BTreeMap::new(),
            },
        ],
    };
    // Source 71FF60 and native 1081EAE sum owning and inventory-slot amounts,
    // then cap the transfer by available reserves. Native's independent equipped
    // lane stays zero. Slots are Kinetic, Energy and Power, not ammunition types.
    // Shadowkeep rounds through its weapon ammunition unit. Unit-one original
    // execution agrees with D1, while other unit sizes retain native semantics.
    for (offset, bits) in [0x78, 0xB0, 0xE8, 0x120, 0x158].into_iter().zip(amounts) {
        let code = graph.blocks.len();
        graph.blocks.push(Block {
            class: 0x80800009,
            count: Some(4),
            bytes: vec![0x34, 0, 0x3E, 0],
            links: BTreeMap::new(),
        });
        graph.blocks.push(Block {
            class: 0x80800090,
            count: Some(1),
            bytes: bits.to_le_bytes().repeat(4),
            links: BTreeMap::new(),
        });
        let root = &mut graph.blocks[0];
        put(&mut root.bytes, offset, &4u64.to_le_bytes())?;
        put(&mut root.bytes, offset + 16, &1u64.to_le_bytes())?;
        put(&mut root.bytes, offset + 32, &1u64.to_le_bytes())?;
        put(&mut root.bytes, offset + 40, &1u64.to_le_bytes())?;
        root.links.insert(offset + 8, code);
        root.links.insert(offset + 24, code + 1);
    }
    graph
        .validate_node(NodeKind::Effect(16))
        .map_err(anyhow::Error::msg)?;
    Ok(Action::Native {
        node: NativeNode {
            kind: 16,
            bytes: graph.emit().map_err(anyhow::Error::msg)?,
        },
    })
}
