//! Literal event counters retain every contribution and rebuild native child routing.
use super::*;
use sundial::package_authoring::sandbox_perk::nodes;

pub(super) fn read(source: &Source, p: &Payload, at: usize) -> Result<NativeNode> {
    let cap = p.f32(at + 0x28)?;
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?)
            && p.u8(at + 4)? == 0xFF
            && p.u8(at + 6)? == 0
            && p.u32(at + 0x2C)? == 0
            && cap > 0.0
            && cap <= 16_777_216.0
            && cap.fract() == 0.0
            && p.f32(at + 0x20)? >= 0.0
            && p.f32(at + 0x20)? <= 16_777_216.0,
        "Source accumulator has unsupported state or count bounds"
    );
    let _ = p.f32(at + 0x24)?;
    // A threshold above the cap intentionally never completes. Its children still update
    // the action value used by retained effects, including reset on trigger release.
    let rows = p.array(at + 8, 32, Some(0x8080316C))?;
    ensure!(
        (1..=3).contains(&rows.len()),
        "Source accumulator requires one to three literal contributions"
    );
    let mut bytes = vec![0; 56];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    put(&mut bytes, 8, &(rows.len() as u64).to_le_bytes())?;
    bytes[0x20..0x28].copy_from_slice(&p.bytes::<8>(at + 0x20)?);
    // D1 only applies the upper cap. A zero native lower bound would alter
    // negative action values. Keep the full finite range below the source cap.
    put(&mut bytes, 0x28, &f32::MIN.to_le_bytes())?;
    put(&mut bytes, 0x2C, &cap.to_le_bytes())?;
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
        if seconds > 0.0 {
            let ticks = seconds * 60.0;
            ensure!(
                (1.0..=32767.0).contains(&ticks) && ticks.fract() == 0.0,
                "Source counter hold needs an exact bounded 60 Hz duration"
            );
            held += 1;
            // D1 visits every positive row during allocation. Shadowkeep stops
            // after the first. One held row preserves the timer's root identity.
            ensure!(
                held == 1,
                "Source counter has multiple independently allocated holds"
            );
        }
        for lane in [8, 20] {
            let value = p.f32(row + lane + 4)?;
            ensure!(
                p.u8(row + lane)? <= 1
                    && p.bytes::<3>(row + lane + 1)? == [0; 3]
                    && value >= 0.0
                    && value <= cap
                    && value.fract() == 0.0,
                "Source accumulator needs literal nonnegative Add or Replace counts"
            );
        }
        let child = p.pointer(row)?;
        let kind = p.u8(child + 5)?;
        ensure!(
            matches!(kind, 2 | 4 | 8 | 14..=19 | 22 | 27) && p.u8(child + 6)? == 0,
            "Source accumulator child requires a qualified leaf event"
        );
        let node = super::read(source, p, child)?.context("Unsupported source counter child")?;
        source_mask |= 1u64 << kind;
        native_mask |= 1u64 << node.kind;
        let class = nodes::condition(node.kind)
            .context("Native counter child class")?
            .class;
        let child = Graph::read(&node.bytes, 0, class).map_err(anyhow::Error::msg)?;
        let target = graph.append(&child).map_err(anyhow::Error::msg)?;
        graph.blocks[1].bytes[index * 32 + 8..index * 32 + 32]
            .copy_from_slice(&p.bytes::<24>(row + 8)?);
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
