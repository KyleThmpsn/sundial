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
    if !ui.fonts_mut(|fonts| fonts.families().contains(&family)) {
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
    if ui.fonts_mut(|fonts| fonts.families().contains(&family)) {
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
    let icon_font = if ui.fonts_mut(|fonts| fonts.families().contains(&family)) {
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
    if ui.fonts_mut(|fonts| fonts.families().contains(&family)) {
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

/// An image tile: an outlined, hoverable card with the thumbnail, the name, a detail at the
/// name's right and the source below. A changed image's name reads brighter.
pub(crate) struct ImageTile<'a> {
    pub width: f32,
    pub thumbnail_height: f32,
    pub label: &'a str,
    pub detail: Option<String>,
    pub source: &'a str,
    pub texture: Option<&'a egui::TextureHandle>,
    pub modified: bool,
    pub selected: bool,
}

pub(crate) fn image_tile(ui: &mut egui::Ui, tile: ImageTile<'_>) -> egui::Response {
    const PADDING: f32 = 8.0;
    let name_font = egui::FontId::proportional(13.0);
    let detail_font = egui::FontId::proportional(11.0);
    let (name_height, detail_height) =
        ui.fonts_mut(|fonts| (fonts.row_height(&name_font), fonts.row_height(&detail_font)));
    let height = 2.0 * PADDING + tile.thumbnail_height + 6.0 + name_height + 2.0 + detail_height;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(tile.width, height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let stroke = if tile.selected {
            visuals.selection.stroke
        } else if response.hovered() {
            visuals.widgets.hovered.bg_stroke
        } else {
            visuals.widgets.noninteractive.bg_stroke
        };
        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            4.0,
            visuals.faint_bg_color,
            stroke,
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(PADDING);
        let thumbnail =
            egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), tile.thumbnail_height));
        super::image_files::draw_contained(ui, thumbnail, tile.texture);
        let name = painter.text(
            egui::pos2(inner.left(), thumbnail.bottom() + 6.0),
            egui::Align2::LEFT_TOP,
            tile.label,
            name_font,
            if tile.modified {
                visuals.text_color()
            } else {
                secondary(visuals)
            },
        );
        if let Some(detail) = tile.detail {
            painter.text(
                egui::pos2(name.right() + 6.0, name.bottom()),
                egui::Align2::LEFT_BOTTOM,
                detail,
                detail_font.clone(),
                secondary(visuals),
            );
        }
        painter.text(
            egui::pos2(inner.left(), name.bottom() + 2.0),
            egui::Align2::LEFT_TOP,
            tile.source,
            detail_font,
            secondary(visuals),
        );
    }
    named_control(response, tile.label)
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
                // Buttons close the menu themselves, so a checkbox or a value inside it keeps it
                // open as it did before egui 0.32 made menus close on any click.
                let (response, inner) = egui::containers::menu::MenuButton::from_button(
                    egui::Button::new(icon).frame(false),
                )
                .config(
                    egui::containers::menu::MenuConfig::new()
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                )
                .ui(ui, contents);
                egui::InnerResponse::new(inner.map(|inner| inner.inner), response)
            },
        )
        .inner;
    focus_ring(ui, &menu.response);
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

/// A section heading, followed by the dot every edited tab and section carries once the section
/// holds a change. The dot is its own label, so the heading's text stays the section's name, and
/// a screen reader hears it as "Changed".
pub(crate) fn heading(ui: &mut egui::Ui, text: &str, changed: bool) -> egui::Response {
    let response = ui.heading(text);
    if changed {
        let dot = ui.add(egui::Label::new(egui::RichText::new("•").heading()).selectable(false));
        named_control(dot, "Changed").on_hover_text("Changed");
    }
    response
}

/// The quiet icon that restores what a card or a line holds. `name` says what it restores to a
/// screen reader, since a page can carry several.
pub(crate) fn reset_icon(ui: &mut egui::Ui, name: &str) -> bool {
    let button = ui.add(
        egui::Button::new(light_icon(
            ui,
            egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE,
        ))
        .frame(false)
        // A hit area larger than the glyph, which stays quiet.
        .min_size(egui::Vec2::splat(20.0)),
    );
    focus_ring(ui, &button);
    named_control(button, name)
        .on_hover_text("Restore the original value")
        .clicked()
}

/// A saved choice the current data no longer has: a warning and its label in the error colour,
/// the detail on hover, and Remove. Returns whether Remove was clicked.
pub(crate) fn missing(ui: &mut egui::Ui, label: &str, detail: &str) -> bool {
    ui.horizontal_wrapped(|ui| {
        let color = ui.visuals().error_fg_color;
        // Painted rather than added as a label, so a screen reader announces the label and not
        // the glyph's private-use character.
        // Coloured before layout. The dark theme's override text colour would otherwise be baked
        // into the galley and win over the colour painted with it.
        let glyph = egui::WidgetText::from(icon(ui, egui_phosphor::regular::WARNING).color(color))
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            );
        let (rect, _) = ui.allocate_exact_size(glyph.size(), egui::Sense::hover());
        ui.painter().galley(rect.min, glyph, color);
        let response = ui.colored_label(color, label);
        if !detail.is_empty() {
            response.on_hover_text(detail);
        }
        named_control(ui.small_button("Remove"), format!("Remove {label}")).clicked()
    })
    .inner
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

