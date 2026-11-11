//! A grouped presentation event and its table, checked against the source event and table.
//!
//! The native event keeps the source group kind, bounds and first two flag bytes, and sets flag
//! byte 2 as every stock event paired with a source event does. A source event without a
//! locator names an empty-named kind-0 locator after the source locators. The emitted table keeps
//! every source byte except array markers, group and row classes and leaf resources. Cue leaves
//! name staged cues, graph leaves staged graphs, and empty leaves stay empty.
use super::*;

const PAIRS: [(u32, u32); 3] = [
    (0x8080_0184, 0x8080_9FBD),
    (0x8080_06C6, 0x8080_8BD5),
    (0x8080_0665, 0x8080_8BD7),
];

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    original: &Payload,
    from: usize,
    owner: &Payload,
    to: usize,
) -> CheckResult<()> {
    assert_eq!(owner.u32(to - 4)?, 0x80804AB4);
    assert_eq!(owner.u32(to + 0x30)?, 30);
    assert_eq!(owner.u32(to + 0x40)?, original.u32(from + 0x20)?, "Group");
    assert_eq!(
        owner.bytes::<8>(to + 0x70)?,
        original.bytes::<8>(from + 0x38)?,
        "Bounds"
    );
    let flags = original.bytes::<4>(from + 0x44)?;
    assert_eq!(owner.bytes::<4>(to + 0x7C)?, [flags[0], flags[1], 1, 0]);
    let source_definition = original.pointer(24)?;
    let declared = original
        .array(source_definition + 0x1B0, 5, Some(0x80800429))?
        .len();
    let locator = usize::try_from(owner.u32(to + 0x78)?)?;
    let definition = owner.pointer(24)?;
    match original.u32(from + 0x40)? {
        u32::MAX => {
            assert_eq!(
                locator, declared,
                "Origin locator follows the source locators"
            );
            let row = owner.array(definition + 0x1B0, 6, Some(0x80808617))?[locator];
            let declarations = owner.array(definition + 0x1C0, 16, Some(0x80808615))?;
            assert_eq!(owner.u8(row + 3)?, 1, "Origin locator declaration count");
            let declaration = declarations[usize::from(owner.u8(row + 2)?)];
            assert_eq!(owner.u8(declaration)?, 0, "Origin declaration kind");
            let names = owner.array(definition + 0x1D0, 4, Some(0x80808614))?;
            assert_eq!(
                owner.u32(names[locator])?,
                0x811C_9DC5,
                "Origin locator name"
            );
        }
        source => assert_eq!(locator, usize::try_from(source)?, "Table locator"),
    }

    let tag = owner.u32(to + 0x50)?;
    assert!(stock.tag(tag, None).is_err(), "Table must be private");
    let table = emitted.tag(tag, Some(0x80808BCD))?;
    let source_tag = original.u32(from + 0x30)?;
    assert_eq!(reader.reference(source_tag)?, 0x808005F7);
    let source = reader.read(source_tag)?;
    assert_eq!(table.0.len(), source.0.len(), "Table size");
    let mut leaves = 0;
    for at in (0..source.0.len() - 3).step_by(4) {
        let (a, b) = (source.u32(at)?, table.u32(at)?);
        if a == b {
            continue;
        }
        if let Some(&(_, native)) = PAIRS.iter().find(|(s, _)| *s == a) {
            assert_eq!(b, native, "Table class word at {at:X}");
            continue;
        }
        let class = match reader.reference(a)? {
            0x8080080A => 0x80809802,
            0x80800734 => 0x80809C0F,
            other => return Err(format!("Table word {at:X} names class {other:08X}").into()),
        };
        // A silent sequence leaves cue leaves empty.
        if !(class == 0x80809802 && b == u32::MAX) {
            emitted.tag(b, Some(class))?;
        }
        leaves += 1;
    }
    assert!(leaves > 0, "Table names no resources");
    Ok(())
}
