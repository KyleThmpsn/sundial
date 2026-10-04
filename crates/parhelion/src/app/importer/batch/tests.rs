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

#[test]
fn losing_all_workers_reports_each_unfinished_item_without_false_success() {
    let cancel = AtomicBool::new(false);
    let mut failures = Vec::new();
    let outcome = run::<()>(
        4,
        1,
        &cancel,
        |_, _| panic!("Injected worker panic"),
        |index, result| {
            failures.push((index, result.unwrap_err()));
        },
    );
    assert_eq!(outcome.completed, 4);
    assert!(!outcome.cancelled);
    assert_eq!(
        failures.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    crate::test_support::artifact("batch-import-all-workers-failed.json", &failures);
}

#[test]
fn cancellation_retains_completed_work_and_reports_a_failed_active_item() {
    for panic in [false, true] {
        let cancel = AtomicBool::new(false);
        let mut results = Vec::new();
        let outcome = run(
            4,
            1,
            &cancel,
            |_, index| {
                cancel.store(true, Ordering::Relaxed);
                assert!(!panic, "Injected active conversion panic");
                Ok(index)
            },
            |index, result| results.push((index, result)),
        );
        assert!(outcome.cancelled);
        assert_eq!(outcome.completed, 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 0);
        assert_eq!(results[0].1.is_err(), panic);
        crate::test_support::artifact(
            if panic {
                "batch-import-cancelled-failure.json"
            } else {
                "batch-import-cancelled.json"
            },
            &serde_json::json!({"completed":outcome.completed,"cancelled":outcome.cancelled,"results":results}),
        );
    }
}

#[test]
fn empty_and_single_item_batches_finish_with_extreme_worker_counts() {
    let cancel = AtomicBool::new(false);
    let empty = run::<()>(
        0,
        usize::MAX,
        &cancel,
        |_, _| panic!("empty batch"),
        |_, _| panic!("empty result"),
    );
    assert_eq!(empty.completed, 0);
    for workers in [0, usize::MAX] {
        let mut values = Vec::new();
        let result = run(
            1,
            workers,
            &cancel,
            |_, index| Ok(index),
            |_, result| values.push(result.unwrap()),
        );
        assert_eq!(result.completed, 1);
        assert_eq!(values, [0]);
    }
}
