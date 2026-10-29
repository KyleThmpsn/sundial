use super::*;

#[test]
fn saved_recipes_keep_their_identity_when_listing_cache_text_is_unusable() {
    let directory = tempfile::tempdir().unwrap();
    let first = RecipeLibrary::open(directory.path().join("first")).unwrap();
    let second = RecipeLibrary::open(directory.path().join("second")).unwrap();
    let first_recipe = WeaponRecipe::new_weapon("parhelion.cache-first").unwrap();
    let second_recipe = WeaponRecipe::new_weapon("parhelion.cache-second").unwrap();
    let first_path = first.root().join("cached.parhelion.json");
    let second_path = second.root().join("cached.parhelion.json");
    let source = |recipe: &WeaponRecipe| {
        let mut bytes = recipe.to_json_pretty().unwrap().into_bytes();
        bytes.resize((1 << 20) + 1024, b' ');
        bytes
    };
    let first_bytes = source(&first_recipe);
    let second_bytes = source(&second_recipe);
    fs::write(&first_path, &first_bytes).unwrap();
    fs::write(&second_path, &second_bytes).unwrap();
    let modified = fs::metadata(&first_path).unwrap().modified().unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&second_path)
        .unwrap()
        .set_modified(modified)
        .unwrap();

    // An intact cache envelope can hold truncated recipe text. Neither a save's identity
    // review nor the library browser may treat it as a broken source recipe.
    let corrupt = cache(&first_path, &first_path, "{");
    first
        .save_new(&WeaponRecipe::new_weapon("parhelion.cache-unrelated").unwrap())
        .unwrap();
    cache(&first_path, &first_path, "{");
    let first_scan = first.scan().unwrap();
    assert!(first_scan.errors.is_empty(), "{:?}", first_scan.errors);
    assert!(
        first_scan
            .entries
            .iter()
            .any(|entry| { entry.path == first_path && entry.namespace == first_recipe.namespace })
    );

    // Sibling libraries can have the same filename, length and timestamp. Their saved
    // identities must still come from their own source, even when the cache was written by
    // the other library.
    let foreign = cache(
        &second_path,
        &first_path,
        &first_recipe.to_json_pretty().unwrap(),
    );
    let mut collision = WeaponRecipe::new_weapon("parhelion.cache-collision").unwrap();
    collision.identity.flavor_hash = second_recipe.identity.flavor_hash.clone();
    let error = second.save_new(&collision).unwrap_err();
    assert!(error.contains("identity hash"), "{error}");
    let second_scan = second.scan().unwrap();
    assert!(second_scan.errors.is_empty(), "{:?}", second_scan.errors);
    assert!(
        second_scan.entries.iter().any(|entry| {
            entry.path == second_path && entry.namespace == second_recipe.namespace
        })
    );
    assert_eq!(fs::read(&first_path).unwrap(), first_bytes);
    assert_eq!(fs::read(&second_path).unwrap(), second_bytes);
    assert_eq!(WeaponRecipe::load_json(&first_path).unwrap(), first_recipe);
    assert_eq!(
        WeaponRecipe::load_json(&second_path).unwrap(),
        second_recipe
    );

    if let Some(output) = crate::test_support::artifacts("recipe-listing-cache-recovery") {
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("first.parhelion.json"), first_bytes).unwrap();
        fs::write(output.join("second.parhelion.json"), second_bytes).unwrap();
        fs::write(output.join("corrupt-cache.json"), corrupt).unwrap();
        fs::write(output.join("foreign-cache.json"), foreign).unwrap();
        fs::write(
            output.join("readback.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "first_namespace": first_recipe.namespace,
                "second_namespace": second_recipe.namespace,
                "refused_collision": error,
                "sources_unchanged": true,
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

fn cache(path: &Path, source: &Path, document: &str) -> Vec<u8> {
    let metadata = fs::metadata(path).unwrap();
    let modified = metadata
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "format": 1,
        "source": fs::canonicalize(source).unwrap(),
        "len": metadata.len(),
        "modified": [modified.as_secs(), modified.subsec_nanos()],
        "document": document,
    }))
    .unwrap();
    let saved = path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("cache/recipe-listings")
        .join(path.file_name().unwrap());
    fs::create_dir_all(saved.parent().unwrap()).unwrap();
    fs::write(saved, &bytes).unwrap();
    bytes
}
