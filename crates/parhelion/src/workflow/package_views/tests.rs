use super::*;

#[cfg(all(windows, target_pointer_width = "64"))]
mod native;

fn marked_view(root: &Path, suffix: &str) -> (PathBuf, ViewLease) {
    let path = root.join(format!("{VIEW_PREFIX}{suffix}"));
    fs::create_dir(&path).unwrap();
    let lease = ViewLease::create(&path).unwrap();
    fs::create_dir(path.join("packages")).unwrap();
    fs::write(path.join("packages/w64_test_058c_0.pkg"), b"owned payload").unwrap();
    (path, lease)
}

#[test]
fn active_views_are_preserved_and_abandoned_marked_views_are_removed() {
    let root = tempfile::tempdir().unwrap();
    let (path, lease) = marked_view(root.path(), "active");
    assert_eq!(prune_stale_views(root.path()).unwrap(), 0);
    assert!(path.join("packages/w64_test_058c_0.pkg").is_file());
    drop(lease);
    assert_eq!(prune_stale_views(root.path()).unwrap(), 1);
    assert!(!path.exists());
}

#[test]
fn legacy_unmarked_unknown_and_partial_markers_are_never_pruned() {
    let root = tempfile::tempdir().unwrap();
    for suffix in ["legacy", "partial", "unknown"] {
        let directory = root.path().join(format!("{VIEW_PREFIX}{suffix}"));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("keep.txt"), b"not owned").unwrap();
    }
    fs::write(
        root.path()
            .join(format!("{VIEW_PREFIX}partial"))
            .join(LEASE_FILE),
        b"",
    )
    .unwrap();
    fs::write(
        root.path()
            .join(format!("{VIEW_PREFIX}unknown"))
            .join(LEASE_FILE),
        vec![b'?'; LEASE_MAGIC.len()],
    )
    .unwrap();
    assert_eq!(prune_stale_views(root.path()).unwrap(), 0);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 4);
    assert!(root.path().join(CLEANUP_LOCK_FILE).is_file());
}

#[test]
fn recognizable_marker_outside_view_namespace_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("not-a-package-view");
    fs::create_dir(&path).unwrap();
    drop(ViewLease::create(&path).unwrap());
    assert_eq!(prune_stale_views(root.path()).unwrap(), 0);
    assert!(path.exists());
}

#[test]
fn concurrent_pruners_tolerate_another_cleaner_finishing_first() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..24 {
        let (_, lease) = marked_view(root.path(), &format!("concurrent-{index}"));
        drop(lease);
    }
    std::thread::scope(|scope| {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let barrier = std::sync::Arc::clone(&barrier);
                let path = root.path();
                scope.spawn(move || {
                    barrier.wait();
                    prune_stale_views(path)
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
    });
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert!(root.path().join(CLEANUP_LOCK_FILE).is_file());
}

#[test]
fn concurrent_pruning_and_owned_cleanup_share_the_root_lock() {
    let root = tempfile::tempdir().unwrap();
    let mut owners = [Vec::new(), Vec::new()];
    for index in 0..24 {
        owners[index % 2].push(marked_view(root.path(), &format!("owned-{index}")));
    }
    std::thread::scope(|scope| {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let mut handles = Vec::new();
        for views in owners {
            let barrier = std::sync::Arc::clone(&barrier);
            handles.push(scope.spawn(move || {
                barrier.wait();
                for (path, lease) in views {
                    remove_owned_view(&path, lease)?;
                }
                Ok::<_, String>(())
            }));
        }
        for _ in 0..2 {
            let barrier = std::sync::Arc::clone(&barrier);
            let path = root.path();
            handles.push(scope.spawn(move || {
                barrier.wait();
                prune_stale_views(path).map(|_| ())
            }));
        }
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
    });
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert!(root.path().join(CLEANUP_LOCK_FILE).is_file());
}

#[test]
fn cleanup_lock_is_exclusive_persistent_and_not_truncated() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(CLEANUP_LOCK_FILE);
    fs::write(&path, b"existing lock content").unwrap();
    let first = lock_view_cleanup(root.path()).unwrap();
    let second = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let error = fs2::FileExt::try_lock_exclusive(&second).unwrap_err();
    assert_eq!(
        error.raw_os_error(),
        fs2::lock_contended_error().raw_os_error()
    );
    drop(first);
    fs2::FileExt::try_lock_exclusive(&second).unwrap();
    drop(second);
    assert_eq!(prune_stale_views(root.path()).unwrap(), 0);
    assert_eq!(fs::read(&path).unwrap(), b"existing lock content");
}

