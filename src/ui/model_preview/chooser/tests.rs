use super::*;

#[test]
fn browsing_and_closing_never_apply_and_confirmation_returns_only_the_highlighted_choice() {
    let ctx = egui::Context::default();
    let mut inspected = 0;
    let mut applied = None;
    let mut confirm = egui::Rect::NOTHING;
    let mut frame = |opened, events| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    applied = show(
                        ui,
                        egui::Id::new("test-chooser"),
                        "Choose Appearance",
                        opened,
                        |ui, _| {
                            crate::ui::catalog::BrowserList {
                                keys: &[10, 20],
                                height: 400.0,
                                reset: false,
                                row_height: 48.0,
                                select: None,
                            }
                            .draw_with_actions(
                                ui,
                                |ui, index, selected| {
                                    ui.selectable_label(selected, format!("Choice {index}"))
                                },
                                |ui, index| {
                                    inspected = [10, 20][index];
                                    let response = ui.button("Apply Choice");
                                    confirm = response.rect;
                                    response.clicked().then_some(inspected)
                                },
                            )
                        },
                    );
                });
            },
        );
        (inspected, applied, confirm)
    };
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(true, vec![]);
    frame(false, vec![]);
    let (inspected, applied, _) = frame(false, vec![key(egui::Key::ArrowDown)]);
    assert_eq!(inspected, 20);
    assert_eq!(applied, None);
    assert_eq!(frame(false, vec![key(egui::Key::Escape)]).1, None);
    assert_eq!(frame(false, vec![]).1, None);
    frame(true, vec![]);
    let (_, _, button) = frame(false, vec![]);
    let pointer = button.center();
    let click = |pressed| egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(false, vec![egui::Event::PointerMoved(pointer), click(true)]);
    assert_eq!(frame(false, vec![click(false)]).1, Some(20));
    assert_eq!(
        frame(false, vec![]).1,
        None,
        "confirmation closes the chooser"
    );
}
