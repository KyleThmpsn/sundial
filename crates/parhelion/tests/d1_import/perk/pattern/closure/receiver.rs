//! Compare property names, initialized scalars and the graph consumers of each actual getter.
use super::*;

fn rows(p: &Payload, native: bool) -> CheckResult<BTreeMap<u32, (u32, usize)>> {
    let instance = p.pointer(16)?;
    let definition = if native {
        usize::try_from(p.u64(instance + 8)?)?
    } else {
        p.pointer(24)?
    };
    let owner = p.u32(instance)?;
    assert_eq!(
        p.u32(instance - 4)?,
        if native { 0x80808BF5 } else { 0x808006F7 }
    );
    assert_eq!(
        p.u32(definition - 4)?,
        if native { 0x80808BF7 } else { 0x808005A8 }
    );
    assert_eq!(p.u64(definition + 8)?, instance as u64);
    let definitions = p.array(
        definition + if native { 0x78 } else { 0x90 },
        if native { 0x40 } else { 0x50 },
        Some(if native { 0x80808BFA } else { 0x80800884 }),
    )?;
    let mut instances = p
        .array(
            instance + 0x30,
            if native { 0x30 } else { 0x28 },
            Some(if native { 0x80808BF9 } else { 0x808004E2 }),
        )?
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(
        !definitions.is_empty(),
        "Required named receiver has no rows"
    );
    assert_eq!(definitions.len(), instances.len());
    let mut result = BTreeMap::new();
    for row in definitions {
        let live = usize::try_from(p.u64(row + 8)?)?;
        assert!(
            instances.remove(&live),
            "Receiver has an absent or repeated runtime row"
        );
        assert_eq!(p.u64(live + 8)?, row as u64);
        assert_eq!(p.u32(live)?, owner);
        assert_eq!(p.u32(row)?, owner);
        let value = p.u32(live + 0x20)?;
        let name = p.u32(row + if native { 0x28 } else { 0x30 })?;
        if native {
            assert_eq!(p.u32(live + 4)?, 0x80808BFA);
            assert_eq!(p.u32(row + 4)?, 0x80808BF9);
            assert_eq!(p.pointer(live + 0x10)?, instance);
            assert_eq!(p.pointer(row + 0x10)?, row);
        } else {
            for lane in 0..4 {
                assert_eq!(p.u32(row + 0x40 + lane * 4)?, value);
            }
        }
        assert!(
            result
                .insert(name, (value, row + if native { 0x10 } else { 0x18 }))
                .is_none(),
            "Repeated receiver name"
        );
    }
    assert!(instances.is_empty(), "Receiver has an unused runtime row");
    Ok(result)
}

fn consumers(
    graph: &Payload,
    owner: u32,
    rows: &BTreeMap<u32, (u32, usize)>,
    native: bool,
    owner_map: &BTreeMap<u32, u32>,
) -> CheckResult<Vec<(u32, u32)>> {
    let mut result = Vec::new();
    for row in graph.array(
        if native { 0x20 } else { 0x30 },
        if native { 72 } else { 88 },
        Some(if native { 0x80809BC9 } else { 0x8080054E }),
    )? {
        if graph.u32(row + if native { 40 } else { 48 })? != owner {
            continue;
        }
        let provider = usize::try_from(graph.u64(row + if native { 48 } else { 56 })?)?;
        let (&name, _) = rows
            .iter()
            .find(|(_, (_, at))| *at == provider)
            .ok_or("Connected receiver getter has no named row")?;
        let consumer = graph.u32(row + 8)?;
        let consumer = if native {
            consumer
        } else {
            *owner_map
                .get(&consumer)
                .ok_or("Receiver consumer was omitted")?
        };
        result.push((consumer, name));
    }
    result.sort();
    Ok(result)
}

pub(super) fn check(
    source: &Payload,
    target: &Payload,
    source_graph: &Payload,
    native_graph: &Payload,
    owner_map: &BTreeMap<u32, u32>,
) -> CheckResult<()> {
    let original = rows(source, false)?;
    let emitted = rows(target, true)?;
    assert_eq!(
        original
            .iter()
            .map(|(&name, &(value, _))| (name, value))
            .collect::<BTreeMap<_, _>>(),
        emitted
            .iter()
            .map(|(&name, &(value, _))| (name, value))
            .collect::<BTreeMap<_, _>>()
    );
    assert_eq!(
        consumers(
            source_graph,
            source.u32(source.pointer(16)?)?,
            &original,
            false,
            owner_map
        )?,
        consumers(
            native_graph,
            target.u32(target.pointer(16)?)?,
            &emitted,
            true,
            owner_map
        )?,
        "A named receiver's consumer changed"
    );
    Ok(())
}
