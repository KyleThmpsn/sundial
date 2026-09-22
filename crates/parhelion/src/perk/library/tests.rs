use super::*;

#[test]
fn restoring_defaults_replaces_edits_and_restores_deleted_perks() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    library.materialize_bundled().unwrap();
    let bundled = bundled_recipes().unwrap();
    assert!(bundled.len() > 1);

    let edited_path = library
        .root()
        .join(format!("{}.perk.json", bundled[0].1.id));
    let mut edited = bundled[0].1.clone();
    edited.description = "Edited locally".into();
    let edited = library
        .save(&edited, Some(bundled[0].0.as_bytes()))
        .unwrap();

    let deleted_path = library
        .root()
        .join(format!("{}.perk.json", bundled[1].1.id));
    let deleted = Library::read(&deleted_path).unwrap();
    library.delete(&deleted.recipe, &deleted.baseline).unwrap();

    let mut custom = PerkRecipe::new();
    custom.name = "Personal Perk".into();
    let custom = library.save(&custom, None).unwrap();

    let preview = library.prepare_restore_defaults().unwrap();
    let backup = library.restore_defaults(&preview).unwrap().unwrap();

    assert_eq!(fs::read(&edited_path).unwrap(), bundled[0].0.as_bytes());
    assert_eq!(fs::read(&deleted_path).unwrap(), bundled[1].0.as_bytes());
    assert_eq!(fs::read(&custom.path).unwrap(), custom.baseline);
    assert_eq!(
        fs::read(backup.join(edited_path.file_name().unwrap())).unwrap(),
        edited.baseline
    );
    assert!(!library.removed_bundled_path().exists());
    assert_eq!(library.scan().unwrap().entries.len(), bundled.len() + 1);
}

#[test]
fn restoring_defaults_rejects_a_stale_preview() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    library.materialize_bundled().unwrap();
    let preview = library.prepare_restore_defaults().unwrap();
    let bundled = bundled_recipes().unwrap();
    let path = library
        .root()
        .join(format!("{}.perk.json", bundled[0].1.id));
    fs::write(&path, b"changed after preview").unwrap();

    assert!(library.restore_defaults(&preview).is_err());
    assert_eq!(fs::read(path).unwrap(), b"changed after preview");
}

/// Simulates a copy left by an earlier release by dropping its applied version.
fn forget_applied_version(library: &Library, id: &str) {
    let path = library.root().join(BUNDLED_VERSIONS_FILE_NAME);
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["applied"].as_object_mut().unwrap().remove(id);
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
}

#[test]
fn materializing_keeps_edits_while_the_bundled_perk_is_unchanged() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    assert!(library.materialize_bundled().unwrap().is_none());
    let bundled = bundled_recipes().unwrap();
    let mut edited = bundled[0].1.clone();
    edited.description = "Edited locally".into();
    let edited = library
        .save(&edited, Some(bundled[0].0.as_bytes()))
        .unwrap();

    assert!(library.materialize_bundled().unwrap().is_none());
    assert_eq!(fs::read(&edited.path).unwrap(), edited.baseline);
}

#[test]
fn a_changed_bundled_perk_replaces_the_stale_copy_and_keeps_edits_as_a_duplicate() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    library.materialize_bundled().unwrap();
    let bundled = bundled_recipes().unwrap();
    let mut edited = bundled[0].1.clone();
    edited.description = "Edited locally".into();
    let edited = library
        .save(&edited, Some(bundled[0].0.as_bytes()))
        .unwrap();
    let deleted = Library::read(
        &library
            .root()
            .join(format!("{}.perk.json", bundled[1].1.id)),
    )
    .unwrap();
    library.delete(&deleted.recipe, &deleted.baseline).unwrap();
    forget_applied_version(&library, &bundled[0].1.id);
    forget_applied_version(&library, &bundled[1].1.id);

    let refresh = library.materialize_bundled().unwrap().unwrap();

    assert_eq!(refresh.names, vec![bundled[0].1.name.clone()]);
    assert_eq!(refresh.copies, vec![format!("{} Copy", bundled[0].1.name)]);
    assert_eq!(fs::read(&edited.path).unwrap(), bundled[0].0.as_bytes());
    assert_eq!(
        fs::read(refresh.backup.join(edited.path.file_name().unwrap())).unwrap(),
        edited.baseline
    );
    assert!(!deleted.path.exists(), "a deleted default stays deleted");
    let scan = library.scan().unwrap();
    assert_eq!(scan.entries.len(), bundled.len());
    let copy = scan
        .entries
        .iter()
        .find(|entry| entry.recipe.name == format!("{} Copy", bundled[0].1.name))
        .expect("the edited copy is kept in the library");
    assert_eq!(copy.recipe.description, "Edited locally");
    assert_ne!(copy.recipe.id, bundled[0].1.id);

    assert!(library.materialize_bundled().unwrap().is_none());
}

#[test]
fn a_stale_copy_that_is_not_a_perk_is_only_backed_up() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    library.materialize_bundled().unwrap();
    let bundled = bundled_recipes().unwrap();
    let path = library
        .root()
        .join(format!("{}.perk.json", bundled[0].1.id));
    fs::write(&path, b"not a perk").unwrap();
    fs::remove_file(library.root().join(BUNDLED_VERSIONS_FILE_NAME)).unwrap();

    let refresh = library.materialize_bundled().unwrap().unwrap();

    assert!(refresh.copies.is_empty());
    assert_eq!(fs::read(&path).unwrap(), bundled[0].0.as_bytes());
    assert_eq!(
        fs::read(refresh.backup.join(path.file_name().unwrap())).unwrap(),
        b"not a perk"
    );
    assert_eq!(library.scan().unwrap().entries.len(), bundled.len());
}
