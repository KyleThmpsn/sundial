use super::*;

/// The fixture's 64-bit lane, the only value the lane table of these tests knows.
const FIXTURE_LANE: u64 = 0x1234_5678_9ABC_DEF0;

fn known_lane(lane: u64) -> bool {
    lane == FIXTURE_LANE
}

fn targets(tags: impl IntoIterator<Item = u32>, lanes: &[(u64, u32)]) -> EntityTargets {
    EntityTargets {
        tags: tags.into_iter().collect(),
        lanes: lanes.iter().copied().collect(),
    }
}

fn target_tags(shard: &Shard, targets: &EntityTargets) -> Vec<u32> {
    entity_references(shard, targets)
        .into_iter()
        .map(|reference| reference.target)
        .collect()
}

/// A one-file package directory and an empty cache directory, with the shard path of
/// package `0x01bb` in that cache. Both directories live as long as the returned guards.
fn package_and_cache() -> (tempfile::TempDir, tempfile::TempDir, Snapshot, PathBuf) {
    let packages = tempfile::tempdir().unwrap();
    std::fs::write(packages.path().join("w64_test_01bb_0.pkg"), b"package").unwrap();
    let snapshot = Snapshot::read(packages.path()).unwrap().for_package(0x01bb);
    let cache = tempfile::tempdir().unwrap();
    let path = shard_path(Some(cache.path()), 0x01bb, &snapshot)
        .unwrap()
        .unwrap();
    (packages, cache, snapshot, path)
}

fn save(path: &Path, snapshot: &Snapshot, shard: Shard) {
    persist_shard(
        path,
        &Saved {
            snapshot: snapshot.clone(),
            package: 0x01bb,
            shard,
        },
    )
    .unwrap();
}

/// The ends of the ranges, where the wrapping differences the layout relies on are the only
/// thing keeping a row readable.
#[test]
fn packed_evidence_survives_values_that_wrap_the_delta_coding() {
    let rows = vec![
        EntityEvidence {
            source: u32::MAX,
            source_class: 0x8080_0001,
            words: vec![u32::MAX, 1, 0x8000_0000],
            lanes: vec![u64::MAX, 0],
        },
        EntityEvidence {
            source: 1,
            source_class: 0x8080_0001,
            words: Vec::new(),
            lanes: vec![7],
        },
        EntityEvidence {
            source: 0x8000_0000,
            source_class: 0x8080_0002,
            words: vec![5, 4, 3],
            lanes: Vec::new(),
        },
    ];
    let shard = Shard {
        evidence: rows.clone(),
        ..Shard::default()
    };
    let encoded = serde_json::to_vec(&shard).unwrap();
    let Ok(decoded) = serde_json::from_slice::<Shard>(&encoded) else {
        panic!("packed evidence does not parse back");
    };
    let mut expected = rows;
    expected.sort_by_key(|row| row.source);
    assert_eq!(decoded.evidence, expected);
}

/// A class index outside the table is a corrupt shard, not a panic.
#[test]
fn packed_evidence_rejects_a_class_index_outside_its_table() {
    let encoded = br#"{"paths":[],"candidates":[],"evidence":{"classes":[],"rows":[[1,0,[],[]]]},"vocabulary":[],"scanned_resources":0,"errors":[]}"#;
    let Err(error) = serde_json::from_slice::<Shard>(encoded) else {
        panic!("a class index outside the table was accepted");
    };
    assert!(error.to_string().contains("class"), "{error}");
}

