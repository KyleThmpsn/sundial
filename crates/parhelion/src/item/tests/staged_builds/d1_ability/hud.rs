//! Persisted resource references must preserve original named keys through both dictionaries.
use super::*;
use std::collections::BTreeMap;

fn names(config: &Payload, group_class: u32) -> BTreeMap<u16, u32> {
    let mut output = BTreeMap::new();
    let mut ordinal = 128u16;
    for group in config.array(8, 16, Some(group_class)).unwrap() {
        for row in config.array(group, 4, Some(0x80800070)).unwrap() {
            assert!(ordinal < 256);
            output.insert(ordinal, config.u32(row).unwrap());
            ordinal += 1;
        }
    }
    assert!(!output.is_empty());
    output
}

#[allow(clippy::cognitive_complexity)]
pub(super) fn readback(
    source: &Source,
    source_owner: &Payload,
    source_event: usize,
    native: &Payload,
    event: usize,
    stock: &PackageManager,
    staged: &PackageManager,
) -> serde_json::Value {
    let configs = source.classes(0x8080287D);
    assert_eq!(configs.len(), 1);
    let source_config = source.read(configs[0]).unwrap();
    let source_names = names(&source_config, 0x80804065);
    let configs = stock.get_all_by_reference(0x808047C1);
    assert_eq!(configs.len(), 1);
    let native_config = Payload(stock.read_tag(configs[0].0).unwrap());
    assert_eq!(staged.read_tag(configs[0].0).unwrap(), native_config.0);
    let native_names = names(&native_config, 0x80809F7F);
    assert_eq!(source_owner.u32(source_event + 0x1C).unwrap(), 8);
    assert_eq!(native.u32(event - 4).unwrap(), 0x80803F24);
    assert_eq!(native.u32(event + 0x30).unwrap(), 13);
    let live = usize::try_from(native.u64(event + 8).unwrap()).unwrap();
    assert_eq!(native.u32(live - 4).unwrap(), 0x80803F23);
    assert_eq!(native.u32(live).unwrap(), native.u32(event).unwrap());
    assert_eq!(native.u32(live + 4).unwrap(), 0x80803F24);
    assert_eq!(native.u64(live + 8).unwrap(), event as u64);
    assert_eq!(native.bytes::<40>(live + 0x18).unwrap(), [0; 40]);
    let from_tag = source_owner.u32(source_event + 0x20).unwrap();
    let to_tag = native.u32(event + 0x38).unwrap();
    assert!(
        stock.get_entry(TagHash(to_tag)).is_none(),
        "HUD resource must be private"
    );
    assert_eq!(source.reference(from_tag).unwrap(), 0x80802784);
    assert_eq!(
        staged.get_entry(TagHash(to_tag)).unwrap().reference,
        0x80803F26
    );
    let original = source.read(from_tag).unwrap();
    let translated = Payload(staged.read_tag(TagHash(to_tag)).unwrap());
    assert_eq!(translated.u64(0).unwrap(), translated.0.len() as u64);
    assert_eq!(original.u32(0x18).unwrap(), u32::MAX);
    assert_eq!(translated.u32(0x18).unwrap(), u32::MAX);
    assert_eq!(original.u32(0x1C).unwrap(), translated.u32(0x1C).unwrap());
    let mut expected = BTreeMap::new();
    for row in original.array(8, 4, Some(0x808022C4)).unwrap() {
        let name = source_names[&original.u16(row).unwrap()];
        assert!(
            expected
                .insert(name, original.u8(row + 2).unwrap())
                .is_none()
        );
    }
    let mut observed = BTreeMap::new();
    let mut previous = None;
    for row in translated.array(8, 4, Some(0x8080619B)).unwrap() {
        let key = translated.u16(row).unwrap();
        assert!(
            previous.is_none_or(|value| key > value),
            "HUD merge requires sorted unique keys"
        );
        previous = Some(key);
        let value = translated.u8(row + 2).unwrap();
        assert!(value <= 1);
        assert_eq!(translated.u8(row + 3).unwrap(), 0);
        assert!(observed.insert(native_names[&key], value).is_none());
    }
    assert_eq!(observed, expected);
    json!({"source_resource":from_tag,"native_resource":to_tag,"values_by_name":observed,
        "source_config_sha256":sha(&source_config.0),"native_config_sha256":sha(&native_config.0),
        "source_sha256":sha(&original.0),"native_sha256":sha(&translated.0)})
}
