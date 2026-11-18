//! Follow the actual query output slots to their material effects. Native visuals are deliberate.
use super::*;

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    reader: &mut Reader,
    original: &Payload,
    emitted: &Payload,
) -> Result<Value> {
    let mut outputs = Vec::new();
    for (slot, (sf, nf)) in [(0x168, 0x178), (0x170, 0x180), (0x178, 0x188)]
        .into_iter()
        .enumerate()
    {
        let sa = resource(original, false)? + sf;
        let na = resource(emitted, true)? + nf;
        assert_eq!(
            original.u64(sa)? == 0,
            emitted.u64(na)? == 0,
            "Query output presence"
        );
        if original.u64(sa)? == 0 {
            continue;
        }
        let s = original.pointer(sa)?;
        let n = emitted.pointer(na)?;
        assert_eq!(original.u32(s - 4)?, 0x80801108);
        assert_eq!(emitted.u32(n - 4)?, 0x8080382A);
        assert_eq!(emitted.u32(n)?, emitted.u32(emitted.pointer(16)?)?);
        let source_graph = original.u32(s + 0x20)?;
        let native_graph = emitted.u32(n + 0x18)?;
        let before = stock.tag(native_graph, Some(0x80809C0F))?;
        let graph = reader.tag(native_graph, Some(0x80809C0F))?;
        assert_eq!(before.0, graph.0, "Native highlight graph changed");
        let (source_owner, se) = source_event(source, source_graph)?;
        let (owner, ne) = material_event(stock, reader, &graph)?;
        assert_eq!(owner.u32(ne - 4)?, 0x80806B98);
        assert_eq!(source_owner.bytes::<8>(se)?, owner.bytes::<8>(ne + 16)?);
        assert_eq!(
            source_owner.bytes::<16>(se + 12)?,
            owner.bytes::<16>(ne + 28)?
        );
        let material = owner.u32(ne + 0x40)?;
        assert_eq!(reader.tag(material, None)?.0, stock.tag(material, None)?.0);
        assert!(
            !reader
                .tag(material, None)?
                .array(0x70, 4, Some(0x80800014))?
                .is_empty()
        );
        outputs.push(json!({"slot":slot,"source_graph":source_graph,"native_graph":native_graph,
            "native_material_array":material,"visuals":"Shadowkeep native highlight","gameplay_verified":false}));
    }
    Ok(json!(outputs))
}

fn material_event(
    stock: &mut Reader,
    reader: &mut Reader,
    graph: &Payload,
) -> Result<(std::sync::Arc<Payload>, usize)> {
    let mut sequences = Vec::new();
    for row in graph.array(0x10, 12, Some(0x80809C04))? {
        let tag = graph.u32(row)?;
        let owner = reader.tag(tag, Some(0x80809C36))?;
        assert_eq!(owner.0, stock.tag(tag, Some(0x80809C36))?.0);
        match owner.u32(owner.pointer(24)? - 4)? {
            0x808084E9 => sequences.push(owner),
            0x80809790 => {}
            class => anyhow::bail!("Native highlight has unexpected owner {class:08X}"),
        }
    }
    assert_eq!(sequences.len(), 1);
    let owner = sequences.remove(0);
    let events = owner.array(owner.pointer(24)? + 0x168, 24, Some(0x808093E6))?;
    assert_eq!(events.len(), 1);
    let ne = owner.pointer(events[0] + 16)?;
    Ok((owner, ne))
}

fn source_event(source: &Source, source_graph: u32) -> Result<(Payload, usize)> {
    let source_graph_data = source.read(source_graph)?;
    let owners = source_graph_data.array(0x20, 12, Some(0x80800715))?;
    assert_eq!(
        owners.len(),
        1,
        "Source highlight has another behavior owner"
    );
    let source_owner = source.read(source_graph_data.u32(owners[0])?)?;
    let d = source_owner.pointer(24)?;
    assert_eq!(source_owner.u32(d - 4)?, 0x80800610);
    assert!(
        source_owner
            .array(d + 0x170, 8, Some(0x80800629))?
            .is_empty()
    );
    let events = source_owner.array(d + 0x180, 8, Some(0x80800629))?;
    assert_eq!(events.len(), 1);
    let se = source_owner.pointer(events[0])?;
    assert_eq!(source_owner.u32(se - 4)?, 0x808007A2);
    Ok((source_owner, se))
}
