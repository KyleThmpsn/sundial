//! Follow persisted constructors and cleanup targets, including non-root helper objects.
use super::*;

#[allow(clippy::cognitive_complexity)]
pub(super) fn readback(
    source: &Source,
    graph: &Payload,
    staged: &PackageManager,
) -> Vec<serde_json::Value> {
    let original = source.read(0x80A6A073).unwrap();
    let from = original.array(0x20, 12, Some(0x80800715)).unwrap();
    let to = graph.array(16, 12, Some(0x80809C04)).unwrap();
    assert_eq!(from.len(), to.len());
    let mut evidence = Vec::new();
    for (occurrence, (a, b)) in from.into_iter().zip(to).enumerate() {
        let source_tag = original.u32(a).unwrap();
        let source_owner = source.read(source_tag).unwrap();
        let rows = source_owner.array(0x30, 16, Some(0x80800930)).unwrap();
        if rows.is_empty() {
            continue;
        }
        let tag = graph.u32(b).unwrap();
        let native = Payload(staged.read_tag(TagHash(tag)).unwrap());
        let runtime = native.pointer(16).unwrap();
        let boundary = native.pointer(24).unwrap();
        let translated = native.array(0x30, 16, Some(0x808091A4)).unwrap();
        assert!(
            !translated.is_empty(),
            "component initialization was dropped"
        );
        let mut readback = Vec::new();
        for row in translated {
            let target = native.pointer(row).unwrap();
            assert!(target >= runtime && target + 16 <= boundary);
            let definition = usize::try_from(native.u64(target + 8).unwrap()).unwrap();
            assert_eq!(native.u32(target).unwrap(), tag);
            assert_eq!(native.u32(definition).unwrap(), tag);
            assert_eq!(native.u64(definition + 8).unwrap(), target as u64);
            let concrete = native.u32(definition + 4).unwrap();
            let implementation = native.u32(row + 8).unwrap();
            assert!(sundial::package_authoring::runtime::native_type_inherits(
                concrete,
                implementation
            ));
            let constructor = native.u16(row + 12).unwrap();
            let cleanup = native.u16(row + 14).unwrap();
            assert!(constructor != u16::MAX || cleanup != u16::MAX);
            assert!(constructor == u16::MAX || constructor <= i16::MAX as u16);
            assert!(cleanup == u16::MAX || cleanup <= i16::MAX as u16);
            readback.push(json!({"target":target,"class":concrete,
                "implementation":implementation,"constructor":constructor,"cleanup":cleanup}));
        }
        let source_class = source_owner
            .u32(source_owner.pointer(16).unwrap() - 4)
            .unwrap();
        if source_class == 0x8080079A {
            assert_eq!(readback.len(), 1);
            assert_eq!(readback[0]["target"], runtime);
            assert_eq!(readback[0]["implementation"], 0x808084D7u32);
            assert_eq!(readback[0]["constructor"], 0);
            assert_eq!(readback[0]["cleanup"], 2);
        }
        if source_class == 0x808025B8 {
            assert!(
                readback.iter().any(|row| row["target"] != runtime),
                "controller's embedded lifecycle targets were dropped"
            );
            let root = readback
                .iter()
                .filter(|row| row["target"] == runtime)
                .collect::<Vec<_>>();
            assert_eq!(root.len(), 2);
            assert_eq!(root[0]["implementation"], 0x80803BB7u32);
            assert_eq!(root[0]["constructor"], 13);
            assert_eq!(root[0]["cleanup"], 15);
            assert_eq!(root[1]["implementation"], 0x80804176u32);
            assert_eq!(root[1]["constructor"], 0);
            assert_eq!(root[1]["cleanup"], 1);
            let methods = readback
                .iter()
                .map(|row| {
                    (
                        row["implementation"].as_u64().unwrap() as u32,
                        row["constructor"].as_u64().unwrap() as u16,
                        row["cleanup"].as_u64().unwrap() as u16,
                    )
                })
                .collect::<Vec<_>>();
            // Original Shadowkeep 4147 owner proves that shape-query cleanup precedes
            // the containing 422D constructor. D1 combines those responsibilities in 234A.
            assert_eq!(
                methods,
                [
                    (0x80804590, u16::MAX, 0),
                    (0x80804589, u16::MAX, 1),
                    (0x808092A4, u16::MAX, 0),
                    (0x8080422D, 0, u16::MAX),
                    (0x80804590, u16::MAX, 0),
                    (0x80803BB7, 13, 15),
                    (0x80804176, 0, 1),
                ]
            );
            assert_eq!(readback[2]["target"], runtime + 0xD30);
            assert_eq!(readback[3]["target"], runtime + 0xCF0);
        }
        evidence.push(
            json!({"occurrence":occurrence,"source":source_tag,"native":tag,
            "source_count":rows.len(),"methods":readback,"sha256":sha(&native.0)}),
        );
    }
    assert!(!evidence.is_empty(), "required lifecycle corpus is empty");
    evidence
}
