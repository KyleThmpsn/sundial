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
mod receiver;

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
    let mut queries = 0;
    for &tag in &tags {
        let owner = reader.read(tag)?;
        if owner.u32(owner.pointer(24)? - 4)? == 0x80803136 {
            queries += 1;
        }
        if owner.u32(owner.pointer(24)? - 4)? == 0x80800610 {
            sequences += 1;
        }
    }
    // A Duration host beside one sequence is the lens form, whose sequence holds the screen
    // color event.
    let host = reader.read(tags[0])?;
    let lens = tags.len() == 2 && sequences == 1 && host.u32(host.pointer(24)? - 4)? == 0x808029C3;
    // A timed status beside an ability restriction, as Felwinter's Covenant attaches.
    let partner = reader.read(tags[tags.len() - 1])?;
    let restriction = tags.len() == 2
        && host.u32(host.pointer(24)? - 4)? == 0x80802698
        && partner.u32(partner.pointer(24)? - 4)? == 0x80802518;
    Ok((sequences == 1
        && FRAMEWORK[..3]
            .iter()
            .all(|(source, _)| tags.contains(source)))
        || (queries == 1 && tags.len() == 2)
        || lens
        || restriction)
}

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    graph: &Payload,
    native_tag: u32,
) -> CheckResult<()> {
    check_without(reader, stock, emitted, graph, native_tag, None)
}

