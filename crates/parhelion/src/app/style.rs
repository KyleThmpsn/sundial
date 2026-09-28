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

/// The icon every overflow menu opens from. A circled ellipsis reads as a button, where bare
/// dots read as a typed "…" and sit too close to the six-dot drag grip.
pub(crate) const MORE: &str = egui_phosphor::regular::DOTS_THREE_CIRCLE;

/// The family the host registers for Phosphor's light weight. It has the same codepoints as
/// the regular icons, so text that names an icon matches in either.
const ICON_LIGHT_FONT_FAMILY: &str = "Sundial Icons Light";
/// The family the host registers for Phosphor alone. The game's symbol fonts lead ordinary
/// text and share Phosphor's private-use range, so an icon whose codepoint a symbol also uses
/// (CARET_DOWN and CHECK) is drawn through this family, where nothing shadows it.
const ICON_FONT_FAMILY: &str = "Sundial Icons";

/// An icon drawn from Phosphor even where a game symbol shares its codepoint.
pub(crate) fn icon(ui: &egui::Ui, icon: &str) -> egui::RichText {
    let text = egui::RichText::new(icon);
    let family = egui::FontFamily::Name(ICON_FONT_FAMILY.into());
    if ui.fonts(|fonts| fonts.families().contains(&family)) {
        let size = egui::TextStyle::Body.resolve(ui.style()).size;
        text.font(egui::FontId::new(size, family))
    } else {
        text
    }
}

/// Text followed by an icon as one label, the icon drawn as `icon` draws it. The colors are
/// left to the widget, so a button's label still follows its hover and selected states.
pub(crate) fn text_with_icon(ui: &egui::Ui, text: &str, icon: &str) -> egui::text::LayoutJob {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let family = egui::FontFamily::Name(ICON_FONT_FAMILY.into());
    let icon_font = if ui.fonts(|fonts| fonts.families().contains(&family)) {
        egui::FontId::new(body.size, family)
    } else {
        body.clone()
    };
    let format = |font_id| egui::TextFormat {
        font_id,
        color: egui::Color32::PLACEHOLDER,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(text, 0.0, format(body));
    job.append(icon, 0.0, format(icon_font));
    job
}

/// An icon in the light weight when the host registered it, otherwise the regular one.
pub(crate) fn light_icon(ui: &egui::Ui, icon: &str) -> egui::RichText {
    let text = egui::RichText::new(icon);
    let family = egui::FontFamily::Name(ICON_LIGHT_FONT_FAMILY.into());
    if ui.fonts(|fonts| fonts.families().contains(&family)) {
        let size = egui::TextStyle::Body.resolve(ui.style()).size;
        text.font(egui::FontId::new(size, family))
    } else {
        text
    }
}

/// The overflow icon at the size it is drawn, large enough for its circle to read.
pub(crate) fn more_icon(ui: &egui::Ui) -> egui::RichText {
    light_icon(ui, MORE).size(16.0)
}

/// Controls drawn after this read quietly: a muted text colour without a fill, brightening
/// to the normal hover colour. For icon buttons and secondary commands beside the one a
/// line is about. Call it inside a scope.
pub(crate) fn quiet(ui: &mut egui::Ui) {
    let muted = if ui.visuals().dark_mode {
        egui::Color32::from_gray(165)
    } else {
        egui::Color32::from_gray(95)
    };
    let visuals = ui.visuals_mut();
    visuals.override_text_color = None;
    let inactive = &mut visuals.widgets.inactive;
    inactive.fg_stroke.color = muted;
    inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    inactive.bg_fill = egui::Color32::TRANSPARENT;
    inactive.bg_stroke = egui::Stroke::NONE;
}

/// An overflow menu, saying what it acts on.
///
/// A perk, one of its effects and one condition inside that effect each carry one of these,
/// and all three are the same glyph a few pixels apart. Naming the subject is what tells them
/// apart before the menu opens, on the hover and to a screen reader alike.
pub(crate) fn more_menu<R>(
    ui: &mut egui::Ui,
    subject: &str,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let name = if subject.is_empty() {
        "More Options".to_owned()
    } else {
        format!("More {subject} Options")
    };
    // The quiet colours need a scope, and a scope placed at the cursor never wraps, so after
    // a long condition title the menu ran past the line. An allocation of the button's own
    // size wraps with the line it sits on.
    let icon = more_icon(ui);
    let galley = egui::WidgetText::from(icon.clone()).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let size = egui::vec2(
        galley.size().x + ui.spacing().button_padding.x * 2.0,
        (galley.size().y + ui.spacing().button_padding.y * 2.0).max(ui.spacing().interact_size.y),
    );
    let mut menu = ui
        .allocate_ui_with_layout(
            size,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                quiet(ui);
                egui::menu::menu_custom_button(ui, egui::Button::new(icon).frame(false), contents)
            },
        )
        .inner;
    menu.response = named_control(menu.response, &name).on_hover_text(name.clone());
    menu
}

