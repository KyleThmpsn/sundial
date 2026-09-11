//! Shared compact help for Sundial and the weapon workbench.

use eframe::egui;

pub(crate) fn tooltip_title_style() -> egui::TextStyle {
    egui::TextStyle::Name("Tooltip Title".into())
}

pub(crate) fn emphasized_text(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mut font = ui
        .style()
        .text_styles
        .get(&tooltip_title_style())
        // Font definitions take effect on the next egui pass, while styles
        // change immediately. Use the body font during that transition.
        .filter(|font| ui.fonts(|fonts| fonts.families().contains(&font.family)))
        .cloned()
        .unwrap_or_else(|| body.clone());
    font.size = body.size;
    egui::RichText::new(text).font(font).strong()
}

pub(crate) fn tooltip_title(ui: &mut egui::Ui, title: impl Into<String>) -> egui::Response {
    let size = egui::TextStyle::Body.resolve(ui.style()).size + 2.0;
    ui.label(emphasized_text(ui, title).size(size))
}

/// Hover for a tooltip, or click/keyboard-activate to keep the help open.
pub(crate) fn info(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    let text = text.into();
    let response = egui::menu::menu_custom_button(
        ui,
        egui::Button::new(egui_phosphor::regular::INFO).frame(false),
        |ui| {
            ui.set_max_width(360.0);
            ui.label(text.clone());
        },
    )
    .response;
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More information")
    });
    response
        .on_hover_cursor(egui::CursorIcon::Help)
        .on_hover_text(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_opens_from_keyboard_focus() {
        let ctx = egui::Context::default();
        let help = "Help remains available without a mouse.";
        let mut found = false;
        for frame in 0..3 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                ..Default::default()
            };
            if frame == 1 {
                input.events.push(egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = info(ui, help);
                    if frame == 0 {
                        response.request_focus();
                    }
                });
            });
            found |= output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == help)
            });
        }
        assert!(found, "Keyboard activation should render the help text");
    }
}
