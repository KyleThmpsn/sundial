//! Follow hit predicates through their emitted declared children and compiled masks.
use super::*;

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(native.u32(n - 4)?, 0x80802F5F);
    for (a, b) in [(0x94, 0x98), (0x95, 0x99), (0x9C, 0xA4), (0x180, 0xC1)] {
        assert_eq!(source.u8(s + a)?, native.u8(n + b)?);
    }
    assert_eq!(source.u32(s + 0x98)?, native.u32(n + 0xA0)?);
    assert_eq!(native.u32(n + 0x9C)?, 0x811C9DC5);
    assert_eq!(native.bytes::<3>(n + 0xC2)?, [0; 3]);
    if source.u64(s + 0x188)? == 0 {
        assert_eq!(native.u64(n + 0xC8)?, 0);
        assert_eq!(native.u8(n + 6)?, 0);
    } else {
        let original = source.pointer(s + 0x188)?;
        let counter = native.pointer(n + 0xC8)?;
        assert_eq!(source.u32(original - 4)?, 0x80802D44);
        assert_eq!(native.u32(counter - 4)?, 0x80803DF0);
        assert_eq!(source.bytes::<12>(original)?, native.bytes::<12>(counter)?);
        assert_eq!(native.u8(n + 6)?, 1);
    }
    assert_eq!(native.u8(n + 0xA8)?, 0, "Object predicates must all pass");
    let predicates = native.array(n + 0xB0, 16, Some(0x80809316))?;
    assert_eq!(predicates.len(), 2);
    let mut found = BTreeSet::new();
    for row in predicates {
        assert_eq!(native.u64(row)?, 0, "Hit predicate was inverted");
        let child = native.pointer(row + 8)?;
        let class = native.u32(child - 4)?;
        assert!(found.insert(class));
        match class {
            0x80804D76 => assert_eq!(source.u8(s + 0x174)?, native.u8(child)?),
            0x80804D73 => labels(reader, stock, source, s + 0xA0, native, child)?,
            _ => return Err("Unexpected hit predicate".into()),
        }
    }
    super::filter::damage(reader, stock, source, s + 8, native, n + 8)
}
