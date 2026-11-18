//! Follow cancellation through the actual emitted victim, timed creation and Duration key.
use super::*;

fn nodes(p: &Payload, field: usize) -> CheckResult<Vec<usize>> {
    p.array(field, 8, None)?
        .into_iter()
        .map(|row| p.pointer(row).map_err(Into::into))
        .collect()
}

fn status(reader: &mut Reader, graph: u32) -> CheckResult<(u32, i32)> {
    let graph = reader.tag(graph, Some(0x80809C0F))?;
    let mut found = Vec::new();
    for row in graph.array(16, 12, Some(0x80809C04))? {
        let p = reader.tag(graph.u32(row)?, Some(0x80809C36))?;
        let d = p.pointer(24)?;
        if p.u32(d - 4)? == 0x80804205 {
            let duration = d + 0xE0 + 0x148;
            found.push((
                p.u32(duration + 0x9C)?,
                i32::from_le_bytes(p.bytes::<4>(duration + 0xA4)?),
            ));
        }
    }
    assert_eq!(
        found.len(),
        1,
        "Assignment graph must have one competing status"
    );
    Ok(found[0])
}

fn marker(
    stock: &mut Reader,
    reader: &mut Reader,
    graph_tag: u32,
    key: u32,
    delay: f32,
    duration: f32,
) -> CheckResult<Value> {
    let graph = reader.tag(graph_tag, Some(0x80809C0F))?;
    let mut candidates = Vec::new();
    for row in graph.array(16, 12, Some(0x80809C04))? {
        let tag = graph.u32(row)?;
        let p = reader.tag(tag, Some(0x80809C36))?;
        let d = p.pointer(24)?;
        if p.u32(d - 4)? != 0x808084E9 {
            continue;
        }
        for event in p.array(d + 0x168, 24, Some(0x808093E6))? {
            let event = p.pointer(event + 16)?;
            if p.u32(event - 4)? != 0x80808881 || p.u8(event + 0x72)? != 0 {
                continue;
            }
            let child_tag = p.u32(event + 0x74)?;
            let child = reader.tag(child_tag, Some(0x80809C0F))?;
            for owner in child.array(16, 12, Some(0x80809C04))? {
                let owner_tag = child.u32(owner)?;
                let host = reader.tag(owner_tag, Some(0x80809C36))?;
                let h = host.pointer(24)?;
                if host.u32(h - 4)? != 0x80803B30 || host.u32(h + 0x48 + 0x9C)? != key {
                    continue;
                }
                assert!(stock.tag(tag, None).is_err());
                assert!(stock.tag(child_tag, None).is_err());
                assert!(stock.tag(owner_tag, None).is_err());
                assert_eq!(p.u8(d + 0x2E0)? & 3, 1, "Marker must start with its parent");
                assert_eq!(p.f32(event + 0x1C)?, delay);
                assert_eq!(p.f32(event + 0x20)?, 0.0);
                assert_eq!(p.f32(event + 0x24)?, duration - delay);
                assert_eq!(p.f32(event + 0x28)?, 0.0);
                assert_eq!(
                    p.u32(event + 0x18)? & 0x18,
                    0x18,
                    "Parent retirement must remove the marker"
                );
                let lifetime = h + 0x48 + 0x128;
                let constants = host.array(lifetime + 0x20, 16, Some(0x80800090))?;
                assert_eq!(constants.len(), 1);
                for lane in 0..4 {
                    assert_eq!(host.f32(constants[0] + lane * 4)?, duration - delay);
                }
                let code = host
                    .array(lifetime + 0x10, 1, Some(0x80800009))?
                    .into_iter()
                    .map(|at| host.u8(at))
                    .collect::<Result<Vec<_>>>()?;
                assert_eq!(code, [0x34, 0, 0x3E, 0]);
                candidates.push(json!({"sequence":tag,"marker_graph":child_tag,
                    "duration_owner":owner_tag,"key":key,"delay":delay,
                    "duration":duration-delay,"event_flags":p.u32(event+0x18)?}));
            }
        }
    }
    assert_eq!(
        candidates.len(),
        1,
        "Hit gate must reach exactly one owned marker"
    );
    Ok(candidates.remove(0))
}

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    reader: &mut Reader,
    actions: &BTreeMap<u32, u32>,
) -> CheckResult<Option<Value>> {
    let mut cancel = None;
    for (&from, &to) in actions {
        let s = source.read(from)?;
        for (index, at) in nodes(&s, 0x20)?.into_iter().enumerate() {
            if s.u32(at - 4)? == 0x8080307B && s.u8(at + 0x175)? == 1 {
                assert!(cancel.is_none(), "Ambiguous assignment cancellation");
                let n = reader.tag(to, Some(0x808040B5))?;
                let hit = nodes(&n, 0x20)?[index];
                let key = n.u32(hit + 0x9C)?;
                let delay = s.f32(at + 0x178)?;
                let duration = s.f32(at + 0x17C)?;
                let attached = nodes(&n, 0x38)?
                    .into_iter()
                    .filter(|&e| n.u8(e).ok() == Some(1) && n.u8(e + 2).ok() == Some(3))
                    .collect::<Vec<_>>();
                assert_eq!(attached.len(), 1);
                cancel = Some((key, delay, duration, n.u32(attached[0] + 0x10)?));
            }
        }
    }
    let Some((key, delay, duration, ending_graph)) = cancel else {
        return Ok(None);
    };
    assert!(delay > 0.0 && delay < duration);
    let ending = status(reader, ending_graph)?;
    let mut starts = Vec::new();
    for (&from, &to) in actions {
        let s = source.read(from)?;
        let n = reader.tag(to, Some(0x808040B5))?;
        let source_effects = nodes(&s, 0x38)?;
        let native_effects = nodes(&n, 0x38)?;
        for (se, ne) in source_effects.into_iter().zip(native_effects) {
            if s.u8(se)? != 0 || s.u8(se + 2)? != 3 {
                continue;
            }
            let graph = source.read(s.u32(se + 0x10)?)?;
            let mut owns_tuning = false;
            for owner in graph.array(0x20, 12, Some(0x80800715))? {
                let owner = source.read(graph.u32(owner)?)?;
                owns_tuning |= owner.u32(owner.pointer(24)? - 4)? == 0x808010FB;
            }
            if !owns_tuning {
                continue;
            }
            let main_graph = n.u32(ne + 0x10)?;
            let initial = status(reader, main_graph)?;
            assert_eq!(
                initial.0, ending.0,
                "Cancellation must replace the active status"
            );
            assert!(
                ending.1 > initial.1,
                "Cancellation must outrank the active status"
            );
            assert_ne!(key, initial.0, "The gate must not pass before its delay");
            starts.push(marker(stock, reader, main_graph, key, delay, duration)?);
        }
    }
    assert_eq!(
        starts.len(),
        1,
        "Cancellation requires one qualified activation graph"
    );
    Ok(Some(json!({"marker":starts[0],"active_status":ending.0,
        "cancellation_graph":ending_graph,"gameplay_verified":false})))
}
