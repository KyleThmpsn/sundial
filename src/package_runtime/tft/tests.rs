use super::*;

pub(super) fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 240];
    let path = b"content\\sandbox\\projectiles\\test.pattern.tft\0";
    bytes[128..128 + path.len()].copy_from_slice(path);
    bytes[16..24].copy_from_slice(&112_i64.to_le_bytes());
    bytes[24..32].copy_from_slice(&0x8152_82E1_u64.to_le_bytes());
    bytes[200..208].copy_from_slice(&(-72_i64).to_le_bytes());
    bytes[208..216].copy_from_slice(&0x1234_5678_9ABC_DEF0_u64.to_le_bytes());
    bytes
}

#[test]
fn paths_require_terminated_content_names_and_paired_valid_tag_lanes() {
    let mut bytes = fixture();
    let paths = content_paths(&bytes);
    assert_eq!(paths.len(), 1);
    let resolve = |lane| match lane {
        0x8152_82E1 | 0x1234_5678_9ABC_DEF0 => Some((0x8152_82E1, 0x8080_9C0F)),
        _ => None,
    };
    let refs = references(&bytes, &paths, resolve);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].0, 24);
    assert_eq!(refs[1].0, 208);
    bytes[16..24].copy_from_slice(&i64::MIN.to_le_bytes());
    bytes[208..216].fill(0);
    assert!(references(&bytes, &paths, resolve).is_empty());
    assert!(content_paths(b"content/test.tft").is_empty());
    assert!(content_paths(b"unrelated/test.tft\0").is_empty());
}

#[test]
fn native_paths_do_not_name_unpaired_or_unresolved_assets() {
    let bytes = fixture();
    assert!(references(&bytes, &content_paths(&bytes), |_| None).is_empty());
    let index = Index {
        references: vec![
            Reference {
                source: 1,
                source_class: 8,
                offset: 16,
                target: 2,
                target_class: 9,
                path: "content/b.tft".into(),
            },
            Reference {
                source: 3,
                source_class: 8,
                offset: 24,
                target: 2,
                target_class: 9,
                path: "content/b.tft".into(),
            },
        ],
        ..Index::default()
    };
    assert_eq!(index.names().get(&2).unwrap(), &["content/b.tft"]);
    assert!(!index.names().contains_key(&1));
    assert!(!index.names().contains_key(&3));
}

#[test]
#[ignore = "requires PARHELION_PROJECTILE_TEST_PACKAGES"]
fn native_tft_map_and_effect_catalog_preserve_evidence() {
    let packages =
        std::path::PathBuf::from(std::env::var_os("PARHELION_PROJECTILE_TEST_PACKAGES").unwrap());
    let manager = crate::package_authoring::open_shadowkeep_package_manager(&packages).unwrap();
    let names = cached(&packages, &manager, |current, total| {
        if current % 100_000 == 0 || current == total {
            eprintln!("Native names: {current}/{total}");
        }
    })
    .unwrap();
    assert!(names.errors.is_empty(), "{:?}", names.errors);
    assert!(names.references.iter().any(|reference| {
        reference.source == 0x80BC_2BBD
            && reference.target == 0x8152_82E1
            && reference.offset == 0x2B8
            && reference
                .path
                .ends_with("grenade_launcher_02_projectile_no_remote.pattern.tft")
    }));
    let dependencies = crate::sandbox_perk::dependencies::inspect(&manager, |_, _| {}).unwrap();
    let catalog =
        crate::sandbox_perk::projectile::catalog::inspect(&manager, &dependencies, &names).unwrap();
    assert!(catalog.errors.is_empty(), "{:?}", catalog.errors);
    for graph in [
        0x80BB_D0CE,
        0x80BB_DAD4,
        0x80BC_4158,
        0x80BC_41C0,
        0x80BC_88A3,
        0x80EF_0A73,
        0x80EF_5872,
    ] {
        let entry = catalog
            .entries
            .iter()
            .find(|entry| entry.graph == graph)
            .expect("observed projectile included");
        assert_eq!(
            entry.kind,
            crate::sandbox_perk::projectile::Kind::Projectile
        );
    }
    assert!(catalog.entries.iter().any(|entry| entry.kind
        == crate::sandbox_perk::projectile::Kind::Emitter
        && !entry.native_paths.is_empty()));
    let output = std::path::PathBuf::from("tmp/projectile-picker-20260910/native-map");
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join("tft-index.json"),
        serde_json::to_vec_pretty(&*names).unwrap(),
    )
    .unwrap();
    std::fs::write(
        output.join("effects.json"),
        serde_json::to_vec_pretty(&catalog).unwrap(),
    )
    .unwrap();
    eprintln!(
        "{} TFT paths, {} paired references, {} projectiles, {} emitters",
        names.paths.len(),
        names.references.len(),
        catalog
            .entries
            .iter()
            .filter(|entry| entry.kind == crate::sandbox_perk::projectile::Kind::Projectile)
            .count(),
        catalog
            .entries
            .iter()
            .filter(|entry| entry.kind == crate::sandbox_perk::projectile::Kind::Emitter)
            .count()
    );
}
