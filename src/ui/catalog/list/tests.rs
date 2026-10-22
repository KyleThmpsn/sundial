use super::*;

#[test]
fn disabled_choices_preserve_selection_and_resume_keyboard_activation_when_enabled() {
    let ctx = egui::Context::default();
    let mut inspected = Vec::new();
    let mut chosen = Vec::new();
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let mut frame = |enabled, events| {
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        let result = BrowserList {
                            keys: &[10, 20, 30],
                            height: 450.0,
                            reset: false,
                            row_height: 30.0,
                            select: None,
                        }
                        .draw_activating(
                            ui,
                            |ui, index, selected| {
                                ui.selectable_label(selected, format!("Choice {index}"))
                            },
                            |ui, index, activated| {
                                inspected.push(index);
                                ui.label(format!("Preview {index}"));
                                activated.then_some(index)
                            },
                        );
                        chosen.extend(result);
                    });
                });
            },
        );
        if !enabled {
            crate::test_support::capture::write(&ctx, &output, "disabled-picker");
        }
        (inspected.last().copied(), chosen.clone())
    };
    frame(true, vec![]);
    frame(true, vec![]);
    let (selection, applied) = frame(
        false,
        vec![key(egui::Key::ArrowDown), key(egui::Key::Enter)],
    );
    assert_eq!(selection, Some(0));
    assert!(applied.is_empty());
    frame(true, vec![key(egui::Key::ArrowDown), key(egui::Key::Enter)]);
    assert_eq!(inspected.last(), Some(&1));
    assert_eq!(chosen, vec![1]);
}
#[test]
fn inspecting_filtering_and_confirming_are_separate_and_keep_stable_identity() {
    for width in [620.0, 1050.0] {
        let ctx = egui::Context::default();
        let mut keys = vec![10, 20, 30];
        let mut inspected = 0;
        let mut results = Vec::new();
        let mut frame = |keys: &[u64], events, reset| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        let value = BrowserList {
                            keys,
                            height: 650.0,
                            reset,
                            row_height: 30.0,
                            select: None,
                        }
                        .draw(
                            ui,
                            |ui, index, selected| {
                                ui.selectable_label(selected, format!("Choice {}", keys[index]))
                            },
                            |ui, index| {
                                inspected = keys[index];
                                ui.label(format!("Preview {}", keys[index]));
                                ui.button("Use Choice").clicked().then_some(keys[index])
                            },
                        );
                        if let Some(value) = value {
                            results.push(value);
                        }
                    });
                },
            )
        };
        let mut output = frame(&keys, vec![], false);
        for _ in 0..2 {
            output = frame(&keys, vec![], false);
        }
        let click =
            |position: egui::Pos2, pressed| crate::test_support::primary_press(position, pressed);
        let position = label(&output, "Choice 20").center();
        frame(&keys, click(position, true), false);
        output = frame(&keys, click(position, false), false);
        label(&output, "Preview 20");
        keys = vec![30, 20];
        output = frame(&keys, vec![], true);
        label(&output, "Preview 20");
        let position = label(&output, "Use Choice").center();
        frame(&keys, click(position, true), false);
        frame(&keys, click(position, false), false);
        assert_eq!(inspected, 20);
        assert_eq!(results, vec![20]);
    }
}

fn label(output: &egui::FullOutput, name: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {name}"))
}