#[test]
fn cached_sources_resolve_against_current_targets() {
    let bytes = super::super::tests::fixture();
    let targets = targets([0x8152_82E1], &[]);
    let shard = candidates(7, 8, &bytes, &known_lane);
    let encoded = serde_json::to_vec(&shard).unwrap();
    let cached: Shard = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(cached.paths.len(), 1);
    assert_eq!(cached.candidates.len(), 2);
    assert_eq!(
        cached.evidence,
        vec![EntityEvidence {
            source: 7,
            source_class: 8,
            words: vec![0x8152_82E1],
            lanes: vec![FIXTURE_LANE],
        }]
    );
    assert_eq!(
        entity_references(&cached, &targets),
        vec![EntityReference {
            source: 7,
            source_class: 8,
            target: 0x8152_82E1,
        }]
    );
    let original = resolve(&cached, |_| Some((10, 11)));
    let changed = resolve(&cached, |_| Some((20, 21)));
    assert!(
        original
            .iter()
            .all(|r| r.target == 10 && r.target_class == 11)
    );
    assert!(
        changed
            .iter()
            .all(|r| r.target == 20 && r.target_class == 21)
    );
    assert!(resolve(&cached, |_| None).is_empty());
    let expected = super::super::references(&bytes, &content_paths(&bytes), |_| Some((10, 11)));
    assert_eq!(
        original
            .iter()
            .map(|r| (r.offset, r.target, r.target_class, r.path.clone()))
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn a_saved_shard_survives_a_changed_entity_set_and_drops_vanished_targets() {
    let (packages, _cache, snapshot, path) = package_and_cache();
    // Written while both graphs were installed. The shard keeps the words, not the
    // verdict.
    let shard = Shard {
        evidence: vec![EntityEvidence {
            source: 1,
            source_class: 2,
            words: vec![0x80B7_795A, 0x8152_82E1],
            lanes: Vec::new(),
        }],
        scanned_resources: 3,
        ..Shard::default()
    };
    save(&path, &snapshot, shard);
    // A different entity set does not force a rescan. The authored graph that was
    // uninstalled since is dropped, the stock one stays.
    let targets = targets([0x8152_82E1, 0x8161_F4DE], &[]);
    let loaded = load_shard(Some(&path), 0x01bb, &snapshot).unwrap();
    assert_eq!(loaded.scanned_resources, 3);
    assert_eq!(target_tags(&loaded, &targets), vec![0x8152_82E1]);
    // A changed package still needs a fresh scan. The new content has a different
    // length: a snapshot is name, size and modified time, and two writes can share a
    // timestamp.
    std::fs::write(
        packages.path().join("w64_test_01bb_0.pkg"),
        b"changed package",
    )
    .unwrap();
    let changed = Snapshot::read(packages.path()).unwrap().for_package(0x01bb);
    assert!(load_shard(Some(&path), 0x01bb, &changed).is_none());
}

#[test]
fn a_warm_shard_finds_a_graph_installed_after_the_scan_like_a_cold_scan() {
    let (_packages, _cache, snapshot, path) = package_and_cache();
    let bytes = super::super::tests::fixture();
    // The graph the fixture refers to by word is not a target when the shard is written.
    let absent = targets([0x8161_F4DE], &[]);
    let scanned = candidates(7, 8, &bytes, &known_lane);
    assert!(entity_references(&scanned, &absent).is_empty());
    save(&path, &snapshot, scanned);
    // It is installed later. The source package has not changed, so the shard is
    // reused, and it must say what a fresh scan says.
    let restored = targets([0x8161_F4DE, 0x8152_82E1], &[]);
    let warm = load_shard(Some(&path), 0x01bb, &snapshot).unwrap();
    let cold = candidates(7, 8, &bytes, &known_lane);
    let expected = vec![EntityReference {
        source: 7,
        source_class: 8,
        target: 0x8152_82E1,
    }];
    assert_eq!(entity_references(&warm, &restored), expected);
    assert_eq!(entity_references(&cold, &restored), expected);
}

#[test]
fn a_warm_shard_follows_a_lane_remapped_to_another_live_graph() {
    let (_packages, _cache, snapshot, path) = package_and_cache();
    let bytes = super::super::tests::fixture();
    // At scan time the fixture's lane maps to graph A. Both A and B are live.
    let before = targets([0x80BB_0001, 0x80BB_0002], &[(FIXTURE_LANE, 0x80BB_0001)]);
    let scanned = candidates(7, 8, &bytes, &known_lane);
    assert_eq!(target_tags(&scanned, &before), vec![0x80BB_0001]);
    save(&path, &snapshot, scanned);
    // An install remaps the lane to graph B without touching the source package.
    let after = targets([0x80BB_0001, 0x80BB_0002], &[(FIXTURE_LANE, 0x80BB_0002)]);
    let warm = load_shard(Some(&path), 0x01bb, &snapshot).unwrap();
    let cold = candidates(7, 8, &bytes, &known_lane);
    assert_eq!(target_tags(&warm, &after), vec![0x80BB_0002]);
    assert_eq!(
        entity_references(&warm, &after),
        entity_references(&cold, &after)
    );
}

#[test]
fn a_shard_of_the_previous_format_is_neither_loaded_nor_kept() {
    let (_packages, cache, snapshot, path) = package_and_cache();
    let key = snapshot.key().unwrap();
    let old = cache.path().join(format!("tft-source-v4-01bb-{key}.json"));
    // The v4 layout: references resolved at scan time and a digest of the entity set.
    let v4 = serde_json::json!({
        "snapshot": snapshot,
        "package": 0x01bb,
        "entity_key": 1,
        "shard": {
            "paths": [],
            "candidates": [],
            "entity_references": [{"source": 1, "source_class": 2, "target": 0x8152_82E1_u32}],
            "vocabulary": [],
            "scanned_resources": 3,
            "errors": [],
        },
    });
    std::fs::write(&old, v4.to_string()).unwrap();
    assert_ne!(path, old);
    assert!(load_shard(Some(&path), 0x01bb, &snapshot).is_none());
    // Nor would its contents pass as the current layout.
    assert!(load_shard(Some(&old), 0x01bb, &snapshot).is_none());
    sweep_shards(cache.path());
    assert!(!old.exists());
}

#[test]
fn sweeping_keeps_the_newest_shards_of_each_package_and_drops_older_formats() {
    let cache = tempfile::tempdir().unwrap();
    // Named from the constant, so bumping the format does not break a test about
    // sweeping. The two older formats stay literal: what they are is the point.
    let current = |name: &str| format!("{SHARD_PREFIX}{name}.json");
    let names = [
        current("01bb-aaaa"),
        current("01bb-bbbb"),
        current("01bb-cccc"),
        current("03c1-dddd"),
        "tft-source-v4-03c1-eeee.json".to_owned(),
        "tft-source-v2-03c1-ffff.json".to_owned(),
        "unrelated.json".to_owned(),
    ];
    for (index, name) in names.iter().enumerate() {
        std::fs::write(cache.path().join(name), b"{}").unwrap();
        let modified = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(1_700_000_000 + index as u64 * 60);
        std::fs::OpenOptions::new()
            .write(true)
            .open(cache.path().join(name))
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }
    sweep_shards(cache.path());
    let mut remaining = std::fs::read_dir(cache.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    remaining.sort();
    assert_eq!(
        remaining,
        vec![
            current("01bb-bbbb"),
            current("01bb-cccc"),
            current("03c1-dddd"),
            "unrelated.json".to_owned(),
        ]
    );
}
