use super::*;

fn save_key() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::S,
        physical_key: Some(egui::Key::S),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }
}

#[test]
fn save_shortcut_writes_the_current_perk_and_keeps_other_files_unchanged() {
    let (_temporary, library, original, entry, mut workbench) = workbench_with_saved_perk();
    let mut sibling = PerkRecipe::new();
    sibling.name = "Sibling".into();
    let sibling = library.save(&sibling, None).unwrap();
    workbench.open = true;
    workbench.documents[0].recipe.description = "Saved with Ctrl+S".into();
    let ctx = egui::Context::default();
    frame(
        &ctx,
        &mut workbench,
        false,
        egui::vec2(1000.0, 720.0),
        vec![save_key()],
    );
    let saved = Library::read(&entry.path).unwrap();
    assert_eq!(saved.recipe.id, original.id);
    assert_eq!(saved.recipe.description, "Saved with Ctrl+S");
    assert_eq!(
        workbench.documents[0].baseline.as_deref(),
        Some(saved.baseline.as_slice())
    );
    assert_eq!(std::fs::read(sibling.path).unwrap(), sibling.baseline);
}

#[test]
fn save_shortcut_preserves_unapplied_parameters_and_closed_workbenches() {
    let (_temporary, _library, _original, entry, mut workbench) = workbench_with_saved_perk();
    let ctx = egui::Context::default();
    workbench.documents[0].recipe.description = "Not saved yet".into();
    frame(
        &ctx,
        &mut workbench,
        false,
        egui::vec2(1000.0, 720.0),
        vec![save_key()],
    );
    assert_eq!(std::fs::read(&entry.path).unwrap(), entry.baseline);
    workbench.open = true;
    workbench.editor = Some(editor(fixture()));
    workbench.editing_effect = Some(405);
    frame(
        &ctx,
        &mut workbench,
        false,
        egui::vec2(1000.0, 720.0),
        vec![save_key()],
    );
    assert_eq!(std::fs::read(&entry.path).unwrap(), entry.baseline);
    assert!(
        workbench
            .error
            .as_deref()
            .unwrap()
            .contains("Apply or discard")
    );
    assert!(workbench.editor.is_some());
    assert!(workbench.documents[0].pending_effect.is_some());
}

#[test]
fn sidebar_search_and_sort_share_a_row_within_the_sidebar() {
    for width in [220.0, 260.0, 290.0] {
        let ctx = egui::Context::default();
        let mut workbench = Workbench::default();
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 400.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        workbench.draw_library(ui, None, false, None);
                    });
                },
            );
        }
        let search = label(&output, "Search Perks").unwrap();
        let sort = label(&output, "Most Recent").unwrap();
        let heading = label(&output, "Custom Perks").unwrap();
        let action = label(&output, "New Perk").unwrap();
        assert!((search.center().y - sort.center().y).abs() < 2.0);
        assert!(search.right() < sort.left());
        assert!(sort.right() < width);
        assert!(heading.bottom() < action.top());
        assert!(action.bottom() < search.top());
    }
}

#[test]
fn truncated_program_summary_opens_only_one_tooltip() {
    use sundial::package_authoring::sandbox_perk::program::{Action, Program};
    let program = Program {
        actions: (1..=8).map(Action::add_rounds).collect(),
        ..Program::default()
    };
    let summary = super::super::guidance::summary_with_assets(&program, None, None);
    let mut recipe = PerkRecipe::new();
    recipe.effects.push(WeaponSandboxPerkRuntimeRecipe {
        program: Some(program),
        ..PerkRecipe::effect(421)
    });
    let mut workbench = Workbench {
        open: true,
        initialized: true,
        documents: vec![Document::new(recipe, None)],
        ..Default::default()
    };
    let ctx = egui::Context::default();
    ctx.style_mut(|style| style.interaction.tooltip_delay = 0.0);
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(
            &ctx,
            &mut workbench,
            false,
            egui::vec2(1000.0, 720.0),
            vec![],
        );
    }
    let position = label(&output, &summary).expect("program summary").center();
    for _ in 0..4 {
        output = frame(
            &ctx,
            &mut workbench,
            false,
            egui::vec2(1000.0, 720.0),
            vec![egui::Event::PointerMoved(position)],
        );
    }
    let summaries = output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == summary)).count();
    assert_eq!(summaries, 2, "one summary label and one tooltip");
}