#[cfg(any(windows, unix))]
#[test]
fn cleanup_refuses_redirected_root_lock_without_touching_views() {
    let root = tempfile::tempdir().unwrap();
    let views = root.path().join("views");
    let outside = root.path().join("outside");
    fs::create_dir(&views).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"outside data").unwrap();
    let (path, lease) = marked_view(&views, "blocked-by-lock");
    directory_link(&outside, &views.join(CLEANUP_LOCK_FILE));
    assert!(
        remove_owned_view(&path, lease)
            .unwrap_err()
            .contains("cleanup lock")
    );
    assert!(
        prune_stale_views(&views)
            .unwrap_err()
            .contains("cleanup lock")
    );
    assert_eq!(fs::read(path.join(LEASE_FILE)).unwrap(), LEASE_MAGIC);
    assert_eq!(
        fs::read(path.join("packages/w64_test_058c_0.pkg")).unwrap(),
        b"owned payload"
    );
    assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"outside data");
}

#[cfg(any(windows, unix))]
#[test]
fn stale_pruning_skips_redirected_view_roots() {
    let root = tempfile::tempdir().unwrap();
    let views = root.path().join("views");
    let outside = root.path().join("outside");
    fs::create_dir(&views).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"outside the owned root").unwrap();
    drop(ViewLease::create(&outside).unwrap());
    let link = views.join(format!("{VIEW_PREFIX}redirected"));
    directory_link(&outside, &link);
    assert_eq!(prune_stale_views(&views).unwrap(), 0);
    assert_eq!(
        fs::read(outside.join("keep.txt")).unwrap(),
        b"outside the owned root"
    );
    assert!(fs::symlink_metadata(&link).is_ok());
}

#[cfg(any(windows, unix))]
#[test]
fn cleanup_refuses_nested_directory_redirects_without_touching_target() {
    let root = tempfile::tempdir().unwrap();
    let (path, lease) = marked_view(root.path(), "nested-redirect");
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.pkg"), b"outside data").unwrap();
    directory_link(&outside, &path.join("bin"));
    assert!(
        remove_owned_view(&path, lease)
            .unwrap_err()
            .contains("linked content")
    );
    assert_eq!(fs::read(outside.join("keep.pkg")).unwrap(), b"outside data");
    assert_eq!(fs::read(path.join(LEASE_FILE)).unwrap(), LEASE_MAGIC);
}

