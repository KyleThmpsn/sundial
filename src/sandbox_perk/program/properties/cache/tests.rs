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
    save(&cache, &saved).unwrap();
    let saved: Saved = serde_json::from_slice(&std::fs::read(cache).unwrap()).unwrap();
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
    assert_eq!(reads, [custom, new]);
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
}

#[test]
fn identical_bytes_in_a_changed_package_reuse_the_cached_decode() {
    let packages = tempfile::tempdir().unwrap();
    let file = packages.path().join("w64_test_0123_0.pkg");
    std::fs::write(&file, b"first").unwrap();
    let tag = TagHash::new(0x123, 1).0;
    let perks = vec![(1, tag)];
    let bytes = payload(42, 1.0);
    let (_, mut saved) = refresh(
        &Snapshot::read(packages.path()).unwrap(),
        None,
        perks.clone(),
        |_| Ok(bytes.clone()),
    );
    saved.actions.get_mut(&tag).unwrap().usage = Err("cached decode marker".into());
    std::fs::write(&file, b"changed another resource").unwrap();
    let (index, _) = refresh(
        &Snapshot::read(packages.path()).unwrap(),
        Some(&saved),
        perks,
        |_| Ok(bytes.clone()),
    );
    assert_eq!(
        index.issues,
        [format!("Action 0x{tag:08X}: cached decode marker")]
    );
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

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn installed_key_cache_is_reused_without_a_second_scan() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").expect("packages");
    let packages = Path::new(&packages);
    let manager = crate::package_authoring::open_shadowkeep_package_manager(packages).unwrap();
    let started = std::time::Instant::now();
    let index = cached(packages, &manager).unwrap();
    let first = started.elapsed();
    let started = std::time::Instant::now();
    let reused = cached_only(packages)
        .unwrap()
        .expect("keys without opening a package reader");
    assert!(Arc::ptr_eq(&index, &reused));
    let keys = KeyCatalog::from_index(&index, |_| Vec::new());
    assert!(!keys.property_keys().is_empty());
    assert!(!keys.removal_keys().is_empty());
    assert!(index.issues.is_empty(), "{:?}", index.issues);
    eprintln!(
        "{} actions, {} property keys, {} ending keys. First load {:?}, reused {:?}",
        index.actions.len(),
        keys.property_keys().len(),
        keys.removal_keys().len(),
        first,
        started.elapsed()
    );
}
