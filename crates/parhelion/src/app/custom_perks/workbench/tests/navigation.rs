use super::*;

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
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 720.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let ctx = ui.ctx().clone();
                    workbench.draw_effect_editor(ui, &ctx, &mut recipe, false, 360.0);
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
    assert!(visible(&output, "Advanced").is_none());
    render(vec![
        egui::Event::PointerMoved(egui::pos2(970.0, 240.0)),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -5000.0),
            modifiers: Default::default(),
            phase: egui::TouchPhase::Move,
        },
    ]);
    for _ in 0..30 {
        output = render(vec![]);
    }
    let last = visible(&output, "Advanced")
        .expect("Last property section is fully reachable from the right side");
    assert!(last.bottom() < footer.top());
    assert_eq!(visible(&output, "Apply and Back"), Some(header));
    assert_eq!(visible(&output, "Destination Footer"), Some(footer));
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
