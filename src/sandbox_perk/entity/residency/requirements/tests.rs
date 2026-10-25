use super::*;

#[test]
fn all_native_components_are_checked_without_named_bindings() {
    // There is deliberately no named-binding table in this isolated component-list fixture.
    let mut p = vec![0; 0x68];
    p[0x10..0x18].copy_from_slice(&2u64.to_le_bytes());
    p[0x18..0x20].copy_from_slice(&0x28u64.to_le_bytes());
    p[0x40..0x48].copy_from_slice(&2u64.to_le_bytes());
    p[0x48..0x4C].copy_from_slice(&0x8080_9C04u32.to_le_bytes());
    p[0x50..0x54].copy_from_slice(&21u32.to_le_bytes());
    p[0x5C..0x60].copy_from_slice(&22u32.to_le_bytes());
    assert_eq!(component_owners(&p).unwrap(), [21, 22].into());
    p.truncate(0x60);
    assert!(component_owners(&p).is_err());
}
