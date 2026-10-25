//! Decode the emitted event filters and masks independently of talent lowering.
use super::*;
mod accumulator;
mod filter;
mod hit;
mod player;

pub(super) fn incoming(
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(native.u32(n - 4)?, 0x80803E3C);
    assert_eq!(native.bytes::<4>(n)?, [33, 1, 0, 0]);
    assert_eq!(source.bytes::<12>(s + 4)?, native.bytes::<12>(n + 4)?);
    filter::damage(stock, source, s + 16, native, n + 16)?;
    assert_eq!(native.u64(n + 0xA0)?, 0x80803E3C);
    assert_eq!(native.u32(n + 0xAC)?, 0x80803E3C);
    assert_eq!(native.u32(n + 0xB0)? as usize, n);
    Ok(())
}

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    let kind = source.u8(s + 5)?;
    let base = match kind {
        33 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DF8);
            assert_eq!(native.u8(n + 5)?, 32);
            assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
            assert_eq!(source.u8(s + 6)?, native.u8(n + 6)?);
            8
        }
        23 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DDA);
            assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
            assert_eq!(source.u8(s + 6)?, native.u8(n + 6)?);
            assert_eq!(source.bytes::<4>(s + 8)?, native.bytes::<4>(n + 8)?);
            return Ok(());
        }
        32 => {
            assert_eq!(native.u32(n - 4)?, 0x80803E04);
            assert_eq!(native.u8(n + 5)?, 31);
            let groups = source.array(s + 8, 32, Some(0x80803181))?;
            let emitted = native.array(n + 8, 32, Some(0x80803E06))?;
            assert_eq!(groups.len(), emitted.len());
            for (s, n) in groups.into_iter().zip(emitted) {
                assert_eq!(source.bytes::<8>(s)?, native.bytes::<8>(n)?);
                let children = source.array(s + 8, 8, None)?;
                let targets = native.array(n + 8, 8, None)?;
                assert_eq!(children.len(), targets.len());
                let mut mask = 0u64;
                for (s, n) in children.into_iter().zip(targets) {
                    let (s, n) = (source.pointer(s)?, native.pointer(n)?);
                    check(reader, stock, source, s, native, n)?;
                    mask |= 1u64 << native.u8(n + 5)?;
                }
                assert_eq!(native.u64(n + 24)?, mask);
            }
            return Ok(());
        }
        36 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DCC);
            assert_eq!(native.u8(n + 5)?, 35);
            player::fields(source, s, native, n)?;
            check(
                reader,
                stock,
                source,
                source.pointer(s + 0xD0)?,
                native,
                native.pointer(n + 0x100)?,
            )?;
            return Ok(());
        }
        20 => return player::check(source, s, native, n),
        12 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DEA);
            assert_eq!(source.u32(s + 8)?, 0x6CEC7A87);
            assert_eq!(source.u32(s + 12)?, 0x18CCBF24);
            assert_eq!(source.bytes::<16>(s)?, native.bytes::<16>(n)?);
            return Ok(());
        }
        26 => return accumulator::check(reader, stock, source, s, native, n),
        22 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DF9);
            assert_eq!(source.bytes::<4>(s + 8)?, native.bytes::<4>(n + 8)?);
            return Ok(());
        }
        4 => return hit::check(reader, stock, source, s, native, n),
        7 | 8 => {
            assert_eq!(
                native.u32(n - 4)?,
                if kind == 7 { 0x80803DFD } else { 0x80803E01 }
            );
            assert_eq!(source.bytes::<8>(s + 8)?, native.bytes::<8>(n + 8)?);
            assert_eq!(source.u64(s + 0x10)?, 0);
            assert_eq!(native.u64(n + 0x10)?, 0);
            assert_eq!(native.u64(n + 0x18)?, 0);
            return Ok(());
        }
        14..=19 => {
            let class = match kind {
                14 => 0x80803DDE,
                15 => 0x80803DFA,
                16 => 0x80803DF7,
                17 => 0x80803DF5,
                18 => 0x80803DDB,
                _ => 0x80803DE2,
            };
            assert_eq!(native.u32(n - 4)?, class);
            assert_eq!(source.u8(s + 8)?, native.u8(n + 8)?);
            let slot = if kind == 19 { 0xb } else { 9 };
            assert_eq!(source.u8(s + 9)?, native.u8(n + slot)?);
            if kind == 19 {
                // D1 accepts both reload event variants. Shadowkeep gates them separately.
                assert_eq!(native.bytes::<2>(n + 9)?, [1; 2]);
                assert_eq!(source.u32(s + 0xc)?, native.u32(n + 0xc)?);
            }
            0x10
        }
        2 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DE7);
            assert_eq!(source.u8(s + 0x1b4)?, native.u8(n + 0x140)?);
            assert_eq!(source.u8(s + 0x1b5)?, native.u8(n + 0x141)?);
            assert_eq!(source.u8(s + 0x1b6)?, 0);
            assert_eq!(native.u8(n + 0x142)?, 0);
            assert_eq!(source.u8(s + 0x1b7)?, native.u8(n + 0x14c)?);
            assert_eq!(source.u32(s + 0x1b8)?, native.u32(n + 0x150)?);
            assert_eq!(source.u32(s + 0x1bc)?, native.u32(n + 0x154)?);
            assert_eq!(native.u32(n + 0x144)?, 0x811c9dc5);
            assert_eq!(native.u32(n + 0x148)?, 0x811c9dc5);
            filter::damage(stock, source, s + 0x128, native, n + 0xB0)?;
            8
        }
        27 => {
            assert_eq!(native.u32(n - 4)?, 0x808029E0);
            assert_eq!(source.bytes::<2>(s + 8)?, native.bytes::<2>(n + 8)?);
            let mode = match source.u8(s + 0xe4)? {
                0 => 0,
                1 => 2,
                2 => 1,
                _ => return Err("Unqualified source event mode".into()),
            };
            assert_eq!(native.u8(n + 0x90)?, mode);
            0x10
        }
        _ => return Ok(()),
    };
    labels(reader, stock, source, s + base, native, n + base)
}

