//! Follow the emitted companion's actual dispatch, initialization and replication tuning.
use super::*;

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    original: &Payload,
    original_graph: &Payload,
    graph: u32,
    ordinal: usize,
) -> CheckResult<()> {
    let graph = emitted.tag(graph, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    let original_owners = original_graph.array(32, 12, Some(0x80800715))?;
    assert_eq!(owners.len(), original_owners.len());
    assert_eq!(
        original_graph.u32(original_owners[ordinal])?,
        original.u32(original.pointer(16)?)?
    );
    let tag = graph.u32(owners[ordinal])?;
    assert!(
        stock.tag(tag, None).is_err(),
        "Network owner must be private"
    );
    let target = emitted.tag(tag, Some(0x80809C36))?;
    let (s, n) = (original.pointer(24)?, target.pointer(24)?);
    let (si, ni) = (original.pointer(16)?, target.pointer(16)?);
    assert_eq!(target.u32(n)?, tag);
    assert_eq!(target.u32(ni)?, tag);
    assert_eq!(target.u64(n + 8)?, ni as u64);
    assert_eq!(target.u64(ni + 8)?, n as u64);
    for (p, at, base, size) in [(&target, ni, 0x808090E6, 96), (&target, n, 0x808090E8, 344)] {
        let class = p.u32(at - 4)?;
        if class != base {
            let schema = emitted.tag(class, Some(0x80800000))?;
            assert_eq!((schema.u32(16)?, schema.u32(20)?), (base, size));
        }
    }
    assert_eq!(original.bytes::<72>(si + 24)?, target.bytes::<72>(ni + 24)?);
    assert_eq!(
        original.bytes::<68>(s + 0x118)?,
        target.bytes::<68>(n + 0x100)?
    );
    assert_eq!(target.u32(n + 0x144)?, 0);
    assert_eq!(target.u64(n + 0x148)?, 0);
    assert_eq!(target.u64(n + 0x150)?, u32::MAX as u64);
    for (a, b, kind) in [
        (0x60, 0x48, 7),
        (0x80, 0x68, 8),
        (0xA0, 0x88, 2),
        (0xB8, 0xA0, 4),
        (0xD0, 0xB8, 3),
        (0xE8, 0xD0, 5),
        (0x100, 0xE8, 6),
    ] {
        assert_eq!(original.pointer(s + a)?, s);
        assert_eq!(target.pointer(n + b)?, n);
        let from = source.read(original.u32(s + a + 8)?)?;
        let to = emitted.tag(target.u32(n + b + 8)?, Some(0x80809C54))?;
        let from_rows = from.array(16, 24, Some(0x808005C0))?;
        let to_rows = to.array(16, 24, Some(0x80809C56))?;
        assert_eq!(from_rows.len(), 1);
        assert_eq!(to_rows.len(), 1);
        assert_eq!(
            (from.u32(from_rows[0])?, to.u32(to_rows[0])?),
            (0x808006F9, 0x808090E6)
        );
        assert_eq!(from.u32(from_rows[0] + 4)?, kind);
        assert_eq!(to.u32(to_rows[0] + 4)?, kind);
        assert_eq!(
            from.bytes::<16>(from_rows[0] + 8)?,
            to.bytes::<16>(to_rows[0] + 8)?
        );
    }
    let original_lifecycle = original.array(0x30, 16, Some(0x80800930))?;
    let lifecycle = target.array(0x30, 16, Some(0x808091A4))?;
    assert_eq!(original_lifecycle.len(), 1);
    assert_eq!(lifecycle.len(), 1);
    assert_eq!(original.pointer(original_lifecycle[0])?, si);
    assert_eq!(target.pointer(lifecycle[0])?, ni);
    assert_eq!(target.u32(lifecycle[0] + 8)?, 0x808090E6);
    assert_eq!(
        original.u32(original_lifecycle[0] + 12)?,
        target.u32(lifecycle[0] + 12)?
    );
    let allocation = emitted.tag(target.u32(0x44)?, None)?;
    assert_eq!(allocation.0.len(), 48);
    assert_eq!(allocation.u64(8)?, 0x811C9DC5);
    assert_eq!(allocation.u64(16)?, 0);
    assert_eq!(allocation.u64(24)?, u32::MAX as u64);
    assert_eq!(allocation.bytes::<16>(32)?, [0; 16]);
    // Match each exported provider by its definition offset, independently of row order.
    // The exclusion flags and component ordinal belong to the graph, not the owner body.
    let source_owner = original.u32(si)?;
    let mut expected = BTreeMap::new();
    for at in original_graph.array(0x68, 40, Some(0x80800370))? {
        if original_graph.u32(at + 16)? == source_owner {
            let offset = usize::try_from(original_graph.u64(at + 24)?)? - s;
            assert!(expected.insert(offset, original_graph.u32(at)?).is_none());
        }
    }
    let exports = graph.array(0x58, 40, Some(0x80809C22))?;
    let references = graph.array(0x68, 24, Some(0x80809C20))?;
    assert_eq!(exports.len(), references.len());
    for (at, reference) in exports.into_iter().zip(references) {
        if graph.u32(at + 16)? != tag {
            continue;
        }
        let offset = usize::try_from(graph.u64(at + 24)?)? - n + 24;
        assert_eq!(Some(graph.u32(at)?), expected.remove(&offset));
        assert_eq!(graph.u32(reference)?, tag);
        assert_eq!(graph.u64(reference + 8)?, ni as u64);
        assert_eq!(graph.u64(reference + 16)?, ordinal as u64);
    }
    assert!(expected.is_empty(), "A network provider export was lost");
    Ok(())
}
