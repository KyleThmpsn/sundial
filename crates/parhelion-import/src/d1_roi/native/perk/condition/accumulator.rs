//! Literal event counters retain every contribution and rebuild native child routing.
use super::*;
use sundial::package_authoring::sandbox_perk::nodes;

pub(super) fn read(
    source: &Source,
    p: &Payload,
    at: usize,
    mut native: Option<&mut Reader>,
) -> Result<NativeNode> {
    let cap = p.f32(at + 0x28)?;
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?)
            && p.u8(at + 4)? == 0xFF
            && p.u8(at + 6)? == 0
            && p.u32(at + 0x2C)? <= 1
            && cap > 0.0
            && cap <= 16_777_216.0
            && p.f32(at + 0x20)?.abs() <= 16_777_216.0,
        "Source accumulator has unsupported state or count bounds"
    );
    let _ = p.f32(at + 0x24)?;
    // A threshold above the cap intentionally never completes. Its children still update
    // the action value used by retained effects, including reset on trigger release.
    let rows = p.array(at + 8, 32, Some(0x8080316C))?;
    ensure!(
        (1..=16).contains(&rows.len()),
        "Source accumulator requires one to sixteen literal contributions"
    );
    let mut bytes = vec![0; 56];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    put(&mut bytes, 8, &(rows.len() as u64).to_le_bytes())?;
    bytes[0x20..0x28].copy_from_slice(&p.bytes::<8>(at + 0x20)?);
    // D1 only applies the upper cap. A zero native lower bound would alter
    // negative action values. Keep the full finite range below the source cap.
    put(&mut bytes, 0x28, &f32::MIN.to_le_bytes())?;
    put(&mut bytes, 0x2C, &cap.to_le_bytes())?;
    // The admission/reset consumer can seed this counter from the owning weapon's
    // magazine capacity. Both builds use the same bounded draw and negative initial
    // value. Native's extra lower bound moves that Boolean from +2C to +30.
    bytes[0x30..0x34].copy_from_slice(&p.bytes::<4>(at + 0x2C)?);
    let mut graph = Graph {
        blocks: vec![
            Block {
                class: 0x80803E30,
                count: None,
                bytes,
                links: BTreeMap::from([(16, 1)]),
            },
            Block {
                class: 0x80803E32,
                count: Some(rows.len()),
                bytes: vec![0; rows.len() * 32],
                links: BTreeMap::new(),
            },
        ],
    };
    let mut source_mask = 0u64;
    let mut native_mask = 0u64;
    let mut held = 0;
    for (index, row) in rows.into_iter().enumerate() {
        let seconds = p.f32(row + 16)?;
        ensure!(
            seconds >= 0.0 && p.u32(row + 28)? == 0,
            "Source accumulator has a negative hold or reserved fields"
        );
        let hold = if seconds > 0.0 {
            // Source 6F5010 uses 474BF0's nearest, ties-to-even clock conversion,
            // then enforces one tick. Native EC3250 stores seconds with a small
            // positive minimum. Preserve the source interval in its 60 Hz domain.
            let ticks = (seconds * 60.0).round_ties_even().max(1.0);
            ensure!(
                (1.0..=32767.0).contains(&ticks),
                "Source counter hold exceeds the signed 60 Hz timer domain"
            );
            held += 1;
            // D1 visits every positive row during allocation. Shadowkeep stops
            // after the first. One held row preserves the timer's root identity.
            ensure!(
                held == 1,
                "Source counter has multiple independently allocated holds"
            );
            ticks / 60.0
        } else {
            seconds
        };
        for lane in [8, 20] {
            let value = p.f32(row + lane + 4)?;
            ensure!(
                p.u8(row + lane)? <= 1
                    && p.bytes::<3>(row + lane + 1)? == [0; 3]
                    && value.abs() <= 16_777_216.0,
                "Source accumulator needs bounded literal Add or Replace values"
            );
        }
        let child = p.pointer(row)?;
        let kind = p.u8(child + 5)?;
        ensure!(
            matches!(kind, 2..=5 | 7..=12 | 14..=19 | 22 | 23 | 27 | 33) && p.u8(child + 6)? == 0,
            "Source accumulator child requires a qualified leaf event"
        );
        let node = super::read(source, p, child, native.as_deref_mut())?
            .context("Unsupported source counter child")?;
        source_mask |= 1u64 << kind;
        native_mask |= 1u64 << node.kind;
        let class = nodes::condition(node.kind)
            .context("Native counter child class")?
            .class;
        let child = Graph::read(&node.bytes, 0, class).map_err(anyhow::Error::msg)?;
        let target = graph.append(&child).map_err(anyhow::Error::msg)?;
        graph.blocks[1].bytes[index * 32 + 8..index * 32 + 32]
            .copy_from_slice(&p.bytes::<24>(row + 8)?);
        put(
            &mut graph.blocks[1].bytes,
            index * 32 + 16,
            &hold.to_le_bytes(),
        )?;
        graph.blocks[1].links.insert(index * 32, target);
    }
    ensure!(
        p.u64(at + 0x18)? == source_mask,
        "Source counter routing disagrees with its children"
    );
    put(&mut graph.blocks[0].bytes, 0x18, &native_mask.to_le_bytes())?;
    graph
        .validate_node(NodeKind::Condition(26))
        .map_err(anyhow::Error::msg)?;
    Ok(NativeNode {
        kind: 26,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}
