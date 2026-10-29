//! How a sweep could lose a conversion that something still reads, or keep one nothing reads:
//!
//! - a recipe names its graph by a verbatim (`\\?\`) path, or with other separators or case,
//! - a linked `candidate-1` shares its prefix with an unlinked `candidate-10`,
//! - only a recipe backup, a package backup, a kept install, a staged run or the open document
//!   names it,
//! - it is the newest finished source export, which the next import reuses,
//! - it is not a conversion at all, such as `result.json` or a folder named by hand, even one
//!   ending in a number,
//! - a saved snapshot cannot be read, so the links it holds are unknown,
//! - the conversion is a link to a folder elsewhere, whose files are not the importer's,
//! - an earlier removal stopped partway and left its files behind,
//! - a sweep of every item reaches into a folder that is not an item's `models/<name>/<HASH>`,
//! - a recipe is saved, or a snapshot rewritten, while the sweep reads before taking the write
//!   lease, and names a folder the first read did not.
use super::*;
use serde_json::json;

fn conversion(item: &Path, name: &str, bytes: usize) -> PathBuf {
    let path = item.join(name);
    fs::create_dir_all(path.join("graph")).unwrap();
    fs::write(path.join("graph/asset-graph.json"), vec![b'x'; bytes]).unwrap();
    path
}

fn snapshot(path: &Path, directory: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let recipe = json!({"overrides": {"imported_graph": {"directory": directory}}});
    fs::write(path, serde_json::to_vec_pretty(&recipe).unwrap()).unwrap();
}

/// What is left in `item`. An item folder a sweep emptied is removed with its last conversion.
fn remaining(item: &Path) -> BTreeSet<String> {
    match fs::read_dir(item) {
        Ok(entries) => entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => panic!("{error}"),
    }
}

