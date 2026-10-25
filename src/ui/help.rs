//! Shared compact help for Sundial and the weapon workbench.

use eframe::egui;

pub(crate) fn tooltip_title_style() -> egui::TextStyle {
    egui::TextStyle::Name("Tooltip Title".into())
}

/// Use the installed medium-weight face with its symbol fallbacks, at the caller's size.
pub(crate) fn emphasized_font(ui: &egui::Ui, size: f32) -> egui::FontId {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mut font = ui
        .style()
        .text_styles
        .get(&tooltip_title_style())
        // Font definitions take effect on the next egui pass, while styles
        // change immediately. Use the body font during that transition.
        .filter(|font| ui.fonts_mut(|fonts| fonts.families().contains(&font.family)))
        .cloned()
        .unwrap_or_else(|| body.clone());
    font.size = size;
    font
}

pub(crate) fn emphasized_text(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .font(emphasized_font(
            ui,
            egui::TextStyle::Body.resolve(ui.style()).size,
        ))
        .strong()
}

/// Shared tooltip title typography for Sundial and Parhelion.
pub fn tooltip_title(ui: &mut egui::Ui, title: impl Into<String>) -> egui::Response {
    let size = egui::TextStyle::Body.resolve(ui.style()).size + 2.0;
    ui.label(emphasized_text(ui, title).size(size))
}

/// Hover for a tooltip, or click/keyboard-activate to keep the help open.
pub fn info(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    compact_help(
        ui,
        egui::RichText::new(egui_phosphor::regular::INFO),
        "More information",
        text.into(),
        None,
    )
}

/// Information with a source link and optional expandable license text.
pub fn info_link(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    label: &str,
    url: &str,
    license: Option<&str>,
) -> egui::Response {
    compact_help(
        ui,
        egui::RichText::new(egui_phosphor::regular::INFO),
        "More information",
        text.into(),
        Some((label, url, license)),
    )
}

/// Compact warning whose explanation stays available by hover, click or keyboard activation.
pub(crate) fn warning(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    compact_help(
        ui,
        egui::RichText::new(egui_phosphor::regular::WARNING).color(ui.visuals().warn_fg_color),
        "Warning details",
        text.into(),
        None,
    )
}

fn compact_help(
    ui: &mut egui::Ui,
    glyph: egui::RichText,
    accessible_label: &'static str,
    text: egui::WidgetText,
    link: Option<(&str, &str, Option<&str>)>,
) -> egui::Response {
    let contents = |ui: &mut egui::Ui| {
        ui.set_max_width(360.0);
        ui.label(text.clone());
        if let Some((label, url, license)) = link {
            ui.hyperlink_to(label, url);
            if let Some(license) = license {
                ui.collapsing("License", |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            ui.label(license);
                        });
                });
            }
        }
    };
    let (response, _) =
        egui::containers::menu::MenuButton::from_button(egui::Button::new(glyph).frame(false))
            .ui(ui, contents);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, accessible_label)
    });
    response
        .on_hover_cursor(egui::CursorIcon::Help)
        .on_hover_ui(contents)
}
