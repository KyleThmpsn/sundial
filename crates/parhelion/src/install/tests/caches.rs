use super::*;

#[test]
fn package_header_cache_names_are_narrowly_scoped() {
    assert!(is_package_header_cache_name("cache_phr_0000e2e9.dat"));
    assert!(is_package_header_cache_name("cache_phr_ABCD.dat"));
    assert!(!is_package_header_cache_name("cache_phr_.dat"));
    assert!(!is_package_header_cache_name("cache_phr_active.tmp"));
    assert!(!is_package_header_cache_name("other_0000e2e9.dat"));
}

#[test]
fn invalidates_existing_sunrise_build_cache_after_verified_install_and_reports_backup() {
    let fixture = Fixture::new();
    let cache_bytes = b"stale Sunrise build-data cache";
    let cache_path = fixture.write_sunrise_cache(cache_bytes);
    let canonical_cache_path = fs::canonicalize(&cache_path).unwrap();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert!(!cache_path.exists());
    let invalidated = report
        .invalidated_sunrise_cache
        .expect("existing cache should be invalidated");
    assert_eq!(invalidated.cache_path, canonical_cache_path);
    assert_eq!(invalidated.byte_length, cache_bytes.len() as u64);
    assert_eq!(invalidated.retained_quarantine_path, None);
    assert_eq!(fs::read(&invalidated.backup_path).unwrap(), cache_bytes);
    assert!(path_is_within(
        &invalidated.backup_path,
        &report.backup_directory
    ));
    assert_eq!(
        invalidated.backup_path,
        report
            .backup_directory
            .join(SUNRISE_CACHE_BACKUP_DIRECTORY)
            .join("build_data.bin")
    );
    assert_eq!(
        invalidated.sha256,
        digest_file(&invalidated.backup_path).unwrap().sha256
    );
    assert!(
        fs::read_dir(cache_path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(SUNRISE_CACHE_QUARANTINE_PREFIX))
    );
}

#[test]
fn invalidates_client_package_header_caches_after_verified_install() {
    let fixture = Fixture::new();
    let cache_bytes = b"stale package-header cache";
    let cache_path = fixture.write_package_header_cache("0000e2e9", cache_bytes);
    let canonical_cache_path = fs::canonicalize(&cache_path).unwrap();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert!(!cache_path.exists());
    assert_eq!(report.invalidated_package_header_caches.len(), 1);
    let invalidated = &report.invalidated_package_header_caches[0];
    assert_eq!(invalidated.cache_path, canonical_cache_path);
    assert_eq!(fs::read(&invalidated.backup_path).unwrap(), cache_bytes);
    assert!(path_is_within(
        &invalidated.backup_path,
        &report.backup_directory
    ));
    assert_eq!(invalidated.retained_quarantine_path, None);
}

#[test]
fn invalidates_root_layout_sunrise_build_cache() {
    let fixture = Fixture::new();
    let cache_bytes = b"stale root-layout Sunrise cache";
    let cache_path = fixture.write_root_sunrise_cache(cache_bytes);
    let canonical_cache_path = fs::canonicalize(&cache_path).unwrap();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert!(!cache_path.exists());
    let invalidated = report
        .invalidated_sunrise_cache
        .expect("root-layout cache should be invalidated");
    assert_eq!(invalidated.cache_path, canonical_cache_path);
    assert_eq!(invalidated.retained_quarantine_path, None);
    assert_eq!(fs::read(invalidated.backup_path).unwrap(), cache_bytes);
}

#[test]
fn dual_sunrise_cache_layouts_are_rejected_before_mutation() {
    let fixture = Fixture::new();
    let root_cache = fixture.write_root_sunrise_cache(b"root cache");
    let bin_cache = fixture.write_sunrise_cache(b"bin cache");

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("Both supported"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(root_cache).unwrap(), b"root cache");
    assert_eq!(fs::read(bin_cache).unwrap(), b"bin cache");
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn missing_sunrise_build_cache_is_left_missing_and_reported_as_noop() {
    let fixture = Fixture::new();
    let cache_path = fixture.sunrise_cache_path();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert!(!cache_path.exists());
    assert_eq!(report.invalidated_sunrise_cache, None);
    assert!(
        !report
            .backup_directory
            .join(SUNRISE_CACHE_BACKUP_DIRECTORY)
            .exists()
    );
}

#[test]
fn non_regular_sunrise_cache_is_rejected_before_package_or_backup_mutation() {
    let fixture = Fixture::new();
    let cache_path = fixture.sunrise_cache_path();
    fs::create_dir_all(&cache_path).unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("must be a regular file"));
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
    assert!(cache_path.is_dir());
}