#[test]
fn a_sweep_removes_only_conversions_no_saved_recipe_names() {
    let data = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(data.path().join("recipes")).unwrap();
    let item = data.path().join("models/Item Name/0000ABCD");
    fs::create_dir_all(&item).unwrap();
    let item = item.canonicalize().unwrap();
    let linked = conversion(&item, "candidate-1", 3);
    let removed = [
        conversion(&item, "candidate-10", 5),
        conversion(&item, "marathon-0", 7),
        conversion(&item, "source-0", 11),
        conversion(&item, "source-2", 13),
    ];
    let backed_up = conversion(&item, "candidate-2", 1);
    let package_backup = conversion(&item, "reach-4", 1);
    let installed = conversion(&item, "d1-roi-0", 1);
    let staged = conversion(&item, "d1-roi-1", 1);
    let open = conversion(&item, "gear-3", 1);
    let source = conversion(&item, "source-1", 1);
    for export in [&removed[2], &source] {
        fs::write(export.join("export.json"), b"{}").unwrap();
    }
    fs::create_dir(item.join("restored-2026-09-20")).unwrap();
    fs::create_dir(item.join("experiment-1")).unwrap();
    fs::write(item.join("result.json"), b"{}").unwrap();
    let outside = data.path().join("elsewhere");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.bin"), b"keep").unwrap();
    directory_link(&outside, &item.join("candidate-7"));

    // The graph sits below the conversion folder, as an import's graph does.
    snapshot(
        &library.root.join("linked.parhelion.json"),
        &linked.join("0000ABCD/rendered/graph"),
    );
    snapshot(
        &data
            .path()
            .join("backups/recipes/recipe-backup-1/a.parhelion.json"),
        &backed_up.join("graph"),
    );
    snapshot(
        &data
            .path()
            .join("backups/packages/backup-0/recipes/a.parhelion.json"),
        &package_backup.join("graph"),
    );
    snapshot(
        &data
            .path()
            .join("installed/0123/mods/recipes/a.parhelion.json"),
        &installed.join("import/graph"),
    );
    // Recipes written by other means may spell the same folder differently.
    let spelled = if cfg!(windows) {
        let path = staged.to_string_lossy();
        PathBuf::from(
            path.trim_start_matches(r"\\?\")
                .replace('\\', "/")
                .to_uppercase(),
        )
    } else {
        staged.clone()
    };
    snapshot(
        &data.path().join("staging/run-1/recipes/a.parhelion.json"),
        &spelled.join("import/graph"),
    );
    let document = json!({"overrides": {"imported_graph": {"directory": open.join("graph")}}});

    let report = library
        .set_aside_unused_conversions(Some(&BTreeSet::from([item.clone()])), &[document])
        .unwrap()
        .remove();

    assert_eq!(
        remaining(&item),
        BTreeSet::from(
            [
                "candidate-1",
                "candidate-2",
                "candidate-7",
                "d1-roi-0",
                "d1-roi-1",
                "experiment-1",
                "gear-3",
                "reach-4",
                "restored-2026-09-20",
                "result.json",
                "source-1",
            ]
            .map(String::from)
        )
    );
    assert_eq!(report.folders, removed.len());
    assert_eq!(report.bytes, 5 + 7 + 11 + 2 + 13);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(fs::read(outside.join("keep.bin")).unwrap(), b"keep");
}

#[test]
fn an_unreadable_snapshot_removes_nothing() {
    let data = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(data.path().join("recipes")).unwrap();
    let item = data.path().join("models/Item Name/0000ABCD");
    fs::create_dir_all(&item).unwrap();
    let item = item.canonicalize().unwrap();
    let named = conversion(&item, "candidate-0", 1);
    let staged = data.path().join("staging/run-1/recipes/a.parhelion.json");
    fs::create_dir_all(staged.parent().unwrap()).unwrap();
    // Cut short after naming the conversion, as an interrupted write leaves it.
    let directory = serde_json::to_string(&named.join("graph")).unwrap();
    fs::write(
        &staged,
        format!(r#"{{"overrides": {{"imported_graph": {{"directory": {directory}"#),
    )
    .unwrap();

    assert!(
        library
            .set_aside_unused_conversions(Some(&BTreeSet::from([item.clone()])), &[])
            .is_err()
    );
    assert_eq!(remaining(&item), BTreeSet::from(["candidate-0".to_owned()]));
}

#[test]
fn a_sweep_of_every_item_sees_saves_made_while_it_read() {
    let data = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(data.path().join("recipes")).unwrap();
    let models = data.path().join("models");
    let first = models.join("First/0000000A");
    let second = models.join("Second/0000000B");
    let by_hand = models.join("Second/restored-2026-09-20");
    for folder in [&first, &second, &by_hand] {
        fs::create_dir_all(folder).unwrap();
    }
    let (first, second, by_hand) = (
        first.canonicalize().unwrap(),
        second.canonicalize().unwrap(),
        by_hand.canonicalize().unwrap(),
    );
    conversion(&first, "candidate-0", 1);
    conversion(&first.join(".removing-earlier"), "candidate-9", 17);
    conversion(&second, "d1-roi-0", 1);
    let saved = conversion(&second, "d1-roi-1", 1);
    let rewritten = conversion(&second, "d1-roi-2", 1);
    conversion(&by_hand, "candidate-0", 1);
    let staged = data.path().join("staging/run-1/recipes/a.parhelion.json");
    snapshot(&staged, &data.path().join("elsewhere"));

    let aside = library
        .set_aside_with(None, &[], Status::default(), || {
            snapshot(
                &library.root.join("saved.parhelion.json"),
                &saved.join("graph"),
            );
            snapshot(&staged, &rewritten.join("import/graph"));
        })
        .unwrap();
    let report = aside.remove();

    assert!(remaining(&first).is_empty());
    assert_eq!(
        remaining(&second),
        BTreeSet::from(["d1-roi-1".to_owned(), "d1-roi-2".to_owned()])
    );
    assert_eq!(
        remaining(&by_hand),
        BTreeSet::from(["candidate-0".to_owned()])
    );
    assert_eq!(report.folders, 3);
    assert_eq!(report.bytes, 1 + 17 + 1);
}

#[cfg(unix)]
fn directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn directory_link(target: &Path, link: &Path) {
    use std::os::windows::process::CommandExt;
    // A junction needs no symbolic-link privilege.
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
