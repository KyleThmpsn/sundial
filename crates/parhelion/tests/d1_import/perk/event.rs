//! Decode the emitted event filters and masks independently of talent lowering.
use super::*;
mod ability_event;
mod accumulator;
mod damage_taken;
mod event_filter;
mod filter;
mod hit;
mod player;

pub(super) fn counter_evidence(native: &Payload) -> CheckResult<Value> {
    accumulator::evidence(native)
}

pub(super) fn incoming(
    reader: &Source,
    stock: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(native.u32(n - 4)?, 0x80803E3C);
    assert_eq!(native.bytes::<4>(n)?, [33, 1, 0, 0]);
    assert_eq!(source.bytes::<12>(s + 4)?, native.bytes::<12>(n + 4)?);
    filter::damage(reader, stock, source, s + 16, native, n + 16)?;
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
        30 | 31 => {
            assert_eq!(
                source.u32(s - 4)?,
                if kind == 30 { 0x808031FA } else { 0x80803174 }
            );
            assert_eq!(
                native.u32(n - 4)?,
                if kind == 30 { 0x80803DEC } else { 0x80803DEB }
            );
            assert_eq!(native.u8(n + 5)?, kind - 1);
            assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
            assert_eq!(native.u8(n + 6)?, 0);
            assert_eq!(source.u32(s + 8)?, native.u32(n + 8)?);
            return Ok(());
        }
        35 => {
            assert_eq!(source.u32(s - 4)?, 0x808030AE);
            assert_eq!(native.u32(n - 4)?, 0x80803DE4);
            assert_eq!(native.u8(n + 5)?, 34);
            assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
            assert_eq!(native.u8(n + 6)?, 0);
            assert_eq!(source.u64(s + 8)?, 0);
            assert_eq!(source.u32(s + 0x10)?, u32::MAX);
            assert_eq!(native.bytes::<16>(n + 8)?, source.bytes::<16>(s + 8)?);
            // Publication carries requested amount first and actual Float32 delta second.
            for offset in [0x18, 0x20] {
                let low = source.f32(s + offset)?;
                let high = source.f32(s + offset + 4)?;
                assert!(low.is_finite() && high.is_finite() && low <= high);
                assert_eq!(
                    source.bytes::<8>(s + offset)?,
                    native.bytes::<8>(n + offset)?
                );
            }
            return Ok(());
        }
        6 => {
            assert_eq!(source.u32(s - 4)?, 0x8080314D);
            assert_eq!(native.u32(n - 4)?, 0x80803DFB);
            assert_eq!(native.u8(n + 5)?, 6);
            assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
            assert_eq!(native.u8(n + 6)?, 0);
            let selected = source.u8(s + 8)?;
            assert_eq!(selected & !7, 0);
            assert_eq!(native.u8(n + 8)?, selected);
            // The pickup producer's first mask identifies recipient inventory slots.
            // Its independent second mask identifies ammo types and must remain unset.
            assert_eq!(native.bytes::<3>(n + 9)?, [0; 3]);
            return Ok(());
        }
        10 | 11 => return ability_event::check(reader, stock, source, s, native, n),
        33 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DF8);
            assert_eq!(native.u8(n + 5)?, 32);
            assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
            assert_eq!(source.u8(s + 6)?, native.u8(n + 6)?);
            8
        }
        34 => {
            assert_eq!(source.u32(s - 4)?, 0x808030ED);
            assert_eq!(native.u32(n - 4)?, 0x80803DE3);
            assert_eq!(native.u8(n + 5)?, 33);
            assert_eq!(source.bytes::<5>(s)?, native.bytes::<5>(n)?);
            assert_eq!(native.u8(n + 6)?, 0);
            assert!(source.u8(s + 8)? <= 1);
            // The original publishers send zero or one. Both callbacks compare
            // byte inequality, so lowering must preserve the stored polarity.
            assert_eq!(source.bytes::<4>(s + 8)?, native.bytes::<4>(n + 8)?);
            assert_eq!(native.bytes::<3>(n + 9)?, [0; 3]);
            return Ok(());
        }
        3 | 9 | 23 | 28 => return event_filter::check(reader, stock, source, s, native, n),
        32 => {
            assert_eq!(native.u32(n - 4)?, 0x80803E04);
            assert_eq!(native.u8(n + 5)?, 31);
            let groups = source.array(s + 8, 32, Some(0x80803181))?;
            let emitted = native.array(n + 8, 32, Some(0x80803E06))?;
            assert_eq!(groups.len(), emitted.len());
            let mut held = 0;
            for &row in &groups {
                held += usize::from(source.f32(row)? > 0.0);
            }
            let mut root_mask = 1u64 << 31;
            for (index, (s, n)) in groups.into_iter().zip(emitted).enumerate() {
                assert_eq!(source.u32(s + 4)?, native.u32(n + 4)?);
                if index < held {
                    // The source visitor creates one root-owned timer per positive
                    // row. Lookup selects those timers by the full group index.
                    let scaled = source.f32(s)? * 60.0;
                    let floor = scaled.floor();
                    let ticks = if scaled - floor > 0.5
                        || (scaled - floor == 0.5 && !(floor as u32).is_multiple_of(2))
                    {
                        floor + 1.0
                    } else {
                        floor
                    }
                    .max(1.0);
                    assert!((1.0..=32767.0).contains(&ticks));
                    assert_eq!(native.f32(n)?.to_bits(), (ticks / 60.0).to_bits());
                } else {
                    // A source suffix row has no timer even when its stored hold
                    // is positive. Keeping that duration would add native state.
                    assert_eq!(native.f32(n)?, 0.0);
                }
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
                root_mask |= mask;
            }
            for (descriptor, compiled) in [(0x20, 0x88), (0x48, 0x90), (0x58, 0x98)] {
                if native
                    .array(descriptor, 8, Some(0x808040BA))?
                    .into_iter()
                    .any(|row| native.pointer(row).ok() == Some(n))
                {
                    assert_eq!(native.u64(compiled)? & root_mask, root_mask);
                }
            }
            return Ok(());
        }
        36 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DCC);
            assert_eq!(native.u8(n + 5)?, 35);
            player::fields(reader, stock, source, s, native, n)?;
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
        20 => return player::check(reader, stock, source, s, native, n),
        12 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DEA);
            assert!(matches!(
                (source.u32(s + 8)?, source.u32(s + 12)?),
                (0x6CEC7A87, 0x18CCBF24) | (0xB3311944, 0xC7161F86)
            ));
            assert_eq!(source.bytes::<7>(s)?, native.bytes::<7>(n)?);
            assert_eq!(source.bytes::<8>(s + 8)?, native.bytes::<8>(n + 8)?);
            return Ok(());
        }
        26 => return accumulator::check(reader, stock, source, s, native, n),
        22 => {
            assert_eq!(native.u32(n - 4)?, 0x80803DF9);
            assert_eq!(source.bytes::<4>(s + 8)?, native.bytes::<4>(n + 8)?);
            return Ok(());
        }
        4 => return hit::check(reader, stock, source, s, native, n),
        5 => return damage_taken::check(reader, stock, source, s, native, n),
        7 | 8 => {
            assert_eq!(
                native.u32(n - 4)?,
                if kind == 7 { 0x80803DFD } else { 0x80803E01 }
            );
            assert_eq!(source.bytes::<8>(s + 8)?, native.bytes::<8>(n + 8)?);
            player::ability_ranges(source, s + 0x10, native, n + 0x10)?;
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
            filter::damage(reader, stock, source, s + 0x128, native, n + 0xB0)?;
            8
        }
        27 => {
            assert_eq!(native.u32(n - 4)?, 0x808029E0);
            assert_eq!(source.bytes::<2>(s + 8)?, native.bytes::<2>(n + 8)?);
            assert_eq!(native.u8(n + 9)? & !7, 0);
            assert_eq!(native.bytes::<32>(n + 0x70)?, [0; 32]);
            // The producers use opposite flag polarity, so both selectors still
            // mean 1 = miss and 2 = hit when evaluated against real packets.
            let mode = match source.u8(s + 0xe4)? {
                mode @ 0..=2 => mode,
                _ => return Err("Unqualified source event mode".into()),
            };
            assert_eq!(native.u8(n + 0x90)?, mode);
            0x10
        }
        _ => return Ok(()),
    };
    labels(reader, stock, source, s + base, native, n + base)
}

