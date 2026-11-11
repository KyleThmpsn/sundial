//! Independent feedback-event readback. Each staged kind-12 event must reproduce one source
//! kind-7 event: its program block, fallback, target and stop bytes, and the complete feedback
//! resource it names, compared word by word with array markers and row classes normalized and
//! member references replaced by their members. A delay, a stock or shared feedback resource, a
//! dropped member, a donor program or a native-only field with another value is a failure.
use super::*;
use serde_json::Value;

/// Array markers and member-row classes of the two builds.
const MARKERS: [u32; 2] = [0x80800184, 0x80809FBD];
const ROWS: [u32; 2] = [0x808022E5, 0x80803D95];
/// Feedback member kinds, source and native.
const MEMBERS: [(u32, u32); 4] = [
    (0x8080272B, 0x80803E94),
    (0x80802668, 0x80803EA5),
    (0x80802099, 0x80803EA0),
    (0x808024A4, 0x80803EAA),
];

fn form(payload: &Payload, member: &mut dyn FnMut(u32) -> Option<Value>) -> Value {
    assert_eq!(payload.u64(0).unwrap(), payload.0.len() as u64);
    let aligned = payload.0.len() - payload.0.len() % 4;
    let mut words = Vec::new();
    for at in (0..aligned).step_by(4) {
        let word = payload.u32(at).unwrap();
        words.push(if MARKERS.contains(&word) {
            json!("marker")
        } else if ROWS.contains(&word) {
            json!("row")
        } else if let Some(member) = member(word) {
            member
        } else {
            json!(format!("{word:08X}"))
        });
    }
    json!({"words":words,"tail":hex::encode(&payload.0[aligned..])})
}

fn source_form(source: &parhelion_import::d1_roi::Source, tag: u32) -> Value {
    assert_eq!(source.reference(tag).unwrap(), 0x80802637);
    form(&source.read(tag).unwrap(), &mut |word| {
        let class = source.reference(word).ok()?;
        MEMBERS
            .iter()
            .any(|&(from, _)| from == class)
            .then(|| form(&source.read(word).unwrap(), &mut |_| None))
    })
}

fn native_form(staged: &PackageManager, stock: &PackageManager, tag: u32) -> Value {
    assert!(
        stock.get_entry(TagHash(tag)).is_none(),
        "feedback event names a stock resource"
    );
    assert_eq!(
        staged.get_entry(TagHash(tag)).unwrap().reference,
        0x80803EB0
    );
    form(
        &Payload(staged.read_tag(TagHash(tag)).unwrap()),
        &mut |word| {
            let class = staged.get_entry(TagHash(word))?.reference;
            MEMBERS.iter().any(|&(_, to)| to == class).then(|| {
                assert!(
                    stock.get_entry(TagHash(word)).is_none(),
                    "feedback member is stock"
                );
                form(
                    &Payload(staged.read_tag(TagHash(word)).unwrap()),
                    &mut |_| None,
                )
            })
        },
    )
}

fn program(p: &Payload, at: usize, source: bool) -> Value {
    let code = p.array_range(at, 1, Some(0x80800009)).unwrap();
    let constants = p.array(at + 16, 16, Some(0x80800090)).unwrap();
    let slots = p.array(at + 48, 4, Some(0x80800007)).unwrap();
    let inputs = p.u64(at + 32).unwrap();
    if source {
        assert_eq!(slots.len() as u64, inputs, "source builtin slots");
        assert!(slots.iter().all(|&slot| p.u32(slot).unwrap() == u32::MAX));
    } else {
        assert!(slots.is_empty(), "builtin feedback input became a slot");
    }
    json!({"code":hex::encode(&p.0[code]),"inputs":inputs,"outputs":p.u32(at + 40).unwrap(),
        "word_44":p.u32(at + 44).unwrap(),
        "constants":constants.into_iter().map(|row| hex::encode(&p.0[row..row + 16])).collect::<Vec<_>>()})
}

/// Every source kind-7 event of one source sequence.
pub(super) fn source(source: &parhelion_import::d1_roi::Source, owner: &Payload) -> Vec<Value> {
    let d = owner.pointer(24).unwrap();
    let mut events = Vec::new();
    for field in [0x170, 0x180] {
        for row in owner.array(d + field, 8, Some(0x80800629)).unwrap() {
            let e = owner.pointer(row).unwrap();
            if owner.u32(e - 4).unwrap() != 0x80802891 {
                continue;
            }
            assert_eq!(owner.u32(e + 0x1C).unwrap(), 7);
            assert!(owner.0[e + 0x2C..e + 0x88].iter().all(|b| *b == 0));
            assert!(owner.0[e + 0x30E..e + 0x310].iter().all(|b| *b == 0));
            let fallback = owner.u32(e + 0x308).unwrap();
            events.push(json!({
                "front":hex::encode(&owner.0[e + 0x20..e + 0x28]),
                "feedback":source_form(source, owner.u32(e + 0x28).unwrap()),
                "programs":(0..10).map(|i| program(owner, e + 0x88 + i * 0x40, true)).collect::<Vec<_>>(),
                "fallback":(fallback != u32::MAX).then(|| source_form(source, fallback)),
                "mode":owner.u8(e + 0x30C).unwrap(),"stop":owner.u8(e + 0x30D).unwrap()}));
        }
    }
    events
}

/// One staged kind-12 event, with its native-only fields checked.
pub(super) fn native(
    owner: &Payload,
    e: usize,
    staged: &PackageManager,
    stock: &PackageManager,
) -> Value {
    assert_eq!(owner.u16(e + 0x30).unwrap(), 12);
    let instance = usize::try_from(owner.u64(e + 8).unwrap()).unwrap();
    assert_eq!(owner.u32(e + 4).unwrap(), 0x80803D8C);
    assert_eq!(owner.u32(instance - 4).unwrap(), 0x80803D8C);
    assert_eq!(owner.u64(instance + 8).unwrap(), e as u64);
    assert_eq!(owner.u32(e + 0x38).unwrap(), 0xFF);
    assert_eq!(owner.u32(e + 0x3C).unwrap(), 0);
    assert!(owner.0[e + 0x4C..e + 0xA8].iter().all(|b| *b == 0));
    assert_eq!(owner.u16(e + 0x32E).unwrap(), 0, "transform source differs");
    // The native-only program is the constant one of the stock witness.
    assert_eq!(
        program(owner, e + 0x330, false),
        json!({"code":"34003e00","inputs":1,"outputs":1,"word_44":0,
            "constants":["0000803f0000803f0000803f0000803f"]})
    );
    let fallback = owner.u32(e + 0x328).unwrap();
    json!({
        "front":hex::encode(&owner.0[e + 0x40..e + 0x48]),
        "feedback":native_form(staged, stock, owner.u32(e + 0x48).unwrap()),
        "programs":(0..10).map(|i| program(owner, e + 0xA8 + i * 0x40, false)).collect::<Vec<_>>(),
        "fallback":(fallback != u32::MAX).then(|| native_form(staged, stock, fallback)),
        "mode":owner.u8(e + 0x32C).unwrap(),"stop":owner.u8(e + 0x32D).unwrap()})
}
