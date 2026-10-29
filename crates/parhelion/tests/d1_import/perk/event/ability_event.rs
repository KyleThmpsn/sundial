//! Follow the emitted ability reference back through both independent definition tables.
use super::*;

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    original: &Payload,
    s: usize,
    emitted: &Payload,
    n: usize,
) -> CheckResult<()> {
    let (source_class, native_class) = match original.u8(s + 5)? {
        10 => (0x808031BF, 0x80803DFF),
        11 => (0x8080308E, 0x80803DFE),
        _ => return Err("Unexpected source ability roster event".into()),
    };
    assert_eq!(original.u32(s - 4)?, source_class);
    assert_eq!(emitted.u32(n - 4)?, native_class);
    assert_eq!(original.bytes::<7>(s)?, emitted.bytes::<7>(n)?);
    assert_eq!(emitted.u64(n + 8)?, 0);
    assert_eq!(original.u32(s + 20)?, 0);
    assert_eq!(emitted.u32(n + 20)?, 0);
    let source_pattern = original.u32(s + 16)?;
    pattern(source, stock, source_pattern, emitted.u32(n + 16)?)
}

pub(super) fn pattern(
    source: &Source,
    stock: &mut Reader,
    source_pattern: u32,
    target: u32,
) -> CheckResult<()> {
    assert_eq!(source.reference(source_pattern)?, 0x80800734);
    source.read(source_pattern)?;

    // This build-pinned legacy root is independent of the importer's name lookup.
    let assets = source.read(0x80A6A000)?;
    let assignments_tag = assets.u32(16)?;
    assert_eq!(source.reference(assignments_tag)?, 0x80800341);
    let assignments = source.read(assignments_tag)?;
    let mut source_keys = BTreeSet::new();
    for row in assignments.array(8, 8, None)? {
        if assignments.u32(row + 4)? == source_pattern {
            assert!(source_keys.insert(assignments.u32(row)?));
        }
    }
    let tables = source.classes(0x80803432);
    assert_eq!(tables.len(), 1);
    let definitions = source.read(tables[0])?;
    let mut matched = Vec::new();
    for row in definitions.array(8, 40, Some(0x8080343C))? {
        let key = definitions.u32(row)?;
        if source_keys.contains(&key) {
            matched.push(key);
        }
    }
    assert_eq!(matched.len(), 1, "Source ability reference is ambiguous");
    let identity = matched[0];

    let tables = stock.classes(0x80805CA4);
    assert_eq!(tables.len(), 1);
    let definitions = stock.tag(tables[0], Some(0x80805CA4))?;
    let mut matched = Vec::new();
    for row in definitions.array(8, 28, Some(0x80805CAA))? {
        if definitions.u32(row + 4)? == identity {
            matched.push(row);
        }
    }
    assert_eq!(matched.len(), 1, "Native ability identity is ambiguous");
    let assets = stock.tag(0x80EC3F62, None)?;
    let assignments = stock.tag(assets.u32(16)?, Some(0x80809780))?;
    let mut matched = Vec::new();
    for row in assignments.array(8, 8, None)? {
        if assignments.u32(row)? == identity {
            matched.push(assignments.u32(row + 4)?);
        }
    }
    assert_eq!(matched, vec![target]);
    stock.tag(target, Some(0x80809C0F))?;
    Ok(())
}
