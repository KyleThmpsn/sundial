//! Compound conditions preserve their children, hold times and event routing.
use super::*;
use sundial::package_authoring::sandbox_perk::nodes;

fn child(source: &Source, p: &Payload, at: usize) -> Result<(u8, Graph)> {
    let kind = p.u8(at + 5)?;
    // Bound recursion and keep compound timer allocation outside leaf children.
    ensure!(
        matches!(kind, 2 | 4 | 7 | 8 | 12 | 14..=20 | 22 | 27),
        "Source compound condition has an unqualified child"
    );
    ensure!(
        p.u8(at + 6)? == 0 && (kind != 20 || p.f32(at + 0xC4)? == 0.0),
        "Source compound child requires independent timer or retained-state allocation"
    );
    let node = super::read(source, p, at)?.context("Source compound condition child")?;
    let class = nodes::condition(node.kind)
        .context("Native condition class")?
        .class;
    let graph = Graph::read(&node.bytes, 0, class).map_err(anyhow::Error::msg)?;
    Ok((node.kind, graph))
}

pub(super) fn read(source: &Source, p: &Payload, at: usize) -> Result<NativeNode> {
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?) && p.u8(at + 4)? == 255 && p.u8(at + 6)? == 0,
        "Source subgroup condition has unqualified state"
    );
    let rows = p.array(at + 8, 32, Some(0x80803181))?;
    ensure!(
        !rows.is_empty() && rows.len() <= 16,
        "Source subgroup count is outside the bounded route"
    );
    let mut bytes = vec![0; 24];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    bytes[5] = 31;
    put(&mut bytes, 8, &(rows.len() as u64).to_le_bytes())?;
    let mut graph = Graph {
        blocks: vec![
            Block {
                class: 0x80803E04,
                count: None,
                bytes,
                links: BTreeMap::from([(16, 1)]),
            },
            Block {
                class: 0x80803E06,
                count: Some(rows.len()),
                bytes: vec![0; rows.len() * 32],
                links: BTreeMap::new(),
            },
        ],
    };
    for (index, row) in rows.into_iter().enumerate() {
        ensure!(
            p.f32(row)? >= 0.0 && p.u32(row + 4)? == 0,
            "Source subgroup hold is invalid"
        );
        let children = p.array(row + 8, 8, None)?;
        ensure!(
            !children.is_empty() && children.len() <= 16,
            "Source subgroup alternatives are empty or excessive"
        );
        let array_index = graph.blocks.len();
        graph.blocks.push(Block {
            class: 0x808040BA,
            count: Some(children.len()),
            bytes: vec![0; children.len() * 8],
            links: BTreeMap::new(),
        });
        graph.blocks[1].bytes[index * 32..index * 32 + 8].copy_from_slice(&p.bytes::<8>(row)?);
        put(
            &mut graph.blocks[1].bytes,
            index * 32 + 8,
            &(children.len() as u64).to_le_bytes(),
        )?;
        graph.blocks[1].links.insert(index * 32 + 16, array_index);
        let (mut source_mask, mut native_mask) = (0u64, 0u64);
        for (i, pointer) in children.into_iter().enumerate() {
            let at = p.pointer(pointer)?;
            let (kind, translated) = child(source, p, at)?;
            source_mask |= 1u64 << p.u8(at + 5)?;
            native_mask |= 1u64 << kind;
            let target = graph.append(&translated).map_err(anyhow::Error::msg)?;
            graph.blocks[array_index].links.insert(i * 8, target);
        }
        ensure!(
            p.u64(row + 24)? == source_mask,
            "Source subgroup event mask disagrees with its children"
        );
        put(
            &mut graph.blocks[1].bytes,
            index * 32 + 24,
            &native_mask.to_le_bytes(),
        )?;
    }
    graph
        .validate_node(NodeKind::Condition(31))
        .map_err(anyhow::Error::msg)?;
    Ok(NativeNode {
        kind: 31,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}

pub(super) fn predicate(source: &Source, p: &Payload, at: usize) -> Result<NativeNode> {
    ensure!(
        p.f32(at + 0xC4)? == 0.0,
        "Compound player hold requires its own scheduler contract"
    );
    let mut graph = player::graph(p, at)?;
    graph.blocks[0].class = 0x80803DCC;
    graph.blocks[0].bytes.resize(264, 0);
    graph.blocks[0].bytes[5] = 35;
    let nested = p.pointer(at + 0xD0)?;
    ensure!(
        p.u8(nested + 6)? == 0,
        "Nested predicate retains unqualified event state"
    );
    let (_, translated) = child(source, p, nested)?;
    let target = graph.append(&translated).map_err(anyhow::Error::msg)?;
    graph.blocks[0].links.insert(0x100, target);
    graph
        .validate_node(NodeKind::Condition(35))
        .map_err(anyhow::Error::msg)?;
    Ok(NativeNode {
        kind: 35,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}
