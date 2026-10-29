//! Follow the emitted query action and compare its allocation shape to an independent native owner.
use super::*;

#[allow(clippy::cognitive_complexity)]
pub(super) fn readback(owner: &Payload, tag: u32, witness: &Payload) -> (usize, serde_json::Value) {
    let live = owner.pointer(16).unwrap();
    let definition = usize::try_from(owner.u64(live + 8).unwrap()).unwrap();
    let boundary = owner.pointer(24).unwrap();
    let actions = owner.array(live + 0xD10, 48, Some(0x80804244)).unwrap();
    let definitions = owner
        .array(definition + 0x950, 24, Some(0x80804245))
        .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(definitions.len(), actions.len());
    let wrapper = actions[0];
    let settings = definitions[0];
    let action = owner.pointer(wrapper + 32).unwrap();
    let action_definition = owner.pointer(settings + 16).unwrap();
    assert_eq!(owner.u32(action - 4).unwrap(), 0x80803C7D);
    assert_eq!(owner.u32(action_definition - 4).unwrap(), 0x80803C7E);
    let recipient = action + 0x20;
    let recipient_definition = action_definition + 0x18;
    for (at, twin, runtime_class, definition_class, parent, size) in [
        (wrapper, settings, 0x80804244, 0x80804245, live + 0xCF0, 48),
        (
            action,
            action_definition,
            0x80803C7D,
            0x80803C7E,
            wrapper,
            0xF0,
        ),
        (
            recipient,
            recipient_definition,
            0x80803B36,
            0x80803B28,
            action,
            0xD0,
        ),
        (
            recipient + 0x50,
            recipient_definition + 0x78,
            0x80808518,
            0x80808519,
            recipient,
            0x30,
        ),
        (
            recipient + 0xA0,
            recipient_definition + 0x18,
            0x808089F5,
            0x808089F6,
            recipient,
            0x30,
        ),
    ] {
        assert!(
            at >= live && at + size <= boundary,
            "runtime must survive instance copy"
        );
        assert_eq!(owner.u32(at).unwrap(), tag);
        assert_eq!(owner.u32(at + 4).unwrap(), definition_class);
        assert_eq!(owner.u64(at + 8).unwrap(), twin as u64);
        assert_eq!(owner.u32(twin).unwrap(), tag);
        assert_eq!(owner.u32(twin + 4).unwrap(), runtime_class);
        assert_eq!(owner.u64(twin + 8).unwrap(), at as u64);
        assert_eq!(owner.pointer(at + 16).unwrap(), parent);
    }
    // This candidate supplies native solar presentation without borrowing a donor's damage.
    assert_eq!(owner.u32(recipient + 0x40).unwrap(), u32::MAX);
    assert_eq!(owner.u32(recipient_definition + 0xA0).unwrap(), u32::MAX);
    let rows = owner.array(recipient + 0x88, 20, Some(0x80803B2C)).unwrap();
    let native_rows = witness.array(0x330 + 0x88, 20, Some(0x80803B2C)).unwrap();
    assert!(!native_rows.is_empty());
    assert_eq!(rows.len(), native_rows.len());
    assert!(rows[0] >= live && rows.last().unwrap() + 20 <= boundary);
    assert_eq!(owner.u32(recipient + 0x98).unwrap(), 0);
    let native_init = witness.array(48, 16, Some(0x808091A4)).unwrap();
    assert!(native_init.iter().any(|&at| {
        witness.pointer(at).unwrap() == 0x330
            && witness.u32(at + 8).unwrap() == 0x80803B36
            && witness.u16(at + 12).unwrap() == 0
            && witness.u16(at + 14).unwrap() == 1
    }));
    (
        recipient,
        json!({"wrapper":wrapper,"action":action,"recipient":recipient,
        "definition":action_definition,"tracking_capacity":rows.len(),"mutable_boundary":boundary,
        "witness":0x80BC2BF4u32,"witness_sha256":sha(&witness.0),"damage_resource":null}),
    )
}

fn children(payload: &Payload, field: usize) -> Vec<serde_json::Value> {
    payload
        .array(field, 40, Some(0x80808852))
        .unwrap()
        .into_iter()
        .map(|at| {
            assert_eq!(
                payload.u64(at + 8).unwrap(),
                0,
                "unexpected indirect action allocation"
            );
            json!({"name":payload.u32(at).unwrap(),"class":payload.u32(at+16).unwrap(),
            "count":payload.u32(at+20).unwrap(),"children":children(payload,at+24)})
        })
        .collect()
}

pub(super) fn allocation(payload: &Payload, witness: &Payload) {
    assert_ne!(
        payload.u64(16).unwrap(),
        0,
        "common base allocation is required"
    );
    let actual = children(payload, 32);
    let mut native = children(witness, 32);
    assert_eq!(native.len(), 1);
    assert_eq!(native[0]["name"], 0x562C8525u32);
    // The same embedded helper occupies a differently named member in the retained controller.
    native[0]["name"] = json!(0xC3BA2CF7u32);
    assert_eq!(actual, native);
    assert_eq!(payload.u64(0).unwrap(), payload.0.len() as u64);
}
