//! Follow a private attachment through its component and compare the numeric contract.
use super::*;
mod connected;
mod hud;
mod nested;

/// Read actual source/emitted programs and compare the resulting modifiers. A source bank
/// may require several native drivers, but every original destination must occur exactly once.
pub(super) fn dynamic(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    effects: &[usize],
) -> CheckResult<usize> {
    use sundial::package_authoring::sandbox_perk::action::native::{Graph, value::Program};
    // Independent original-VM captures, including the before-first-knot zero and
    // nonuniform lane selection. These qualify the evaluator used by this readback.
    for (input, expected) in [
        ([-1.0; 4], 0.0f32),
        ([0.5; 4], 10.0),
        ([1.0, 2.0, 3.0, 4.0], 40.0),
        ([2.0, 1.0, 0.0, -1.0], 20.0),
    ] {
        use sundial::package_authoring::sandbox_perk::action::native::value::Instruction;
        let witness = Program {
            instructions: [(52, 5), (55, 0), (62, 0)]
                .map(|(opcode, operand)| Instruction {
                    opcode,
                    operand: Some(operand),
                })
                .to_vec(),
            constants: [
                [0.0; 4],
                [0.0; 4],
                [0.0; 4],
                [10.0, 20.0, 30.0, 40.0],
                [0.0, 1.0, 2.0, 3.0],
                input,
            ]
            .map(|v| v.map(f32::to_bits))
            .to_vec(),
            fast_path: 0,
        };
        assert_eq!(
            witness.evaluate(0.0).map(f32::to_bits),
            Some(expected.to_bits())
        );
    }
    let original_graph = reader.read(source.u32(s + 16)?)?;
    let owners = original_graph.array(32, 12, Some(0x80800715))?;
    assert_eq!(owners.len(), 1);
    let owner = reader.read(original_graph.u32(owners[0])?)?;
    let rows = owner.array(owner.pointer(24)? + 96, 96, Some(0x8080205F))?;
    let original = Program::read(&Graph::read(&source.0, s, 0x80803E44)?, 0, 32)?;
    let mut remaining = std::collections::BTreeMap::new();
    for row in rows {
        let component = owner.u8(row + 84)?;
        let destination = (
            component,
            ability(&owner, row)?,
            input(reader, stock, component, owner.u16(row + 82)?)?,
        );
        assert!(remaining.insert(destination, row).is_none());
    }
    let mut consumed = 0;
    let mut private_graphs = std::collections::BTreeSet::new();
    while !remaining.is_empty() {
        let n = native.pointer(*effects.get(consumed).ok_or("Missing dynamic driver")?)?;
        consumed += 1;
        assert_eq!(native.u32(n - 4)?, 0x80803E44);
        assert_eq!(native.u8(n)?, 2);
        assert_eq!(source.bytes::<3>(s + 1)?, native.bytes::<3>(n + 1)?);
        assert_eq!(source.bytes::<8>(s + 24)?, native.bytes::<8>(n + 24)?);
        assert_eq!(source.bytes::<24>(s + 64)?, native.bytes::<24>(n + 64)?);
        let driver = Program::read(&Graph::read(&native.0, n, 0x80803E44)?, 0, 32)?;
        let tag = native.u32(n + 16)?;
        assert!(stock.tag(tag, None).is_err() && private_graphs.insert(tag));
        let graph = emitted.tag(tag, Some(0x80809C0F))?;
        let owners = graph.array(16, 12, Some(0x80809C04))?;
        assert_eq!(owners.len(), 1);
        assert_eq!(graph.bytes::<32>(32)?, [0; 32]);
        let tag = graph.u32(owners[0])?;
        assert!(stock.tag(tag, None).is_err());
        let target = emitted.tag(tag, Some(0x80809C36))?;
        let rows = target.array(target.pointer(24)? + 88, 88, Some(0x80803B06))?;
        assert!(!rows.is_empty());
        for to in rows {
            let destination = (
                target.u8(to + 76)?,
                target.u16(to + 72)?,
                target.u16(to + 74)?,
            );
            let from = remaining
                .remove(&destination)
                .ok_or("Extra or duplicate modifier")?;
            let operation = owner.u8(from + 52)?;
            assert_eq!(target.u8(to + 44)?, operation);
            assert_eq!(target.u16(to + 72)?, ability(&owner, from)?);
            let amount = owner.f32(from + 48)?
                * unit(reader, stock, owner.u8(from + 84)?, owner.u16(from + 82)?)?;
            let factor = target.f32(to + 40)?;
            let mut inputs = vec![
                -128.0, -1.0, 0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0, 2.0, 3.0, 10.0, 17.999, 18.0,
                18.001, 32.4, 35.999, 36.0, 36.001, 44.0, 100.0, 127.0,
            ];
            // Exercise the original curve's stored breakpoints and both adjacent sides.
            for value in original
                .constants
                .iter()
                .flatten()
                .map(|&b| f32::from_bits(b))
            {
                if value.is_finite() && value.abs() <= 128.0 {
                    inputs.extend([value - 0.00001, value, value + 0.00001]);
                }
            }
            for input in inputs {
                let weight = original
                    .evaluate(input)
                    .ok_or("Source expression evaluation")?;
                let lowered = driver
                    .evaluate(input)
                    .ok_or("Emitted expression evaluation")?;
                for value in [0.0f32, 0.25, 1.0, 12.0, 100.0] {
                    let change = amount * weight;
                    let expected = if operation == 0 {
                        value + change
                    } else if change > 0.0 {
                        value * (1.0 + change)
                    } else {
                        value / (1.0 - change)
                    };
                    let actual = if operation == 0 {
                        value + factor * lowered
                    } else {
                        value * (1.0 + (factor - 1.0) * lowered)
                    };
                    assert!((actual - expected).abs() <= 2e-5 * expected.abs().max(1.0));
                }
            }
            let live = usize::try_from(target.u64(to + 8)?)?;
            assert_eq!(target.u32(live)?, tag);
            assert_eq!(target.u64(live + 8)?, to as u64);
            assert_eq!(target.f32(live + 64)?, 1.0);
            assert_eq!(target.u8(live + 68)?, 1);
            assert_eq!(target.u64(to + 32)?, 0);
            assert_eq!(target.u64(to + 64)?, 1);
        }
    }
    Ok(consumed)
}

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(source.u8(s)?, 0);
    assert_eq!(native.u8(n)?, 1);
    assert_eq!(source.bytes::<3>(s + 1)?, native.bytes::<3>(n + 1)?);
    assert_eq!(source.bytes::<24>(s + 24)?, native.bytes::<24>(n + 24)?);
    let graph = reader.read(source.u32(s + 16)?)?;
    let owners = graph.array(32, 12, Some(0x80800715))?;
    if owners.len() == 1 {
        let owner = reader.read(graph.u32(owners[0])?)?;
        if owner.u32(owner.pointer(24)? - 4)? == 0x80802A94 {
            return nested::check(reader, stock, emitted, &graph, native.u32(n + 16)?);
        }
    }
    if owners.len() == 2 {
        let owner = reader.read(graph.u32(owners[0])?)?;
        if owner.u32(owner.pointer(24)? - 4)? == 0x808029C3 {
            return connected::check(reader, stock, emitted, &graph, native.u32(n + 16)?);
        }
        let companion = reader.read(graph.u32(owners[1])?)?;
        if companion.u32(companion.pointer(24)? - 4)? == 0x808020DE {
            let tag = native.u32(n + 16)?;
            status(reader, stock, emitted, &owner, tag, 2)?;
            let converted = emitted.tag(tag, Some(0x80809C0F))?;
            let targets = converted.array(16, 12, Some(0x80809C04))?;
            assert_eq!(targets.len(), 2);
            let tag = converted.u32(targets[1])?;
            assert!(stock.tag(tag, None).is_err());
            let target = emitted.tag(tag, Some(0x80809C36))?;
            return modifier_rows(
                reader,
                stock,
                &companion,
                &target,
                tag,
                companion.array(companion.pointer(24)? + 96, 96, Some(0x8080205F))?,
            );
        }
        return conditional_status(reader, stock, emitted, &graph, native.u32(n + 16)?);
    }
    assert_eq!(owners.len(), 1);
    let owner = reader.read(graph.u32(owners[0])?)?;
    if owner.u32(owner.pointer(24)? - 4)? == 0x80802698 {
        return status(reader, stock, emitted, &owner, native.u32(n + 16)?, 1);
    }
    let rows = owner.array(owner.pointer(24)? + 96, 96, Some(0x8080205F))?;
    let graph_tag = native.u32(n + 16)?;
    assert!(
        stock.tag(graph_tag, None).is_err(),
        "Attachment must be private"
    );
    let graph = emitted.tag(graph_tag, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    assert_eq!(owners.len(), 1, "Attachment acquired a companion effect");
    let target_tag = graph.u32(owners[0])?;
    assert!(
        stock.tag(target_tag, None).is_err(),
        "Modifier must be private"
    );
    let target = emitted.tag(target_tag, Some(0x80809C36))?;
    modifier_rows(reader, stock, &owner, &target, target_tag, rows)
}

