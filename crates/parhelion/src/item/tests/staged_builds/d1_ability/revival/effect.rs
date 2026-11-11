//! Read a staged private buff graph back against the source effect graph it translates.
//! Owner order, status policy and lifetime, modifier rows, receiver rows, channel names and
//! sequence signatures come from the source packages. Only owner class 80800372 may be absent.
use super::*;
mod hud;

fn instance(payload: &Payload) -> (usize, usize) {
    let live = payload.pointer(16).unwrap();
    (
        live,
        usize::try_from(payload.u64(live + 8).unwrap()).unwrap(),
    )
}

fn bytes(payload: &Payload, at: usize, stride: usize, class: u32) -> Vec<Vec<u8>> {
    payload
        .array(at, stride, Some(class))
        .unwrap()
        .into_iter()
        .map(|row| payload.0[row..row + stride].to_vec())
        .collect()
}

fn lookup(payload: &Payload, at: usize) -> Vec<[u32; 3]> {
    payload
        .array(at, 12, None)
        .unwrap()
        .into_iter()
        .map(|row| {
            [
                payload.u32(row).unwrap(),
                payload.u32(row + 4).unwrap(),
                payload.u32(row + 8).unwrap(),
            ]
        })
        .collect()
}

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub(super) fn readback(
    source: &Source,
    staged: &PackageManager,
    stock: &PackageManager,
    source_tag: u32,
    tag: u32,
) -> serde_json::Value {
    assert!(
        stock.get_entry(TagHash(tag)).is_none(),
        "buff graph is stock"
    );
    assert_eq!(
        staged.get_entry(TagHash(tag)).unwrap().reference,
        0x80809C0F
    );
    let graph = Payload(staged.read_tag(TagHash(tag)).unwrap());
    let source_graph = source.read(source_tag).unwrap();
    let source_owners = source_graph
        .array(0x20, 12, Some(0x80800715))
        .unwrap()
        .into_iter()
        .map(|at| source_graph.u32(at).unwrap())
        .filter(|&owner| {
            let p = source.read(owner).unwrap();
            p.u32(p.pointer(24).unwrap() - 4).unwrap() != 0x8080046A
        })
        .collect::<Vec<_>>();
    let owners = graph
        .array(16, 12, Some(0x80809C04))
        .unwrap()
        .into_iter()
        .map(|at| graph.u32(at).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(owners.len(), source_owners.len(), "buff graph owner count");
    let mut evidence = Vec::new();
    let mut source_sequences = BTreeMap::new();
    let mut sequences = BTreeMap::new();
    for (&owner, &source_owner) in owners.iter().zip(&source_owners) {
        assert!(
            stock.get_entry(TagHash(owner)).is_none(),
            "stock buff owner"
        );
        assert_eq!(
            staged.get_entry(TagHash(owner)).unwrap().reference,
            0x80809C36
        );
        let p = Payload(staged.read_tag(TagHash(owner)).unwrap());
        let s = source.read(source_owner).unwrap();
        let (live, definition) = instance(&p);
        let (source_live, sd) = (s.pointer(16).unwrap(), s.pointer(24).unwrap());
        let (native_class, source_class) = (p.u32(live - 4).unwrap(), s.u32(sd - 4).unwrap());
        let row = match (native_class, source_class) {
            (0x80804209, 0x80802698) => {
                let hud = sd + 0xF0;
                let duration = hud + 0x160;
                let controller = duration + 0xB8;
                let lifetime = duration + 0x138;
                let h = definition + 0xE0;
                let d = h + 0x148;
                let e = d + 0x128;
                assert_eq!(
                    p.bytes::<20>(d + 0x98).unwrap(),
                    s.bytes::<20>(duration + 0xA0).unwrap(),
                    "status Duration policy"
                );
                assert_eq!(
                    p.bytes::<8>(d + 0x120).unwrap(),
                    s.bytes::<8>(controller + 0x78).unwrap(),
                    "status Timer flag"
                );
                assert_eq!(p.u8(definition + 0x58).unwrap(), s.u8(sd + 0xB0).unwrap());
                assert_eq!(
                    p.f32(definition + 0x5C).unwrap(),
                    i32::from_le_bytes(s.bytes::<4>(hud + 0x8C).unwrap()) as f32,
                    "status competition priority"
                );
                let display = hud::readback(
                    source,
                    staged,
                    stock,
                    s.u32(hud + 0x88).unwrap(),
                    p.u32(h + 0x80).unwrap(),
                );
                let schema = p.pointer(24).unwrap();
                assert_eq!(
                    p.u32(schema + 0x160).unwrap(),
                    p.u32(h + 0x80).unwrap(),
                    "sealed status HUD identity"
                );
                assert_eq!(
                    p.bytes::<8>(h + 0x84).unwrap(),
                    s.bytes::<8>(hud + 0x8C).unwrap()
                );
                let code = |payload: &Payload, at| bytes(payload, at, 1, 0x80800009).concat();
                assert_eq!(
                    code(&p, e + 0x10),
                    code(&s, lifetime + 24),
                    "lifetime program"
                );
                assert_eq!(
                    bytes(&p, e + 0x20, 16, 0x80800090),
                    bytes(&s, lifetime + 40, 16, 0x80800090),
                    "lifetime constants"
                );
                let source_inputs = s.array(lifetime + 72, 48, Some(0x80800927)).unwrap();
                let inputs = p.array(e + 0x40, 40, Some(0x80809789)).unwrap();
                assert_eq!(inputs.len(), source_inputs.len());
                for (&to, &from) in inputs.iter().zip(&source_inputs) {
                    assert_eq!(p.u32(to + 0x20).unwrap(), s.u32(from + 40).unwrap());
                }
                json!({"family":"status","duration":hex::encode(p.bytes::<20>(d + 0x98).unwrap()),
                    "lifetime":hex::encode(code(&p, e + 0x10)),"inputs":inputs.len(),"hud":display})
            }
            (0x80803AFD, 0x808020DE) => {
                let source_rows = s.array(sd + 0x60, 96, Some(0x8080205F)).unwrap();
                let rows = p.array(definition + 88, 88, Some(0x80803B06)).unwrap();
                assert_eq!(rows.len(), source_rows.len(), "modifier row count");
                let mut values = Vec::new();
                for (&to, &from) in rows.iter().zip(&source_rows) {
                    let component = s.u8(from + 0x54).unwrap();
                    let operation = s.u8(from + 0x34).unwrap();
                    let value = s.f32(from + 0x30).unwrap();
                    // D1 applies a signed change to the multiplier, native the final factor.
                    let amount = match operation {
                        0 => value,
                        _ if value > 0.0 => 1.0 + value,
                        _ => 1.0 / (1.0 - value),
                    };
                    assert_eq!(p.u8(to + 76).unwrap(), component);
                    assert_eq!(p.u8(to + 44).unwrap(), operation);
                    assert_eq!(p.f32(to + 40).unwrap(), amount);
                    if component == 9 {
                        assert_eq!(p.u16(to + 72).unwrap(), s.u16(from + 0x50).unwrap());
                        assert_eq!(p.u16(to + 74).unwrap(), s.u16(from + 0x52).unwrap());
                    }
                    values
                        .push(json!({"component":component,"operation":operation,"amount":amount}));
                }
                json!({"family":"modifier","rows":values})
            }
            (0x80808BF5, 0x808005A8) => {
                let source_rows = s.array(sd + 0x90, 0x50, Some(0x80800884)).unwrap();
                let source_values = s.array(source_live + 0x30, 0x28, Some(0x808004E2)).unwrap();
                let rows = p.array(definition + 0x78, 0x40, Some(0x80808BFA)).unwrap();
                let values = p.array(live + 0x30, 0x30, Some(0x80808BF9)).unwrap();
                assert_eq!(rows.len(), source_rows.len(), "receiver row count");
                assert_eq!(values.len(), source_values.len());
                for i in 0..rows.len() {
                    assert_eq!(
                        p.u32(rows[i] + 0x28).unwrap(),
                        s.u32(source_rows[i] + 0x30).unwrap()
                    );
                    assert_eq!(
                        p.f32(values[i] + 0x20).unwrap(),
                        s.f32(source_values[i] + 0x20).unwrap(),
                        "receiver initial scalar"
                    );
                }
                json!({"family":"receiver","rows":rows.len()})
            }
            (0x8080979F, 0x808008F6) => {
                let names = |payload: &Payload, at, class| {
                    payload
                        .array(at, 112, Some(class))
                        .unwrap()
                        .into_iter()
                        .map(|row| payload.u32(row).unwrap())
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    names(&p, definition + 0xD8, 0x808097A1),
                    names(&s, sd + 0xD8, 0x80800883),
                    "channel declarations"
                );
                assert_eq!(lookup(&p, definition + 0x108), lookup(&s, sd + 0x108));
                assert_eq!(lookup(&p, definition + 0x118), lookup(&s, sd + 0x118));
                json!({"family":"channel","declarations":names(&p, definition + 0xD8, 0x808097A1)})
            }
            (0x808084D7, 0x80800610) => {
                let count = |field| {
                    p.array(definition + field, 24, Some(0x808093E6))
                        .unwrap()
                        .len()
                };
                let shape = signature(count(0x158), count(0x168));
                let source_count = |field| s.array(sd + field, 8, Some(0x80800629)).unwrap().len();
                // An untimed empty parallel control schedules nothing and is not emitted.
                let source_controls = s
                    .array(sd + 0x160, 8, Some(0x80800629))
                    .unwrap()
                    .into_iter()
                    .filter(|&row| {
                        let control = s.pointer(row).unwrap();
                        s.u64(control + 0x20).unwrap() != 0
                    })
                    .count();
                let source_shape =
                    signature(source_controls, source_count(0x170) + source_count(0x180));
                let mut source_parallel = s
                    .array(sd + 0x160, 8, Some(0x80800629))
                    .unwrap()
                    .into_iter()
                    .map(|row| s.pointer(row).unwrap())
                    .filter(|&at| {
                        s.u32(at - 4).unwrap() == 0x80800691 && s.u64(at + 0x20).unwrap() != 0
                    })
                    .map(|at| {
                        (
                            s.u32(at).unwrap(),
                            s.u32(at + 8).unwrap(),
                            s.bytes::<16>(at + 12).unwrap(),
                            s.u8(at + 0x1E).unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                let mut native_parallel = p
                    .array(definition + 0x158, 24, Some(0x808093E6))
                    .unwrap()
                    .into_iter()
                    .map(|row| p.pointer(row + 16).unwrap())
                    .filter(|&at| p.u32(at - 4).unwrap() == 0x808093D7)
                    .map(|at| {
                        (
                            p.u32(at + 0x10).unwrap(),
                            p.u32(at + 0x18).unwrap(),
                            p.bytes::<16>(at + 0x1C).unwrap(),
                            p.u8(at + 0x32).unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                source_parallel.sort();
                native_parallel.sort();
                assert_eq!(
                    native_parallel, source_parallel,
                    "parallel timing and repeat policy"
                );
                *sequences.entry(shape).or_insert(0usize) += 1;
                *source_sequences.entry(source_shape).or_insert(0usize) += 1;
                json!({"family":"sequence","signature":shape,"parallel_controls":native_parallel})
            }
            (native_class, _) => {
                // Generated networks keep their native class. Their source owner is the
                // generated network derived from 80800362.
                assert!(
                    native_class == 0x808090E6
                        || staged
                            .read_tag(TagHash(native_class))
                            .is_ok_and(|schema| Payload(schema).u32(16).ok() == Some(0x808090E6)),
                    "unexpected buff owner {native_class:08X} for source {source_class:08X}"
                );
                let schema = source.read(source_class).unwrap();
                assert_eq!(schema.u32(16).unwrap(), 0x80800362);
                json!({"family":"network"})
            }
        };
        let allocation = TagHash(p.u32(0x44).unwrap());
        assert_eq!(staged.get_entry(allocation).unwrap().reference, 0x80809BBB);
        if native_class == 0x808084D7 {
            assert!(
                stock.get_entry(allocation).is_none(),
                "a translated sequence keeps a stock allocation"
            );
        }
        evidence.push(
            json!({"owner":owner,"source":source_owner,"translation":row,
            "allocation":allocation.0,"private_allocation":stock.get_entry(allocation).is_none()}),
        );
    }
    assert_eq!(sequences, source_sequences, "buff sequence occurrences");
    let replication_tag = graph.u32(0x88).unwrap();
    assert!(stock.get_entry(TagHash(replication_tag)).is_none());
    let replication = Payload(staged.read_tag(TagHash(replication_tag)).unwrap());
    assert_eq!(replication.u32(8).unwrap(), tag);
    assert_eq!(
        replication.array(48, 32, Some(0x80809BB8)).unwrap().len(),
        owners.len()
    );
    json!({"graph":tag,"source":source_tag,"owners":evidence,"replication":replication_tag,
        "sha256":sha(&graph.0)})
}