/// The external key keeps a stock publisher and never rewrites that publisher privately.
pub(super) fn duration_stock(
    source: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    key: u32,
) -> CheckResult<()> {
    assert_eq!(key, 0x3B210AE5);
    let original = source.read(0x80A752C8)?;
    let settings = original.pointer(24)?;
    assert_eq!(original.u32(settings - 4)?, 0x80802698);
    let duration = settings + 0x250;
    let policy = original.bytes::<20>(duration + 0xA0)?;
    assert_eq!(original.u32(duration + 0xA4)?, key);
    let hud = original.bytes::<8>(settings + 0x178)?;
    for (graph_tag, owner_tag) in [(0x80BEB776, 0x80C3DE2C), (0x80BC8307, 0x80BC49CC)] {
        let graph = emitted.tag(graph_tag, Some(0x80809C0F))?;
        assert!(
            graph
                .array(16, 12, Some(0x80809C04))?
                .into_iter()
                .any(|row| graph.u32(row).ok() == Some(owner_tag))
        );
        let owner = emitted.tag(owner_tag, Some(0x80809C36))?;
        assert_eq!(owner.0, stock.tag(owner_tag, Some(0x80809C36))?.0);
        let settings = owner.pointer(24)?;
        assert_eq!(owner.u32(settings - 4)?, 0x80804205);
        let duration = settings + 0x228;
        assert_eq!(owner.bytes::<20>(duration + 0x98)?, policy);
        assert_eq!(owner.bytes::<8>(settings + 0x160)?, hud);
        let instance = usize::try_from(owner.u64(duration + 8)?)?;
        assert_eq!(owner.u32(duration)?, owner_tag);
        assert_eq!(owner.u32(duration + 4)?, 0x80803B32);
        assert_eq!(owner.u32(instance + 4)?, 0x80803B33);
        assert_eq!(owner.u64(instance + 8)?, duration as u64);
        let provider = emitted.tag(owner.u32(duration + 0x58)?, Some(0x80809C54))?;
        assert_eq!(provider.u32(8)?, 0x80809ADE);
        assert_eq!(provider.u32(12)?, 0x80809ADD);
        let methods = provider.array(16, 24, None)?;
        assert_eq!(methods.len(), 1);
        assert_eq!(provider.u32(methods[0])?, 0x80803B32);
        assert_eq!(provider.u32(methods[0] + 4)?, 1);
        assert!(owner.array(0x30, 16, None)?.into_iter().any(|row| {
            owner.pointer(row).ok() == Some(instance)
                && owner.u32(row + 8).ok() == Some(0x80803B32)
                && owner.u32(row + 12).ok() == Some(0x20000)
        }));
    }
    Ok(())
}