/// [`check`] for a graph translated without `detached`, an owner no connection row names, which
/// the caller holds to its own attachment.
pub(super) fn check_without(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    graph: &Payload,
    native_tag: u32,
    detached: Option<u32>,
) -> CheckResult<()> {
    assert!(
        stock.tag(native_tag, None).is_err(),
        "Attached graph must be private"
    );
    let native = emitted.tag(native_tag, Some(0x80809C0F))?;
    // The source AI tuning owner has no channel connections. Its native adaptation adds a
    // timed cancellation marker, checked across the choice's activating and cancelling actions.
    let mut owners = Vec::new();
    let mut assignment = false;
    for row in graph.array(32, 12, Some(0x80800715))? {
        let owner = reader.read(graph.u32(row)?)?;
        let left_out = detached == Some(graph.u32(row)?);
        if !left_out && !matches!(owner.u32(owner.pointer(24)? - 4)?, 0x80802818 | 0x808010FB) {
            owners.push(row);
        } else {
            assignment |= owner.u32(owner.pointer(24)? - 4)? == 0x808010FB;
            let omitted = graph.u32(row)?;
            for connection in graph.array(0x30, 88, Some(0x8080054E))? {
                assert_ne!(
                    graph.u32(connection + 8)?,
                    omitted,
                    "Omitted state has an input"
                );
                assert_ne!(
                    graph.u32(connection + 48)?,
                    omitted,
                    "Omitted state has a consumer"
                );
            }
        }
    }
    let targets = native.array(16, 12, Some(0x80809C04))?;
    assert_eq!(
        owners.len() + usize::from(assignment),
        targets.len(),
        "Attached graph owner count"
    );
    let owner_map = owners
        .iter()
        .zip(&targets)
        .map(|(&from, &to)| Ok((graph.u32(from)?, native.u32(to)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut acts = 0;
    for (ordinal, (&from, &to)) in owners.iter().zip(&targets).enumerate() {
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
            0x80803136 => {
                assert_eq!(target.u32(target.pointer(16)? - 4)?, 0x8080381F);
                let report =
                    crate::readback::check_query(reader, stock, emitted, &original, &target)?;
                fs::write(
                    emitted
                        .output
                        .join(format!("attached-query-{source_tag:08X}-{tag:08X}.json")),
                    serde_json::to_vec_pretty(&report)?,
                )?;
                acts += 1;
            }
            0x808005A8 => receiver::check(&original, &target, graph, &native, &owner_map)?,
            0x80800362 => super::network::check(
                reader, stock, emitted, &original, graph, native_tag, ordinal,
            )?,
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
            0x808027FA => {
                incoming(&original, &target)?;
                acts += 1;
            }
            0x80802518 => {
                // The four ability masks carry over in order. Every other native setting is
                // the default the stock owners hold.
                let (from, to) = (original.pointer(24)?, target.pointer(24)?);
                assert_eq!(target.u32(target.pointer(16)? - 4)?, 0x80803A77);
                assert_eq!(target.u32(to - 4)?, 0x80803A78);
                let masks = original.bytes::<4>(from + 0x88)?;
                assert!(masks != [0; 4] && masks.iter().all(|mask| mask & !0x3F == 0));
                assert_eq!(target.bytes::<4>(to + 0x90)?, masks);
                assert_eq!(original.bytes::<12>(from + 0x8C)?, [0; 12]);
                assert_eq!(target.u32(to + 0x78)?, 0x811C9DC5);
                assert_eq!(target.bytes::<16>(to + 0x80)?, [0; 16]);
                assert_eq!(target.bytes::<12>(to + 0x94)?, [0; 12]);
                println!("Ability restriction {source_tag:08X} to {tag:08X}: masks {masks:02X?}");
                acts += 1;
            }
            _ if volume_script(reader.read(class).ok().as_ref(), 0x80802361) => {
                volume(emitted, graph, &native, &original, source_tag, &target, tag)?;
                acts += 1;
            }
            _ => return Err(format!("Unexpected attached owner {class:08X}").into()),
        }
    }
    assert!(
        acts > 0,
        "Translated graph neither queries, creates nor applies a status"
    );
    Ok(())
}

/// An incoming damage provider multiplies the damage its entity takes by each matching row's
/// factor. The emitted modifier keeps the source's gates and, row for row, its damage type
/// masks, their bypass flags and its factor. A source row without labels skips both native label
/// tests. Which labels a row with labels keeps is the label registry translation's contract and
/// is not compared here.
fn incoming(original: &Payload, target: &Payload) -> CheckResult<()> {
    let (from, to) = (original.pointer(24)?, target.pointer(24)?);
    assert_eq!(target.u32(target.pointer(16)? - 4)?, 0x80803F8B);
    assert_eq!(target.u32(to - 4)?, 0x80803F8C);
    assert_eq!(
        original.bytes::<8>(from + 0x100)?,
        target.bytes::<8>(to + 0xE0)?,
        "Incoming damage property gate"
    );
    assert_eq!(
        original.bytes::<3>(from + 0x108)?,
        target.bytes::<3>(to + 0xE8)?,
        "Incoming damage owner, relation and accumulator"
    );
    let rows = original.array(from + 0x88, 0x50, Some(0x808029F8))?;
    let emitted = target.array(to + 0x70, 0xA0, Some(0x80802A1C))?;
    assert!(!rows.is_empty());
    assert_eq!(rows.len(), emitted.len(), "Incoming damage row count");
    for (row, native) in rows.into_iter().zip(emitted) {
        assert_eq!(
            original.bytes::<8>(row)?,
            target.bytes::<8>(native + 0x10)?,
            "Incoming damage type masks"
        );
        assert_eq!(
            original.bytes::<2>(row + 0x48)?,
            target.bytes::<2>(native + 0x28)?,
            "Incoming damage type bypass"
        );
        let code = target
            .array(native + 0x60, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| target.u8(at))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let constants = target.array(native + 0x70, 16, Some(0x80800090))?;
        assert_eq!(code, [0x34, 0, 0x3E, 0], "Incoming damage factor program");
        assert_eq!(constants.len(), 1);
        assert_eq!(
            original.u32(row + 0x4C)?,
            target.u32(constants[0])?,
            "Incoming damage factor"
        );
        if original.bytes::<2>(row + 0x4A)? == [1, 1] {
            let masks = target.pointer(native + 0x20)?;
            assert_eq!(
                target.bytes::<2>(masks + 80)?,
                [1, 1],
                "Incoming damage row gained a label test"
            );
        }
    }
    Ok(())
}

const VOLUME: &[u8] = b"abilities.ability_components.c_condition_vol_component_definition\0";

/// Whether a definition schema is the condition volume script on component base `base`.
fn volume_script(schema: Option<&Payload>, base: u32) -> bool {
    schema.is_some_and(|schema| {
        schema.u32(16).ok() == Some(base) && schema.0.windows(VOLUME.len()).any(|w| w == VOLUME)
    })
}

/// A condition volume assembled beside owners of another stock graph. The emitted owner is the
/// native script, and every source row that reads one of its getters has an emitted row reading
/// the native getter of the same name, which both games store `18` bytes into the getter.
fn volume(
    emitted: &mut Reader,
    graph: &Payload,
    native: &Payload,
    original: &Payload,
    source_tag: u32,
    target: &Payload,
    tag: u32,
) -> CheckResult<()> {
    let class = target.u32(target.pointer(24)? - 4)?;
    let schema = emitted.tag(class, Some(0x80800000))?;
    assert!(
        volume_script(Some(&*schema), 0x80803C57),
        "Emitted volume is not the native condition volume script"
    );
    let mut names = Vec::new();
    for row in graph.array(0x30, 88, Some(0x8080054E))? {
        if graph.u32(row + 48)? == source_tag {
            let getter = usize::try_from(graph.u64(row + 56)?)?;
            names.push(original.u32(getter + 0x18)?);
        }
    }
    let mut found = Vec::new();
    for row in native.array(32, 72, None)? {
        if native.u32(row + 40)? == tag {
            assert_eq!(native.u32(row + 44)?, 0x80809AE1);
            let getter = usize::try_from(native.u64(row + 48)?)?;
            found.push(target.u32(getter + 0x18)?);
        }
    }
    assert!(names.iter().all(|&name| name != 0x811C9DC5));
    assert_eq!(
        names, found,
        "Volume getter names differ from the source rows"
    );
    println!("Condition volume {source_tag:08X} to {tag:08X}: getters {names:08X?}");
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
            // A translated screen color event names a private record, at the source locator,
            // with three programs of constant one. An untranslated one stays a delay.
            0x8080052F if owner.u32(to - 4)? == 0x808093C2 => {
                assert_eq!(owner.u32(to + 0x18)?, flags);
                assert_eq!(owner.u32(to + 0x30)?, 23);
                assert_eq!(original.u32(from + 0x20)?, owner.u32(to + 0x40)?);
                let record = owner.u32(to + 0x44)?;
                assert!(
                    stock.tag(record, None).is_err(),
                    "Screen color record must be private"
                );
                assert_eq!(emitted.tag(record, Some(0x8080707E))?.u32(8)?, 1000);
                for at in [0x48, 0x90, 0xD8] {
                    let constants = owner.array(to + at + 16, 16, Some(0x80800090))?;
                    assert_eq!(constants.len(), 1);
                    assert_eq!(
                        owner.bytes::<16>(constants[0])?[..4],
                        1f32.to_le_bytes(),
                        "Screen color program is not constant one"
                    );
                    assert_eq!(
                        owner.array(to + at, 1, Some(0x80800009))?.len(),
                        owner.array(to + 0x48, 1, Some(0x80800009))?.len()
                    );
                }
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

/// A source cone keeps its near distance, far distance and half angle on the native cone, whose
/// private baked hull must be that cone's solid: every vertex inside the source cone, the hull
/// reaching its near rim, its far rim and the far point of its axis, and every plane supporting
/// the hull. The hull is read here from the Havok packfile's own fixups.
fn cone(
    stock: &mut Reader,
    emitted: &mut Reader,
    original: &Payload,
    shape: usize,
    owner: &Payload,
    native: usize,
) -> CheckResult<()> {
    assert_eq!(owner.u32(native - 4)?, 0x808092AB);
    assert_eq!(owner.u32(native)?, 4);
    assert_eq!(
        original.bytes::<12>(shape + 0xA8)?,
        owner.bytes::<12>(native + 4)?,
        "Cone distances and angle"
    );
    assert_eq!(original.bytes::<12>(shape + 0xB4)?, [0; 12]);
    assert_eq!(owner.bytes::<16>(native + 0x10)?, [0; 16]);
    assert_eq!(owner.u32(native + 0x2C)?, u32::MAX);
    assert_eq!(
        original.u64(shape + 0xC0)? == 0,
        owner.u64(native + 0x20)? == 0,
        "Cone bound presence"
    );
    if owner.u64(native + 0x20)? != 0 {
        let bound = owner.pointer(native + 0x20)?;
        assert_eq!(owner.u32(bound - 4)?, 0x808092BE);
        assert_eq!(
            original.u32(original.pointer(shape + 0xC0)? - 4)?,
            0x8080264C
        );
    }
    let hull = owner.u32(native + 0x28)?;
    assert!(stock.tag(hull, None).is_err(), "Cone hull must be private");
    let file = emitted.tag(hull, None)?;
    assert_eq!(file.0.get(40..54), Some(&b"hk_2012.2.0-r1"[..]));
    let data = file.u32(0xB4)? as usize;
    let mut fixups = BTreeMap::new();
    let mut at = data + file.u32(0xB8)? as usize;
    while at + 8 <= data + file.u32(0xBC)? as usize && file.u32(at)? != u32::MAX {
        fixups.insert(file.u32(at)?, data + file.u32(at + 4)? as usize);
        at += 8;
    }
    let rows = fixups[&80];
    let vertices = (0..file.u32(data + 96)? as usize)
        .map(|index| {
            let lane = rows + index / 4 * 48 + index % 4 * 4;
            Ok([
                f64::from(file.f32(lane)?),
                f64::from(file.f32(lane + 16)?),
                f64::from(file.f32(lane + 32)?),
            ])
        })
        .collect::<CheckResult<Vec<_>>>()?;
    let planes = (0..file.u32(data + 112)? as usize)
        .map(|index| {
            let row = fixups[&104] + index * 16;
            Ok([
                f64::from(file.f32(row)?),
                f64::from(file.f32(row + 4)?),
                f64::from(file.f32(row + 8)?),
                f64::from(file.f32(row + 12)?),
            ])
        })
        .collect::<CheckResult<Vec<_>>>()?;
    let near = f64::from(original.f32(shape + 0xA8)?);
    let far = f64::from(original.f32(shape + 0xAC)?);
    let angle = f64::from(original.f32(shape + 0xB0)?).to_radians();
    let slack = far * 1e-3;
    // The hull's frame starts at the near plane. From the apex, each vertex has a distance, a
    // distance along the axis and an angle off it.
    let polar = vertices
        .iter()
        .map(|v| {
            let along = v[0] + near;
            let off = v[1].hypot(v[2]);
            (along.hypot(off), along, off.atan2(along))
        })
        .collect::<Vec<_>>();
    assert!(vertices.len() >= 4 && planes.len() >= 4, "Cone hull size");
    for &(distance, along, turn) in &polar {
        assert!(
            distance <= far + slack && along >= near - slack && turn <= angle + 1e-3,
            "Cone hull vertex lies outside the source cone"
        );
    }
    let reaches =
        |want: &dyn Fn(f64, f64, f64) -> bool| polar.iter().any(|&(d, a, t)| want(d, a, t));
    assert!(
        reaches(&|d, _, t| (d - far).abs() <= slack && t.abs() <= 1e-3),
        "Cone hull misses the far point of the axis"
    );
    assert!(
        reaches(&|d, _, t| (d - far).abs() <= slack && (t - angle).abs() <= 1e-3),
        "Cone hull misses the far rim"
    );
    assert!(
        reaches(&|_, a, t| (a - near).abs() <= slack && (t - angle).abs() <= 1e-3),
        "Cone hull misses the near rim"
    );
    for plane in &planes {
        let side = |v: &[f64; 3]| plane[0] * v[0] + plane[1] * v[1] + plane[2] * v[2] + plane[3];
        let length = (plane[0] * plane[0] + plane[1] * plane[1] + plane[2] * plane[2]).sqrt();
        assert!((length - 1.0).abs() <= 1e-4, "Cone hull plane normal");
        assert!(
            vertices.iter().all(|v| side(v) <= slack)
                && vertices.iter().filter(|v| side(v).abs() <= slack).count() >= 3,
            "Cone hull plane does not support the hull"
        );
    }
    Ok(())
}

/// A response list, which a query's damage block and a damage profile both hold: each graph row
/// names a graph held to the source graph, and each program row runs the source program between
/// the same words.
fn responses(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    (original, from): (&Payload, usize),
    (owner, to): (&Payload, usize),
) -> CheckResult<()> {
    let rows = original.array(from, 8, Some(0x80802769))?;
    let native_rows = owner.array(to, 8, Some(0x80804B0D))?;
    assert_eq!(rows.len(), native_rows.len(), "Response list length");
    for (from, to) in rows.into_iter().zip(native_rows) {
        let (from, to) = (original.pointer(from)?, owner.pointer(to)?);
        match original.u32(from - 4)? {
            0x80802830 => {
                assert_eq!(owner.u32(to - 4)?, 0x80804B09);
                assert_eq!(owner.u64(to)?, 0);
                assert_eq!(owner.u32(to + 12)?, 0);
                created_graph(
                    reader,
                    stock,
                    emitted,
                    original.u32(from + 8)?,
                    owner.u32(to + 8)?,
                )?;
            }
            0x8080268E => {
                assert_eq!(owner.u32(to - 4)?, 0x80804B08);
                assert_eq!(original.u64(from)?, owner.u64(to)?);
                program(original, from + 8, owner, to + 8, true)?;
                assert_eq!(
                    original.bytes::<16>(from + 0x38)?,
                    owner.bytes::<16>(to + 0x38)?
                );
            }
            class => return Err(format!("Unexpected response {class:08X}").into()),
        }
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
    // Geometry: the source sphere's radius and bound scale on the native sphere pair, or the
    // source cone on the native cone.
    let shape = original.pointer(s + 0x28)?;
    if original.u32(shape - 4)? == 0x808022E0 {
        cone(
            stock,
            emitted,
            original,
            shape,
            owner,
            owner.pointer(n + 0x48)?,
        )?;
    } else {
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
    }
    // Broadphase block, divisor and budgets.
    assert_eq!(
        original.bytes::<48>(s + 0x30)?,
        owner.bytes::<48>(n + 0x70)?
    );
    assert_eq!(
        original.bytes::<12>(s + 0xEC)?,
        owner.bytes::<12>(n + 0xA4)?
    );
    // The relationship mask travels on the actions, so the query keeps no native predicate. Its
    // scale bit has no native field.
    let exclusion = original.u8(s + 0x61)?;
    assert_eq!(exclusion & 0x40, 0);
    // Source 005ACBA0 only evaluates this scale when relationship bit 6 is set.
    // With the mask empty, finite zero and unit scales are equally inert.
    assert!(original.f32(s + 0x64)?.is_finite());
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
    responses(
        reader,
        stock,
        emitted,
        (original, s + 0x220),
        (owner, n + 0x1D8),
    )?;
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
        // An attachment creates a private graph held to the source graph on each candidate,
        // under the query's relationship mask merged with its own.
        if matches!(original.u32(action - 4)?, 0x808020BC | 0x80803207) {
            let own = if original.u32(action - 4)? == 0x80803207 {
                original.u8(action + 0x10)?
            } else {
                0
            };
            let mask = exclusion | own;
            if mask == 0 {
                assert_eq!(owner.u32(native_action - 4)?, 0x80804AC7);
            } else {
                assert_eq!(owner.u32(native_action - 4)?, 0x80804AC6);
                assert_eq!(owner.u8(native_action + 0x10)?, mask);
            }
            created_graph(
                reader,
                stock,
                emitted,
                original.u32(action + 8)?,
                owner.u32(native_action + 8)?,
            )?;
            continue;
        }
        assert_eq!(original.u32(action - 4)?, 0x8080243F);
        if exclusion == 0 {
            assert_eq!(owner.u32(native_action - 4)?, 0x80804AC9);
        } else {
            assert_eq!(owner.u32(native_action - 4)?, 0x80804AC8);
            assert_eq!(owner.u8(native_action + 0xE8)?, exclusion);
        }
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
            responses(
                reader,
                stock,
                emitted,
                (&original, s + 0x108),
                (&*target, n + 0xB8),
            )?;
            assert_eq!(original.u8(s + 0x8C)?, target.u8(n + 0x20)?);
            // Each source response keeps its native class. An audio response whose rows all
            // play nothing leaves, and a name no stock profile holds is left out, so those two
            // may be absent. A name that stays is the source's.
            let mut classes = BTreeMap::new();
            for row in target.array(n + 0x78, 8, Some(0x80804B3D))? {
                let at = target.pointer(row)?;
                assert!(
                    classes.insert(target.u32(at - 4)?, at).is_none(),
                    "Profile repeats a response"
                );
            }
            let mut expected = BTreeSet::new();
            let mut optional = BTreeSet::new();
            for row in original.array(s + 0x28, 8, Some(0x8080276E))? {
                let at = original.pointer(row)?;
                match original.u32(at - 4)? {
                    0x808023B0 => expected.insert(0x80804B57),
                    0x80802B30 => expected.insert(0x80804B56),
                    0x808021B3 => optional.insert(0x80804B46),
                    0x808022F5 => {
                        if let Some(&name) = classes.get(&0x80804B55) {
                            assert_eq!(
                                original.u32(at)?,
                                target.u32(name)?,
                                "Profile response name"
                            );
                        }
                        optional.insert(0x80804B55)
                    }
                    class => return Err(format!("Unexpected profile response {class:08X}").into()),
                };
            }
            let held = classes.keys().copied().collect::<BTreeSet<_>>();
            assert!(
                expected.is_subset(&held)
                    && held
                        .difference(&expected)
                        .all(|class| optional.contains(class)),
                "Profile responses {held:08X?} for source responses {expected:08X?} and {optional:08X?}"
            );
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
    created_graph(
        reader,
        stock,
        emitted,
        original.u32(s + 0x24)?,
        owner.u32(n + 0x74)?,
    )
}

/// A graph an event creates, attaches or answers with. A ported graph keeps the stock graph
/// that the native port of its sequence names, whose owners are stock, so only its class is
/// checked. Any other is private, with an owner for each source owner that publishes, and
/// keeps the stock effect context where the source graph holds the source's.
fn created_graph(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source: u32,
    created: u32,
) -> CheckResult<()> {
    if stock.tag(created, Some(0x80809C0F)).is_ok() {
        return Ok(());
    }
    let source_graph = reader.read(source)?;
    let graph = emitted.tag(created, Some(0x80809C0F))?;
    // A state selector that no connection names publishes to nobody and is left out.
    let mut expected = 0;
    let mut context = false;
    for row in source_graph.array(32, 12, Some(0x80800715))? {
        let tag = source_graph.u32(row)?;
        let owner = reader.read(tag)?;
        if owner.u32(owner.pointer(24)? - 4)? != 0x80802818 {
            expected += 1;
        }
        context |= tag == 0x80AA_D54B;
    }
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    assert_eq!(expected, owners.len(), "Created graph owner count");
    if context {
        assert!(
            owners
                .iter()
                .any(|&row| graph.u32(row).ok() == Some(0x80C7_0B84)),
            "Created graph lost the stock effect context"
        );
    }
    Ok(())
}
