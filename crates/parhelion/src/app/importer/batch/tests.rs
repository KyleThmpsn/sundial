use super::*;
use crate::{RecipeLibrary, WeaponRecipe};

#[test]
fn a_panicked_converter_reports_failure_while_other_recipes_save_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut saved = Vec::new();
    let mut errors = Vec::new();
    let outcome = run(
        8,
        2,
        &cancel,
        |_, index| {
            if index == 1 {
                panic!("Injected converter panic");
            }
            let recipe = WeaponRecipe::new_weapon(format!("parhelion.batch-{index}")).unwrap();
            let path = directory.path().join(format!("converted-{index}.json"));
            recipe.save_json(&path).unwrap();
            Ok(path)
        },
        |index, result| match result
            .and_then(|path| super::super::save_imported_recipe(&library, &[], &path))
        {
            Ok(path) => saved.push((index, WeaponRecipe::load_json(path).unwrap())),
            Err(error) => errors.push((index, error)),
        },
    );
    assert_eq!(outcome.completed, 8);
    assert!(!outcome.cancelled);
    assert_eq!(saved.len(), 7);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, 1);
    assert!(errors[0].1.contains("worker"));
    let reopened = RecipeLibrary::open(library.root().to_path_buf()).unwrap();
    let scan = reopened.scan().unwrap();
    assert!(scan.errors.is_empty());
    for (_, recipe) in &saved {
        assert_eq!(
            scan.entries
                .iter()
                .filter(|entry| entry.namespace == recipe.namespace)
                .count(),
            1
        );
    }
    saved.sort_by_key(|(index, _)| *index);
    crate::test_support::artifact(
        "batch-import-worker-failure.json",
        &serde_json::json!({
            "completed":outcome.completed,"cancelled":outcome.cancelled,"saved":saved,"errors":errors,
        }),
    );
}
