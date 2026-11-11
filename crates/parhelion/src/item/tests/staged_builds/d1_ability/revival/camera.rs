//! Independent camera-event readback through the staged resource references.
use super::*;
use serde_json::Value;

fn programs(p: &Payload, at: usize, source: bool) -> Value {
    let code = p.array_range(at, 1, Some(0x80800009)).unwrap();
    let constants = p.array(at + 16, 16, Some(0x80800090)).unwrap();
    let slots = p.array(at + 48, 4, Some(0x80800007)).unwrap();
    if source {
        assert_eq!(slots.len(), 1);
        assert_eq!(p.u32(slots[0]).unwrap(), u32::MAX);
    } else {
        assert!(
            slots.is_empty(),
            "builtin camera input became an additional input"
        );
    }
    assert_eq!(p.u64(at + 32).unwrap(), 1);
    assert_eq!(p.u32(at + 40).unwrap(), 1);
    // Word +44 is compared between builds rather than assumed. Every examined source camera
    // program stores zero there.
    json!({"code":hex::encode(&p.0[code]),"word_44":p.u32(at + 44).unwrap(),
        "constants":constants.into_iter()
        .map(|row| hex::encode(&p.0[row..row+16])).collect::<Vec<_>>()})
}

/// The whole transition object: leading values, nine pointer and program pairs at stride `48`
/// and the two trailing values. Source `808024FB` and native `808066C4` share this layout, and
/// only the source lists each program's builtin input.
fn transition(p: &Payload, at: usize, source: bool) -> Value {
    assert_eq!(
        p.u32(at - 4).unwrap(),
        if source { 0x808024FB } else { 0x808066C4 }
    );
    let mut pairs = Vec::new();
    for k in 0..9 {
        let pair = at + 0x18 + k * 0x48;
        assert_eq!(p.u64(pair).unwrap(), 0, "transition pointer {k} is present");
        let program = pair + 8;
        let code = p.array_range(program, 1, Some(0x80800009)).unwrap();
        let constants = p.array(program + 16, 16, Some(0x80800090)).unwrap();
        let slots = p.array(program + 48, 4, Some(0x80800007)).unwrap();
        if source {
            assert!(slots.iter().all(|&slot| p.u32(slot).unwrap() == u32::MAX));
        } else {
            assert!(slots.is_empty(), "builtin transition input became a slot");
        }
        pairs.push(json!({"code":hex::encode(&p.0[code]),
            "inputs":p.u64(program + 32).unwrap(),"outputs":p.u32(program + 40).unwrap(),
            "word_44":p.u32(program + 44).unwrap(),
            "constants":constants.into_iter().map(|row| hex::encode(&p.0[row..row + 16])).collect::<Vec<_>>()}));
    }
    json!({"head":hex::encode(&p.0[at..at + 0x18]),"programs":pairs,
        "tail":hex::encode(&p.0[at + 0x2A0..at + 0x2A8])})
}

pub(super) fn readback(
    source: &parhelion_import::d1_roi::Source,
    original: &Payload,
    emitted: &Payload,
    staged: &PackageManager,
    stock: &PackageManager,
) -> Value {
    let views = [(original, true), (emitted, false)]
        .map(|(p, old)| camera_events(source, p, old, (staged, stock)));
    assert_eq!(
        views[0], views[1],
        "staged camera differs from the source camera"
    );
    json!(views[0])
}

fn camera_events(
    source: &parhelion_import::d1_roi::Source,
    p: &Payload,
    old: bool,
    (staged, stock): (&PackageManager, &PackageManager),
) -> Vec<Value> {
    let d = p.pointer(24).unwrap();
    let table = if old { 0x180 } else { 0x168 };
    let rows = p.array(d + table, if old { 8 } else { 24 }, None).unwrap();
    let mut events = Vec::new();
    for row in rows {
        let e = p.pointer(row + if old { 0 } else { 16 }).unwrap();
        if p.u32(e - 4).unwrap() != if old { 0x80802045 } else { 0x808066A3 } {
            continue;
        }
        assert_eq!(
            p.u32(e + if old { 0x1C } else { 0x30 }).unwrap(),
            if old { 18 } else { 21 }
        );
        let delta = if old { 0 } else { 0x20 };
        let values = read_camera_fields(p, e, delta, old);
        let programs = [0x48, 0x90, 0x110, 0x1B0, 0x1F0].map(|at| programs(p, e + at + delta, old));
        let rails = read_rails(source, (staged, stock), p, e + 0xF0 + delta, old);
        let blend = transition(p, p.pointer(e + 0x238 + delta).unwrap(), old);
        events.push(json!({"name":p.u32(e+if old {0} else {0x10}).unwrap(),
                "values":values,"programs":programs,"rails":rails,
                "transition":blend,
                "input":p.u16(e+if old {0x240} else {0x270}).unwrap()}));
    }
    events.sort_by_key(Value::to_string);

    events
}

fn read_rails(
    source: &parhelion_import::d1_roi::Source,
    (staged, stock): (&PackageManager, &PackageManager),
    p: &Payload,
    field: usize,
    old: bool,
) -> Vec<Value> {
    let mut rails = Vec::new();
    for row in p
        .array(field, 16, Some(if old { 0x8080261A } else { 0x808066FA }))
        .unwrap()
    {
        let tag = p.u32(row + 8).unwrap();
        let rail = if old {
            assert_eq!(source.reference(tag).unwrap(), 0x8080288F);
            source.read(tag).unwrap()
        } else {
            assert!(
                stock.get_entry(TagHash(tag)).is_none(),
                "camera rail is not private"
            );
            assert_eq!(
                staged.get_entry(TagHash(tag)).unwrap().reference,
                0x808066FB
            );
            Payload(staged.read_tag(TagHash(tag)).unwrap())
        };
        let points = rail
            .array(8, 32, Some(if old { 0x8080229B } else { 0x808066FD }))
            .unwrap();
        assert!(!points.is_empty());
        rails.push(json!({"selection":hex::encode(&p.0[row..row+8]),
            "points":points.into_iter().map(|at|hex::encode(&rail.0[at..at+32])).collect::<Vec<_>>()}));
    }
    rails
}

fn read_camera_fields(p: &Payload, e: usize, delta: usize, old: bool) -> Vec<String> {
    let mut values = Vec::new();
    values.push(hex::encode(
        &p.0[e + if old { 0x20 } else { 0x38 }..e + if old { 0x28 } else { 0x40 }],
    ));
    for (start, end) in [
        (0x28, 0x48),
        (0x88, 0x90),
        (0xD0, 0xF0),
        (0x100, 0x110),
        (0x150, 0x178),
        (0x1A8, 0x1B0),
    ] {
        values.push(hex::encode(&p.0[e + start + delta..e + end + delta]));
    }
    for offset in [0x178, 0x180, 0x188, 0x190, 0x198, 0x1A0, 0x230] {
        assert_eq!(
            p.u64(e + offset + delta).unwrap(),
            0,
            "unexpected camera extension"
        );
    }
    if !old {
        for offset in [0x40, 0x260, 0x268] {
            assert_eq!(
                p.u64(e + offset).unwrap(),
                0,
                "native camera extension is active"
            );
        }
    }
    values
}
