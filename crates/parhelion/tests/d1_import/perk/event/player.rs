//! Read original and emitted player predicates at their consumer-owned offsets.
use super::*;

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(source.u32(s - 4)?, 0x80803101);
    assert_eq!(native.u32(n - 4)?, 0x80803DCE);
    assert_eq!(native.u8(n + 5)?, 20);
    fields(reader, stock, source, s, native, n)
}

pub(super) fn fields(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    // The native gate caches a named bank binding in linked state. Its state flag and
    // condition ordinal are compiled metadata, independent of the source header.
    assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
    assert_eq!(source.u8(s + 6)?, 0);
    for at in (8..0x38).step_by(8) {
        assert_eq!(source.bytes::<8>(s + at)?, native.bytes::<8>(n + at)?);
    }
    assert_eq!(source.u8(s + 0x38)?, native.u8(n + 0x38)?);
    assert_eq!(source.u8(s + 0x60)?, native.u8(n + 0x80)?);
    let mask = source.u8(s + 0x61)?;
    assert_eq!(native.u8(n + 0x81)? & 1, mask & 1);
    assert_eq!((native.u8(n + 0x81)? >> 2) & 1, (mask >> 1) & 1);
    assert_eq!(native.u8(n + 0x81)? & !5, 0);
    for index in 0..7 {
        assert_eq!(
            source.bytes::<8>(s + 0x64 + index * 8)?,
            native.bytes::<8>(n + 0x84 + index * 8)?
        );
    }
    let key = source.u32(s + 0x9C)?;
    assert_eq!(native.u32(n + 0xD4)?, key);
    assert_eq!(native.u8(n + 6)?, u8::from(key != 0x811C9DC5));
    if key != 0x811C9DC5 {
        let source_bank = reader.read(0x80A746E2)?;
        let property = named_property(&source_bank, key, 0x808008F6)?;
        for tag in [0x80EF1439, 0x80EF62F7, 0x80C0C498] {
            let bank = stock.tag(tag, Some(0x80809C36))?;
            assert_eq!(named_property(&bank, key, 0x80809790)?, property);
        }
    }
    assert_eq!(source.bytes::<8>(s + 0xA0)?, native.bytes::<8>(n + 0xD8)?);
    assert_eq!(source.bytes::<8>(s + 0xA8)?, [0; 8]);
    assert_eq!(source.u32(s + 0xBC)?, 0);
    assert_eq!(native.u32(n + 0xCC)?, 0);
    assert_eq!(source.bytes::<3>(s + 0xC1)?, [0; 3]);
    assert_eq!(native.bytes::<3>(n + 0xD1)?, [0; 3]);
    let pattern = source.u32(s + 0xB8)?;
    if pattern == u32::MAX {
        assert_eq!(source.u64(s + 0xB0)?, 0);
        assert_eq!(source.u8(s + 0xC0)?, 0);
        assert_eq!(native.u32(n + 0xC8)?, u32::MAX);
        assert_eq!(native.u8(n + 0xD0)?, 0);
    } else {
        assert_eq!(source.u8(s + 0xC0)?, 3);
        assert_eq!(native.u8(n + 0xD0)?, 3);
        if source.u64(s + 0xB0)? != 0 {
            let path = source.pointer(s + 0xB0)?;
            assert!(source.0[path..].contains(&0));
        }
        ability_event::pattern(reader, stock, pattern, native.u32(n + 0xC8)?)?;
    }
    // Source setter 6F5010 arms a positive hold as whole 60 Hz ticks, nearest with ties to even
    // and at least one. The native timer holds those ticks in seconds.
    let hold = source.f32(s + 0xC4)?;
    let expected = if hold > 0.0 {
        (hold * 60.0).round_ties_even().max(1.0) / 60.0
    } else {
        0.0
    };
    assert_eq!(
        native.f32(n + 0xE0)?.to_bits(),
        expected.to_bits(),
        "Player hold"
    );
    assert_eq!(source.u8(s + 0xC8)?, native.u8(n + 0xF8)?);
    ability_ranges(source, s + 0x40, native, n + 0x60)?;
    named_ranges(source, s + 0x50, native, n + 0x70)?;
    assert_eq!(native.u64(n + 0xC0)?, 0);
    for at in [0x48, 0xE8] {
        assert_eq!(native.bytes::<16>(n + at)?, [0; 16]);
    }
    Ok(())
}

fn named_ranges(source: &Payload, s: usize, native: &Payload, n: usize) -> CheckResult<()> {
    let original = source.array(s, 8, Some(0x8080316F))?;
    let emitted = native.array(n, 8, Some(0x808040C2))?;
    assert_eq!(original.len(), emitted.len());
    if original.is_empty() {
        assert_eq!(native.bytes::<16>(n)?, [0; 16]);
    }
    for (original, emitted) in original.into_iter().zip(emitted) {
        let original = source.pointer(original)?;
        let emitted = native.pointer(emitted)?;
        assert_eq!(source.u32(original - 4)?, 0x808031C6);
        assert_eq!(native.u32(emitted - 4)?, 0x808040C7);
        assert!(matches!(
            source.u32(original)?,
            0xAD7C4580 | 0xBB7679EB | 0xCF6062E0
        ));
        assert_eq!(source.bytes::<12>(original)?, native.bytes::<12>(emitted)?);
    }
    Ok(())
}

fn named_property(bank: &Payload, name: u32, class: u32) -> CheckResult<u32> {
    let definition = bank.pointer(24)?;
    assert_eq!(bank.u32(definition - 4)?, class);
    let declarations = bank.array(definition + 0xD8, 112, None)?;
    let names = bank.array(definition + 0xF8, 12, None)?;
    let found = names
        .into_iter()
        .filter(|&row| {
            bank.u32(row).ok() == Some(0x811C9DC5) && bank.u32(row + 4).ok() == Some(name)
        })
        .collect::<Vec<_>>();
    assert_eq!(found.len(), 1, "Public player value binding");
    let output = declarations[bank.u32(found[0] + 8)? as usize];
    assert_eq!(bank.u32(output)?, name);
    let instructions = bank
        .array(output + 8, 1, None)?
        .into_iter()
        .map(|at| bank.u8(at))
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(instructions, [0x3C, 0, 0x3E, 0]);
    let inputs = bank.array(output + 56, 4, None)?;
    assert_eq!(inputs.len(), 1);
    let input_index = bank.u32(inputs[0])?;
    let input = declarations[input_index as usize];
    let property = bank.u32(input)?;
    let inherited = bank
        .array(definition + 0x108, 12, None)?
        .into_iter()
        .filter(|&row| {
            bank.u32(row).ok() == Some(0x811C9DC5)
                && bank.u32(row + 4).ok() == Some(property)
                && bank.u32(row + 8).ok() == Some(input_index)
        })
        .count();
    assert_eq!(inherited, 1, "Inherited player property binding");
    for offset in [8, 24, 56] {
        assert_eq!(bank.u64(input + offset)?, 0);
    }
    Ok(property)
}

pub(super) fn ability_ranges(
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    let original = source.array(s, 12, Some(0x808031E8))?;
    let emitted = native.array(n, 12, Some(0x808040CA))?;
    assert_eq!(original.len(), emitted.len());
    if original.is_empty() {
        assert_eq!(native.bytes::<16>(n)?, [0; 16]);
    }
    for (original, emitted) in original.into_iter().zip(emitted) {
        assert!(source.u32(original)? <= 2);
        assert_eq!(source.bytes::<12>(original)?, native.bytes::<12>(emitted)?);
    }
    Ok(())
}
