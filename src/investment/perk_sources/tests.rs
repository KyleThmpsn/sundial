use super::*;
use crate::catalog::{ItemDef, ItemPackageMetadata, SocketDef};

fn source(hash: u32, name: &str) -> PerkSource {
    PerkSource {
        hash,
        name: name.into(),
        type_name: "Trait".into(),
    }
}

#[test]
fn shared_effects_keep_all_aliases_searchable_without_claiming_one_name() {
    let sources: PerkSources = [
        (453, source(1, "Thorn Catalyst")),
        (453, source(2, "Masterwork Weapon")),
        (453, source(3, "Masterwork: Range")),
        (453, source(1, "Thorn Catalyst")),
        (421, source(4, "Outlaw")),
        (421, source(5, "Outlaw")),
    ]
    .into_iter()
    .collect();
    assert_eq!(sources.label(453), "Shared Effect 453");
    assert_eq!(
        sources.shared_label(453).as_deref(),
        Some("Shared Masterwork Effect")
    );
    assert_eq!(sources.shared_label(421), None);
    assert_eq!(sources.get(453).len(), 3);
    assert!(sources.matches_query(453, "thorn 453"));
    assert!(sources.matches_query(453, "masterwork range"));
    assert!(sources.matches_query(453, "00000002"));
    assert!(!sources.matches_query(421, "thorn"));
    assert_eq!(sources.label(421), "Effect 421 · From Outlaw");
    assert_eq!(
        sources.get(421).len(),
        2,
        "Same-named source hashes stay distinct"
    );
    assert_eq!(sources.label(999), "Effect 999");
    assert!(!sources.has_names(999));
}

#[test]
fn provenance_includes_optional_plugs_without_claiming_default_usage() {
    let metadata = |index| ItemPackageMetadata {
        weapon_pattern_index: Some(12),
        sandbox_perks: serde_json::from_value(serde_json::json!([{"perk_index": index}])).unwrap(),
        ..Default::default()
    };
    let catalog = InvestmentCatalog {
        catalog: Catalog::for_test(
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
        ),
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

#[test]
#[ignore = "requires SUNDIAL_TEST_INSTALL; verifies installed masterwork aliases"]
fn installed_masterwork_sources_do_not_become_thorn_identity() {
    let install = std::path::PathBuf::from(std::env::var_os("SUNDIAL_TEST_INSTALL").unwrap());
    let catalog = InvestmentCatalog::load(&install, false, |_| {}).unwrap();
    let sources = catalog.perk_sources();
    assert_eq!(sources.label(453), "Shared Effect 453");
    assert!(
        sources
            .get(453)
            .iter()
            .any(|source| source.name == "Thorn Catalyst")
    );
    assert!(
        sources
            .get(453)
            .iter()
            .any(|source| source.name.starts_with("Masterwork"))
    );
    assert!(sources.matches_query(453, "thorn catalyst"));
    assert_eq!(sources.label(463), "Effect 463 · From The Fundamentals");
}