#[cfg(unix)]
#[test]
fn symlinked_sunrise_cache_is_rejected_before_mutation() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let cache_path = fixture.sunrise_cache_path();
    fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
    let real_cache = fixture._temporary.path().join("real-build-data.bin");
    fs::write(&real_cache, b"cache").unwrap();
    symlink(&real_cache, &cache_path).unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("must be a regular file"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(real_cache).unwrap(), b"cache");
}

#[cfg(windows)]
#[test]
fn symlinked_sunrise_cache_is_rejected_before_mutation_when_supported() {
    use std::os::windows::fs::symlink_file;

    let fixture = Fixture::new();
    let cache_path = fixture.sunrise_cache_path();
    fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
    let real_cache = fixture._temporary.path().join("real-build-data.bin");
    fs::write(&real_cache, b"cache").unwrap();
    if symlink_file(&real_cache, &cache_path).is_err() {
        return;
    }

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("must be a regular file"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(real_cache).unwrap(), b"cache");
}

#[cfg(unix)]
#[test]
fn sunrise_cache_ancestor_redirect_outside_game_root_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let external = tempfile::tempdir().unwrap();
    let external_sunrise = external.path().join("Sunrise");
    let external_cache = external_sunrise.join("cache").join("build_data.bin");
    fs::create_dir_all(external_cache.parent().unwrap()).unwrap();
    fs::write(&external_cache, b"external cache").unwrap();
    symlink(&external_sunrise, fixture._temporary.path().join("Sunrise")).unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("outside the validated game root"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(external_cache).unwrap(), b"external cache");
}

#[cfg(windows)]
#[test]
fn sunrise_cache_ancestor_redirect_outside_game_root_is_rejected_when_supported() {
    use std::os::windows::fs::symlink_dir;

    let fixture = Fixture::new();
    let external = tempfile::tempdir().unwrap();
    let external_sunrise = external.path().join("Sunrise");
    let external_cache = external_sunrise.join("cache").join("build_data.bin");
    fs::create_dir_all(external_cache.parent().unwrap()).unwrap();
    fs::write(&external_cache, b"external cache").unwrap();
    if symlink_dir(&external_sunrise, fixture._temporary.path().join("Sunrise")).is_err() {
        return;
    }

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("outside the validated game root"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(external_cache).unwrap(), b"external cache");
}

#[test]
fn cache_quarantine_rename_failure_rolls_packages_back_and_preserves_cache() {
    let fixture = Fixture::new();
    let cache_path = fixture.write_sunrise_cache(b"cache before failed quarantine");
    let cache_ops = CacheInvalidationOps {
        rename: fail_cache_quarantine_rename,
        cleanup: cleanup_cache_quarantine,
    };

    let error = install_staged_packages_inner(&fixture.request(), None, cache_ops).unwrap_err();

    assert!(error.message.contains("atomically quarantine"));
    assert!(error.rollback.as_ref().unwrap().succeeded());
    assert_eq!(
        fs::read(&cache_path).unwrap(),
        b"cache before failed quarantine"
    );
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
    assert!(
        fs::read_dir(cache_path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(SUNRISE_CACHE_QUARANTINE_PREFIX))
    );
}

#[test]
fn successful_cache_rename_is_commit_boundary_and_retained_quarantine_is_reported() {
    let fixture = Fixture::new();
    let cache_bytes = b"cache retained in quarantine";
    let cache_path = fixture.write_sunrise_cache(cache_bytes);
    let cache_ops = CacheInvalidationOps {
        rename: rename_cache_into_quarantine,
        cleanup: retain_cache_quarantine,
    };

    let report = install_staged_packages_inner(&fixture.request(), None, cache_ops).unwrap();

    assert!(!cache_path.exists());
    let invalidated = report.invalidated_sunrise_cache.unwrap();
    let retained = invalidated
        .retained_quarantine_path
        .expect("injected cleanup should retain quarantine");
    assert_eq!(fs::read(&retained).unwrap(), cache_bytes);
    assert!(
        retained
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name
                .to_string_lossy()
                .starts_with(SUNRISE_CACHE_QUARANTINE_PREFIX))
    );
    assert_eq!(fs::read(invalidated.backup_path).unwrap(), cache_bytes);
    for artifact in report.artifacts {
        assert_eq!(
            fs::read(&artifact.target_path).unwrap(),
            fixture.staged_bytes[&artifact.file_name]
        );
    }
}
