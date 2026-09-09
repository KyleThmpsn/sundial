use super::*;
use crate::package_profile::PARHELION_ASSET_PACKAGE;

fn write_payload(run: &StagedRun) {
    fs::write(
        run.directory.join(PARHELION_ASSET_PACKAGE.file_name),
        b"synthetic staged package",
    )
    .unwrap();
    fs::write(run.directory.join(MANIFEST_FILE_NAME), b"{}").unwrap();
    fs::create_dir(run.directory.join("recipes")).unwrap();
    fs::write(run.directory.join("recipes/weapon.json"), b"{}").unwrap();
}

fn completed(root: &Path, slug: &str) -> PathBuf {
    let run = StagedRun::begin(root, slug).unwrap();
    write_payload(&run);
    let directory = run.directory.clone();
    run.finish(Ok(())).unwrap();
    directory
}

#[test]
fn keeps_only_the_newest_completed_build() {
    let root = tempfile::tempdir().unwrap();
    let first = completed(root.path(), "first");
    let second = completed(root.path(), "second");
    assert!(!first.exists());
    assert!(second.join(MANIFEST_FILE_NAME).is_file());
    assert!(lease_for_read(&second).unwrap().is_some());
}

#[test]
fn active_builds_survive_and_failed_builds_are_removed() {
    let root = tempfile::tempdir().unwrap();
    let active = StagedRun::begin(root.path(), "active").unwrap();
    write_payload(&active);
    let active_path = active.directory.clone();
    let complete = completed(root.path(), "complete");
    assert!(active_path.is_dir());
    assert!(lease_for_read(&active_path).is_err());
    let result: Result<(), String> = active.finish(Err("synthetic build failure".into()));
    assert_eq!(result.unwrap_err(), "synthetic build failure");
    assert!(!active_path.exists());
    assert!(complete.is_dir());
}

#[test]
fn interrupted_marked_partial_builds_are_pruned() {
    let root = tempfile::tempdir().unwrap();
    let mut interrupted = StagedRun::begin(root.path(), "interrupted").unwrap();
    write_payload(&interrupted);
    fs::write(
        interrupted.directory.join("recipes/weapon.json"),
        b"{\"schema\":",
    )
    .unwrap();
    let old = interrupted.directory.clone();
    drop(interrupted.lease.take());
    // Simulate a process exit without running the failed-build guard.
    interrupted.completed = true;
    drop(interrupted);
    let next = completed(root.path(), "next");
    assert!(!old.exists());
    assert!(next.is_dir());
}

#[test]
fn unmarked_legacy_and_unknown_content_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let legacy = root.path().join("legacy");
    fs::create_dir(&legacy).unwrap();
    fs::write(legacy.join(MANIFEST_FILE_NAME), b"{}").unwrap();
    let first = completed(root.path(), "owned");
    fs::write(first.join("personal-notes.txt"), b"keep this").unwrap();
    let second = completed(root.path(), "next");
    assert!(legacy.is_dir());
    assert!(lease_for_read(&legacy).unwrap().is_none());
    assert_eq!(
        fs::read(first.join("personal-notes.txt")).unwrap(),
        b"keep this"
    );
    assert!(second.is_dir());
}

#[test]
fn installers_keep_old_runs_alive_until_the_last_reader_finishes() {
    let root = tempfile::tempdir().unwrap();
    let first = completed(root.path(), "first");
    let reader = lease_for_read(&first).unwrap().unwrap();
    let nested_reader = lease_for_read(&first).unwrap().unwrap();
    let second = completed(root.path(), "second");
    assert!(first.join(MANIFEST_FILE_NAME).is_file());
    drop(nested_reader);
    assert!(first.is_dir());
    drop(reader);
    assert!(!first.exists());
    assert!(second.is_dir());
}

#[test]
fn invalid_or_missing_completion_markers_cannot_be_installed() {
    let root = tempfile::tempdir().unwrap();
    let first = completed(root.path(), "first");
    fs::write(first.join(COMPLETE_FILE), b"not a completion").unwrap();
    assert!(lease_for_read(&first).is_err());
    let second = completed(root.path(), "second");
    assert!(!first.exists());
    assert!(second.is_dir());
}

