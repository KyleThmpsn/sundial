use super::*;
use crate::catalog::{ItemDef, ItemPackageMetadata, SocketDef};

#[test]
fn provenance_includes_optional_plugs_without_claiming_default_usage() {
    let metadata = |index| ItemPackageMetadata {
        weapon_pattern_index: Some(12),
        sandbox_perks: serde_json::from_value(serde_json::json!([{"perk_index": index}])).unwrap(),
        ..Default::default()
    };
    let catalog = InvestmentCatalog {
        catalog: std::sync::Arc::new(Catalog::for_test(
            vec![ItemDef {
                hash: 1,
                name: "Test Weapon".into(),
                type_name: "Auto Rifle".into(),
                bucket_hash: 1_498_876_634,
                class_type: 3,
                default_plugs: vec![Some("0x65".into())],
                sockets: vec![SocketDef {
                    socket_type: 700,
                    allowed: vec![101, 102],
                    ..Default::default()
                }],
                abilities: Default::default(),
            }],
            [
                (1, metadata(450)),
                (101, metadata(421)),
                (102, metadata(453)),
            ]
            .into(),
        )),
        authorable_weapon_stat_indices: Vec::new(),
    };
    let references = catalog.perk_sources();
    assert_eq!(references.get(450)[0].hash, 1);
    assert_eq!(references.get(421)[0].hash, 101);
    assert_eq!(references.get(453)[0].hash, 102);
    assert!(
        !catalog
            .perk_pattern_uses()
            .iter()
            .any(|usage| usage.perk_index == 453)
    );
}