#[cfg(unix)]
fn directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn directory_link(target: &Path, link: &Path) {
    use std::os::windows::process::CommandExt;

    // Directory junctions exercise reparse-point rejection without developer-mode symlink
    // privileges. Both endpoints are freshly created test directories outside live data.
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not create test junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cleanup_refuses_unknown_content_and_retains_recoverable_marker() {
    let root = tempfile::tempdir().unwrap();
    let (path, lease) = marked_view(root.path(), "unknown-content");
    let unknown = path.join("personal-note.txt");
    fs::write(&unknown, b"preserve this").unwrap();
    let error = remove_owned_view(&path, lease).unwrap_err();
    assert!(error.contains("unrecognized"));
    assert_eq!(fs::read(&unknown).unwrap(), b"preserve this");
    assert_eq!(fs::read(path.join(LEASE_FILE)).unwrap(), LEASE_MAGIC);
    assert!(prune_stale_views(root.path()).is_err());
    fs::remove_file(unknown).unwrap();
    assert_eq!(prune_stale_views(root.path()).unwrap(), 1);
    assert!(!path.exists());
}

#[test]
fn filtered_view_closes_without_changing_hard_linked_source_files() {
    let root = tempfile::tempdir().unwrap();
    let packages = root.path().join("source/packages");
    let views = root.path().join("views");
    fs::create_dir_all(&packages).unwrap();
    fs::create_dir(&views).unwrap();
    let stock = packages.join("w64_stock_058c_0.pkg");
    let ignored = packages.join("w64_stock_058c_1.pkg");
    fs::write(&stock, b"stock payload").unwrap();
    fs::write(&ignored, b"excluded malformed authored data").unwrap();
    initialize_source_decoder(&packages).unwrap();
    let view = FilteredPackageView::create_in_root(
        &packages,
        &["w64_stock_058c_1.pkg".to_owned()],
        &views,
    )
    .unwrap();
    let view_path = view.path().parent().unwrap().to_path_buf();
    assert_eq!(
        fs::read(view.path().join("w64_stock_058c_0.pkg")).unwrap(),
        b"stock payload"
    );
    assert!(!view.path().join("w64_stock_058c_1.pkg").exists());
    assert_eq!(view.finish(Ok(42)), Ok(42));
    assert!(!view_path.exists());
    assert_eq!(fs::read(stock).unwrap(), b"stock payload");
    assert_eq!(
        fs::read(ignored).unwrap(),
        b"excluded malformed authored data"
    );
}

#[test]
fn compilation_and_cleanup_errors_are_both_reported() {
    assert_eq!(finish_with_cleanup(Ok(42), Ok(())), Ok(42));
    assert_eq!(
        finish_with_cleanup::<()>(Err("compile failed".to_owned()), Ok(())),
        Err("compile failed".to_owned())
    );
    assert_eq!(
        finish_with_cleanup(Ok(42), Err("cleanup failed".to_owned())),
        Err("cleanup failed".to_owned())
    );
    assert_eq!(
        finish_with_cleanup::<()>(
            Err("compile failed".to_owned()),
            Err("cleanup failed".to_owned())
        ),
        Err("compile failed\ncleanup failed".to_owned())
    );
}

#[cfg(windows)]
#[test]
fn locked_payload_cleanup_keeps_lease_for_retry_after_handle_release() {
    use std::os::windows::fs::OpenOptionsExt;

    let root = tempfile::tempdir().unwrap();
    let (path, lease) = marked_view(root.path(), "locked");
    let held = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path.join("packages/w64_test_058c_0.pkg"))
        .unwrap();
    assert!(remove_owned_view(&path, lease).is_err());
    assert_eq!(fs::read(path.join(LEASE_FILE)).unwrap(), LEASE_MAGIC);
    drop(held);
    assert_eq!(prune_stale_views(root.path()).unwrap(), 1);
    assert!(!path.exists());
}

#[cfg(windows)]
#[test]
fn decoder_bootstrap_rejects_bin_packages_without_loading_source_packages() {
    let root = tempfile::tempdir().unwrap();
    let packages = root.path().join("packages");
    let bin = root.path().join("bin");
    fs::create_dir(&packages).unwrap();
    fs::create_dir_all(bin.join("x64")).unwrap();
    fs::write(packages.join("excluded.pkg"), b"malformed authored overlay").unwrap();
    fs::write(bin.join("x64/oo2core_3_win64.dll"), b"not a real DLL").unwrap();
    fs::write(bin.join("unexpected.pkg"), b"do not read this as a package").unwrap();
    let error = initialize_source_decoder(&packages).unwrap_err();
    assert!(error.contains("unexpected package data"));
    assert_eq!(
        fs::read(packages.join("excluded.pkg")).unwrap(),
        b"malformed authored overlay"
    );
}
