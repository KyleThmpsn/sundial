//! Local workbench density and contrast; never changes Sundial's global theme.

/// Name controls whose visible hint/icon is not an accessibility label.
pub(super) fn named_control(response: egui::Response, name: impl Into<String>) -> egui::Response {
    let name: String = name.into();
    response
        .ctx
        .accesskit_node_builder(response.id, |node| node.set_label(name));
    response
}

pub(super) fn success_color(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        egui::Color32::from_rgb(126, 215, 133)
    } else {
        egui::Color32::from_rgb(25, 105, 40)
    }
}

pub(crate) fn workbench_style(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    style.spacing.interact_size.y = 24.0;
    style.spacing.button_padding = egui::vec2(7.0, 3.0);
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
    if style.visuals.dark_mode {
        style.visuals.override_text_color = Some(egui::Color32::from_gray(240));
        style.visuals.error_fg_color = egui::Color32::from_rgb(255, 128, 128);
        style.visuals.warn_fg_color = egui::Color32::from_rgb(255, 180, 84);
    } else {
        style.visuals.warn_fg_color = egui::Color32::from_rgb(143, 74, 0);
        style.visuals.error_fg_color = egui::Color32::from_rgb(175, 0, 0);
    }
}

/// Keep the perk workbench close to the surrounding editor's text scale.
pub(crate) fn perk_style(ui: &mut egui::Ui) {
    workbench_style(ui);
    let style = ui.style_mut();
    for (text, minimum) in [
        (egui::TextStyle::Body, 14.0),
        (egui::TextStyle::Button, 14.0),
        (egui::TextStyle::Small, 12.0),
        (egui::TextStyle::Monospace, 12.0),
        (egui::TextStyle::Heading, 18.0),
    ] {
        if let Some(font) = style.text_styles.get_mut(&text) {
            font.size = font.size.max(minimum);
        }
    }
    style.spacing.interact_size.y = style.spacing.interact_size.y.max(26.0);
    style.spacing.button_padding = egui::vec2(7.0, 3.0);
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
}

/// A virtualized row must allocate exactly the height passed to `show_rows`.
pub(crate) fn list_row_height(ui: &egui::Ui) -> f32 {
    ui.spacing()
        .interact_size
        .y
        .max(ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y)
}

pub(crate) fn list_row(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    ui.scope(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        let height = list_row_height(ui);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), height),
            egui::Layout::left_to_right(egui::Align::Center)
                .with_main_align(egui::Align::Min)
                .with_main_justify(true),
            |ui| ui.add(egui::SelectableLabel::new(selected, label)),
        )
        .inner
    })
    .inner
    .on_hover_text(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: egui::Color32) -> f32 {
        let channels = color.to_array()[..3]
            .iter()
            .map(|value| {
                let value = f32::from(*value) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            })
            .collect::<Vec<_>>();
        channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
    }

    #[test]
    fn status_text_has_normal_text_contrast_in_both_themes() {
        for visuals in [egui::Visuals::light(), egui::Visuals::dark()] {
            let ctx = egui::Context::default();
            ctx.set_visuals(visuals);
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench_style(ui);
                    let visuals = ui.visuals();
                    for foreground in [
                        success_color(visuals),
                        visuals.warn_fg_color,
                        visuals.error_fg_color,
                    ] {
                        for background in [
                            visuals.panel_fill,
                            visuals.window_fill(),
                            visuals.extreme_bg_color,
                        ] {
                            let foreground = luminance(foreground);
                            let background = luminance(background);
                            let contrast = (foreground.max(background) + 0.05)
                                / (foreground.min(background) + 0.05);
                            assert!(
                                contrast >= 4.5,
                                "status contrast {contrast}, dark={}",
                                visuals.dark_mode
                            );
                        }
                    }
                });
            });
        }
    }
}
