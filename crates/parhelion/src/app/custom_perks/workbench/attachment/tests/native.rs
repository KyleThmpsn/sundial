use super::*;
use crate::app::custom_perks::editor::tests::{editor, fixture, set_test_speed};

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES for workbench navigation"]
fn native_reopening_a_different_perk_does_not_reuse_the_previous_parameter_editor() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let temporary = tempfile::tempdir().unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &temporary.path().join("catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let donor = catalog.weapon_donor(0x4CE3_CE93).unwrap();
    let weapon = weapon(&donor, 13);
    let expanded = socket_editor::socket_editor_donor(&donor, &weapon).into_owned();
    let first = Target::capture(&weapon, &expanded, 0, 0).unwrap();
    let second = Target::capture(&weapon, &expanded, 0, 12).unwrap();
    let mut parameter_editor = editor(fixture());
    let graph = parameter_editor.graph.clone().unwrap();
    set_test_speed(&graph, &mut parameter_editor.draft, 1.5);
    let expected = parameter_editor.draft.clone();
    let mut workbench = Workbench::default();
    workbench.set_test_editor(parameter_editor);
    workbench.documents[0].target = Some(first.clone());
    workbench.open = false;
    workbench.open_target(second, &catalog);
    assert_eq!(workbench.selected, 1);
    assert!(workbench.editor.is_none());
    let second_before = workbench.documents[1].recipe.clone();
    let ctx = egui::Context::default();
    for _ in 0..3 {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 1100.0),
                )),
                ..Default::default()
            },
            |ctx| {
                workbench.show(
                    ctx,
                    &packages,
                    Some(&catalog),
                    &[],
                    false,
                    (&weapon, Some(&expanded)),
                );
            },
        );
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == "Apply and Back")));
    }
    assert_eq!(workbench.documents[1].recipe, second_before);
    assert!(workbench.documents[1].pending_effect.is_none());
    assert_eq!(
        workbench.documents[0]
            .pending_effect
            .as_ref()
            .unwrap()
            .values,
        expected
    );
    workbench.open_target(first, &catalog);
    assert_eq!(workbench.selected, 0);
    assert_eq!(
        workbench.documents[0]
            .pending_effect
            .as_ref()
            .unwrap()
            .values,
        expected
    );
}