pub(super) fn labels(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    let original = reader.read(source.u32(s + 0x48)?)?;
    let labels = stock.tag(native.u32(n + 0x48)?, None)?;
    let dictionary = |p: &Payload, native: bool| -> CheckResult<BTreeMap<u32, Vec<u8>>> {
        let width = if native { 40 } else { 32 };
        let mut entries = BTreeMap::new();
        for (index, row) in p.array(8, 4, Some(0x80800070))?.into_iter().enumerate() {
            let mut mask = vec![0; width];
            mask[index / 8] = 1 << (index % 8);
            assert!(entries.insert(p.u32(row)?, mask).is_none());
        }
        for row in p.array(24, width + 4, None)? {
            assert!(
                entries
                    .insert(p.u32(row)?, p.0[row + 4..row + 4 + width].to_vec())
                    .is_none()
            );
        }
        Ok(entries)
    };
    let original_dictionary = dictionary(&original, false)?;
    let native_dictionary = dictionary(&labels, true)?;
    let predicate = n + 0x50;
    let mode = native.u8(predicate)?;
    let compiled = if mode == 255 {
        None
    } else {
        Some(native.pointer(predicate + 8)?)
    };
    let operations: &[usize] = if mode == 1 { &[0, 1, 2, 3] } else { &[0, 2] };
    for operation in 0..4 {
        let names = source
            .array(s + operation * 16, 24, Some(0x80800365))?
            .into_iter()
            .map(|row| source.u32(row))
            .collect::<Result<Vec<_>>>()?;
        let emitted = native
            .array(n + operation * 16, 24, Some(0x808094B3))?
            .into_iter()
            .map(|row| native.u32(row))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(
            names, emitted,
            "An event label or Boolean operation changed"
        );
        let mut original_mask = vec![0; 32];
        let mut expected = vec![0; 40];
        for name in &names {
            for (out, bits) in original_mask.iter_mut().zip(&original_dictionary[name]) {
                *out |= bits;
            }
            for (out, bits) in expected.iter_mut().zip(&native_dictionary[name]) {
                *out |= bits;
            }
        }
        assert_eq!(
            source.0[s + 0x50 + operation * 32..s + 0x50 + (operation + 1) * 32],
            original_mask
        );
        if let Some(index) = operations.iter().position(|&op| op == operation)
            && let Some(compiled) = compiled
        {
            assert_eq!(
                native.0[compiled + index * 40..compiled + (index + 1) * 40],
                expected
            );
            assert_eq!(
                native.u8(compiled + operations.len() * 40 + index)? != 0,
                names.is_empty()
            );
        } else {
            assert!(
                names.is_empty(),
                "Nonempty event filter has no compiled mask"
            );
        }
    }
    Ok(())
}
