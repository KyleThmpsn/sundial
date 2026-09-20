use super::*;

#[test]
fn sunrise_and_dawn_v6_show_no_conversion_or_format_warning() {
    for dawn in [false, true] {
        let (_directory, mut app, mut inspection) = setup();
        inspection.copies[0].dawn = dawn;
        inspection.copies[0].dawn_runtime =
            dawn.then(|| Runtime::inspect(&inspection.copies[0].dll_path));
        app.runtime_choice.inspection = inspection;
        let before = app.document.clone();
        let context = egui::Context::default();
        for _ in 0..2 {
            let output = context.run(egui::RawInput::default(), |ctx| {
                app.draw_runtime_banner(ctx);
                egui::CentralPanel::default().show(ctx, |ui| app.draw_runtime_preferences(ui));
            });
            for shape in output.shapes {
                if let egui::Shape::Text(text) = shape.shape {
                    assert!(!text.galley.text().contains("requires settings"));
                }
            }
        }
        assert_eq!(app.document, before);
    }
}

#[test]
fn runtime_preferences_show_dawn_and_the_v18_account_mismatch() {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for size in [egui::vec2(640.0, 480.0), egui::vec2(1240.0, 900.0)] {
            let (directory, mut app, inspection) = setup();
            let database = crate::persistence::investment_path(&app.settings_path);
            crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
            app.document = WorkspaceDocument::load(
                serde_json::from_str(include_str!(
                    "../../../../../tests/fixtures/sunrise-v18-169fd29-defaults.json"
                ))
                .unwrap(),
                &app.settings_path,
                false,
            );
            app.runtime_choice.inspection = inspection;
            let before = app.document.clone();
            let context = egui::Context::default();
            context.set_theme(theme);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for _ in 0..3 {
                let output = context.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| {
                        app.draw_runtime_banner(ctx);
                        egui::CentralPanel::default().show(ctx, |ui| {
                            egui::ScrollArea::vertical()
                                .show(ui, |ui| app.draw_runtime_preferences(ui));
                        });
                    },
                );
                // Dawn keeps its account in player-state.db at every settings schema, so the
                // v18 storage rule is Sunrise's own and must not be reported against Dawn.
                assert!(!output.shapes.iter().any(|shape| matches!(
                    &shape.shape,
                    egui::Shape::Text(text) if text.galley.text().contains("with JSON accounts")
                )));
                {
                    // The runtime's name and version moved into the Installation facts grid the
                    // preferences page draws; what this view still names is the copy it found.
                    let expected = "Dawn";
                    let text = output
                        .shapes
                        .iter()
                        .find_map(|shape| {
                            if let egui::Shape::Text(text) = &shape.shape
                                && text.galley.text().contains(expected)
                            {
                                Some(text)
                            } else {
                                None
                            }
                        })
                        .expect("runtime and mismatched account source must be visible");
                    assert!(screen.contains_rect(text.galley.rect.translate(text.pos.to_vec2())));
                }
                crate::app::tests::capture::write(
                    &context,
                    &output,
                    &format!("dawn-save-{theme:?}-{}", size.x),
                );
            }
            assert_eq!(app.document, before);
            assert!(!directory.0.join(".sunrise").exists());
        }
    }
}
