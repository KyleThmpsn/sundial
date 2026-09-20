use super::*;

#[test]
fn imports_are_deduplicated_and_stay_independent_after_edits_and_deletion() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().to_owned()).unwrap();
    let original = PerkRecipe::new();
    let mut duplicate = original.clone();
    duplicate.id = PerkRecipe::new().id;
    let report = library
        .import_embedded([original.clone(), duplicate.clone()])
        .unwrap();
    assert_eq!(report.added, 1);
    assert!(report.errors.is_empty());
    let entry = library.scan().unwrap().entries.remove(0);
    let mut edited = entry.recipe.clone();
    edited.description = "An independent library edit".into();
    let saved = library.save(&edited, Some(&entry.baseline)).unwrap();
    assert_eq!(
        library.import_embedded([duplicate.clone()]).unwrap().added,
        0
    );
    assert_eq!(Library::read(&entry.path).unwrap().recipe, edited);
    library.delete(&edited, &saved.baseline).unwrap();
    let reopened = Library::open(temp.path().to_owned()).unwrap();
    assert_eq!(reopened.import_embedded([duplicate]).unwrap().added, 0);
    assert!(reopened.scan().unwrap().entries.is_empty());
    // A changed embedded version remains a distinct perk, even with the same name.
    assert_eq!(reopened.import_embedded([edited]).unwrap().added, 1);
}

#[test]
fn matching_standalone_perks_are_not_overwritten_or_duplicated() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().to_owned()).unwrap();
    let perk = PerkRecipe::new();
    let saved = library.save(&perk, None).unwrap();
    assert_eq!(library.import_embedded([perk.clone()]).unwrap().added, 0);
    assert_eq!(fs::read(&saved.path).unwrap(), saved.baseline);
    library.delete(&perk, &saved.baseline).unwrap();
    assert_eq!(library.import_embedded([perk]).unwrap().added, 0);
}

#[test]
fn invalid_imports_are_reported_without_preventing_valid_perks() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().to_owned()).unwrap();
    let mut invalid = PerkRecipe::new();
    invalid.template_plug = 0.into();
    let report = library
        .import_embedded([invalid, PerkRecipe::new()])
        .unwrap();
    assert_eq!(report.added, 1);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(library.scan().unwrap().entries.len(), 1);
}

#[test]
fn corrupt_history_or_existing_files_pause_import_without_overwriting() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().to_owned()).unwrap();
    let path = temp.path().join("broken.perk.json");
    fs::write(&path, b"unreadable original").unwrap();
    assert!(library.import_embedded([PerkRecipe::new()]).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"unreadable original");
    fs::remove_file(&path).unwrap();
    let history = temp.path().join("imported-weapon-perks.json");
    fs::write(&history, b"unreadable history").unwrap();
    assert!(library.import_embedded([PerkRecipe::new()]).is_err());
    assert_eq!(fs::read(history).unwrap(), b"unreadable history");
    assert!(library.scan().unwrap().entries.is_empty());
}

#[test]
fn standalone_export_and_import_preserve_full_recipe() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("perks")).unwrap();
    let mut perk: PerkRecipe = serde_json::from_str(crate::perk::bundled::RECIPES[0]).unwrap();
    perk.id = PerkRecipe::new().id;
    library.import_embedded([perk.clone()]).unwrap();
    let entry = library.scan().unwrap().entries.remove(0);
    let path = temp.path().join("shared.perk.json");
    library.export(&entry.recipe, &path).unwrap();
    let reimported = Library::read(&path).unwrap();
    assert_eq!(reimported.recipe, entry.recipe);
    assert_eq!(reimported.recipe.effects, perk.effects);
}