/// Follow the emitted attachment to the actual Duration definition supplying its removal key.
pub(super) fn duration_end(
    emitted: &mut Reader,
    action: &Payload,
    condition: usize,
) -> CheckResult<()> {
    let key = action.u32(condition + 8)?;
    assert_ne!(key, 0x811C9DC5);
    let mut producers = 0;
    for row in action.array(0x38, 8, Some(0x808040AC))? {
        let at = action.pointer(row)?;
        if action.u8(at)? != 1 || action.u32(at + 24)? != key {
            continue;
        }
        assert_eq!(action.u8(at + 1)?, 1);
        assert!(action.u8(at + 2)? <= 1);
        assert_eq!(action.u32(at + 28)?, 0x811C9DC5);
        let graph = emitted.tag(action.u32(at + 16)?, Some(0x80809C0F))?;
        let owners = graph.array(16, 12, Some(0x80809C04))?;
        let tag = graph.u32(*owners.first().ok_or("Duration owner")?)?;
        let owner = emitted.tag(tag, Some(0x80809C36))?;
        let settings = owner.pointer(24)?;
        assert_eq!(owner.u32(settings - 4)?, 0x80804205);
        let definition = settings + 0x228;
        let instance = usize::try_from(owner.u64(definition + 8)?)?;
        assert_eq!(owner.u32(definition)?, tag);
        assert_eq!(owner.u32(definition + 4)?, 0x80803B32);
        assert_eq!(owner.u32(instance + 4)?, 0x80803B33);
        assert_eq!(owner.u64(instance + 8)?, definition as u64);
        assert_eq!(owner.u32(definition + 0x98)?, 9);
        assert_eq!(owner.u32(definition + 0x9C)?, key);
        producers += 1;
    }
    assert!(
        producers > 0,
        "Duration removal has no emitted status producer"
    );
    Ok(())
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
        let declared = source
            .array(s + operation * 16, 24, Some(0x80800365))?
            .into_iter()
            .map(|row| source.u32(row))
            .collect::<Result<Vec<_>>>()?;
        let emitted = native
            .array(n + operation * 16, 24, Some(0x808094B3))?
            .into_iter()
            .map(|row| native.u32(row))
            .collect::<Result<Vec<_>>>()?;
        let mut names = Vec::new();
        let mut original_mask = vec![0; 32];
        let mut expected = vec![0; 40];
        for name in declared {
            let bits = match original_dictionary.get(&name) {
                Some(bits) => bits,
                None => {
                    assert_eq!(name, 0x811C9DC5, "Unregistered source label");
                    continue;
                }
            };
            for (out, bits) in original_mask.iter_mut().zip(bits) {
                *out |= bits;
            }
            if bits.iter().all(|&bit| bit == 0) {
                continue;
            }
            names.push(name);
            for (out, bits) in expected.iter_mut().zip(&native_dictionary[&name]) {
                *out |= bits;
            }
        }
        // The source's empty flags describe compiled masks, including declarations
        // that contribute no bits. Every nonempty label keeps its symbolic identity.
        assert_eq!(
            names, emitted,
            "An effective event label or operation changed"
        );
        assert_eq!(
            source.0[s + 0x50 + operation * 32..s + 0x50 + (operation + 1) * 32],
            original_mask
        );
        assert_eq!(
            source.u8(s + 0xD0 + operation)?,
            u8::from(original_mask.iter().all(|&bit| bit == 0))
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
