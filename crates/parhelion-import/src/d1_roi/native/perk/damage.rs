//! Literal damage-event rows and offsets with qualified empty ability filters.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph, NodeKind};

pub(super) fn read(
    source: &Source,
    p: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<Action> {
    ensure!(
        p.u8(at + 1)? == 0
            && p.bytes::<0x8E>(at + 2)? == [0; 0x8E]
            && p.bytes::<4>(at + 0x90)? == [1; 4]
            && p.u8(at + 0x94)? <= 1
            && p.bytes::<3>(at + 0x95)? == [0; 3],
        "Source damage modifier has additional ability filters or lifecycle state"
    );
    ensure!(
        p.u32(at + 0x16C)? == 0 && p.u8(at + 0x194)? == 0xFF && p.bytes::<3>(at + 0x195)? == [0; 3],
        "Source damage modifier requires stat-input translation"
    );
    let _ = p.f32(at + 0x190)?;
    let mut bytes = vec![0; 0x150];
    bytes[0] = 40;
    bytes[0xA0] = 0xFF;
    bytes[0xB0..0xB2].fill(1);
    bytes[0xB8] = p.u8(at + 0x94)?;
    bytes[0x110] = 0xFF;
    let value = p.bytes::<4>(at + 0x190)?;
    let mut graph = Graph {
        blocks: vec![
            Block {
                class: 0x80802F16,
                count: None,
                bytes,
                links: BTreeMap::from([(0x140, 1)]),
            },
            Block {
                class: 0x80802F1A,
                count: None,
                // Native ordinary and alternate event contexts select separate constants.
                // Source 71D9C0 uses one literal in both. Both engines add one at runtime.
                bytes: value.repeat(2),
                links: BTreeMap::new(),
            },
        ],
    };
    for (from, to) in [(0x170, 0x120), (0x180, 0x130)] {
        let rows = p.array(at + from, 12, Some(0x80802069))?;
        ensure!(
            rows.len() <= 32,
            "Source damage row count exceeds the bounded route"
        );
        let mut bytes = Vec::new();
        for row in &rows {
            ensure!(
                p.u32(*row)? == 0
                    && p.f32(*row + 4)?.is_finite()
                    && p.bytes::<4>(*row + 8)? == [255, 0, 0, 0],
                "Source damage row requires another lane or a stat provider"
            );
            bytes.extend_from_slice(&p.bytes::<12>(*row)?);
        }
        if !rows.is_empty() {
            // Original 71DB72/71DD08 and native 108EF97/108F019 assign and multiply
            // the event's base-damage lane in list order. The separate +140 value
            // adds one before scaling the overall damage, and is not a row factor.
            let target = graph.blocks.len();
            put(
                &mut graph.blocks[0].bytes,
                to,
                &(rows.len() as u64).to_le_bytes(),
            )?;
            graph.blocks[0].links.insert(to + 8, target);
            graph.blocks.push(Block {
                class: 0x80803E22,
                count: Some(rows.len()),
                bytes,
                links: BTreeMap::new(),
            });
        }
    }
    label::translate(source, p, at + 0x98, &mut graph, 0xC0, native)?;
    graph
        .validate_node(NodeKind::Effect(40))
        .map_err(anyhow::Error::msg)?;
    Ok(Action::Native {
        node: NativeNode {
            kind: 40,
            bytes: graph.emit().map_err(anyhow::Error::msg)?,
        },
    })
}