fn modifier_rows(
    reader: &Source,
    stock: &mut Reader,
    owner: &Payload,
    target: &Payload,
    target_tag: u32,
    rows: Vec<usize>,
) -> CheckResult<()> {
    let settings = target.array(target.pointer(24)? + 88, 88, Some(0x80803B06))?;
    assert_eq!(rows.len(), settings.len());
    for (from, to) in rows.into_iter().zip(settings) {
        assert_eq!(owner.u8(from + 84)?, target.u8(to + 76)?);
        assert_eq!(
            input(reader, stock, owner.u8(from + 84)?, owner.u16(from + 82)?)?,
            target.u16(to + 74)?
        );
        assert_eq!(target.u16(to + 72)?, ability(owner, from)?);
        let op = owner.u8(from + 52)?;
        assert_eq!(op, target.u8(to + 44)?);
        let amount = owner.f32(from + 48)?
            * unit(reader, stock, owner.u8(from + 84)?, owner.u16(from + 82)?)?;
        let factor = target.f32(to + 40)?;
        // Compare actual arithmetic over several base values, independently of
        // the converter's factor encoder. The negative branch divides in D1.
        for weight in [1.0] {
            for value in [0.0f32, 0.25, 1.0, 12.0, 100.0] {
                let change = amount * weight;
                let expected = if op == 0 {
                    value + change
                } else if change > 0.0 {
                    value * (1.0 + change)
                } else {
                    value / (1.0 - change)
                };
                let actual = if op == 0 {
                    value + factor * weight
                } else {
                    value * (1.0 + (factor - 1.0) * weight)
                };
                assert!((actual - expected).abs() <= 2e-5 * expected.abs().max(1.0));
            }
        }
        let live = usize::try_from(target.u64(to + 8)?)?;
        assert_eq!(target.u32(live)?, target_tag);
        assert_eq!(target.u64(live + 8)?, to as u64);
        assert_eq!(target.f32(live + 64)?, 1.0);
        assert_eq!(target.u8(live + 68)?, 1);
        assert_eq!(target.u64(to + 32)?, 0);
        assert_eq!(target.u64(to + 64)?, 1);
    }
    Ok(())
}