/// A muted orange for a state worth noticing that is not a problem, such as a value another
/// setting turns off. Quieter than a warning and still 4.5:1 or better on the card fills.
pub(crate) fn muted_warning(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        egui::Color32::from_rgb(201, 142, 78)
    } else {
        egui::Color32::from_rgb(150, 88, 24)
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

/// What a value's name line offers to restore.
#[derive(Clone, Copy)]
enum Restore<'a> {
    /// Nothing: the value is its original.
    Nothing,
    /// Reset, for a value whose original has no short reading.
    Reset,
    /// The original as its field reads it, which the button names and restores.
    To(&'a str),
}

/// Where a name line leaves its hint for the tile around it, which shows the hint once the tile's
/// control has keyboard focus.
const HINT: &str = "tile-hint";

/// A value's name, small, with what it does on hover: grey while the value is its original, and
/// white with a way back once it is not. Returns the name and whether the way back was clicked.
fn name_line(
    ui: &mut egui::Ui,
    width: f32,
    (label, hint): (&str, &str),
    restore: Restore<'_>,
    status: Option<(&str, &str)>,
) -> (egui::Response, bool) {
    if !hint.is_empty() {
        let id = ui.id().with(HINT);
        ui.data_mut(|data| data.insert_temp(id, hint.to_owned()));
    }
    ui.allocate_ui_with_layout(
        egui::vec2(width, TILE_NAME_HEIGHT),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            // One height with or without Reset, so a line of controls stays level.
            ui.set_min_height(TILE_NAME_HEIGHT);
            let reset = match restore {
                Restore::Nothing => false,
                // Small, so it stays inside the name's line instead of taking a control's height
                // and lowering the field.
                Restore::Reset => {
                    let response = ui
                        .add(
                            egui::Button::new(egui::RichText::new("Reset").size(11.0))
                                .frame(false)
                                .small(),
                        )
                        .on_hover_text("Restore the original value");
                    focus_ring(ui, &response);
                    named_control(response, format!("Reset {label}")).clicked()
                }
                Restore::To(original) => self::restore(ui, label, original),
            };
            // A state beside the name, at its size, so it adds no line and the field stays level
            // with its neighbours'.
            if let Some((text, hover)) = status {
                let text = egui::RichText::new(text)
                    .size(12.0)
                    .color(muted_warning(ui.visuals()));
                ui.add(egui::Label::new(text).selectable(false))
                    .on_hover_text(hover);
            }
            let name = ui
                .with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let text = egui::RichText::new(label).size(12.0);
                    let text = if matches!(restore, Restore::Nothing) {
                        text.color(secondary(ui.visuals()))
                    } else {
                        text
                    };
                    let hover = if hint.is_empty() {
                        label.to_owned()
                    } else {
                        format!("{label}\n{hint}")
                    };
                    let name = ui.add(cut_label(ui, text));
                    // The hint a pointer finds on hover, for a screen reader too.
                    if !hint.is_empty() {
                        ui.ctx()
                            .accesskit_node_builder(name.id, |node| node.set_description(hint));
                    }
                    name.on_hover_text(hover)
                })
                .inner;
            (name, reset)
        },
    )
    .inner
}

/// A quiet button that restores `label`'s value and names the original it restores, so the
/// original stays in view beside the edit, as Destiny keeps a stat's base beside its change.
/// Returns whether it was clicked.
pub(crate) fn restore(ui: &mut egui::Ui, label: &str, original: &str) -> bool {
    let response = ui
        .add(
            egui::Button::new(restore_text(ui, original))
                .frame(false)
                .small(),
        )
        .on_hover_text(format!("Restore {original}"));
    focus_ring(ui, &response);
    named_control(response, format!("Restore {label} to {original}")).clicked()
}

/// A frameless control's keyboard focus, drawn as the selection outline. egui paints no frame
/// around a frameless button, so without this its focus shows only as brighter text.
pub(crate) fn focus_ring(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(1.0),
            3.0,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Outside,
        );
    }
}

