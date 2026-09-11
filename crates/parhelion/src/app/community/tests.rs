use super::*;

pub(super) fn recipe() -> Downloaded {
    let recipe = WeaponRecipe::new_weapon("parhelion.community-ui-test").unwrap();
    let bytes = recipe.to_json_pretty().unwrap().into_bytes();
    let entry = service::Entry {
        listing: service::Listing {
            id: "community-ui-test".into(),
            author: "Creator".into(),
            description: "A community weapon".into(),
            tags: vec!["test".into()],
            version: 1,
            license: "GPL-3.0-only".into(),
            source_url: "https://github.com/KyleThmpsn/parhelion-recipes".into(),
            tested_with: service::TestedWith {
                sundial: "unknown".into(),
                sunrise: "unknown".into(),
            },
            gameplay_status: "unverified".into(),
            gameplay_notes: "Not tested in game".into(),
            remix_of: None,
        },
        name: recipe.name.clone(),
        namespace: recipe.namespace.clone(),
        download: "recipes/community-ui-test/recipe.parhelion.json".into(),
        sha256: service::checksum(&bytes),
        bytes: bytes.len(),
        downloads: 0,
    };
    Downloaded::checked(entry, &bytes).unwrap()
}

#[test]
fn community_update_preserves_unsaved_workbench_edits() {
    let temporary = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(temporary.path().join("recipes")).unwrap();
    let first = recipe();
    let path = service::install(&library, &first).unwrap();
    let mut app = PackageAuthoringApp {
        recipe: first.recipe.clone(),
        recipe_baseline: first.recipe.clone(),
        recipe_path: Some(path.clone()),
        recipe_library: Some(library),
        ..Default::default()
    };
    app.recipe.flavor = "An unsaved local edit".into();
    let edited = app.recipe.clone();
    let mut window = Window::default();
    let error = app
        .apply_community_action(Action::Install(Box::new(first.clone())), &mut window, true)
        .unwrap_err();
    assert!(error.contains("open edits"));
    assert_eq!(app.recipe, edited);
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), first.recipe);
}

#[test]
fn community_window_does_not_start_network_or_mutate_recipes_while_rendering() {
    let downloaded = recipe();
    let current = downloaded.recipe.clone();
    for tab in [Tab::Browse, Tab::Downloads, Tab::Share] {
        let ctx = egui::Context::default();
        let mut window = Window {
            open: true,
            tab,
            catalog: Some(Catalog {
                schema: 1,
                recipes: vec![downloaded.entry.clone()],
            }),
            downloaded: Some(downloaded.clone()),
            ..Default::default()
        };
        window.prepare_share(&current);
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ctx| window.show(ctx, &current, true),
            );
            assert!(!output.shapes.is_empty());
        }
        assert!(window.worker.is_none());
        assert!(window.action.is_none());
        assert_eq!(window.share.recipe.as_ref(), Some(&current));
    }
}
