use super::*;
use crate::app::custom_perks::editor::tests::set_test_speed;

#[test]
fn property_scroll_reaches_the_end_from_the_right_side_and_keeps_the_footer_clear() {
    let mut loaded = fixture();
    loaded.warnings = (0..40)
        .map(|index| format!("Property Warning {index}"))
        .collect();
    let mut parameter_editor = editor(loaded);
    parameter_editor.entity_source = Some(0x8152_9C54);
    let mut workbench = Workbench::default();
    workbench.set_test_editor(parameter_editor);
    let mut recipe = workbench.documents[0].recipe.clone();
    let ctx = egui::Context::default();
    let mut render = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 720.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench.draw_effect_editor(ui, ctx, &mut recipe, false, 360.0);
                    ui.label("Destination Footer");
                });
            },
        )
    };
    let visible = |output: &egui::FullOutput, name: &str| {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => {
                let rect = text.galley.rect.translate(text.pos.to_vec2());
                shape.clip_rect.contains_rect(rect).then_some(rect)
            }
            _ => None,
        })
    };
    let mut output = render(vec![]);
    for _ in 0..3 {
        output = render(vec![]);
    }
    let header = visible(&output, "Apply and Back").unwrap();
    let footer = visible(&output, "Destination Footer").unwrap();
    assert!(visible(&output, "Native Structure").is_none());
    render(vec![
        egui::Event::PointerMoved(egui::pos2(970.0, 240.0)),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -5000.0),
            modifiers: Default::default(),
        },
    ]);
    for _ in 0..30 {
        output = render(vec![]);
    }
    let last = visible(&output, "Native Structure")
        .expect("Last property section is fully reachable from the right side");
    assert!(last.bottom() < footer.top());
    assert_eq!(visible(&output, "Apply and Back"), Some(header));
    assert_eq!(visible(&output, "Destination Footer"), Some(footer));
}

#[test]
fn switching_documents_preserves_pending_edits_under_their_original_owner() {
    for existing in [false, true] {
        let mut parameter_editor = editor(fixture());
        let graph = parameter_editor.graph.clone().unwrap();
        set_test_speed(&graph, &mut parameter_editor.draft, 1.5);
        let expected = parameter_editor.draft.clone();
        let mut workbench = Workbench::default();
        workbench.set_test_editor(parameter_editor);
        let original = workbench.documents[0].recipe.clone();
        workbench.open = false;
        let mut second = PerkRecipe::new();
        second.name = "Another Perk".into();
        second.effects.push(PerkRecipe::effect(1178));
        if existing {
            workbench
                .documents
                .push(Document::new(second.clone(), None));
            workbench.select_document(1);
        } else {
            workbench.add_document(Document::new(second.clone(), None));
        }
        assert_eq!(workbench.selected, 1);
        assert!(workbench.editor.is_none());
        assert!(workbench.editing_effect.is_none());
        assert!(workbench.editing_program_action.is_none());
        assert_eq!(workbench.documents[0].recipe, original);
        assert_eq!(
            workbench.documents[0]
                .pending_effect
                .as_ref()
                .unwrap()
                .values,
            expected
        );
        assert_eq!(workbench.documents[1].recipe, second);
        assert!(workbench.documents[1].pending_effect.is_none());
        workbench.select_document(0);
        assert_eq!(
            workbench.documents[0]
                .pending_effect
                .as_ref()
                .unwrap()
                .values,
            expected
        );
    }
}

#[test]
fn reselecting_the_current_document_keeps_its_active_editor() {
    let mut workbench = Workbench::default();
    workbench.set_test_editor(editor(fixture()));
    let recipe = workbench.documents[0].recipe.clone();
    workbench.add_document(Document::new(recipe, None));
    assert_eq!(workbench.documents.len(), 1);
    assert!(workbench.editor.is_some());
    assert_eq!(workbench.editing_effect, Some(1178));
}

#[test]
fn switching_documents_keeps_an_unfinished_worker_owned_until_it_finishes() {
    let mut parameter_editor = editor(fixture());
    let (sender, receiver) = mpsc::channel();
    parameter_editor.receiver = Some(receiver);
    let mut workbench = Workbench::default();
    workbench.set_test_editor(parameter_editor);
    workbench.editing_program_action = Some(0);
    workbench.add_document(Document::new(PerkRecipe::new(), None));
    assert!(workbench.editor.is_none());
    assert!(workbench.editing_program_action.is_none());
    assert_eq!(
        workbench.documents[0]
            .pending_effect
            .as_ref()
            .unwrap()
            .action,
        Some(0)
    );
    assert_eq!(workbench.retired_editors.len(), 1);
    assert!(workbench.busy());
    drop(sender);
    workbench.open = false;
    workbench.show(
        &egui::Context::default(),
        Path::new(""),
        None,
        &[],
        false,
        (&WeaponRecipe::every_end(), None),
    );
    assert!(workbench.retired_editors.is_empty());
    assert!(!workbench.busy());
}

#[test]
fn new_draft_clears_search_and_is_visible_above_a_long_library() {
    let ctx = egui::Context::default();
    let mut workbench = Workbench {
        initialized: true,
        query: "Old Search".into(),
        ..Default::default()
    };
    for index in 0..40 {
        let mut recipe = PerkRecipe::new();
        recipe.name = format!("Older Perk {index}");
        workbench.documents.push(Document::new(recipe, None));
    }
    let mut recipe = PerkRecipe::new();
    recipe.name = "New Visible Draft".into();
    workbench.add_document(Document::new(recipe, None));
    assert!(workbench.query.is_empty());
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 400.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| workbench.draw_library(ui, None, true));
            },
        );
    }
    let rect = label(&output, "New Visible Draft · Draft").expect("new draft visible");
    assert!(rect.top() >= 0.0 && rect.bottom() < 400.0);
    workbench.select_document(0);
    workbench.select_document(40);
    assert_eq!(workbench.documents[40].recipe.name, "New Visible Draft");
}
