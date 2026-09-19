use super::*;
use serde_json::json;

const FIELDS: &[Field] = &[
    Field {
        key: "package_name",
        label: "Package Name",
        input: Input::Text,
        optional: false,
    },
    Field {
        key: "current_activity_from_launch",
        label: "Set Current Activity on Launch",
        input: Input::Bool,
        optional: true,
    },
];

fn frame(
    context: &egui::Context,
    source: &Value,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<Action>) {
    let mut action = None;
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                action = draw(ui, "form", source, FIELDS, true, |_| Ok(()));
            });
        },
    );
    (output, action)
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
fn apply_and_remove_buttons_return_explicit_actions() {
    let context = egui::Context::default();
    let source = json!({"package_name":"raid_gluttony_0","opaque":{"keep":true}});
    let (output, _) = frame(&context, &source, Vec::new());
    let (_, action) = frame(&context, &source, click_label(&output, "Apply"));
    match action {
        Some(Action::Apply(value)) => assert_eq!(value, source),
        _ => panic!("Apply must produce a value without altering the source"),
    }
    let (output, _) = frame(&context, &source, Vec::new());
    let (_, action) = frame(&context, &source, click_label(&output, "Remove"));
    assert!(matches!(action, Some(Action::Remove)));
}

#[test]
fn optional_field_edits_stay_staged_and_cancel_discards_them() {
    let context = egui::Context::default();
    let source = json!({"package_name":"x","opaque":[1,2]});
    let (output, _) = frame(&context, &source, Vec::new());
    let (_, action) = frame(
        &context,
        &source,
        click_label(&output, "Set Current Activity on Launch"),
    );
    assert!(action.is_none());
    assert!(source.get("current_activity_from_launch").is_none());
    let (output, _) = frame(&context, &source, Vec::new());
    let (_, action) = frame(&context, &source, click_label(&output, "Apply"));
    match action {
        Some(Action::Apply(value)) => {
            assert_eq!(value["current_activity_from_launch"], false);
            assert_eq!(value["opaque"], source["opaque"]);
        }
        _ => panic!("Apply should return the staged edit"),
    }
    let (output, _) = frame(&context, &source, Vec::new());
    let (_, action) = frame(&context, &source, click_label(&output, "Cancel"));
    assert!(action.is_none());
    let (output, _) = frame(&context, &source, Vec::new());
    let (_, action) = frame(&context, &source, click_label(&output, "Apply"));
    match action {
        Some(Action::Apply(value)) => assert_eq!(value, source),
        _ => panic!("Apply should return the original after Cancel"),
    }
}
