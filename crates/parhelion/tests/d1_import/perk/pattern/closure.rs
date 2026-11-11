//! Follow a graph attached whole from the source attachment to its staged native graph.
//!
//! Both builds build these graphs from shared framework owners (network, effect context,
//! category, invisibility) plus private effect sequences and sometimes a private bank, status,
//! Duration host or modifier bank. The victim explosion form's sequence holds an area query and
//! object creation events. Other forms create graphs of their own, as Outbreak Prime's Virulence
//! does, or apply a profile to their host, as Red Death's heal does. Native events are paired with source events by walking
//! the source tree depth first, the order the native tree is allocated in, because authored node
//! names repeat.
use super::*;

/// Shared framework owners by role, source then native. The last is the shared invisibility
/// owner, which acts on the host itself.
const FRAMEWORK: [(u32, u32); 4] = [
    (0x80AA_CEA6, 0x80C7_09E1),
    (0x80AA_D54B, 0x80C7_0B84),
    (0x80AA_D0F9, 0x80C7_0C0E),
    (0x80AA_2EA8, 0x80B9_FEC4),
];

/// Whether a source graph has the explosion shape this checker owns.
pub(super) fn matches(reader: &Source, graph: &Payload) -> CheckResult<bool> {
    let owners = graph.array(32, 12, Some(0x80800715))?;
    let tags = owners
        .iter()
        .map(|&row| graph.u32(row))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut sequences = 0;
    for &tag in &tags {
        let owner = reader.read(tag)?;
        if owner.u32(owner.pointer(24)? - 4)? == 0x80800610 {
            sequences += 1;
        }
    }
    Ok(sequences == 1
        && FRAMEWORK[..3]
            .iter()
            .all(|(source, _)| tags.contains(source)))
}

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    graph: &Payload,
    native_tag: u32,
) -> CheckResult<()> {
    assert!(
        stock.tag(native_tag, None).is_err(),
        "Attached graph must be private"
    );
    let native = emitted.tag(native_tag, Some(0x80809C0F))?;
    // A state selector that no connection names publishes to nobody and is left out.
    let mut owners = Vec::new();
    for row in graph.array(32, 12, Some(0x80800715))? {
        let owner = reader.read(graph.u32(row)?)?;
        if owner.u32(owner.pointer(24)? - 4)? != 0x80802818 {
            owners.push(row);
        }
    }
    let targets = native.array(16, 12, Some(0x80809C04))?;
    assert_eq!(owners.len(), targets.len(), "Attached graph owner count");
    let mut acts = 0;
    for (&from, &to) in owners.iter().zip(&targets) {
        let source_tag = graph.u32(from)?;
        let tag = native.u32(to)?;
        if let Some(&(_, shared)) = FRAMEWORK.iter().find(|(s, _)| *s == source_tag) {
            // A replicated graph names an unchanged private copy, which differs from the stock
            // owner only in the words naming itself. The stock owner stays unmodified.
            let original = stock.tag(shared, Some(0x80809C36))?;
            assert_eq!(
                emitted.tag(shared, Some(0x80809C36))?.0,
                original.0,
                "Shared framework owner was modified"
            );
            if tag != shared {
                let staged = emitted.tag(tag, Some(0x80809C36))?;
                assert!(
                    stock.tag(tag, None).is_err() && staged.0.len() == original.0.len(),
                    "Framework owner {source_tag:08X} copy is not private"
                );
                for at in (0..original.0.len().saturating_sub(3)).step_by(4) {
                    let (a, b) = (original.u32(at)?, staged.u32(at)?);
                    assert!(
                        a == b || (a == shared && b == tag) || at < 8,
                        "Framework owner {source_tag:08X} copy differs at {at:X}"
                    );
                }
            }
            if source_tag == 0x80AA_2EA8 {
                acts += 1;
            }
            continue;
        }
        assert!(
            stock.tag(tag, None).is_err(),
            "Attached owner must be private"
        );
        let original = reader.read(source_tag)?;
        let class = original.u32(original.pointer(24)? - 4)?;
        let target = emitted.tag(tag, Some(0x80809C36))?;
        match class {
            0x80800610 => acts += sequence(reader, stock, emitted, &original, &target, tag)?,
            0x808008F6 => assert_eq!(target.u32(target.pointer(24)? - 4)?, 0x80809790),
            // A timed status and its modifier bank act on the host themselves.
            0x80802698 => {
                assert_eq!(target.u32(target.pointer(16)? - 4)?, 0x80804209);
                acts += 1;
            }
            0x808020DE => {
                assert_eq!(target.u32(target.pointer(16)? - 4)?, 0x80803AFD);
                acts += 1;
            }
            0x808029C3 => super::duration::check(&original, &target)?,
            _ => return Err(format!("Unexpected attached owner {class:08X}").into()),
        }
    }
    assert!(
        acts > 0,
        "Translated graph neither queries, creates nor applies a status"
    );
    Ok(())
}

