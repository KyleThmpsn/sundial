//! Direct Duration events retain keys with a qualified private or stock publisher.
use super::*;

pub(super) fn read(
    source: &Source,
    p: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<NativeNode> {
    let kind = p.u8(at + 5)?;
    let group = if kind == 30 { 0x20 } else { 0x48 };
    ensure!(
        p.array(group, 8, None)?
            .into_iter()
            .any(|row| p.pointer(row).ok() == Some(at)),
        "Source duration events require direct activation or removal"
    );
    let key = p.u32(at + 8)?;
    if key == 0x3B210AE5 {
        stock(
            source,
            native.context("Source external duration events need native stock publishers")?,
            key,
        )?;
    } else {
        ensure!(
            kind == 31,
            "Source duration start has no qualified publisher"
        );
        pattern::duration_end(source, p, key)?;
    }
    // Source 7B6970/7B6A90 and native C9ADE0/C9A0B0 publish the paired key.
    // Player forwarders retain their recipients while shifting kinds 30/31 to 29/30.
    // A stock key needs matching providers and lifecycle metadata in both builds.
    // Other removal keys remain scoped to this action's translated private status.
    let mut bytes = p.bytes::<12>(at)?.to_vec();
    bytes[5] = kind - 1;
    Ok(NativeNode {
        kind: kind - 1,
        bytes,
    })
}

fn stock(source: &Source, native: &mut Reader, key: u32) -> Result<()> {
    let tag = 0x80A752C8;
    ensure!(
        source.reference(tag)? == 0x80800861,
        "Source external duration publisher class differs"
    );
    let owner = source.read(tag)?;
    let (instance, duration) = publisher(&owner, tag, true, key)?;
    for graph in [0x80A6B0F3, 0x80B00C2F] {
        ensure!(
            source.reference(graph)? == 0x80800734,
            "Source external duration graph class differs"
        );
        contains(&source.read(graph)?, 32, tag)?;
    }
    let provider = source.read(owner.u32(duration + 0x60)?)?;
    methods(&owner, &provider, instance, true)?;
    for (graph, tag) in [(0x80BEB776, 0x80C3DE2C), (0x80BC8307, 0x80BC49CC)] {
        let graph = native.tag(graph, Some(0x80809C0F))?;
        contains(&graph, 16, tag)?;
        let owner = native.tag(tag, Some(0x80809C36))?;
        let (instance, duration) = publisher(&owner, tag, false, key)?;
        let provider = native.tag(owner.u32(duration + 0x58)?, Some(0x80809C54))?;
        methods(&owner, &provider, instance, false)?;
    }
    Ok(())
}

fn contains(graph: &Payload, descriptor: usize, tag: u32) -> Result<()> {
    let owners = graph.array(descriptor, 12, None)?;
    ensure!(
        owners
            .into_iter()
            .filter(|&row| graph.u32(row).ok() == Some(tag))
            .count()
            == 1,
        "External duration publisher is missing or repeated in its stock graph"
    );
    Ok(())
}

fn publisher(p: &Payload, tag: u32, from: bool, key: u32) -> Result<(usize, usize)> {
    let live = p.pointer(16)?;
    let settings = p.pointer(24)?;
    let instance = live + 0x50;
    let duration = settings + if from { 0x250 } else { 0x228 };
    let implementation = if from { 0x8080255D } else { 0x80803B32 };
    let definition = if from { 0x8080223C } else { 0x80803B33 };
    ensure!(
        p.u32(settings - 4)? == if from { 0x80802698 } else { 0x80804205 }
            && p.u32(live - 4)? == if from { 0x80802A75 } else { 0x80804209 },
        "External duration publisher has another root pair"
    );
    for (at, other, class) in [
        (duration, instance, implementation),
        (instance, duration, definition),
    ] {
        ensure!(
            p.u32(at)? == tag
                && p.u64(at + 8)? == other as u64
                && p.u32(at + if from { 16 } else { 4 })? == class
                && (!from || (p.u32(at + 4)? == 0 && p.u32(at + 20)? == 0)),
            "External duration publisher has a different reciprocal pair"
        );
    }
    let policy = duration + if from { 0xA0 } else { 0x98 };
    for (index, expected) in [5, key, 1, 99, 0].into_iter().enumerate() {
        ensure!(
            p.u32(policy + index * 4)? == expected,
            "External duration publisher key or policy differs"
        );
    }
    let hud = settings + if from { 0x178 } else { 0x160 };
    ensure!(
        p.u32(hud)? == 0xD491B5AC && p.u32(hud + 4)? == 99,
        "External duration publisher has another status identity"
    );
    Ok((instance, duration))
}

fn methods(owner: &Payload, provider: &Payload, instance: usize, from: bool) -> Result<()> {
    let class = if from { 0x8080255D } else { 0x80803B32 };
    ensure!(
        provider.u32(8)? == if from { 0x80800581 } else { 0x80809ADE }
            && provider.u32(12)? == if from { 0x808008FE } else { 0x80809ADD },
        "External duration publisher creation interface differs"
    );
    let rows = provider.array(16, 24, None)?;
    ensure!(
        rows.len() == 1
            && provider.u32(rows[0])? == class
            && provider.u32(rows[0] + 4)? == if from { 2 } else { 1 },
        "External duration publisher creation method differs"
    );
    ensure!(
        owner
            .array(0x30, 16, None)?
            .into_iter()
            .filter(|&row| {
                owner.pointer(row).ok() == Some(instance)
                    && owner.u32(row + 8).ok() == Some(class)
                    && owner.u32(row + 12).ok() == Some(if from { 0x30001 } else { 0x20000 })
            })
            .count()
            == 1,
        "External duration publisher cleanup method differs"
    );
    Ok(())
}
