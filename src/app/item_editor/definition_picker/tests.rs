use super::*;

#[test]
fn large_picker_scrolls_and_filters_without_building_offscreen_widgets() {
    let catalog = Catalog::for_test(Vec::new(), Default::default());
    let definitions = (0..5000)
        .map(|index| DefinitionChoice {
            hash: index,
            name: format!("Reward {index:04}"),
            type_name: "Material".into(),
            group: None,
        })
        .collect::<Vec<_>>();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut query = String::new();
    let mut action = None;
    let frame = |query: &mut String, action: &mut Option<ItemEditorAction>, open, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let anchor = ui.button("Choose Reward");
                    *action = draw_definition_picker_with_open_request_and_item_filter(
                        ui,
                        &catalog,
                        "reward-scroll-check",
                        query,
                        PickerHeight {
                            min: 200.0,
                            max: 300.0,
                        },
                        (Some(&anchor), open),
                        |_, query, _| {
                            (
                                DefinitionPickerChoices {
                                    definitions: definitions
                                        .iter()
                                        .filter(|row| row.name.contains(query))
                                        .cloned()
                                        .collect(),
                                    existing_inventory: vec![],
                                    clear: None,
                                    random_item_builder_hash: None,
                                    empty_message: "No matching rewards".into(),
                                },
                                false,
                            )
                        },
                    );
                });
            },
        )
    };
    frame(&mut query, &mut action, true, vec![]);
    let output = frame(&mut query, &mut action, false, vec![]);
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .len()
            < 100,
        "Offscreen choices must not create thousands of widgets"
    );
    let names = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if text.galley.job.text.starts_with("Reward ")
                        && text.galley.job.text.contains("  (") =>
                {
                    Some(text.galley.job.text.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let first = names(&output);
    assert!(!first.is_empty());
    let events = vec![
        egui::Event::PointerMoved(egui::pos2(180.0, 170.0)),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -1200.0),
            modifiers: egui::Modifiers::NONE,
        },
    ];
    frame(&mut query, &mut action, false, events);
    let mut output = egui::FullOutput::default();
    for _ in 0..20 {
        output = frame(&mut query, &mut action, false, vec![]);
    }
    assert_ne!(
        first,
        names(&output),
        "The picker must scroll to later rewards"
    );
    query = "Reward 4999".into();
    for _ in 0..3 {
        output = frame(&mut query, &mut action, false, vec![]);
    }
    assert_eq!(
        names(&output).len(),
        1,
        "Filtering after scrolling must reveal the matching reward"
    );
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("Reward 4999  (") => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .unwrap();
    for pressed in [true, false] {
        frame(
            &mut query,
            &mut action,
            false,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(action, Some(ItemEditorAction::SetDefinition { hash: 4999 }));
    crate::app::tests::capture::write(&ctx, &output, "reward-picker-filtered");
}
