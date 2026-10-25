use super::*;
use crate::sandbox_perk::program::properties::tests::payload;

#[test]
fn refresh_reuses_packages_across_restart_and_reads_only_changed_or_new_actions() {
    let packages = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let stock_package = packages.path().join("w64_test_0123_0.pkg");
    let custom_package = packages.path().join("w64_test_0456_0.pkg");
    std::fs::write(&stock_package, b"stock").unwrap();
    std::fs::write(&custom_package, b"custom").unwrap();
    let stock = TagHash::new(0x123, 1).0;
    let custom = TagHash::new(0x456, 1).0;
    let new = TagHash::new(0x456, 2).0;
    let perks = vec![(1, stock), (2, custom), (3, stock)];
    let before = Snapshot::read(packages.path()).unwrap();
    let mut reads = Vec::new();
    let (first, saved) = refresh(&before, None, perks.clone(), |tag| {
        reads.push(tag);
        Ok(payload(tag, 1.0))
    });
    assert_eq!(reads.len(), 2, "Shared assignments read an action once");
    let cache = storage.path().join("keys.json");
    let legacy: Saved = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert_eq!(
        refresh(&before, Some(&legacy), perks.clone(), |_| panic!(
            "Legacy observations must be reused"
        ))
        .0,
        first
    );
    save(&cache, &saved).unwrap();
    let saved: Saved =
        crate::package_runtime::cache_file::read(&std::fs::read(cache).unwrap()).unwrap();
    let (second, _) = refresh(&before, Some(&saved), perks.clone(), |_| {
        panic!("Warm cache must not read actions")
    });
    assert_eq!(first, second);
    std::fs::write(
        packages.path().join("w64_unrelated_0789_0.pkg"),
        b"new item icons",
    )
    .unwrap();
    let unrelated = Snapshot::read(packages.path()).unwrap();
    refresh(&unrelated, Some(&saved), perks.clone(), |_| {
        panic!("Unrelated packages must not invalidate actions")
    });
    std::fs::write(&custom_package, b"updated custom package").unwrap();
    let changed = Snapshot::read(packages.path()).unwrap();
    reads.clear();
    let (updated, saved) = refresh(
        &changed,
        Some(&saved),
        vec![(1, stock), (2, custom), (4, new)],
        |tag| {
            reads.push(tag);
            Ok(payload(tag, 4.0))
        },
    );
    assert_eq!(reads.len(), 2);
    assert_eq!(
        reads
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        [custom, new].into_iter().collect()
    );
    let keys = KeyCatalog::from_index(&updated, |_| Vec::new());
    assert_eq!(keys.property_key(stock).unwrap().values, [1.0]);
    assert_eq!(keys.property_key(custom).unwrap().values, [4.0]);
    let (removed, saved) = refresh(&changed, Some(&saved), vec![(1, stock)], |_| {
        panic!("Removing a perk does not change stock actions")
    });
    assert_eq!(removed.actions.len(), 1);
    assert_eq!(
        saved.actions.len(),
        1,
        "Removed actions must not linger in incremental cache"
    );
    if let Some(output) = std::env::var_os("SUNDIAL_CACHE_VERIFICATION_DIRECTORY") {
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("perk-keys.json"), serde_json::to_vec_pretty(&serde_json::json!({"changed_actions": reads, "stock_values": keys.property_key(stock).unwrap().values, "custom_values": keys.property_key(custom).unwrap().values, "remaining_actions": saved.actions.len()})).unwrap()).unwrap();
    }
}

#[test]
fn another_installation_cannot_reuse_observations_without_reading_its_actions() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    for dir in [&a, &b] {
        std::fs::write(dir.path().join("w64_test_0123_0.pkg"), b"package").unwrap();
    }
    let tag = TagHash::new(0x123, 1).0;
    let perks = vec![(1, tag)];
    let (_, saved) = refresh(
        &Snapshot::read(a.path()).unwrap(),
        None,
        perks.clone(),
        |_| Ok(payload(10, 1.0)),
    );
    let mut reads = 0;
    let (other, _) = refresh(
        &Snapshot::read(b.path()).unwrap(),
        Some(&saved),
        perks,
        |_| {
            reads += 1;
            Ok(payload(20, 1.0))
        },
    );
    assert_eq!(reads, 1);
    let keys = KeyCatalog::from_index(&other, |_| Vec::new());
    assert!(keys.property_key(10).is_none());
    assert!(keys.property_key(20).is_some());
}
