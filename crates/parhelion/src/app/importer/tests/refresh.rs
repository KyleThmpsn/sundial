//! Persisted library scan through the collective revision review.
use super::*;
use serde_json::json;

#[test]
fn library_review_collects_old_graphs_and_dismisses_the_whole_batch() {
    let root = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(root.path().join("library")).unwrap();
    let current = parhelion_import::CONVERTER_REVISION;
    let mut expected = BTreeSet::new();
    for (name, revision, attachment) in [
        ("Older Weapon", current - 1, None),
        ("Older Vehicle", 0, None),
        (
            "Current Armor With Older Attachment",
            current,
            Some(current - 1),
        ),
        ("Current Import", current, None),
        ("Future Import", current + 1, None),
    ] {
        let mut recipe = WeaponRecipe::new_unbound_kind(ItemKind::Sparrow).unwrap();
        recipe.set_donor(0x3001, "Native Gear");
        recipe.name = name.into();
        recipe.namespace = format!("parhelion.reach.{}", name.replace(' ', "-").to_lowercase());
        recipe.identity = crate::WeaponCloneIdentity::from_namespace(&recipe.namespace)
            .unwrap()
            .into();
        let path = library.save_new(&recipe).unwrap();
        let mut document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        document["overrides"]["imported_graph"] = json!({
            "converter_revision": revision, "sha256": "unused-in-listing",
            "embedded_assets": {"schema":1,"graph":"{}","files":{}},
            "attachments": attachment.map(|revision| vec![json!({"converter_revision":revision})]).unwrap_or_default()
        });
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        if revision < current || attachment.is_some() {
            expected.insert(name.to_owned());
        }
    }
    let scan = library.scan().unwrap();
    assert!(scan.errors.is_empty(), "{:?}", scan.errors);
    let mut app = app_with_catalog();
    app.recipe_entries = scan.entries;
    app.importer.refresh.observe(&app.recipe_entries);
    assert_eq!(
        app.importer
            .refresh
            .pending
            .iter()
            .map(|row| row.name.clone())
            .collect::<BTreeSet<_>>(),
        expected
    );
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..2 {
        output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(980., 640.),
                )),
                ..Default::default()
            },
            |ui| {
                app.draw_import_refresh(ui.ctx());
            },
        );
    }
    assert!(text(&output).contains("3 were converted by an older importer"));
    capture::write(&ctx, &output, "import-library-revision-review");
    app.importer.refresh.dismiss();
    app.importer
        .refresh
        .observe(&library.scan().unwrap().entries);
    assert!(
        app.importer.refresh.pending.is_empty(),
        "A dismissed library scan prompted again"
    );
}
