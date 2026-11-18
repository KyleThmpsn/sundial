//! Event label and numeric effects that run from an event callback alone.
//!
//! Rise of Iron installs kinds 32, 33 and 34 with only an event callback, as Shadowkeep does
//! kinds 37, 38 and 40. Kind 34 is the damage modifier in [`super::damage`]. Kind 32 adds labels
//! to an event that passes a damage filter, and kind 33 scales an event that passes a label
//! filter. Radiance's own action uses both while the Super is active: an event labeled `scorch`
//! gains a second label and has its base damage lane multiplied.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph, NodeKind};

/// Source kind 32, class `80802BAB`, to native kind 37, class `80803E1A`. Both are 208 bytes
/// with the damage filter at `+08`, the added label list at `+98` and its compiled set at
/// `+A8`. The native set is 40 bytes where the source's is 32, and package assembly compiles it
/// against Shadowkeep's registry.
pub(super) fn labels(
    source: &Source,
    p: &Payload,
    at: usize,
    mut native: Option<&mut Reader>,
) -> Result<Action> {
    ensure!(
        p.bytes::<7>(at + 1)? == [0; 7] && p.u64(at + 0xC8)? == 0,
        "Source event label effect has retained state or trailing fields"
    );
    let mut graph = Graph {
        blocks: vec![Block {
            class: 0x80803E1A,
            count: None,
            bytes: vec![0; 0xD0],
            links: BTreeMap::new(),
        }],
    };
    graph.blocks[0].bytes[0] = 37;
    let filter = at + 8;
    if p.bytes::<4>(filter + 0x88)? == [1; 4] {
        ensure!(
            p.bytes::<0x88>(filter)? == [0; 0x88],
            "Source event label effect has an unqualified empty damage filter"
        );
        graph.blocks[0].bytes[0x80] = 0xFF;
        graph.blocks[0].bytes[0x90..0x92].fill(1);
    } else {
        // The filter names its dictionary through its label rows. A filter with damage types
        // alone has none to check and keeps the type lists.
        let row = [filter + 0x20, filter + 0x30]
            .into_iter()
            .map(|list| p.array(list, 24, Some(0x80800365)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .next();
        match row {
            Some(row) => filter::damage(
                source,
                p,
                filter,
                row + 8,
                &mut graph,
                8,
                native.as_deref_mut(),
            )?,
            None => filter::types(p, filter, &mut graph, 8)?,
        }
    }
    label::added(source, p, at + 0x98, at + 0xA8, &mut graph, 0x98, native)?;
    graph
        .validate_node(NodeKind::Effect(37))
        .map_err(anyhow::Error::msg)?;
    Ok(Action::Native {
        node: NativeNode {
            kind: 37,
            bytes: graph.emit().map_err(anyhow::Error::msg)?,
        },
    })
}

/// Source kind 33, class `808025D8`, to native kind 38, class `80803E20`. Source `71D420` and
/// native `10841A0` are the same routine: union the other object's labels into the event's,
/// test the label filter, assign and multiply event lanes in list order, then scale the overall
/// value by one plus the trailing float. The source holds the filter at `+08`, its rows at
/// `+E0` and `+F0`, the float at `+100` and its stat selector at `+104`. The native node holds
/// them at `+08`, `+68`, `+78`, `+88` and `+8C`, with the `80803E22` rows kind 40 uses. No stock
/// action uses native kind 38, so its load and dispatch are unobserved in game.
pub(super) fn scale(
    source: &Source,
    p: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<Action> {
    ensure!(
        p.bytes::<7>(at + 1)? == [0; 7],
        "Source event numeric effect has retained state"
    );
    ensure!(
        p.u8(at + 0x104)? == 0xFF && p.bytes::<3>(at + 0x105)? == [0; 3],
        "Source event numeric effect requires stat-input translation"
    );
    let overall = p.f32(at + 0x100)?;
    ensure!(
        overall.is_finite(),
        "Source event numeric effect has a nonfinite overall offset"
    );
    let mut bytes = vec![0; 0x90];
    bytes[0] = 38;
    bytes[0x58] = 0xFF;
    bytes[0x88..0x8C].copy_from_slice(&overall.to_le_bytes());
    bytes[0x8C] = 0xFF;
    let mut graph = Graph {
        blocks: vec![Block {
            class: 0x80803E20,
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    for (from, to) in [(0xE0, 0x68), (0xF0, 0x78)] {
        let rows = p.array(at + from, 12, Some(0x80802069))?;
        ensure!(
            rows.len() <= 32,
            "Source event row count exceeds the bounded route"
        );
        let mut bytes = Vec::new();
        for row in &rows {
            // Both routines index the event's Float32 lanes by the signed byte at row `+0`.
            // The event carries four lanes, and a row with another selector reads a stat.
            ensure!(
                p.u32(*row)? < 4
                    && p.f32(*row + 4)?.is_finite()
                    && p.bytes::<4>(*row + 8)? == [255, 0, 0, 0],
                "Source event row requires another lane or a stat provider"
            );
            bytes.extend_from_slice(&p.bytes::<12>(*row)?);
        }
        if !rows.is_empty() {
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
    label::translate(source, p, at + 8, &mut graph, 8, native)?;
    graph
        .validate_node(NodeKind::Effect(38))
        .map_err(anyhow::Error::msg)?;
    Ok(Action::Native {
        node: NativeNode {
            kind: 38,
            bytes: graph.emit().map_err(anyhow::Error::msg)?,
        },
    })
}