/// Source events in depth-first tree order, the order native allocation uses.
fn events(original: &Payload) -> CheckResult<Vec<usize>> {
    let d = original.pointer(24)?;
    let tables = [0x160, 0x170, 0x180]
        .into_iter()
        .map(|field| {
            original
                .array(d + field, 8, Some(0x80800629))?
                .into_iter()
                .map(|row| original.pointer(row))
                .collect::<std::result::Result<Vec<_>, _>>()
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut order = Vec::new();
    let mut stack = vec![(0usize, 0usize)];
    while let Some((domain, index)) = stack.pop() {
        let at = tables[domain][index];
        if domain != 0 {
            order.push(at);
            continue;
        }
        let rows = original.array(at + 0x20, 4, Some(0x8080052C))?;
        for row in rows.into_iter().rev() {
            stack.push((
                usize::from(original.u16(row)?),
                usize::from(original.u16(row + 2)?),
            ));
        }
    }
    assert_eq!(
        order.len(),
        tables[1].len() + tables[2].len(),
        "Source tree reaches every event once"
    );
    Ok(order)
}

fn sequence(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    original: &Payload,
    owner: &Payload,
    owner_tag: u32,
) -> CheckResult<usize> {
    let n = owner.pointer(24)?;
    assert_eq!(owner.u32(n - 4)?, 0x808084E9);
    let source_events = events(original)?;
    let native_events = owner
        .array(n + 0x168, 24, Some(0x808093E6))?
        .into_iter()
        .map(|row| owner.pointer(row + 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(source_events.len(), native_events.len());
    let mut acts = 0;
    for (&from, &to) in source_events.iter().zip(&native_events) {
        // Name, parent and the four timing values keep their source bytes.
        assert_eq!(original.bytes::<8>(from)?, owner.bytes::<8>(to + 0x10)?);
        assert_eq!(
            original.bytes::<16>(from + 12)?,
            owner.bytes::<16>(to + 0x1C)?
        );
        let live = usize::try_from(owner.u64(to + 8)?)?;
        assert_eq!(owner.u32(live)?, owner_tag);
        // A sealed owner keeps its original records and appends a copy with the definition view,
        // so the live record may name the original of this event, byte for byte the same.
        let named = usize::try_from(owner.u64(live + 8)?)?;
        assert!(
            named == to
                || (owner.0.get(named..named + 0x30).is_some()
                    && owner.0.get(named..named + 0x30) == owner.0.get(to..to + 0x30)),
            "Live event record names another event"
        );
        let flags = original.u32(from + 8)?;
        match original.u32(from - 4)? {
            0x808028A6 => {
                // The native completion byte and flag bit 10 travel together on every stock
                // query. Rise of Iron completes an empty query without either.
                assert_eq!(owner.u32(to + 0x18)?, flags | 0x10);
                query(reader, stock, emitted, original, from, owner, to)?;
                acts += 1;
            }
            0x808003BB => {
                assert_eq!(owner.u32(to + 0x18)?, flags);
                spawn(reader, stock, emitted, original, from, owner, to)?;
                acts += 1;
            }
            0x8080237E => {
                assert_eq!(owner.u32(to + 0x18)?, flags);
                super::table::check(reader, stock, emitted, original, from, owner, to)?;
            }
            0x80802402 => {
                assert_eq!(owner.u32(to + 0x18)?, flags);
                super::profile::check(reader, stock, emitted, original, from, owner, to)?;
                acts += 1;
            }
            _ => assert_eq!(owner.u32(to + 0x18)?, flags),
        }
    }
    Ok(acts)
}

/// A Rise of Iron program and its native counterpart run the same bytecode and constants.
/// Rise of Iron lists each builtin input as `FFFFFFFF`. Native supplies builtins implicitly.
fn program(
    original: &Payload,
    from: usize,
    owner: &Payload,
    to: usize,
    raw: bool,
) -> CheckResult<()> {
    if original.bytes::<48>(from)? == [0; 48] {
        assert_eq!(owner.bytes::<48>(to)?, [0; 48]);
        return Ok(());
    }
    let bytes = |p: &Payload, at: usize| -> CheckResult<Vec<u8>> {
        p.array(at, 1, Some(0x80800009))?
            .into_iter()
            .map(|row| p.u8(row).map_err(Into::into))
            .collect()
    };
    let constants = |p: &Payload, at: usize| -> CheckResult<Vec<[u8; 16]>> {
        p.array(at + 16, 16, Some(0x80800090))?
            .into_iter()
            .map(|row| p.bytes::<16>(row).map_err(Into::into))
            .collect()
    };
    assert_eq!(bytes(original, from)?, bytes(owner, to)?);
    assert_eq!(constants(original, from)?, constants(owner, to)?);
    assert_eq!(original.u64(from + 32)?, owner.u64(to + 32)?);
    assert_eq!(original.u32(from + 40)?, 1);
    assert_eq!(owner.u64(to + 40)?, 1);
    if !raw {
        let slots = original.array(from + 48, 4, Some(0x80800007))?;
        assert_eq!(slots.len() as u64, original.u64(from + 32)?);
        for slot in slots {
            assert_eq!(original.u32(slot)?, u32::MAX);
        }
        assert!(owner.array(to + 48, 4, Some(0x80800007))?.is_empty());
    }
    Ok(())
}

fn query(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    original: &Payload,
    s: usize,
    owner: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(owner.u32(n - 4)?, 0x80804AB9);
    assert_eq!(owner.u32(n + 0x30)?, 29);
    assert_eq!(
        original.u32(s + 0x20)?,
        owner.u32(n + 0x40)?,
        "Query locator"
    );
    // Geometry: the source sphere's radius and bound scale on the native sphere pair.
    let shape = original.pointer(s + 0x28)?;
    assert_eq!(original.u32(shape - 4)?, 0x8080250A);
    let sphere = owner.pointer(n + 0x48)?;
    assert_eq!(owner.u32(sphere - 4)?, 0x808092B1);
    assert_eq!(owner.u32(sphere)?, 2);
    assert_eq!(original.f32(shape + 0xA8)?, owner.f32(sphere + 4)?);
    let bound = owner.pointer(sphere + 8)?;
    assert_eq!(owner.u32(bound - 4)?, 0x808092C3);
    let scale = if original.u64(shape + 0xB0)? == 0 {
        1.0
    } else {
        original.f32(original.pointer(shape + 0xB0)? + 0x80)?
    };
    assert_eq!(scale, owner.f32(bound + 0x60)?);
    // Broadphase block, divisor and budgets.
    assert_eq!(
        original.bytes::<48>(s + 0x30)?,
        owner.bytes::<48>(n + 0x70)?
    );
    assert_eq!(
        original.bytes::<12>(s + 0xEC)?,
        owner.bytes::<12>(n + 0xA4)?
    );
    // An empty relationship exclusion and label filter has no native predicate.
    assert_eq!(original.u8(s + 0x61)?, 0);
    assert_eq!(original.f32(s + 0x64)?, 1.0);
    assert_eq!(original.bytes::<128>(s + 0x68)?, [0; 128]);
    assert_eq!(owner.bytes::<32>(n + 0x50)?, [0; 32]);
    program(original, s + 0xF8, owner, n + 0xB0, true)?;
    program(original, s + 0x128, owner, n + 0xE0, false)?;
    program(original, s + 0x168, owner, n + 0x120, false)?;
    assert_eq!(
        original.bytes::<16>(s + 0x1D0)?,
        owner.bytes::<16>(n + 0x188)?
    );
    program(original, s + 0x1E0, owner, n + 0x198, true)?;
    assert_eq!(original.u32(s + 0x218)?, u32::MAX);
    assert_eq!(owner.u32(n + 0x1D0)?, u32::MAX);
    for i in 0..10 {
        program(
            original,
            s + 0x298 + i * 64,
            owner,
            n + 0x250 + i * 64,
            false,
        )?;
    }
    assert_eq!(
        original.bytes::<8>(s + 0x290)?,
        owner.bytes::<8>(n + 0x248)?
    );
    assert_eq!(
        original.bytes::<8>(s + 0x518)?,
        owner.bytes::<8>(n + 0x518)?
    );
    assert_eq!(owner.u8(n + 0x4D0)?, 1);
    let unit = owner.array(n + 0x4D8 + 16, 16, Some(0x80800090))?;
    assert_eq!(unit.len(), 1);
    for lane in 0..4 {
        assert_eq!(owner.f32(unit[0] + lane * 4)?, 1.0);
    }
    // Damage block resource and action rows name private translated resources.
    let feedback = owner.u32(n + 0x1F0)?;
    if original.u32(s + 0x238)? == u32::MAX {
        assert_eq!(feedback, u32::MAX);
    } else {
        assert!(
            stock.tag(feedback, None).is_err(),
            "Query feedback must be private"
        );
    }
    let rows = original.array(s + 0x1C0, 0x58, Some(0x8080238E))?;
    let native_rows = owner.array(n + 0x178, 0x58, Some(0x80804ABF))?;
    assert_eq!(rows.len(), native_rows.len());
    for (from, to) in rows.into_iter().zip(native_rows) {
        program(original, from, owner, to, false)?;
        assert_eq!(original.u32(from + 0x48)?, owner.u32(to + 0x48)?);
        let action = original.pointer(from + 0x50)?;
        let native_action = owner.pointer(to + 0x50)?;
        assert_eq!(original.u32(action - 4)?, 0x8080243F);
        assert_eq!(owner.u32(native_action - 4)?, 0x80804AC9);
        let profile = original.u32(action + 8)?;
        let native_profile = owner.u32(native_action + 8)?;
        if profile == u32::MAX {
            assert_eq!(native_profile, u32::MAX);
        } else {
            assert_eq!(reader.reference(profile)?, 0x80800861);
            assert!(
                stock.tag(native_profile, None).is_err(),
                "Query profile must be private"
            );
            let target = emitted.tag(native_profile, Some(0x80809C36))?;
            let n = target.pointer(24)?;
            assert_eq!(target.u32(n - 4)?, 0x80804B02);
            let original = reader.read(profile)?;
            let s = original.pointer(24)?;
            // Amount bounds, extent and damage type keep their source values.
            assert_eq!(
                original.bytes::<12>(s + 0x80)?,
                target.bytes::<12>(n + 0x14)?
            );
            assert_eq!(original.u8(s + 0x8C)?, target.u8(n + 0x20)?);
            let mut classes = target
                .array(n + 0x78, 8, Some(0x80804B3D))?
                .into_iter()
                .map(|row| target.u32(target.pointer(row)? - 4).map_err(Into::into))
                .collect::<CheckResult<Vec<_>>>()?;
            classes.sort_unstable();
            assert_eq!(classes, [0x80804B46, 0x80804B56, 0x80804B57]);
            let feedback = target.u32(n + 0xB0)?;
            if original.u32(s + 0x100)? != u32::MAX {
                assert!(
                    stock.tag(feedback, None).is_err(),
                    "Profile feedback must be private"
                );
            }
        }
        // Both source programs are constant one in the admitted form. Native stock damage
        // actions evaluate (1, 1, 0).
        for (offset, value) in [(0x28, 1.0f32), (0x68, 1.0), (0xA8, 0.0)] {
            let constants = owner.array(native_action + offset + 16, 16, Some(0x80800090))?;
            assert_eq!(constants.len(), 1);
            assert_eq!(owner.f32(constants[0])?, value);
        }
    }
    Ok(())
}

fn spawn(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    original: &Payload,
    s: usize,
    owner: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(owner.u32(n - 4)?, 0x80808881);
    assert_eq!(owner.u32(n + 0x30)?, 15);
    assert_eq!(original.u8(s + 0x20)?, 2, "World creation");
    assert_eq!(owner.bytes::<2>(n + 0x72)?, [2, 0xFF]);
    assert_eq!(
        original.u32(s + 0x3C)?,
        owner.u32(n + 0x98)?,
        "Creation locator"
    );
    assert_eq!(
        original.bytes::<8>(s + 0x40)?,
        owner.bytes::<8>(n + 0x9C)?,
        "Speed"
    );
    // Source spread objects keep their form after three stride-20 rows, native ones after three
    // stride-18 rows. An absent spread is the single-object path in both.
    if original.u64(s + 0x48)? == 0 {
        assert_eq!(owner.u64(n + 0xB0)?, 0, "Spread");
    } else {
        let from = original.pointer(s + 0x48)?;
        let to = owner.pointer(n + 0xB0)?;
        assert_eq!(owner.u32(to - 4)?, 0x80808891, "Spread class");
        assert_eq!(
            original.bytes::<20>(from + 0x60)?,
            owner.bytes::<20>(to + 0x48)?,
            "Spread form"
        );
    }
    assert_eq!(original.u32(s + 0x50)?, owner.u32(n + 0xB8)?);
    // The parameter list's rows follow each array to its own data, so a descriptor copied without
    // its rows points at whatever comes next. Native drops the source's builtin time input.
    let from = original.array(s + 0x28, 0x48, Some(0x808008A1))?;
    let to = owner.array(n + 0x88, 0x48, Some(0x8080889B))?;
    assert_eq!(from.len(), to.len(), "Creation parameters");
    for (a, b) in from.into_iter().zip(to) {
        assert_eq!(
            original.u32(a + 0x40)?,
            owner.u32(b + 0x40)?,
            "Parameter name"
        );
        let program = |p: &Payload, row: usize| -> CheckResult<Vec<u8>> {
            Ok(p.0[p.array_range(row, 1, Some(0x80800009))?].to_vec())
        };
        assert_eq!(
            program(original, a)?,
            program(owner, b)?,
            "Parameter program"
        );
        let inputs = |p: &Payload, row: usize| -> CheckResult<Vec<u32>> {
            p.array(row + 0x30, 4, Some(0x80800007))?
                .into_iter()
                .map(|at| p.u32(at).map_err(Into::into))
                .collect()
        };
        let source = inputs(original, a)?;
        assert_eq!(source.first(), Some(&u32::MAX), "Source builtin time");
        assert_eq!(source[1..], inputs(owner, b)?, "Parameter inputs");
        assert_eq!((owner.u64(b + 0x20)?, owner.u64(b + 0x28)?), (2, 1));
    }
    assert_eq!(owner.bytes::<12>(n + 0x40)?, [0; 12]);
    assert_eq!(
        owner.bytes::<16>(n + 0x50)?,
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x80, 0x3F]
    );
    let created = owner.u32(n + 0x74)?;
    if stock.tag(created, Some(0x80809C0F)).is_ok() {
        // A ported graph keeps the stock graph that the native port of this sequence creates.
        // Its owners belong to the stock graph, so only its class is checked here.
        return Ok(());
    }
    let source_graph = reader.read(original.u32(s + 0x24)?)?;
    let graph = emitted.tag(created, Some(0x80809C0F))?;
    // A state selector that no connection names publishes to nobody and is left out.
    let mut expected = 0;
    for row in source_graph.array(32, 12, Some(0x80800715))? {
        let owner = reader.read(source_graph.u32(row)?)?;
        if owner.u32(owner.pointer(24)? - 4)? != 0x80802818 {
            expected += 1;
        }
    }
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    assert_eq!(expected, owners.len(), "Created graph owner count");
    // The created graph keeps the stock effect context and a private sequence.
    assert!(
        owners
            .iter()
            .any(|&row| graph.u32(row).ok() == Some(0x80C7_0B84))
    );
    Ok(())
}
