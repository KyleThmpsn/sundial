use super::*;

#[test]
fn closing_the_review_keeps_its_worker_and_recipe_changes_discard_the_result() {
    for stale in [false, true] {
        let mut app = PackageAuthoringApp::default();
        let before = app.recipe.clone();
        let (release, wait) = mpsc::channel();
        let (sender, receiver) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let worker = thread::spawn(move || {
            wait.recv().unwrap();
            let _ = sender.send(Err("Synthetic settings check".into()));
            let _ = finished.send(());
        });
        app.runtime_donors.review_job = Some(ReviewJob {
            recipe: before,
            binding_hash: 1,
            donor_hash: 2,
            generation: app.runtime_donors.generation,
            receiver,
            worker,
        });
        app.runtime_donors.close();
        app.poll_runtime_swap();
        assert!(app.runtime_donors.busy());
        if stale {
            app.recipe.overrides.ammo_type = Some(crate::RecipeAmmoType::Heavy);
        }
        let current = app.recipe.clone();
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
        app.poll_runtime_swap();
        assert!(!app.runtime_donors.busy());
        assert_eq!(app.runtime_donors.reviews.is_empty(), stale);
        assert_eq!(app.recipe, current);
    }
}
