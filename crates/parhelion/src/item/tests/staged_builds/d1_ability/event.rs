//! Persisted animation and completion events retain dispatch and runtime binding contracts.
use super::*;

#[allow(clippy::cognitive_complexity)]
pub(super) fn readback(
    source: &Source,
    graph: &Payload,
    staged: &PackageManager,
) -> Vec<serde_json::Value> {
    let original = source.read(0x80A6A073).unwrap();
    let source_components = original.array(0x20, 12, Some(0x80800715)).unwrap();
    let native_components = graph.array(16, 12, Some(0x80809C04)).unwrap();
    assert_eq!(source_components.len(), native_components.len());
    let mut evidence = Vec::new();
    for (occurrence, (from, to)) in source_components
        .into_iter()
        .zip(native_components)
        .enumerate()
    {
        let source_tag = original.u32(from).unwrap();
        let source_owner = source.read(source_tag).unwrap();
        let source_definition = source_owner.pointer(24).unwrap();
        if source_owner.u32(source_definition - 4).unwrap() != 0x80800610 {
            continue;
        }
        let source_events = source_owner
            .array(source_definition + 0x180, 8, Some(0x80800629))
            .unwrap()
            .into_iter()
            .map(|row| source_owner.pointer(row).unwrap())
            .filter(|&at| matches!(source_owner.u32(at - 4).unwrap(), 0x808025C1 | 0x808027EA))
            .collect::<Vec<_>>();
        let tag = graph.u32(to).unwrap();
        let native = Payload(staged.read_tag(TagHash(tag)).unwrap());
        let (_, definition) = pair(&native, tag, (0x808084D7, 0x808084E9));
        let events = native
            .array(definition + 0x168, 24, Some(0x808093E6))
            .unwrap()
            .into_iter()
            .map(|row| native.pointer(row + 16).unwrap())
            .filter(|&at| matches!(native.u32(at - 4).unwrap(), 0x80803FE9 | 0x80804595))
            .collect::<Vec<_>>();
        assert_eq!(source_events.len(), events.len());
        for source_event in source_events {
            let name = source_owner.u32(source_event).unwrap();
            let candidates = events
                .iter()
                .copied()
                .filter(|&at| native.u32(at + 16).unwrap() == name)
                .collect::<Vec<_>>();
            assert_eq!(
                candidates.len(),
                1,
                "configured source animation names are unique within each owner"
            );
            let event = candidates[0];
            let live = usize::try_from(native.u64(event + 8).unwrap()).unwrap();
            if source_owner.u32(source_event - 4).unwrap() == 0x808027EA {
                assert_eq!(source_owner.u32(source_event + 0x1C).unwrap(), 14);
                assert_eq!(native.u32(event - 4).unwrap(), 0x80804595);
                assert_eq!(native.u32(event + 0x30).unwrap(), 19);
                assert_eq!(native.u32(live - 4).unwrap(), 0x80804594);
                assert_eq!(native.u32(live).unwrap(), tag);
                assert_eq!(native.u32(live + 4).unwrap(), 0x80804595);
                assert_eq!(native.u64(live + 8).unwrap(), event as u64);
                assert_eq!(native.bytes::<40>(live + 0x18).unwrap(), [0; 40]);
                assert_eq!(
                    native.u8(live + 0x40).unwrap(),
                    255,
                    "controller startup supplies the selected ability slot"
                );
                assert_eq!(native.bytes::<15>(live + 0x41).unwrap(), [0; 15]);
                evidence.push(json!({"occurrence":occurrence,"source_owner":source_tag,
                    "native_owner":tag,"source_event":source_event,"event":event,
                    "kind":"ability_completion","slot_binding":"controller_startup",
                    "native_owner_sha256":sha(&native.0)}));
                continue;
            }
            assert_eq!(native.u32(event + 0x30).unwrap(), 18);
            assert_eq!(source_owner.u32(source_event + 0x1C).unwrap(), 13);
            assert_eq!(
                source_owner.u32(source_event + 0x28).unwrap(),
                0,
                "qualified source extension is zero"
            );
            assert_ne!(
                native.u32(event + 0x38).unwrap(),
                source_owner.u32(source_event + 0x20).unwrap(),
                "imported events must not replace the stock action route"
            );
            assert!(!matches!(
                native.u32(event + 0x38).unwrap(),
                0 | 0x811C9DC5 | u32::MAX
            ));
            for (a, b) in [(0x24, 0x3C), (0x25, 0x3D), (0x26, 0x3F)] {
                assert_eq!(
                    native.u8(event + b).unwrap(),
                    source_owner.u8(source_event + a).unwrap()
                );
            }
            assert_eq!(
                native.u8(event + 0x3E).unwrap(),
                0,
                "no added native animation policy"
            );
            assert_eq!(
                native.bytes::<72>(event + 0x40).unwrap(),
                [0; 72],
                "no inherited duration override"
            );
            assert_eq!(native.u32(live - 4).unwrap(), 0x80803FE8);
            assert_eq!(native.u32(live).unwrap(), tag);
            assert_eq!(native.u32(live + 4).unwrap(), 0x80803FE9);
            assert_eq!(native.u64(live + 8).unwrap(), event as u64);
            assert_eq!(
                native.u64(live + 0x28).unwrap(),
                0,
                "no serialized source ticks"
            );
            assert_eq!(
                native.u64(live + 0x40).unwrap(),
                u64::MAX,
                "animation provider binds at startup"
            );
            assert_eq!(native.u64(live + 0x48).unwrap(), 0);
            assert_eq!(native.u64(live + 0x50).unwrap(), 0);
            evidence.push(
                json!({"occurrence":occurrence,"source_owner":source_tag,"native_owner":tag,
                "event":event,"source_event":source_event,"action":native.u32(event+0x38).unwrap(),
                "native_owner_sha256":sha(&native.0)}),
            );
        }
    }
    assert!(
        !evidence.is_empty(),
        "required source animation corpus is empty"
    );
    evidence
}
