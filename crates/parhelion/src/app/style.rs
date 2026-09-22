//! Local workbench density and contrast; never changes Sundial's global theme.

/// Name controls whose visible hint/icon is not an accessibility label.
/// The family the host registers for the game's own symbols.
const DESTINY_TEXT_FONT_FAMILY: &str = "Sundial Destiny text";

/// Text drawn in a family that has the game's symbol glyphs.
///
/// Perk descriptions carry the Champion marks as private-use characters, `U+E070` and its
/// neighbours. The default family has no glyph for those, so a tooltip quoting a perk's own words
/// drew an empty box where the symbol belongs. Eriana's Vow says it fires shield-piercing rounds
/// that way. The family is registered by the host, so this falls back to ordinary text when
/// Parhelion is drawn without it.
pub(crate) fn destiny_text(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    let text = egui::RichText::new(text.into());
    let family = egui::FontFamily::Name(DESTINY_TEXT_FONT_FAMILY.into());
    if !ui.fonts(|fonts| fonts.families().contains(&family)) {
        return text;
    }
    let mut font_id = egui::TextStyle::Body.resolve(ui.style());
    font_id.family = family;
    text.font(font_id)
}

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
    // Rows carry one line of text, so the control only needs room for that line and a
    // little around it. The earlier 24 high button with 3 of vertical padding added a
    // visible band of nothing to every row, and a dense page stacks dozens of them.
    style.spacing.interact_size.y = 20.0;
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    if style.visuals.dark_mode {
        style.visuals.override_text_color = Some(egui::Color32::from_gray(240));
        style.visuals.error_fg_color = egui::Color32::from_rgb(255, 128, 128);
        style.visuals.warn_fg_color = egui::Color32::from_rgb(255, 180, 84);
    } else {
        style.visuals.warn_fg_color = egui::Color32::from_rgb(143, 74, 0);
        style.visuals.error_fg_color = egui::Color32::from_rgb(175, 0, 0);
    }
}

/// Perk descriptions, property hints and source rows are working information.
/// Keep them at the reader's body size, including in independently opened dialogs.
pub(crate) fn perk_workbench_style(ui: &mut egui::Ui) {
    workbench_style(ui);
    let body = egui::TextStyle::Body.resolve(ui.style());
    ui.style_mut()
        .text_styles
        .insert(egui::TextStyle::Small, body);
}

/// Compact header controls retain the normal Parhelion font.
pub(crate) fn compact_controls(ui: &mut egui::Ui) {
    ui.spacing_mut().interact_size.y = 20.0;
    ui.spacing_mut().button_padding = egui::vec2(4.0, 1.0);
    ui.spacing_mut().item_spacing.x = 4.0;
}

/// A block inside a card. The card owns the only outline on the page, so a block set off by
/// a faint fill reads as part of it rather than as another card of equal weight.
pub(crate) fn block(style: &egui::Style) -> egui::Frame {
    egui::Frame::new()
        .fill(style.visuals.faint_bg_color)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .corner_radius(4)
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

/// Paints the checkerboard that makes transparent artwork readable behind a preview.
///
/// Artwork that will be composited in game, over a rarity plate or as a silhouette, has to show
/// where it is clear. Against a flat panel a transparent pixel and a dark opaque one look alike.
pub(crate) fn transparency_backdrop(ui: &egui::Ui, rect: egui::Rect) {
    const CHECK: f32 = 8.0;
    let painter = ui.painter().with_clip_rect(rect);
    // Both checks have to read against the panel, so pair the darkest fill with the inactive
    // widget fill rather than the faint row tint, which is nearly the same color.
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    let light = ui.visuals().widgets.inactive.bg_fill;
    let columns = (rect.width() / CHECK).ceil() as usize;
    let rows = (rect.height() / CHECK).ceil() as usize;
    for row in 0..rows {
        for column in 0..columns {
            if (row + column) % 2 == 0 {
                continue;
            }
            let min = rect.min + egui::vec2(column as f32 * CHECK, row as f32 * CHECK);
            painter.rect_filled(
                egui::Rect::from_min_size(min, egui::Vec2::splat(CHECK)).intersect(rect),
                0.0,
                light,
            );
        }
    }
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

    #[test]
    fn perk_text_follows_body_size_without_changing_the_global_theme() {
        let ctx = egui::Context::default();
        ctx.style_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(18.0));
        });
        let before = egui::TextStyle::Small.resolve(&ctx.style());
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                perk_workbench_style(ui);
                assert_eq!(egui::TextStyle::Small.resolve(ui.style()).size, 18.0);
            });
        });
        assert_eq!(egui::TextStyle::Small.resolve(&ctx.style()), before);
    }

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
