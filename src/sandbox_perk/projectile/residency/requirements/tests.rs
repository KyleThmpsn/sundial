use super::*;

#[test]
fn missing_owner_and_nested_layout_are_both_reported() {
    let required = [
        Requirement {
            tag: 21,
            referenced_by: 10,
            role: "component resource".into(),
        },
        Requirement {
            tag: 22,
            referenced_by: 10,
            role: "component resource".into(),
        },
        Requirement {
            tag: 31,
            referenced_by: 22,
            role: "component layout".into(),
        },
    ];
    let result = report(10, &[10, 21].into(), required.clone());
    assert_eq!(result.missing, required[1..]);
    let message = result.check("Projectile", true).unwrap_err();
    assert!(message.contains("2 resources"));
    assert!(message.contains("component layout 0x0000001F referenced by 0x00000016"));
    assert!(result.check("Emitter", false).is_err());
    assert_eq!(result.additions(), [22, 31].into());
}

#[test]
fn direct_reference_requires_graph_enrollment_even_when_components_are_loaded() {
    let result = report(
        10,
        &[21].into(),
        [Requirement {
            tag: 21,
            referenced_by: 10,
            role: "component resource".into(),
        }],
    );
    assert!(result.check("Copied graph", true).is_ok());
    assert!(result.check("Direct reference", false).is_err());
    assert_eq!(result.additions(), [10].into());
    let loaded = report(10, &[10, 21].into(), []);
    assert!(loaded.check("Direct reference", false).is_ok());
}

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
