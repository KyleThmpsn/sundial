use super::*;
use crate::{RecipeLibrary, WeaponRecipe};

pub(super) fn downloaded() -> Downloaded {
    let recipe = WeaponRecipe::new_weapon("parhelion.community-test").unwrap();
    let bytes = recipe.to_json_pretty().unwrap().into_bytes();
    let listing = Listing {
        id: "community-test".into(),
        author: "A Creator".into(),
        description: "A test recipe".into(),
        tags: vec!["test".into()],
        version: 1,
        license: "GPL-3.0-only".into(),
        source_url: "https://github.com/KyleThmpsn/parhelion-recipes".into(),
        tested_with: TestedWith {
            sundial: "unknown".into(),
            sunrise: "unknown".into(),
        },
        gameplay_status: "unverified".into(),
        gameplay_notes: "Not tested in game.".into(),
        remix_of: None,
    };
    let entry = Entry {
        listing,
        name: recipe.name.clone(),
        namespace: recipe.namespace.clone(),
        download: "recipes/community-test/recipe.parhelion.json".into(),
        sha256: checksum(&bytes),
        bytes: bytes.len(),
        downloads: 0,
    };
    Downloaded::checked(entry, &bytes).unwrap()
}

fn next_version(mut downloaded: Downloaded) -> Downloaded {
    downloaded.entry.listing.version += 1;
    downloaded.recipe.flavor = "Updated by the creator".into();
    let bytes = downloaded.recipe.to_json_pretty().unwrap().into_bytes();
    downloaded.entry.bytes = bytes.len();
    downloaded.entry.sha256 = checksum(&bytes);
    Downloaded::checked(downloaded.entry, &bytes).unwrap()
}

#[test]
fn rejects_tampered_download_and_catalog_paths() {
    let downloaded = downloaded();
    assert!(
        Downloaded::checked(downloaded.entry.clone(), b"{}")
            .unwrap_err()
            .contains("checksum")
    );
    let mut catalog = Catalog {
        schema: 1,
        recipes: vec![downloaded.entry],
    };
    catalog.validate().unwrap();
    catalog.recipes[0].download = "../../secret".into();
    assert!(catalog.validate().is_err());
    catalog.recipes[0].download = "https://example.com/recipe.json".into();
    assert!(catalog.validate().is_err());
}

#[test]
fn updates_untouched_recipe_and_preserves_edited_recipe() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let first = downloaded();
    let path = install(&library, &first).unwrap();
    let second = next_version(first.clone());
    assert_eq!(install(&library, &second).unwrap(), path);
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), second.recipe);
    let mut edited = second.recipe.clone();
    edited.flavor = "My local changes".into();
    library.save_existing(&path, &edited).unwrap();
    let third = next_version(second);
    assert!(install(&library, &third).unwrap_err().contains("edited"));
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), edited);
    assert_eq!(
        load_receipt(&library, "community-test")
            .unwrap()
            .unwrap()
            .version,
        2
    );
}

#[test]
fn can_adopt_identical_local_recipe_but_never_overwrite_a_different_one() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let downloaded = downloaded();
    let path = library.save_new(&downloaded.recipe).unwrap();
    assert_eq!(install(&library, &downloaded).unwrap(), path);
    let mut modified = downloaded.recipe.clone();
    modified.flavor = "Preserve me".into();
    library.save_existing(&path, &modified).unwrap();
    assert!(install(&library, &downloaded).is_err());
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), modified);
}

#[test]
fn remix_has_an_independent_identity_and_preserves_original() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let downloaded = downloaded();
    let original = install(&library, &downloaded).unwrap();
    let copy = remix(&library, &downloaded, "My Community Remix").unwrap();
    let recipe = WeaponRecipe::load_json(&copy).unwrap();
    assert_ne!(recipe.identity, downloaded.recipe.identity);
    assert_ne!(recipe.namespace, downloaded.recipe.namespace);
    assert_eq!(
        remix_origin(&library, &recipe.namespace).unwrap(),
        Some("community-test".into())
    );
    assert_eq!(
        WeaponRecipe::load_json(&original).unwrap(),
        downloaded.recipe
    );
}

#[test]
fn receipt_cannot_point_outside_library() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let downloaded = downloaded();
    install(&library, &downloaded).unwrap();
    let mut receipt = load_receipt(&library, "community-test").unwrap().unwrap();
    receipt.file_name = "../outside.parhelion.json".into();
    std::fs::write(
        library.root().join("community-community-test.receipt"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    assert!(load_receipt(&library, "community-test").is_err());
}

#[test]
fn updates_cannot_change_identity_or_move_backwards() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let first = downloaded();
    let second = next_version(first.clone());
    let path = install(&library, &second).unwrap();
    assert!(install(&library, &first).unwrap_err().contains("older"));
    let mut third = next_version(second.clone());
    third
        .recipe
        .rename_authored_item("Different Identity")
        .unwrap();
    assert!(install(&library, &third).unwrap_err().contains("identity"));
    assert_eq!(WeaponRecipe::load_json(path).unwrap(), second.recipe);
}