/// The restore icon and the original value, at the size of a name line's Reset. Colours are left
/// to the button, so it brightens on hover as Reset does.
fn restore_text(ui: &egui::Ui, original: &str) -> egui::text::LayoutJob {
    const SIZE: f32 = 11.0;
    let family = egui::FontFamily::Name(ICON_FONT_FAMILY.into());
    let icon_font = if ui.fonts_mut(|fonts| fonts.families().contains(&family)) {
        egui::FontId::new(SIZE, family)
    } else {
        egui::FontId::proportional(SIZE)
    };
    let format = |font_id| egui::TextFormat {
        font_id,
        color: egui::Color32::PLACEHOLDER,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(
        egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE,
        0.0,
        format(icon_font),
    );
    job.append(original, 3.0, format(egui::FontId::proportional(SIZE)));
    job
}

/// A field's name over a control its column lays out, in the tile style: grey until the value
/// differs from the base item's, then white with Reset beside it. Returns the name, to label the
/// control by, and whether Reset was clicked.
pub(crate) fn field_name(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    modified: bool,
) -> (egui::Response, bool) {
    let restore = if modified {
        Restore::Reset
    } else {
        Restore::Nothing
    };
    let width = ui.available_width();
    name_line(ui, width, (label, hint), restore, None)
}

/// A field's name whose way back names the base's value, as a stock tile's does: `original` is
/// that value as the field reads it, given once the field's value differs from it.
pub(crate) fn stock_field_name(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    original: Option<&str>,
) -> (egui::Response, bool) {
    let restore = original.map_or(Restore::Nothing, Restore::To);
    let width = ui.available_width();
    name_line(ui, width, (label, hint), restore, None)
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
    salt: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    hint: &str,
    modified: bool,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    let restore = if modified {
        Restore::Reset
    } else {
        Restore::Nothing
    };
    tile_with(ui, (width, salt), (label, hint), restore, None, control)
}

/// A tile whose way back names the original value: `original` is that value as the field reads
/// it, given once the value differs from it. An edited tile shows what it was without a hover.
pub(crate) fn stock_tile<R>(
    ui: &mut egui::Ui,
    (width, salt): (f32, impl std::hash::Hash + std::fmt::Debug),
    (label, hint): (&str, &str),
    original: Option<&str>,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    let restore = original.map_or(Restore::Nothing, Restore::To);
    tile_with(ui, (width, salt), (label, hint), restore, None, control)
}

/// A stock tile with a state beside its name, such as Inactive, with what it means on hover.
pub(crate) fn stock_tile_marked<R>(
    ui: &mut egui::Ui,
    (width, salt): (f32, impl std::hash::Hash + std::fmt::Debug),
    (label, hint): (&str, &str),
    original: Option<&str>,
    status: Option<(&str, &str)>,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    let restore = original.map_or(Restore::Nothing, Restore::To);
    tile_with(ui, (width, salt), (label, hint), restore, status, control)
}

/// The shortest length a timer's length field gives. Zero or less ends a timer at once, unless
/// its Unlimited flag is set.
pub(crate) const SHORTEST_LENGTH: f64 = 0.05;

/// A timer's length as its field reads it: Unlimited for one that never ends, else seconds.
pub(crate) fn length_text(seconds: f64, unlimited: bool) -> String {
    if unlimited {
        "Unlimited".to_owned()
    } else {
        format!(
            "{} s",
            egui::emath::format_with_decimals_in_range(seconds, 0..=2)
        )
    }
}

/// A typed timer length: Unlimited, No Limit or a negative number for one that never ends,
/// which reads as -1, else seconds with or without the unit.
pub(crate) fn parse_length(text: &str) -> Option<f64> {
    let text = text.trim().to_lowercase();
    if ["unl", "no", "inf"]
        .iter()
        .any(|word| text.starts_with(word))
    {
        return Some(-1.0);
    }
    text.trim_end_matches('s').trim().parse().ok()
}

fn tile_with<R>(
    ui: &mut egui::Ui,
    (width, salt): (f32, impl std::hash::Hash + std::fmt::Debug),
    name: (&str, &str),
    restore: Restore<'_>,
    status: Option<(&str, &str)>,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    tile_column(ui, (width, salt), |ui| {
        let (_, reset) = name_line(ui, width, name, restore, status);
        (control(ui), reset)
    })
}

/// A tile's room for a field that draws its own name with `field_name`, so a page's fields flow
/// as its tiles do. `salt` scopes the field's widgets, as `tile`'s does.
pub(crate) fn tile_column<R>(
    ui: &mut egui::Ui,
    (width, salt): (f32, impl std::hash::Hash + std::fmt::Debug),
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
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
            let (inner, scope) = ui.push_id(salt, |ui| (content(ui), ui.id())).inner;
            focus_hint(ui, scope);
            inner
        },
    )
    .inner
}

