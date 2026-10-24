use super::*;

#[test]
fn browser_keeps_layout_and_closes_dropdown_before_window_on_escape() {
    for width in [640.0, 1320.0] {
        let ctx = egui::Context::default();
        let mut query = String::new();
        let mut frame = |events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        let result = browser(
                            ui,
                            "test-browser",
                            "Choose",
                            "Choose an Asset",
                            &mut query,
                            |ui, _, _, height| {
                                ui.label("Asset Preview");
                                egui::ComboBox::from_id_salt("test-browser-source")
                                    .selected_text("All Sources")
                                    .show_ui(ui, |ui| {
                                        let _ = ui.selectable_label(false, "Other Source");
                                    });
                                assert!(height > 0.0);
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
        let position = label(&output, "All Sources").center();
        for events in crate::test_support::driver::tap(position) {
            frame(events);
        }
        for _ in 0..2 {
            output = frame(vec![]);
        }
        let _ = label(&output, "Other Source");
        sundial::test_support::capture::write(
            &ctx,
            &output,
            &format!("picker-dropdown-open-{width}"),
        );
        for pressed in [true, false] {
            output = frame(vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        assert!(crate::test_support::driver::label(&output, "Other Source").is_none());
        let _ = label(&output, "Asset Preview");
        sundial::test_support::capture::write(
            &ctx,
            &output,
            &format!("picker-dropdown-closed-{width}"),
        );
        frame(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        output = frame(vec![]);
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Asset Preview")));
        sundial::test_support::capture::write(
            &ctx,
            &output,
            &format!("picker-window-closed-{width}"),
        );
    }
}

fn label(output: &egui::FullOutput, name: &str) -> egui::Rect {
    crate::test_support::driver::label(output, name).unwrap_or_else(|| panic!("missing {name}"))
}
