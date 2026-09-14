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

/// Compact header controls retain the normal Parhelion font.
pub(crate) fn compact_controls(ui: &mut egui::Ui) {
    ui.spacing_mut().interact_size.y = 20.0;
    ui.spacing_mut().button_padding = egui::vec2(4.0, 1.0);
    ui.spacing_mut().item_spacing.x = 4.0;
}

/// The one action a page leads to, in the accent fill Build & Stage uses. Build it here and
/// add it with `ui.add` or `ui.add_enabled`.
pub(crate) fn primary(ui: &egui::Ui, label: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(label.to_owned()).strong())
        .fill(ui.visuals().selection.bg_fill)
}

pub(crate) fn more_menu<R>(
    ui: &mut egui::Ui,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let mut menu =
        egui::menu::menu_custom_button(ui, egui::Button::new("…").frame(false), contents);
    menu.response = named_control(menu.response, "More Options").on_hover_text("More Options");
    menu
}

/// A raised card for one effect or one block: a faint fill over the window and a rounded
/// outline, so a card reads as one thing and its controls sit inside it.
pub(crate) fn card<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(6)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            content(ui)
        })
        .inner
}

/// A quiet explanation under a heading or a control. Reads after the control, never
/// competes with it.
pub(crate) fn hint(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Label::new(egui::RichText::new(text).small().weak()).wrap())
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
