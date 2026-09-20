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
