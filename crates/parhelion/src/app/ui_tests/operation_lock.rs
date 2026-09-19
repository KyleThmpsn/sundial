use super::*;

fn frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    mut draw: impl FnMut(&egui::Context),
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| draw(ctx),
    )
}

fn set_operation(app: &mut PackageAuthoringApp, installing: bool) {
    if installing {
        app.install_receiver = Some(mpsc::channel().1);
    } else {
        app.build_receiver = Some(mpsc::channel().1);
    }
}

#[test]
fn icon_dialog_keeps_its_draft_while_package_operations_run() {
    for installing in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut app = PackageAuthoringApp::default();
        app.icon_editor = Some(WeaponIconEditor::open(
            &directory.path().join("missing-packages"),
            1,
            "Test Weapon",
            TagHash(1),
            crate::AuthoredWeaponRarity::Legendary,
            app.recipe.overrides.icon_edit.clone(),
        ));
        let before = app.recipe.clone();
        set_operation(&mut app, installing);
        let ctx = egui::Context::default();
        let output = frame(&ctx, vec![], |ctx| app.draw_icon_editor(ctx));
        assert!(output.shapes.is_empty());
        assert!(app.icon_editor.is_some());
        assert_eq!(app.recipe, before);
        app.build_receiver = None;
        app.install_receiver = None;
        frame(&ctx, vec![], |ctx| app.draw_icon_editor(ctx));
        let output = frame(&ctx, vec![], |ctx| app.draw_icon_editor(ctx));
        assert!(text(&output).contains("Edit Weapon Icon"));
        assert_eq!(app.recipe, before);
    }
}

#[test]
fn artwork_dialog_keeps_its_draft_while_package_operations_run() {
    for installing in [false, true] {
        let mut app = PackageAuthoringApp::default();
        let ctx = egui::Context::default();
        let draw = |ctx: &egui::Context, app: &mut PackageAuthoringApp| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.presentation_editor
                    .draw_corner(ui, &mut app.recipe.overrides);
            });
        };
        frame(&ctx, vec![], |ctx| draw(ctx, &mut app));
        let output = frame(&ctx, vec![], |ctx| draw(ctx, &mut app));
        let position = text_origin(&output, "Edit Artwork…") + egui::vec2(8.0, 6.0);
        for pressed in [true, false] {
            frame(
                &ctx,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                |ctx| draw(ctx, &mut app),
            );
        }
        assert!(app.presentation_editor.editing());
        let before = app.recipe.clone();
        set_operation(&mut app, installing);
        let ctx = egui::Context::default();
        let output = frame(&ctx, vec![], |ctx| app.draw_artwork_editor(ctx));
        assert!(output.shapes.is_empty());
        assert!(app.presentation_editor.editing());
        assert_eq!(app.recipe, before);
        app.build_receiver = None;
        app.install_receiver = None;
        frame(&ctx, vec![], |ctx| app.draw_artwork_editor(ctx));
        let output = frame(&ctx, vec![], |ctx| app.draw_artwork_editor(ctx));
        assert!(text(&output).contains("Edit Release Watermark"));
        assert_eq!(app.recipe, before);
    }
}
