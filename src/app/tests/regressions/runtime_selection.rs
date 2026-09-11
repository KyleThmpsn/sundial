use super::*;

#[test]
fn runtime_selection_fits_small_windows_and_preserves_files() {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for size in [egui::vec2(640.0, 480.0), egui::vec2(1240.0, 900.0)] {
            let directory = TestDirectory::new("runtime-selection-layout");
            for location in crate::package_runtime::installation::RuntimeLocation::ALL {
                let root = location.directory(&directory.0);
                std::fs::create_dir_all(root.join("Sunrise")).unwrap();
                std::fs::write(root.join("steam_api64.dll"), b"runtime").unwrap();
                std::fs::write(root.join("Sunrise/settings.json"), br#"{"version":8}"#).unwrap();
            }
            let mut app = app(directory.0.clone());
            let before = app.document.clone();
            assert!(app.runtime_choice.open);
            let context = egui::Context::default();
            context.set_theme(theme);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            draw_and_check(&mut app, &context, screen, "choose_runtime_copy");
            assert_eq!(app.document, before);
            assert!(!directory.0.join(".sunrise").exists());
            assert!(directory.0.join("steam_api64.dll").exists());
            assert!(directory.0.join("bin/x64/steam_api64.dll").exists());
            let backup = crate::package_runtime::installation::archive_other_runtime(
                &directory.0,
                &app.runtime_choice.inspection,
                crate::package_runtime::installation::RuntimeLocation::BinX64,
                || Ok(()),
            )
            .unwrap();
            app.runtime_choice.pending_restore = Some(
                crate::package_runtime::installation::preview_runtime_restore(
                    &directory.0,
                    &backup,
                )
                .unwrap(),
            );
            draw_and_check(&mut app, &context, screen, "restore_runtime_backup");
            assert!(backup.join("steam_api64.dll").exists());
            assert!(!directory.0.join("steam_api64.dll").exists());
            assert_eq!(app.document, before);
        }
    }
}

fn draw_and_check(app: &mut SundialApp, context: &egui::Context, screen: egui::Rect, id: &str) {
    for _ in 0..3 {
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| app.draw_runtime_choice(ctx),
        );
        super::schema_smoke::capture_preferences(
            context,
            output,
            &format!("{id}-{}-{:?}", screen.width(), context.theme()),
            screen.width(),
        );
    }
    let modal = context.memory(|m| m.area_rect(egui::Id::new(id))).unwrap();
    assert!(
        screen.shrink(8.0).contains_rect(modal),
        "{id}: modal {modal:?}, screen {screen:?}"
    );
}
