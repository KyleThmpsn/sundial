//! Player predicates retain ranges while rebuilding the target's field and state domains.
use super::*;

pub(super) fn read(
    source: &Source,
    p: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<NativeNode> {
    let graph = graph(source, p, at, native)?;
    graph
        .validate_node(NodeKind::Condition(20))
        .map_err(anyhow::Error::msg)?;
    Ok(NativeNode {
        kind: 20,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}

pub(super) fn graph(
    source: &Source,
    p: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<Graph> {
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?)
            && p.u8(at + 4)? == 255
            && p.u8(at + 6)? == 0
            && p.u8(at + 0x38)? & !0x3F == 0
            && p.bytes::<7>(at + 0x39)? == [0; 7],
        "Source player predicate has unqualified state flags"
    );
    ensure!(
        p.u8(at + 0x60)? <= 4
            && p.u8(at + 0x61)? & !3 == 0
            && p.bytes::<2>(at + 0x62)? == [0; 2]
            && p.u32(at + 0x9C)? == 0x811C9DC5
            && p.bytes::<8>(at + 0xA8)? == [0; 8]
            && p.u32(at + 0xBC)? == 0
            && p.bytes::<3>(at + 0xC1)? == [0; 3]
            && p.u8(at + 0xC8)? <= 1
            && p.bytes::<7>(at + 0xC9)? == [0; 7],
        "Source player predicate has an unqualified named value or activity restriction"
    );
    let pattern = p.u32(at + 0xB8)?;
    let selected = if pattern == u32::MAX {
        ensure!(
            p.u64(at + 0xB0)? == 0 && p.u8(at + 0xC0)? == 0,
            "Source empty ability selector has unqualified metadata"
        );
        u32::MAX
    } else {
        ensure!(
            p.u8(at + 0xC0)? == 3,
            "Source selected ability uses an unqualified slot"
        );
        if p.u64(at + 0xB0)? != 0 {
            let path = p.pointer(at + 0xB0)?;
            ensure!(
                p.0[path..].contains(&0),
                "Source selected ability path is unterminated"
            );
        }
        ability_event::resolve(
            source,
            native.context("Source selected ability needs native ability definitions")?,
            pattern,
        )?
    };
    let hold = p.f32(at + 0xC4)?;
    ensure!(
        hold >= 0.0 && hold * 60.0 <= i16::MAX as f32 && (hold * 60.0).fract() == 0.0,
        "Source player hold exceeds the qualified source timer domain"
    );
    for offset in (8..0x38)
        .step_by(8)
        .chain((0x64..0x9C).step_by(8))
        .chain([0xA0])
    {
        let low = p.f32(at + offset)?;
        let high = p.f32(at + offset + 4)?;
        ensure!(
            low.is_finite() && high.is_finite() && low <= high,
            "Source player predicate has an invalid range"
        );
    }
    // Source 76D610 and target 107FC20 call corresponding range and mask helpers.
    // The target inserts a compiled predicate at +40, moving the source's two
    // additional predicate arrays to +60/+70. Each array owns its relocated children.
    let mut bytes = vec![0; 0x100];
    bytes[..0x39].copy_from_slice(&p.bytes::<0x39>(at)?);
    bytes[0x80] = p.u8(at + 0x60)?;
    // Holding is bit 0 in both engines. D1's ADS bit 1 became target bit 2.
    // The intervening target bit has a different producer and remains clear.
    let mask = p.u8(at + 0x61)?;
    bytes[0x81] = (mask & 1) | ((mask & 2) << 1);
    bytes[0x84..0xBC].copy_from_slice(&p.bytes::<0x38>(at + 0x64)?);
    // Source 7782D0 selects a roster object at +300 + slot*4 and reads its pattern.
    // Native F04840 selects the corresponding reference through B9CFA0, then follows
    // the object definition to the canonical pattern. Only stock Jump slot identities
    // are qualified. Private imported ability instances require a separate binding.
    bytes[0xC8..0xCC].copy_from_slice(&selected.to_le_bytes());
    bytes[0xD0] = p.u8(at + 0xC0)?;
    bytes[0xD4..0xE0].copy_from_slice(&p.bytes::<12>(at + 0x9C)?);
    // Both schedulers arm on a true predicate, cancel on false and expire once.
    // D1 stores integral simulation ticks, while the native timer stores seconds.
    bytes[0xE0..0xE4].copy_from_slice(&hold.to_le_bytes());
    bytes[0xF8] = p.u8(at + 0xC8)?;
    let mut graph = Graph {
        blocks: vec![Block {
            class: 0x80803DCE,
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    ability_ranges(p, at + 0x40, &mut graph, 0x60)?;
    named_ranges(p, at + 0x50, &mut graph)?;
    Ok(graph)
}

pub(super) fn ability_ranges(
    p: &Payload,
    descriptor: usize,
    graph: &mut Graph,
    to: usize,
) -> Result<()> {
    let rows = p.array(descriptor, 12, Some(0x808031E8))?;
    ensure!(
        rows.len() <= 16,
        "Source ability range list exceeds the bounded translation route"
    );
    if rows.is_empty() {
        ensure!(
            p.bytes::<16>(descriptor)? == [0; 16],
            "Source empty ability range list has a nonempty descriptor"
        );
        return Ok(());
    }
    let mut bytes = Vec::with_capacity(rows.len() * 12);
    for row in &rows {
        // Both original helpers select a signed-byte ability slot, treat a missing
        // ability value as zero and AND inclusive range results. Only the shared
        // Grenade, Super and Melee slots have qualified cross-version identities.
        ensure!(
            p.u32(*row)? <= 2,
            "Source ability range uses an unqualified slot or reserved bytes"
        );
        let low = p.f32(*row + 4)?;
        let high = p.f32(*row + 8)?;
        ensure!(
            low.is_finite() && high.is_finite() && low <= high,
            "Source ability range is invalid"
        );
        bytes.extend_from_slice(&p.bytes::<12>(*row)?);
    }
    let index = graph.blocks.len();
    graph.blocks.push(Block {
        class: 0x808040CA,
        count: Some(rows.len()),
        bytes,
        links: BTreeMap::new(),
    });
    put(
        &mut graph.blocks[0].bytes,
        to,
        &(rows.len() as u64).to_le_bytes(),
    )?;
    graph.blocks[0].links.insert(to + 8, index);
    Ok(())
}

fn named_ranges(p: &Payload, descriptor: usize, graph: &mut Graph) -> Result<()> {
    let rows = p.array(descriptor, 8, Some(0x8080316F))?;
    ensure!(
        rows.len() <= 16,
        "Source named range list exceeds the bounded translation route"
    );
    if rows.is_empty() {
        ensure!(
            p.bytes::<16>(descriptor)? == [0; 16],
            "Source empty named range list has a nonempty descriptor"
        );
        return Ok(());
    }
    let index = graph.blocks.len();
    graph.blocks.push(Block {
        class: 0x808040C2,
        count: Some(rows.len()),
        bytes: vec![0; rows.len() * 8],
        links: BTreeMap::new(),
    });
    for (i, pointer) in rows.iter().enumerate() {
        ensure!(p.u64(*pointer)? != 0, "Source named range has no predicate");
        let at = p.pointer(*pointer)?;
        ensure!(
            at >= 4 && p.u32(at - 4)? == 0x808031C6,
            "Source named range requires a qualified Float32 predicate"
        );
        // These global keys feed the corresponding damage multipliers in both
        // engines. Other keys need an independently qualified native consumer.
        ensure!(
            matches!(p.u32(at)?, 0xAD7C4580 | 0xBB7679EB | 0xCF6062E0),
            "Source named range has no qualified native global key"
        );
        let low = p.f32(at + 4)?;
        let high = p.f32(at + 8)?;
        ensure!(
            low.is_finite() && high.is_finite() && low <= high,
            "Source named range is invalid"
        );
        let child = graph.blocks.len();
        graph.blocks.push(Block {
            class: 0x808040C7,
            count: None,
            bytes: p.bytes::<12>(at)?.to_vec(),
            links: BTreeMap::new(),
        });
        graph.blocks[index].links.insert(i * 8, child);
    }
    put(
        &mut graph.blocks[0].bytes,
        0x70,
        &(rows.len() as u64).to_le_bytes(),
    )?;
    graph.blocks[0].links.insert(0x78, index);
    Ok(())
}
