use super::*;

#[test]
fn browser_opens_with_search_and_closes_on_escape_without_a_selection() {
    for width in [640.0, 1320.0] {
        let ctx = egui::Context::default();
        let mut query = String::new();
        let mut frame = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let result = browser(
                            ui,
                            "test-browser",
                            "Choose",
                            "Choose an Asset",
                            &mut query,
                            |ui, _, _, height| {
                                ui.label("Asset Preview");
                                assert!(height > 100.0);
                                None::<u32>
                            },
                        );
                        assert!(result.is_none());
                    });
                },
            )
        };
        let mut output = frame(vec![]);
        for _ in 0..2 {
            output = frame(vec![]);
        }
        let position = label(&output, "Choose").center();
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        for _ in 0..3 {
            output = frame(vec![]);
        }
        for name in ["Asset Preview", "Clear", "Choose an Asset"] {
            let rect = label(&output, name);
            assert!(rect.min.x >= 0.0 && rect.max.x <= width, "{name}: {rect:?}");
        }
        frame(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        output = frame(vec![]);
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Asset Preview")));
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

#[test]
fn show_all_starts_off_and_large_lists_have_no_two_hundred_row_cutoff() {
    let ctx = egui::Context::default();
    let keys = (0..500).collect::<Vec<_>>();
    let mut indices = Vec::new();
    for _ in 0..3 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1050.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(!show_all(ui).0);
                    // Keep a result after the old group cap selected and inspectable.
                    ui.data_mut(|state| {
                        state.insert_temp(ui.make_persistent_id("inspected-choice"), 450u64)
                    });
                    BrowserList {
                        keys: &keys,
                        height: 500.0,
                        reset: false,
                        row_height: 30.0,
                        select: None,
                    }
                    .draw(
                        ui,
                        |ui, index, selected| {
                            indices.push(index);
                            ui.selectable_label(selected, format!("Choice {index}"))
                        },
                        |ui, index| {
                            assert_eq!(index, 450);
                            ui.label("Preview 450");
                            None::<()>
                        },
                    );
                });
            },
        );
    }
    assert!(indices.len() < 100, "only visible rows should be laid out");
}
