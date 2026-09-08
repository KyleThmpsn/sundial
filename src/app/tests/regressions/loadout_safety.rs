use super::*;

fn frame(app: &mut SundialApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| equipment::draw_randomize_dialogs(app, ctx, 0, None),
    )
}

fn text_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    fn find(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
        .unwrap_or_else(|| panic!("missing {label}"))
}

fn click(app: &mut SundialApp, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
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
}

#[test]
fn loadout_perk_safety_is_visible_and_selectable_with_warnings_hidden() {
    let directory = TestDirectory::new("loadout-perk-safety");
    let mut app = app(directory.0.clone());
    app.preferences.show_safety_warnings = false;
    app.preferences.really_unsafe_warning_acknowledged = false;
    app.plug_selection_mode = PlugSelectionMode::SocketAndGearType;
    let before = app.document.clone();
    let ctx = egui::Context::default();
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new(("equipment-randomize-loadout", 0_usize)),
            true,
        )
    });
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    text_position(&output, "Perk Safety");
    let selector = text_position(&output, "Socket + Gear Type");
    click(&mut app, &ctx, selector);
    let output = frame(&mut app, &ctx, vec![]);
    for mode in PlugSelectionMode::ALL {
        text_position(&output, mode.label());
    }
    click(&mut app, &ctx, text_position(&output, "Compatible"));
    assert_eq!(app.plug_selection_mode, PlugSelectionMode::Supported);
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, text_position(&output, "Compatible"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, text_position(&output, "All"));
    assert_eq!(app.plug_selection_mode, PlugSelectionMode::Supported);
    assert!(matches!(
        app.confirmation,
        Some(ConfirmationDialog::ReallyUnsafe)
    ));
    assert_eq!(app.document, before);
    assert!(!app.dirty);
    assert!(!app.settings_path.exists());
}