/// Reset for one value. It appears once the value differs from its original, so a form
/// nobody has touched carries no dead buttons.
pub(crate) fn reset(ui: &mut egui::Ui, modified: bool) -> bool {
    modified
        && ui
            .add(egui::Button::new("Reset").small())
            .on_hover_text("Restore the original value")
            .clicked()
}

/// The width `connector` takes for `word`, so a list can keep a gutter of that width.
pub(crate) fn connector_width(ui: &egui::Ui, word: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        word.to_owned(),
        egui::FontId::proportional(11.0),
        ui.visuals().text_color(),
    );
    galley.size().x + 10.0
}

/// A logic word between conditions, in the theme badge style so a list of alternatives and
/// requirements scans at a glance.
pub(crate) fn connector(ui: &mut egui::Ui, word: &str) -> egui::Response {
    badge(ui, word)
}

/// A few words in the theme badge style: a faint fill, a hairline border and slight corners.
pub(crate) fn badge(ui: &mut egui::Ui, word: &str) -> egui::Response {
    let visuals = ui.visuals();
    let (fill, stroke, color) = (
        visuals.faint_bg_color,
        visuals.widgets.noninteractive.bg_stroke,
        visuals.text_color(),
    );
    let galley =
        ui.painter()
            .layout_no_wrap(word.to_owned(), egui::FontId::proportional(11.0), color);
    let padding = egui::vec2(5.0, 1.0);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + 2.0 * padding, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect(
            rect,
            egui::CornerRadius::same(3),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.min + padding, galley, color);
    }
    response
}

/// Secondary text: a tile's name, a folded card's summary. egui's weak text fades halfway to
/// the widget fill, which reads at about 4.3:1 on a dark card and 2.4:1 on a light one. These
/// hold 5:1 or better on the card and block fills of both themes.
pub(crate) fn secondary(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        egui::Color32::from_gray(150)
    } else {
        egui::Color32::from_gray(105)
    }
}

/// The narrowest a value tile gets before its line holds one fewer.
const TILE_MIN_WIDTH: f32 = 150.0;
/// The widest a value tile gets. A wide pane keeps four readable tiles and its spare room,
/// rather than stretching a percentage across a quarter of the screen.
const TILE_MAX_WIDTH: f32 = 240.0;
/// The most tiles on one line, so each name stays near its value in a wide pane.
const TILES_PER_LINE: usize = 4;
/// Height of a tile's name line.
const TILE_NAME_HEIGHT: f32 = 18.0;

/// Values as tiles, flowing three or four to a line and fewer in a narrow pane. `content`
/// receives the width each tile takes and draws them with `tile`.
pub(crate) fn tiles<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui, f32) -> R) -> R {
    let layout = egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
    ui.with_layout(layout, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(12.0, 8.0);
        let gap = ui.spacing().item_spacing.x;
        let line = ui.available_width();
        let count = (((line + gap) / (TILE_MIN_WIDTH + gap)) as usize).clamp(1, TILES_PER_LINE);
        // Rounded down so the last tile of a line never wraps onto the next.
        let width = ((line - gap * (count - 1) as f32) / count as f32)
            .floor()
            .min(TILE_MAX_WIDTH);
        content(ui, width)
    })
    .inner
}

/// One value: its name, small, over the control. Reset sits beside the name once the value
/// changed, so the control keeps the whole width of its tile. Returns the control's result and
/// whether Reset was clicked.
///
/// `salt` scopes the tile's widgets. Wrapping a tile in `push_id` instead adds a scope the line
/// places without wrapping, so every tile after it would run off the line.
pub(crate) fn tile<R>(
    ui: &mut egui::Ui,
    width: f32,
    salt: impl std::hash::Hash,
    label: &str,
    hint: &str,
    modified: bool,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            // A control wider than its tile would paint over the next one.
            let clip = ui.clip_rect();
            ui.set_clip_rect(egui::Rect::from_x_y_ranges(
                ui.max_rect().x_range(),
                clip.y_range(),
            ));
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
            ui.push_id(salt, |ui| {
                let reset = ui
                    .allocate_ui_with_layout(
                        egui::vec2(width, TILE_NAME_HEIGHT),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            // One height with or without Reset, so a line of controls stays level.
                            ui.set_min_height(TILE_NAME_HEIGHT);
                            let reset = modified
                                && ui
                                    .add(
                                        egui::Button::new(egui::RichText::new("Reset").size(11.0))
                                            .frame(false),
                                    )
                                    .on_hover_text("Restore the original value")
                                    .clicked();
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let text = egui::RichText::new(label).size(12.0);
                                    let text = if modified {
                                        text
                                    } else {
                                        text.color(secondary(ui.visuals()))
                                    };
                                    let response = ui.add(egui::Label::new(text).truncate());
                                    let hover = if hint.is_empty() {
                                        label.to_owned()
                                    } else {
                                        format!("{label}\n{hint}")
                                    };
                                    response.on_hover_text(hover);
                                },
                            );
                            reset
                        },
                    )
                    .inner;
                (control(ui), reset)
            })
            .inner
        },
    )
    .inner
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
