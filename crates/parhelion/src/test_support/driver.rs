//! Finds what a headless egui frame painted and builds the frames of a click, the pieces every
//! headless UI test shares, so a fix to how a target is found reaches all of them.

/// The two frames of a click at `at` as a pointer reports it: both carry the position, the
/// first with the press and the second with the release.
pub(crate) fn tap(at: egui::Pos2) -> [Vec<egui::Event>; 2] {
    button_frames(at, egui::PointerButton::Primary)
}

/// The two frames of a click of `button` at `at`.
pub(crate) fn button_frames(at: egui::Pos2, button: egui::PointerButton) -> [Vec<egui::Event>; 2] {
    [true, false].map(|pressed| {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    })
}

/// Every text a frame painted, clipped or not, with where it was drawn, in paint order.
pub(crate) fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    let mut found = Vec::new();
    for clipped in &output.shapes {
        collect(&clipped.shape, &mut found);
    }
    found
}

fn collect(shape: &egui::Shape, found: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => found.push((
            text.galley.job.text.clone(),
            text.galley.rect.translate(text.pos.to_vec2()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, found);
            }
        }
        _ => {}
    }
}

/// Where the first text reading exactly `name` was painted.
pub(crate) fn label(output: &egui::FullOutput, name: &str) -> Option<egui::Rect> {
    texts(output)
        .into_iter()
        .find(|(text, _)| text == name)
        .map(|(_, rect)| rect)
}

/// Where the accessibility tree places the node labelled `name`. It needs
/// `ctx.enable_accesskit()` before the frame.
pub(crate) fn accessible(output: &egui::FullOutput, name: &str) -> Option<egui::Rect> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .find_map(|(_, node)| {
            if node.label() != Some(name) {
                return None;
            }
            let bounds = node.bounds()?;
            Some(egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            ))
        })
}

/// Where the accessibility tree places the first node whose label starts with `prefix`, for a
/// control whose value depends on the data, such as "Animations: …".
pub(crate) fn accessible_starting(output: &egui::FullOutput, prefix: &str) -> Option<egui::Rect> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .find_map(|(_, node)| {
            if !node.label().is_some_and(|label| label.starts_with(prefix)) {
                return None;
            }
            let bounds = node.bounds()?;
            Some(egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            ))
        })
}