#[test]
fn concurrent_build_completions_leave_one_run() {
    let root = tempfile::tempdir().unwrap();
    let ready = std::sync::Arc::new(std::sync::Barrier::new(12));
    let handles = (0..12)
        .map(|index| {
            let root = root.path().to_owned();
            let ready = ready.clone();
            std::thread::spawn(move || {
                let run = StagedRun::begin(&root, &format!("build-{index}")).unwrap();
                write_payload(&run);
                ready.wait();
                run.finish(Ok(())).unwrap();
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }
    let directories = fs::read_dir(root.path())
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .count();
    assert_eq!(directories, 1);
}

#[test]
fn pruning_hard_links_does_not_delete_the_source_file() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("source.pkg");
    fs::write(&source, b"keep original").unwrap();
    let run = StagedRun::begin(root.path(), "first").unwrap();
    fs::hard_link(
        &source,
        run.directory.join(PARHELION_ASSET_PACKAGE.file_name),
    )
    .unwrap();
    let old = run.directory.clone();
    run.finish(Ok(())).unwrap();
    completed(root.path(), "next");
    assert!(!old.exists());
    assert_eq!(fs::read(source).unwrap(), b"keep original");
}

#[test]
fn drop_guard_removes_an_unfinished_owned_run() {
    let root = tempfile::tempdir().unwrap();
    let run = StagedRun::begin(root.path(), "unfinished").unwrap();
    write_payload(&run);
    let directory = run.directory.clone();
    drop(run);
    assert!(!directory.exists());
}

#[test]
fn new_staging_roots_and_copied_completed_runs_are_supported() {
    let root = tempfile::tempdir().unwrap();
    let staging = root.path().join("new-staging-root");
    let directory = completed(&staging, "first");
    fs::remove_file(staging.join(ROOT_LOCK)).unwrap();
    assert!(lease_for_read(&directory).unwrap().is_some());
    assert!(staging.join(ROOT_LOCK).is_file());
}

#[test]
fn staged_recipe_writes_match_normalized_saves_without_overwriting_or_temp_files() {
    let root = tempfile::tempdir().unwrap();
    let recipe = crate::recipe::WeaponRecipe::every_end();
    let path = root.path().join("weapon.parhelion.json");
    super::super::write_staged_recipe(&recipe, &path).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        format!("{}\n", recipe.to_json_pretty().unwrap())
    );
    assert!(super::super::write_staged_recipe(&recipe, &path).is_err());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn deleted_run_paths_are_not_reused_by_same_slug_completions() {
    let root = tempfile::tempdir().unwrap();
    let mut identities = std::collections::BTreeSet::new();
    for _ in 0..24 {
        let directory = completed(root.path(), "same-slug");
        assert!(identities.insert(directory));
    }
    assert_eq!(identities.iter().filter(|path| path.exists()).count(), 1);
}

#[cfg(any(windows, unix))]
#[test]
fn redirected_run_roots_and_recipe_directories_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_run = completed(outside.path(), "outside");
    directory_link(&outside_run, &root.path().join("redirected-run"));
    let run = StagedRun::begin(root.path(), "nested-redirect").unwrap();
    let nested = run.directory.clone();
    run.finish(Ok(())).unwrap();
    directory_link(&outside_run.join("recipes"), &nested.join("recipes"));
    let next = completed(root.path(), "next");
    assert!(nested.is_dir());
    assert!(next.is_dir());
    assert!(outside_run.join(MANIFEST_FILE_NAME).is_file());
    assert_eq!(
        fs::read(outside_run.join("recipes/weapon.json")).unwrap(),
        b"{}"
    );
    assert!(fs::symlink_metadata(root.path().join("redirected-run")).is_ok());
}

#[cfg(any(windows, unix))]
#[test]
fn redirected_root_coordination_lock_cannot_be_opened() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep.txt"), b"outside data").unwrap();
    directory_link(outside.path(), &root.path().join(ROOT_LOCK));
    assert!(StagedRun::begin(root.path(), "blocked").is_err());
    assert_eq!(
        fs::read(outside.path().join("keep.txt")).unwrap(),
        b"outside data"
    );
}

#[cfg(unix)]
fn directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn directory_link(target: &Path, link: &Path) {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not create staging test junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
