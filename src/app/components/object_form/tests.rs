use super::*;
use serde_json::json;

const FIELDS: &[Field] = &[
    Field {
        key: "package_name",
        label: "Package name",
        input: Input::Text,
        optional: false,
    },
    Field {
        key: "current_activity_from_launch",
        label: "Set current activity on launch",
        input: Input::Bool,
        optional: true,
    },
];

fn frame(
    context: &egui::Context,
    source: &Value,
    width: f32,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<Action>, egui::Rect) {
    let mut action = None;
    let mut rect = egui::Rect::NOTHING;
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 800.0),
            )),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                action = draw(ui, "form", source, FIELDS, true, |_| Ok(()));
                rect = ui.min_rect();
            });
        },
    );
    (output, action, rect)
}

fn click_label(output: &egui::FullOutput, label: &str) -> Vec<egui::Event> {
    let position = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
        .expect("button label was rendered");
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[test]
fn staged_form_fits_narrow_layouts_and_opening_it_is_lossless() {
    for width in [320.0, 420.0, 800.0] {
        let context = egui::Context::default();
        let source = json!({"package_name":"city_tower_social_d2","opaque":[1,2]});
        let (_, action, rect) = frame(&context, &source, width, Vec::new());
        assert!(action.is_none());
        assert!(
            rect.right() <= width,
            "form overflowed at {width}: {rect:?}"
        );
        assert_eq!(source["opaque"], json!([1, 2]));
    }
}

#[test]
fn apply_and_remove_buttons_return_explicit_actions() {
    let context = egui::Context::default();
    let source = json!({"package_name":"raid_gluttony_0","opaque":{"keep":true}});
    let (output, _, _) = frame(&context, &source, 600.0, Vec::new());
    let (_, action, _) = frame(&context, &source, 600.0, click_label(&output, "Apply"));
    match action {
        Some(Action::Apply(value)) => assert_eq!(value, source),
        _ => panic!("Apply must produce a value without altering the source"),
    }
    let (output, _, _) = frame(&context, &source, 600.0, Vec::new());
    let (_, action, _) = frame(&context, &source, 600.0, click_label(&output, "Remove"));
    assert!(matches!(action, Some(Action::Remove)));
}

#[test]
fn optional_field_edits_stay_staged_and_cancel_discards_them() {
    let context = egui::Context::default();
    let source = json!({"package_name":"x","opaque":[1,2]});
    let (output, _, _) = frame(&context, &source, 600.0, Vec::new());
    let (_, action, _) = frame(
        &context,
        &source,
        600.0,
        click_label(&output, "Set current activity on launch"),
    );
    assert!(action.is_none());
    assert!(source.get("current_activity_from_launch").is_none());
    let (output, _, _) = frame(&context, &source, 600.0, Vec::new());
    let (_, action, _) = frame(&context, &source, 600.0, click_label(&output, "Apply"));
    match action {
        Some(Action::Apply(value)) => {
            assert_eq!(value["current_activity_from_launch"], false);
            assert_eq!(value["opaque"], source["opaque"]);
        }
        _ => panic!("Apply should return the staged edit"),
    }
    let (output, _, _) = frame(&context, &source, 600.0, Vec::new());
    let (_, action, _) = frame(&context, &source, 600.0, click_label(&output, "Cancel"));
    assert!(action.is_none());
    let (output, _, _) = frame(&context, &source, 600.0, Vec::new());
    let (_, action, _) = frame(&context, &source, 600.0, click_label(&output, "Apply"));
    match action {
        Some(Action::Apply(value)) => assert_eq!(value, source),
        _ => panic!("Apply should return the original after Cancel"),
    }
}
