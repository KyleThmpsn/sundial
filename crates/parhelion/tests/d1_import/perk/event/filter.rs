//! Read damage declarations and native ability masks through emitted references.
use super::*;

/// Shadowkeep has no `player_dot` label. The adaptation names its damage-over-time labels.
const PLAYER_DOT: u32 = 0x59EE13FA;
const OVER_TIME: [u32; 3] = [0x5417D662, 0x543B0EA1, 0x8E2155ED];

fn names(p: &Payload, at: usize, class: u32) -> CheckResult<Vec<u32>> {
    Ok(p.array(at, 24, Some(class))?
        .into_iter()
        .map(|row| p.u32(row))
        .collect::<Result<Vec<_>>>()?)
}

/// An incoming-damage effect whose source also carries a second, four-operation label filter.
/// The source lists are read from the original record and the expected native lists are
/// built here, independently of the importer: one native filter holding both.
pub(super) fn merged(
    stock: &mut Reader,
    source: &Payload,
    (s, second): (usize, usize),
    native: &Payload,
    n: usize,
) -> CheckResult<Value> {
    types(source, s, native, n)?;
    let ability = [
        names(source, s + 0x20, 0x80800365)?,
        names(source, s + 0x30, 0x80800365)?,
    ];
    let mut object = Vec::new();
    for operation in 0..4 {
        object.push(names(source, second + operation * 16, 0x80800365)?);
    }
    assert!(
        object.iter().any(|list| !list.is_empty()),
        "Source has no second label filter"
    );
    assert!(ability[0].len() <= 1 && object[0].is_empty());
    let mut expected = object.clone();
    expected[1].extend(&ability[0]);
    expected[2].extend(&ability[1]);
    let dictionary = stock.tag(native.u32(n + 0x68)?, None)?;
    let atoms = dictionary.array(8, 4, None)?;
    let groups = dictionary.array(24, 44, None)?;
    let held = |name: u32| atoms.iter().any(|&r| dictionary.u32(r).ok() == Some(name));
    let adapted = expected[1].contains(&PLAYER_DOT);
    if adapted {
        assert!(!held(PLAYER_DOT), "Shadowkeep declares the source label");
        expected[1].retain(|&name| name != PLAYER_DOT);
        expected[0] = OVER_TIME.into_iter().filter(|&name| held(name)).collect();
        assert_eq!(
            expected[0].len(),
            3,
            "A damage-over-time label is undeclared"
        );
    }
    let mode = native.u8(n + 0x78)?;
    assert_eq!(
        mode, 1,
        "A require-all list needs the four-operation predicate"
    );
    let compiled = native.pointer(n + 0x80)?;
    assert_eq!(native.u32(compiled - 4)?, 0x808093F5);
    for (operation, expected) in expected.iter().enumerate() {
        let emitted = names(native, n + 0x20 + operation * 16, 0x808094B3)?;
        assert_eq!(&emitted, expected, "Native label operation {operation}");
        let mut mask = [0u8; 40];
        for name in expected {
            if let Some(index) = atoms
                .iter()
                .position(|&r| dictionary.u32(r).ok() == Some(*name))
            {
                mask[index / 8] |= 1 << (index % 8);
            } else {
                let row = groups
                    .iter()
                    .find(|&&r| dictionary.u32(r).ok() == Some(*name))
                    .ok_or("Emitted label is unregistered")?;
                for (out, bits) in mask.iter_mut().zip(dictionary.bytes::<40>(row + 4)?) {
                    *out |= bits;
                }
            }
        }
        assert_eq!(native.bytes::<40>(compiled + operation * 40)?, mask);
        assert_eq!(
            native.u8(compiled + 160 + operation)?,
            u8::from(expected.is_empty())
        );
    }
    Ok(
        json!({"source_ability_lists":ability,"source_object_lists":object,
        "native_lists":expected,"player_dot_adapted":adapted,
        "limits":"Stored filter only. Which Shadowkeep damage carries these labels in play is unobserved."}),
    )
}

pub(super) fn damage(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    types(source, s, native, n)?;
    labels(reader, stock, source, s, native, n)
}

