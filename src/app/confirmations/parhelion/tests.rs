use super::*;

fn frame(
    ctx: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> (Option<bool>, egui::FullOutput) {
    let mut decision = None;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            decision = introduction(ctx);
        },
    );
    (decision, output)
}

fn text_center(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
            Some(text.pos + text.galley.rect.center().to_vec2())
        }
        egui::epaint::Shape::Vec(shapes) => {
            shapes.iter().find_map(|shape| text_center(shape, label))
        }
        _ => None,
    }
}

#[test]
fn introduction_buttons_stay_visible_and_dispatch_their_decisions() {
    for size in [egui::vec2(1100.0, 800.0), egui::vec2(640.0, 480.0)] {
        for (label, accepted) in [("I understand", true), ("Cancel", false)] {
            let ctx = egui::Context::default();
            for _ in 0..3 {
                assert_eq!(frame(&ctx, size, vec![]).0, None);
            }
            let (_, output) = frame(&ctx, size, vec![]);
            let pos = output
                .shapes
                .iter()
                .find_map(|shape| text_center(&shape.shape, label))
                .expect("action is rendered");
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains(pos));
            let events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            assert_eq!(frame(&ctx, size, events).0, Some(accepted));
        }
    }
}

#[test]
fn escape_dismisses_without_accepting() {
    let ctx = egui::Context::default();
    let size = egui::vec2(1100.0, 800.0);
    frame(&ctx, size, vec![]);
    let event = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    assert_eq!(frame(&ctx, size, vec![event]).0, Some(false));
}
