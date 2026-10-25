//! Follow the emitted selection through the untouched native movement controller.
use super::*;

pub(super) fn check(
    stock: &mut Reader,
    native: &mut Reader,
    source: &Payload,
    row: usize,
    report: &Value,
) -> CheckResult<Value> {
    assert_eq!(source.bytes::<4>(source.pointer(row + 32)?)?, [0, 1, 0, 0]);
    assert_eq!(report["route"], "native_air_evade");
    assert_eq!(number(&report["slot"])?, 4);
    let bank = number(&report["native_bank"])?;
    let key = number(&report["property_key"])?;
    let before = stock.tag(bank, Some(0x80809C36))?;
    let emitted = native.tag(bank, Some(0x80809C36))?;
    assert_eq!(
        before.0, emitted.0,
        "Air evade must not mutate the shared movement bank"
    );
    let properties = emitted.array(emitted.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let selected = properties
        .into_iter()
        .filter(|&r| emitted.u32(r + 16).ok() == Some(key))
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    let r = selected[0];
    assert_eq!(emitted.u32(r + 24)?, 255);
    assert_eq!(emitted.u32(r + 32)?, 255);
    assert_eq!(emitted.u64(r + 40)?, 0);
    let modifier = emitted.pointer(r + 48)?;
    assert_eq!(emitted.u32(modifier + 4)?, 0x80804121);
    let profile_key = emitted.u32(modifier + 16)?;
    let graph = stock.tag(number(&report["native_graph"])?, Some(0x80809C0F))?;
    let mut profiles = Vec::new();
    for component in graph.array(16, 12, None)? {
        let owner = graph.u32(component)?;
        let data = stock.tag(owner, Some(0x80809C36))?;
        let settings = data.pointer(24)?;
        if data.u32(settings - 4)? != 0x808044E0 {
            continue;
        }
        assert_eq!(native.tag(owner, Some(0x80809C36))?.0, data.0);
        let config = data.pointer(settings + 0x800)?;
        assert_eq!(data.u32(config - 4)?, 0x8080410F);
        for p in data.array(config + 16, 128, Some(0x80804106))? {
            if data.u32(p + 16)? != profile_key {
                continue;
            }
            assert_eq!(data.u8(p + 0x58)?, 2, "Air-only movement gate");
            assert!(
                data.array(p + 0x30, 8, None)?.is_empty(),
                "Unexpected profile conditions"
            );
            assert_eq!(data.u32(p + 0x70)?, 0x476D0367, "Air-evade input identity");
            assert_eq!(data.u32(p + 0x14)?, 0.5f32.to_bits());
            assert_eq!(data.u32(p + 0x18)?, 5.0f32.to_bits());
            assert_eq!(data.u32(p + 0x40)?, 0.5f32.to_bits());
            assert_eq!(data.u32(p + 0x44)?, 1.0f32.to_bits());
            stock.tag(data.u32(p + 0x50)?, Some(0x80809C0F))?;
            profiles.push(json!({"owner":owner,"key":profile_key,"offset":p}));
        }
    }
    assert_eq!(
        profiles.len(),
        1,
        "Selected air-evade profile is missing or ambiguous"
    );
    Ok(json!({"bank":bank,"key":key,"profile":profiles[0],"gameplay_verified":false}))
}
