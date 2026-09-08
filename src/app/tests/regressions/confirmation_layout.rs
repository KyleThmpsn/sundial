use super::*;

#[test]
fn save_review_stays_within_minimum_window_with_many_long_changes() {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for size in [egui::vec2(720.0, 520.0), egui::vec2(1240.0, 900.0)] {
            let directory = TestDirectory::new("save-review-compact");
            let mut app = app(directory.0.clone());
            for index in 0..CHANGE_REVIEW_LIMIT + 1 {
                app.document.json_mut()[format!("future_setting_{index}")] =
                    serde_json::json!("A long setting value with spaces ".repeat(8));
            }
            let expected = app.document.clone();
            app.confirmation = Some(ConfirmationDialog::ReviewSave);
            app.pending_save_action = Some(SaveAction::SaveAndExit);
            let context = egui::Context::default();
            context.set_theme(theme);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for _ in 0..3 {
                let _ = context.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| app.draw_save_review_confirmation(ctx),
                );
            }
            let modal = context
                .memory(|memory| memory.area_rect("review_settings_changes"))
                .expect("save review is open");
            assert!(
                screen.shrink(8.0).contains_rect(modal),
                "modal {modal:?}, screen {screen:?}"
            );
            assert_eq!(app.document, expected);
            assert!(app.confirmation == Some(ConfirmationDialog::ReviewSave));
        }
    }
}