/// Original dispatchers select a signed ability slot before selecting its numeric input.
/// Other component interfaces ignore that field and use the native unused sentinel.
fn ability(owner: &Payload, row: usize) -> CheckResult<u16> {
    if owner.u8(row + 84)? == 9 {
        let slot = owner.u16(row + 80)?;
        assert!(slot <= 2, "Unqualified ability slot");
        Ok(slot)
    } else {
        assert_eq!(owner.u16(row + 80)?, 0);
        Ok(u16::MAX)
    }
}

/// Independent native layout oracles for the ordinal changes. Read the actual
/// stock modifier rows for precision, directional recoil and paired flinch.
fn input(reader: &Source, stock: &mut Reader, component: u8, source: u16) -> CheckResult<u16> {
    if component == 10 {
        assert!(matches!(source, 5 | 6));
        // Original provider records join these selectors to the concrete numeric
        // getter. The final timing consumers use the smaller of inputs five and six.
        for (provider, interface, implementation) in [
            (
                std::sync::Arc::new(reader.read(0x80A74685)?),
                0x80802764,
                0x8080246C,
            ),
            (
                stock.tag(0x80BB274F, Some(0x80809C54))?,
                0x80803B56,
                0x80803B53,
            ),
        ] {
            assert_eq!(provider.u32(12)?, interface);
            let methods = provider.array(16, 24, None)?;
            assert_eq!(methods.len(), 6);
            assert_eq!(provider.u32(methods[0])?, implementation);
            assert_eq!(provider.u32(methods[0] + 4)?, 2);
            assert_eq!(provider.u32(methods[2])?, implementation);
            assert_eq!(provider.u32(methods[2] + 4)?, 4);
        }
        return Ok(source);
    }
    if component == 2 && matches!(source, 30 | 38 | 39 | 45 | 47 | 49) {
        return compiled_input(reader, stock, source);
    }
    // These owners are reached from the native finished perk catalog, including
    // Fan Fire, Thunderer, Zen Moment, Dornröschen and Strange Protractor.
    let witness = match (component, source) {
        (0, 5) => Some((0x80BC57F5, 0)),
        (0, 6) => Some((0x80BC57F5, 1)),
        (2, 11) => Some((0x80FEF7D1, 8)),
        (2, 13) => Some((0x80FEF7D1, 9)),
        (2, 34) => Some((0x81A699CE, 6)),
        (2, 43) => Some((0x80FEF7BC, 6)),
        (2, 50) => Some((0x80BC5823, 0)),
        (2, 51) => Some((0x80FEF7BC, 7)),
        (2, 52) => Some((0x80BC58A9, 2)),
        (2, 60) => Some((0x80FEF7BE, 1)),
        (5, 2) => Some((0x80BC57D6, 3)),
        (5, 6) => Some((0x80BB7424, 2)),
        (5, 7) => Some((0x80BB7424, 3)),
        (5, 9) => Some((0x80BC57D6, 0)),
        (9, 0) => Some((0x80FEF68E, 0)),
        (13, 1) => Some((0x81532C99, 0)),
        (14, 15) => Some((0x80FF05F5, 1)),
        _ => None,
    };
    if let Some((tag, index)) = witness {
        let owner = stock.tag(tag, Some(0x80809C36))?;
        let rows = owner.array(owner.pointer(24)? + 88, 88, Some(0x80803B06))?;
        assert_eq!(owner.u8(rows[index] + 76)?, component);
        return Ok(owner.u16(rows[index] + 74)?);
    }
    let (graph, index) = match (component, source) {
        (2, 33) => (0x80BC5847, 0),
        (2, 42) => (0x80BC58B3, 0),
        (2, 44) => (0x80BC58B3, 2),
        (2, 46) => (0x80BC58B3, 1),
        (2, 48) => (0x80BC58B3, 3),
        (2, 57) => (0x80BC584D, 0),
        (2, 58) => (0x80BC584D, 1),
        _ => return Ok(source),
    };
    let graph = stock.tag(graph, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    assert_eq!(owners.len(), 1);
    let owner = stock.tag(graph.u32(owners[0])?, Some(0x80809C36))?;
    let rows = owner.array(owner.pointer(24)? + 88, 88, Some(0x80803B06))?;
    assert_eq!(owner.u8(rows[index] + 76)?, component);
    Ok(owner.u16(rows[index] + 74)?)
}

/// Original compiled stat programs independently identify these destination inputs.
/// No importer mapping or authored recipe supplies the expected native ordinal.
fn compiled_input(reader: &Source, stock: &mut Reader, input: u16) -> CheckResult<u16> {
    let (source_tag, native_tag) = if matches!(input, 38 | 39) {
        (0x80AA2D68, 0x8152A4F5)
    } else {
        (0x80AA2D0A, 0x80FF0147)
    };
    let source = reader.read(source_tag)?;
    let native = stock.tag(native_tag, Some(0x80809C36))?;
    let from = compiled_entries(&source, 0x80800184, 0x80802945, 0x80802013)?;
    let to = compiled_entries(&native, 0x80809FBD, 0x80803975, 0x80803981)?;
    let speed = if input == 38 {
        Some(compiled_input(reader, stock, 39)?)
    } else {
        None
    };
    let mut destinations = BTreeSet::new();
    for (mut row, stat) in from
        .into_iter()
        .filter(|(r, _)| r[0] == 2 && r[1] == u32::from(input))
    {
        if let Some(speed) = speed {
            assert_eq!(
                (row[3], row[4], row[5], row[13]),
                (2, 2, 39, 0.5f32.to_bits())
            );
            row[5] = u32::from(speed);
        }
        // Entry order and destination ordinal are the only independent fields.
        row[1] = 0;
        row[19] = 0;
        let mut matched = false;
        for (candidate, target_stat) in &to {
            let mut body = *candidate;
            let destination = body[1];
            body[1] = 0;
            body[19] = 0;
            if row == body && stat == *target_stat {
                destinations.insert(u16::try_from(destination)?);
                matched = true;
            }
        }
        assert!(matched, "Original stat route has no native equivalent");
    }
    assert_eq!(
        destinations.len(),
        1,
        "Stat translator does not uniquely identify input {input}"
    );
    Ok(*destinations.first().unwrap())
}

fn compiled_entries(
    owner: &Payload,
    marker: u32,
    group: u32,
    entry: u32,
) -> CheckResult<Vec<([u32; 20], Option<u32>)>> {
    let mut entries = Vec::new();
    for at in (12..owner.0.len().saturating_sub(24)).step_by(4) {
        if owner.u32(at)? != group || owner.u32(at - 12)? != marker {
            continue;
        }
        let count = usize::try_from(owner.u64(at - 8)?)?;
        assert!((1..32).contains(&count));
        for group in (at + 8..at + 8 + count * 48).step_by(48) {
            let stats = owner.array(group, 56, None)?;
            for row in owner.array(group + 16, 80, Some(entry))? {
                let mut words = [0; 20];
                for (index, word) in words.iter_mut().enumerate() {
                    *word = owner.u32(row + index * 4)?;
                }
                let stat = words[7]
                    .checked_sub(0x1E00)
                    .and_then(|channel| stats.get(channel as usize / 32))
                    .map(|&row| owner.u32(row + 32))
                    .transpose()?;
                entries.push((words, stat));
            }
        }
    }
    assert!(!entries.is_empty());
    Ok(entries)
}

fn unit(reader: &Source, stock: &mut Reader, component: u8, input: u16) -> CheckResult<f32> {
    if (component, input) != (13, 1) {
        return Ok(1.0);
    }
    // The two MIDA banks provide an independent original value oracle. Comparing
    // emitted deltas in their native units catches accidentally copying two tiers
    // as two points, including inside a retained dynamic driver.
    let source = reader.read(0x80A74B1A)?;
    let target = stock.tag(0x81532C99, Some(0x80809C36))?;
    let s = source.array(source.pointer(24)? + 96, 96, Some(0x8080205F))?[0];
    let n = target.array(target.pointer(24)? + 88, 88, Some(0x80803B06))?[0];
    assert_eq!((source.u8(s + 84)?, source.u16(s + 82)?), (13, 1));
    assert_eq!((target.u8(n + 76)?, target.u16(n + 74)?), (13, 1));
    assert_eq!((source.u8(s + 52)?, target.u8(n + 44)?), (0, 0));
    assert_eq!((source.f32(s + 48)?, target.f32(n + 40)?), (2.0, 20.0));
    Ok(target.f32(n + 40)? / source.f32(s + 48)?)
}

fn host_competition(source: &Payload, target: &Payload) -> CheckResult<()> {
    let s = source.pointer(24)?;
    let n = target.pointer(24)?;
    let priority = i32::from_le_bytes(source.bytes::<4>(s + 0x17C)?);
    assert_eq!(target.u8(n + 0x58)?, source.u8(s + 0xB0)?);
    assert_eq!(target.bytes::<3>(n + 0x59)?, [0; 3]);
    assert_eq!(target.f32(n + 0x5C)?, priority as f32);
    let instance = usize::try_from(target.u64(n + 0x50)?)?;
    assert_eq!(target.u32(n + 0x4C)?, 0x80803A60);
    assert_eq!(target.u32(instance + 4)?, 0x80803A61);
    assert_eq!(target.u64(instance + 8)?, (n + 0x48) as u64);
    Ok(())
}

fn status(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source: &Payload,
    graph: u32,
    owner_count: usize,
) -> CheckResult<()> {
    assert!(stock.tag(graph, None).is_err());
    let graph = emitted.tag(graph, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    assert_eq!(owners.len(), owner_count);
    let tag = graph.u32(owners[0])?;
    assert!(stock.tag(tag, None).is_err());
    let target = emitted.tag(tag, Some(0x80809C36))?;
    host_competition(source, &target)?;
    let from = source.pointer(24)? + 0xF0;
    let to = target.pointer(24)? + 0xE0;
    assert_eq!(target.u32(target.pointer(24)? - 4)?, 0x80804205);
    assert_eq!(target.u32(to + 4)?, 0x80804211);
    hud::check(
        reader,
        stock,
        emitted,
        source.u32(from + 0x88)?,
        target.u32(to + 0x80)?,
    )?;
    assert_eq!(
        source.bytes::<8>(from + 0x8C)?,
        target.bytes::<8>(to + 0x84)?
    );
    assert_eq!(target.u32(to + 0xE8)?, 0x811C9DC5);
    assert_eq!(target.u8(to + 0xEC)?, 0);
    let duration_from = from + 0x160;
    let duration_to = to + 0x148;
    assert_eq!(
        source.bytes::<20>(duration_from + 0xA0)?,
        target.bytes::<20>(duration_to + 0x98)?
    );
    assert_eq!(
        source.u64(duration_from + 0x130)?,
        target.u64(duration_to + 0x120)?
    );
    // The actual emitted count and lifetime programs, including their constants, determine
    // display and cleanup. Follow each descriptor instead of consulting recipe patches.
    for (s, n) in [
        (from + 0xA0, to + 0x98),
        (duration_from + 0x138, duration_to + 0x128),
    ] {
        let sc = source.array(s + 0x18, 1, Some(0x80800009))?;
        let nc = target.array(n + 0x10, 1, Some(0x80800009))?;
        assert_eq!(sc.len(), 4);
        assert_eq!(nc.len(), 4);
        for (s, n) in sc.into_iter().zip(nc) {
            assert_eq!(source.u8(s)?, target.u8(n)?);
        }
        let sv = source.array(s + 0x28, 16, Some(0x80800090))?;
        let nv = target.array(n + 0x20, 16, Some(0x80800090))?;
        assert_eq!(sv.len(), 1);
        assert_eq!(nv.len(), 1);
        assert_eq!(source.bytes::<16>(sv[0])?, target.bytes::<16>(nv[0])?);
    }
    let live = usize::try_from(target.u64(to + 8)?)?;
    assert_eq!(target.u32(live)?, tag);
    assert_eq!(target.u64(live + 8)?, to as u64);
    let duration_live = live + 0x20;
    assert_eq!(target.u64(duration_live + 8)?, duration_to as u64);
    // Both native execution outputs stay unbound. They must not acquire gameplay receivers.
    for at in graph.array(32, 72, Some(0x80809BC9))? {
        assert_eq!(graph.u32(at + 40)?, u32::MAX);
        assert_eq!(graph.u64(at + 48)?, 0);
    }
    Ok(())
}

fn conditional_status(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source_graph: &Payload,
    tag: u32,
) -> CheckResult<()> {
    assert!(stock.tag(tag, None).is_err());
    let graph = emitted.tag(tag, Some(0x80809C0F))?;
    let source_owners = source_graph.array(32, 12, Some(0x80800715))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    assert_eq!(owners.len(), 2);
    let mut status = None;
    let mut channel = None;
    for (s, n) in source_owners.into_iter().zip(owners) {
        let source = reader.read(source_graph.u32(s)?)?;
        let tag = graph.u32(n)?;
        assert!(stock.tag(tag, None).is_err());
        let target = emitted.tag(tag, Some(0x80809C36))?;
        let a = source.pointer(24)?;
        let b = target.pointer(24)?;
        if source.u32(a - 4)? == 0x80802698 {
            host_competition(&source, &target)?;
            assert_eq!(target.u32(b - 4)?, 0x80804205);
            let (h, k) = (a + 0xF0, b + 0xE0);
            hud::check(
                reader,
                stock,
                emitted,
                source.u32(h + 0x88)?,
                target.u32(k + 0x80)?,
            )?;
            assert_eq!(source.bytes::<8>(h + 0x8C)?, target.bytes::<8>(k + 0x84)?);
            assert_eq!(
                source.bytes::<20>(h + 0x200)?,
                target.bytes::<20>(k + 0x1E0)?
            );
            for (s, n) in [
                (h + 0xA0, k + 0x98),
                (h + 0x108, k + 0xF8),
                (h + 0x298, k + 0x270),
            ] {
                for (delta, stride, class) in [(24, 1, 0x80800009), (40, 16, 0x80800090)] {
                    let left = source.array(s + delta, stride, Some(class))?;
                    let right = target.array(n + delta - 8, stride, Some(class))?;
                    assert_eq!(left.len(), right.len());
                    for (s, n) in left.into_iter().zip(right) {
                        assert_eq!(&source.0[s..s + stride], &target.0[n..n + stride]);
                    }
                }
            }
            let inputs = target.array(k + 0x98 + 64, 40, Some(0x80809789))?;
            assert_eq!(inputs.len(), 1);
            let original_inputs = source.array(h + 0xA0 + 72, 48, Some(0x80800927))?;
            assert_eq!(original_inputs.len(), 1);
            assert_eq!(
                source.u32(original_inputs[0] + 40)?,
                target.u32(inputs[0] + 32)?
            );
            status = Some((tag, inputs[0]));
        } else {
            assert_eq!(source.u32(a - 4)?, 0x808008F6);
            assert_eq!(target.u32(b - 4)?, 0x80809790);
            assert_eq!(source.u32(a + 0x128)?, target.u32(b + 0x128)?);
            for (offset, stride, source_class, target_class) in [
                (0xD8, 112, 0x80800883, 0x808097A1),
                (0x108, 12, 0x8080087B, 0x808097A0),
            ] {
                let left = source.array(a + offset, stride, Some(source_class))?;
                let right = target.array(b + offset, stride, Some(target_class))?;
                assert_eq!(left.len(), 1);
                assert_eq!(right.len(), 1);
                if stride == 12 {
                    assert_eq!(source.bytes::<12>(left[0])?, target.bytes::<12>(right[0])?);
                } else {
                    assert_eq!(source.u32(left[0])?, target.u32(right[0])?);
                    assert_eq!(target.u64(right[0] + 8)?, 0);
                    assert_eq!(target.u16(right[0] + 72)?, 0);
                }
            }
            channel = Some((tag, b + 0x60));
        }
    }
    let (status_tag, input) = status.ok_or("Missing status owner")?;
    let (channel_tag, receiver) = channel.ok_or("Missing channel owner")?;
    let connections = graph.array(32, 72, Some(0x80809BC9))?;
    let mut linked = 0;
    for at in connections {
        if graph.u32(at + 40)? == u32::MAX {
            assert_eq!(graph.u64(at + 48)?, 0);
        } else {
            assert_eq!(graph.u32(at + 8)?, status_tag);
            assert_eq!(graph.u64(at + 16)?, input as u64);
            assert_eq!(graph.u32(at + 40)?, channel_tag);
            assert_eq!(graph.u64(at + 48)?, receiver as u64);
            assert_eq!(graph.u64(at + 56)?, 1);
            linked += 1;
        }
    }
    assert_eq!(linked, 1);
    Ok(())
}