/// The hint a tile's name shows on hover, under its control while that control has keyboard
/// focus, so a keyboard reaches what a pointer does.
fn focus_hint(ui: &egui::Ui, scope: egui::Id) {
    let ctx = ui.ctx();
    if !keyboard_focus(ctx) {
        return;
    }
    let Some(focused) = ctx.memory(|memory| memory.focused()) else {
        return;
    };
    let Some(control) = ctx.read_response(focused) else {
        return;
    };
    if !ui.min_rect().contains_rect(control.rect) {
        return;
    }
    let Some(hint) = ctx.data(|data| data.get_temp::<String>(scope.with(HINT))) else {
        return;
    };
    egui::Tooltip::always_open(ctx.clone(), ui.layer_id(), focused.with(HINT), control.rect).show(
        |ui| {
            ui.label(hint);
        },
    );
}

/// Whether focus last moved by keyboard. Tab sets it and a pointer press clears it, so a field
/// clicked into shows no hint the pointer did not ask for.
fn keyboard_focus(ctx: &egui::Context) -> bool {
    let id = egui::Id::new("parhelion-keyboard-focus");
    let (tab, pressed) = ctx.input(|input| {
        (
            input.key_pressed(egui::Key::Tab),
            input.pointer.any_pressed(),
        )
    });
    ctx.data_mut(|data| {
        let keyboard = data.get_temp_mut_or(id, false);
        if tab {
            *keyboard = true;
        } else if pressed {
            *keyboard = false;
        }
        *keyboard
    })
}

/// A tile's field across its whole width with its value centered, as `add_sized` lays out the
/// fields every other tile holds, for a field a helper adds itself.
pub(crate) fn tile_field<R>(
    ui: &mut egui::Ui,
    width: f32,
    field: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    let layout = egui::Layout::centered_and_justified(egui::Direction::LeftToRight);
    ui.allocate_ui_with_layout(size, layout, field).inner
}

/// A stat's bar as Destiny's item tooltip draws one, across `rect`: filled to the value on a dim
/// track, `scale` filling it. A change from the base shows as its own segment, green where it adds
/// and red where it takes away.
pub(crate) fn stat_bar(ui: &egui::Ui, rect: egui::Rect, (base, value): (i32, i32), scale: i32) {
    const HEIGHT: f32 = 8.0;
    let bar = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), HEIGHT));
    let painter = ui.painter();
    let visuals = ui.visuals();
    painter.rect_filled(bar, 1.0, visuals.widgets.inactive.bg_fill);
    let x = |amount: i32| {
        bar.left() + bar.width() * (amount.max(0) as f32 / scale.max(1) as f32).min(1.0)
    };
    let fill = |from: f32, to: f32, color: egui::Color32| {
        if to > from {
            painter.rect_filled(
                egui::Rect::from_x_y_ranges(from..=to, bar.y_range()),
                1.0,
                color,
            );
        }
    };
    let (low, high) = (base.min(value), base.max(value));
    fill(bar.left(), x(low), visuals.text_color());
    let change = if value > base {
        success_color(visuals)
    } else {
        visuals.error_fg_color
    };
    fill(x(low), x(high), change);
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

/// Cards side by side that end at one height: each reaches the tallest card's height from the
/// frame before, so a line of cards has one bottom edge.
pub(crate) struct CardLine {
    id: egui::Id,
    height: f32,
    tallest: f32,
}

impl CardLine {
    pub(crate) fn new(ui: &egui::Ui, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        let id = ui.id().with(salt);
        let height = ui
            .ctx()
            .data(|data| data.get_temp::<f32>(id))
            .unwrap_or(0.0);
        Self {
            id,
            height,
            tallest: 0.0,
        }
    }

    /// One card of the line.
    pub(crate) fn card<R>(
        &mut self,
        ui: &mut egui::Ui,
        content: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        card(ui, |ui| {
            let inner = content(ui);
            let drawn = ui.min_rect();
            self.tallest = self.tallest.max(drawn.height());
            // Measured from the card's top. `set_min_height` would add the height below what is
            // already drawn.
            ui.expand_to_include_rect(egui::Rect::from_min_size(
                drawn.min,
                egui::vec2(0.0, self.height),
            ));
            inner
        })
    }

    /// Keeps this frame's tallest card for the next, after the line's last card.
    pub(crate) fn finish(self, ctx: &egui::Context) {
        if (self.tallest - self.height).abs() > 0.5 {
            ctx.data_mut(|data| data.insert_temp(self.id, self.tallest));
            ctx.request_repaint();
        }
    }
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

/// A cut label whose tooltip is the caller's alone.
pub(crate) use sundial::ui::cut_label;

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
            |ui| ui.add(egui::Button::selectable(selected, label)),
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
        ctx.global_style_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(18.0));
        });
        let before = egui::TextStyle::Small.resolve(&ctx.global_style());
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                perk_workbench_style(ui);
                assert_eq!(egui::TextStyle::Small.resolve(ui.style()).size, 18.0);
            });
        });
        assert_eq!(egui::TextStyle::Small.resolve(&ctx.global_style()), before);
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
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
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
