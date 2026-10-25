//! Damage and ability declarations share one checked layout in hit and kill conditions.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph};

pub(super) fn damage(
    source: &Source,
    p: &Payload,
    at: usize,
    dictionary: usize,
    graph: &mut Graph,
    to: usize,
    native: Option<&mut Reader>,
) -> Result<()> {
    graph.blocks[0].bytes[to + 0x78] = 0xFF;
    for operation in 0..2 {
        let rows = p.array(at + operation * 16, 1, Some(0x80804F9B))?;
        let values = rows.iter().map(|&r| p.u8(r)).collect::<Result<Vec<_>>>()?;
        ensure!(
            values.iter().all(|&v| v < 8),
            "Source damage filter exceeds its enum"
        );
        let mask = values.iter().fold(0u32, |mask, &v| mask | (1 << v));
        ensure!(
            p.u32(at + 0x40 + operation * 4)? == mask
                && p.u8(at + 0x88 + operation)? == u8::from(rows.is_empty()),
            "Source damage declarations and compiled filter disagree"
        );
        put(
            &mut graph.blocks[0].bytes,
            to + 0x70 + operation * 4,
            &mask.to_le_bytes(),
        )?;
        graph.blocks[0].bytes[to + 0x88 + operation] = u8::from(rows.is_empty());
        if !values.is_empty() {
            let index = graph.blocks.len();
            graph.blocks.push(Block {
                class: 0x80806B02,
                count: Some(values.len()),
                bytes: values,
                links: BTreeMap::new(),
            });
            put(
                &mut graph.blocks[0].bytes,
                to + operation * 16,
                &(rows.len() as u64).to_le_bytes(),
            )?;
            graph.blocks[0].links.insert(to + 8 + operation * 16, index);
        }
    }
    label::ability(source, p, at, dictionary, graph, to + 0x20, native)
}