fn types(source: &Payload, s: usize, native: &Payload, n: usize) -> CheckResult<()> {
    for operation in 0..2 {
        let original = source.array(s + operation * 16, 1, None)?;
        let emitted = native.array(n + operation * 16, 1, Some(0x80806B02))?;
        let original = original
            .into_iter()
            .map(|r| source.u8(r))
            .collect::<Result<Vec<_>>>()?;
        let emitted = emitted
            .into_iter()
            .map(|r| native.u8(r))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(original, emitted);
        let mask = original
            .iter()
            .fold(0u32, |mask, &value| mask | (1 << value));
        assert_eq!(source.u32(s + 0x40 + operation * 4)?, mask);
        assert_eq!(native.u32(n + 0x70 + operation * 4)?, mask);
        assert_eq!(
            native.u8(n + 0x88 + operation)?,
            u8::from(original.is_empty())
        );
    }
    Ok(())
}

fn labels(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    let dictionary = stock.tag(native.u32(n + 0x68)?, None)?;
    let atoms = dictionary.array(8, 4, None)?;
    let groups = dictionary.array(24, 44, None)?;
    let mode = native.u8(n + 0x78)?;
    assert!(matches!(mode, 0 | 255));
    for (op, target_op) in [(0, 0), (1, 2)] {
        let rows = source.array(s + 0x20 + op * 16, 24, Some(0x80800365))?;
        let mut original = Vec::new();
        let mut source_mask = [0u8; 32];
        if let Some(&first) = rows.first() {
            let tag = source.u32(first + 16)?;
            let dictionary = reader.read(tag)?;
            let atoms = dictionary.array(8, 4, Some(0x80800070))?;
            let groups = dictionary.array(24, 36, Some(0x80800513))?;
            for row in rows {
                assert_eq!(source.u32(row + 16)?, tag);
                let name = source.u32(row)?;
                let mut bits = [0u8; 32];
                if let Some(index) = atoms
                    .iter()
                    .position(|&a| dictionary.u32(a).ok() == Some(name))
                {
                    bits[index / 8] = 1 << (index % 8);
                } else if let Some(&group) = groups
                    .iter()
                    .find(|&&g| dictionary.u32(g).ok() == Some(name))
                {
                    bits = dictionary.bytes::<32>(group + 4)?;
                } else {
                    assert_eq!(name, 0x811C9DC5, "Unregistered source ability label");
                }
                if bits.iter().any(|&bit| bit != 0) {
                    original.push(name);
                }
                for (out, bits) in source_mask.iter_mut().zip(bits) {
                    *out |= bits;
                }
            }
        }
        assert_eq!(source.bytes::<32>(s + 0x48 + op * 32)?, source_mask);
        assert_eq!(source.u8(s + 0x8A + op)?, u8::from(source_mask == [0; 32]));
        let emitted = native
            .array(n + 0x20 + target_op * 16, 24, Some(0x808094B3))?
            .into_iter()
            .map(|r| native.u32(r))
            .collect::<Result<Vec<_>>>()?;
        // No Shadowkeep damage carries a label its registry lacks, so a list loses that label
        // unless a native label takes its place. A match-any list left empty could never hold.
        let held = |name: u32| {
            atoms
                .iter()
                .chain(&groups)
                .any(|&r| dictionary.u32(r).ok() == Some(name))
        };
        let declared = std::mem::take(&mut original);
        let mut dropped = false;
        for name in declared {
            match super::adapted(name, target_op, &held) {
                Some(names) if names == [name] => original.push(name),
                Some(names) => {
                    for name in names {
                        if !original.contains(&name) {
                            original.push(name);
                        }
                    }
                }
                None => dropped = true,
            }
        }
        assert!(
            !(dropped && target_op == 0 && original.is_empty()),
            "A match-any ability filter with only absent labels was kept"
        );
        assert_eq!(original, emitted);
        let mut mask = [0u8; 40];
        for name in &original {
            if let Some(index) = atoms
                .iter()
                .position(|&r| dictionary.u32(r).ok() == Some(*name))
            {
                mask[index / 8] |= 1 << (index % 8);
            } else {
                let row = groups
                    .iter()
                    .find(|&&r| dictionary.u32(r).ok() == Some(*name))
                    .ok_or("Emitted event label is unregistered")?;
                for (out, bits) in mask.iter_mut().zip(dictionary.bytes::<40>(row + 4)?) {
                    *out |= bits;
                }
            }
        }
        if mode == 255 {
            assert!(original.is_empty());
        } else {
            let compiled = native.pointer(n + 0x80)?;
            assert_eq!(native.bytes::<40>(compiled + op * 40)?, mask);
            assert_eq!(
                native.u8(compiled + 80 + op)?,
                u8::from(original.is_empty())
            );
        }
    }
    assert_eq!(native.u64(n + 0x30)?, 0);
    assert_eq!(native.u64(n + 0x50)?, 0);
    Ok(())
}
