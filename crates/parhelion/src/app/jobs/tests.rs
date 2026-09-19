use super::*;

#[test]
fn invalidated_build_does_not_become_installable_when_its_worker_finishes() {
    let mut app = PackageAuthoringApp::default();
    let (sender, receiver) = mpsc::channel();
    app.build_receiver = Some(receiver);
    app.recipe.flavor = "An edit after the build snapshot".into();
    app.synchronize_recipe_dirty();
    sender
        .send(BuildWorkerEvent::Finished {
            result: Ok(BuildReport {
                weapons: vec![],
                run_directory: "older-staged-run".into(),
                manifest_path: "older-staged-run/manifest.json".into(),
                artifacts: vec![],
                selection_fingerprint: "older-selection".into(),
                staged_recipe_paths: vec![],
            }),
            elapsed: Duration::from_secs(1),
        })
        .unwrap();
    app.poll_build();
    assert!(app.build_receiver.is_none());
    assert!(
        app.latest_build
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .contains("changed")
    );
    assert!(app.replacement_review.is_none());
}

#[test]
fn build_saves_current_edits_before_snapshot_without_changing_selection() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.root().join("every-end.parhelion.json");
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library),
        ..Default::default()
    };
    assert!(app.open_recipe_path(&path));
    app.enabled_recipe_paths = BTreeSet::from([path.clone()]);
    app.recipe.flavor = "Edits saved by Build & Stage".into();
    app.recipe.overrides.socket_columns = vec![Some(WeaponSocketColumnRecipe {
        socket_type: Some(700),
        choices: vec![0x1234.into()],
        ..Default::default()
    })];
    // Do not rely on the previous frame's dirty flag.
    assert!(!app.recipe_dirty);
    app.save_edits_for_build().unwrap();
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), app.recipe);
    assert_eq!(app.recipe_baseline, app.recipe);
    assert_eq!(app.observed_recipe, app.recipe);
    assert!(!app.recipe_dirty);
    assert_eq!(app.enabled_recipe_paths, BTreeSet::from([path]));
    let request = app.batch_request().unwrap();
    assert_eq!(request.recipes, vec![app.recipe.clone()]);
}

#[test]
fn build_save_failure_preserves_draft_and_does_not_start_worker() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.root().join("every-end.parhelion.json");
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library.clone()),
        ..Default::default()
    };
    assert!(app.open_recipe_path(&path));
    let baseline = app.recipe_baseline.clone();
    let mut external = app.recipe.clone();
    external.flavor = "Changed by another editor".into();
    library.save_existing(&path, &external).unwrap();
    app.recipe.flavor = "Keep my draft".into();
    app.synchronize_recipe_dirty();
    let draft = app.recipe.clone();
    app.start_build();
    assert!(app.build_receiver.is_none());
    assert!(
        app.latest_build
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .contains("changed on disk")
    );
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), external);
    assert_eq!(app.recipe, draft);
    assert_eq!(app.recipe_baseline, baseline);
    assert!(app.recipe_dirty);
}

#[test]
fn build_saves_new_draft_without_silently_including_it() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let recipe = WeaponRecipe::new_weapon("parhelion.autosave-new").unwrap();
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library),
        recipe: recipe.clone(),
        recipe_baseline: recipe,
        recipe_requires_initial_save: true,
        ..Default::default()
    };
    app.save_edits_for_build().unwrap();
    let path = app.recipe_path.as_ref().unwrap();
    assert_eq!(WeaponRecipe::load_json(path).unwrap(), app.recipe);
    assert!(!app.recipe_requires_initial_save);
    assert!(app.enabled_recipe_paths.is_empty());
}

fn key(hash: u32) -> RuntimeGraphKey {
    RuntimeGraphKey::new(None, hash, [])
}

#[test]
fn stale_worker_error_preserves_the_current_error() {
    let (sender, receiver) = mpsc::channel();
    sender.send(Err("old failure".to_owned())).unwrap();
    drop(sender);
    let current_error = Some((key(2), "current failure".to_owned()));
    let mut app = PackageAuthoringApp {
        runtime_graph_target: Some(key(2)),
        runtime_graph_error: current_error.clone(),
        runtime_graph_job: Some(RuntimeGraphJob {
            key: key(1),
            receiver,
            worker: thread::spawn(|| {}),
        }),
        ..PackageAuthoringApp::default()
    };
    app.poll_runtime_graph();
    assert!(app.runtime_graph_job.is_none());
    assert_eq!(app.runtime_graph_error, current_error);
}

#[test]
fn runtime_worker_remains_owned_until_completion_and_reports_only_current_disconnects() {
    for target in [1, 2] {
        let (sender, receiver) = mpsc::channel();
        let mut app = PackageAuthoringApp {
            runtime_graph_target: Some(key(target)),
            runtime_graph_job: Some(RuntimeGraphJob {
                key: key(1),
                receiver,
                worker: thread::spawn(|| {}),
            }),
            ..PackageAuthoringApp::default()
        };
        app.poll_runtime_graph();
        assert!(app.runtime_graph_job.is_some());
        assert!(app.runtime_graph_error.is_none());
        assert!(app.has_background_work());
        drop(sender);
        app.poll_runtime_graph();
        assert!(app.runtime_graph_job.is_none());
        assert_eq!(app.runtime_graph_target, Some(key(target)));
        if target == 1 {
            let (failed, message) = app.runtime_graph_error.unwrap();
            assert_eq!(failed, key(1));
            assert!(message.contains("without a result"));
        } else {
            assert!(app.runtime_graph_error.is_none());
        }
    }
}
