//! How the game draws an item: rarity bands in its item header colours, names in capitals in the
//! medium cut, and the faces, weights and measurements Dawn's Loadout Studio sets them with.
//! Dawn authors every length in pixels against 16 px body text and scales the whole UI with the
//! game's viewport, 1.5 times at 1440p. [`Scale`] sets Dawn's body text at this UI's body size
//! and every authored length with it, so the proportions are Dawn's.

pub(super) mod tooltip;

use std::sync::Arc;

use eframe::egui;

use crate::catalog::{Catalog, ItemRarity};

use super::ui::{GameFace, game_font};

/// The game's item header colours.
const COMMON: egui::Color32 = egui::Color32::from_rgb(195, 188, 180);
const UNCOMMON: egui::Color32 = egui::Color32::from_rgb(54, 111, 66);
const RARE: egui::Color32 = egui::Color32::from_rgb(80, 118, 163);
const LEGENDARY: egui::Color32 = egui::Color32::from_rgb(82, 47, 101);
const EXOTIC: egui::Color32 = egui::Color32::from_rgb(206, 174, 51);
const DARK_BAND_TEXT: egui::Color32 = egui::Color32::from_rgb(20, 20, 23);
/// Muted band text keeps 0.72 of the text colour's opacity, as Dawn's does.
const MUTED_BAND_OPACITY: u8 = 184;
/// The icon backdrop is the band at this weight, which keeps art legible on it.
const BACKDROP_WEIGHT: f32 = 0.17;
/// One extra strike per this many pixels of weight, which keeps a stroke solid with no blur.
const BOLD_STRIKE_STEP: f32 = 0.6;
/// Dawn's body text size.
const AUTHORED_BODY: f32 = 16.0;
/// Dawn clips a line this far short of its column, so an ellipsis keeps clear of the edge.
const CLIP_SLACK: f32 = 10.0;

/// Dawn's sizes at this UI's scale: its body text, and its authored lengths.
#[derive(Clone, Copy, Debug)]
pub(super) struct Scale {
    /// Body text as a Dear ImGui size, which [`super::ui::game_font`] converts to an em size.
    pub(super) body: f32,
}

impl Scale {
    /// Dawn's body text at the UI's own body size.
    pub(super) fn of(ui: &egui::Ui) -> Self {
        let body = egui::TextStyle::Body.resolve(ui.style()).size;
        Self {
            body: body / super::ui::game_face_em(ui.ctx(), GameFace::Text),
        }
    }

    /// An authored pixel length.
    pub(super) fn px(self, authored: f32) -> f32 {
        authored * self.body / AUTHORED_BODY
    }
}

/// The height of a line of `font` as Dear ImGui measures it, which Dawn lays out by: the face's
/// ascent less its descent. egui adds the face's line gap, which the game faces leave at zero.
pub(super) fn line_height(ui: &egui::Ui, font: &egui::FontId) -> f32 {
    ui.fonts_mut(|fonts| fonts.row_height(font))
}

/// The band an item's rarity gives it. An unclassified item takes Common's, as in game.
pub(super) const fn band_color(rarity: ItemRarity) -> egui::Color32 {
    match rarity {
        ItemRarity::Unknown | ItemRarity::Common => COMMON,
        ItemRarity::Uncommon => UNCOMMON,
        ItemRarity::Rare => RARE,
        ItemRarity::Legendary => LEGENDARY,
        ItemRarity::Exotic => EXOTIC,
    }
}

/// The text colour on a band, or its muted form for the line under the name. The light bands,
/// Common and Exotic, take dark text.
pub(super) fn band_text(rarity: ItemRarity, muted: bool) -> egui::Color32 {
    let text = match rarity {
        ItemRarity::Unknown | ItemRarity::Common | ItemRarity::Exotic => DARK_BAND_TEXT,
        _ => egui::Color32::WHITE,
    };
    if !muted {
        return text;
    }
    let [red, green, blue, _] = text.to_array();
    egui::Color32::from_rgba_unmultiplied(red, green, blue, MUTED_BAND_OPACITY)
}

/// The band darkened to its backdrop weight, opaque.
fn backdrop(band: egui::Color32) -> egui::Color32 {
    let [red, green, blue] =
        [band.r(), band.g(), band.b()].map(|channel| (f32::from(channel) * BACKDROP_WEIGHT) as u8);
    egui::Color32::from_rgb(red, green, blue)
}

/// The item's icon on a backdrop of its band, square in `rect`.
pub(super) fn icon(
    ui: &egui::Ui,
    catalog: &Catalog,
    hash: u64,
    rarity: ItemRarity,
    rect: egui::Rect,
) {
    ui.painter()
        .rect_filled(rect, 0.0, backdrop(band_color(rarity)));
    if let Some(texture) = catalog.icon_texture(ui.ctx(), hash) {
        egui::Image::new(&texture).paint_at(ui, rect);
    }
}

/// A name as the game sets it: every letter in capitals, accented ones too, so Jötunn reads
/// JÖTUNN. Dawn folds only ASCII because a byte-wise fold is unsafe on UTF-8, which a Rust string
/// never risks.
pub(super) fn shout(text: &str) -> String {
    text.to_uppercase()
}

/// One line of text cut with an ellipsis to `width`, less Dawn's clipping slack.
pub(super) fn single_line(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> Arc<egui::Galley> {
    tracked_line(ui, text, (font, color), width, 0.0)
}

/// [`single_line`] with `tracking` pixels added after each letter.
pub(super) fn tracked_line(
    ui: &egui::Ui,
    text: &str,
    (font, color): (egui::FontId, egui::Color32),
    width: f32,
    tracking: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    if let Some(section) = job.sections.first_mut() {
        section.format.extra_letter_spacing = tracking;
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width((width - CLIP_SLACK).max(0.0));
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// Text wrapped to `width`.
pub(super) fn wrapped(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> Arc<egui::Galley> {
    ui.fonts_mut(|fonts| fonts.layout(text.to_owned(), font, color, width.max(0.0)))
}

/// Capitals set with the game's letter spacing.
pub(super) fn spaced_capitals(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    tracking: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &shout(text),
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: tracking,
            ..Default::default()
        },
    );
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// The extra weight a heavier game face is struck with: none when the install has the face,
/// else `weight`, since the text falls back to a lighter face. Dawn's `push_title` and
/// `push_figure` answer the same way.
pub(super) fn strike(ui: &egui::Ui, face: GameFace, weight: f32) -> f32 {
    if super::ui::game_face_installed(ui.ctx(), face) {
        0.0
    } else {
        weight
    }
}

/// Paints a galley struck `weight` pixels heavier. The strikes ring the glyph so it thickens
/// evenly and its own centre stays put.
pub(super) fn bold(
    painter: &egui::Painter,
    at: egui::Pos2,
    galley: &Arc<egui::Galley>,
    color: egui::Color32,
    weight: f32,
) {
    if weight > 0.0 {
        let radius = weight * 0.5;
        let centre = at + egui::vec2(radius, 0.0);
        let strikes = ((radius / BOLD_STRIKE_STEP) as usize).max(1);
        for ring in 1..=strikes {
            let offset = radius * ring as f32 / strikes as f32;
            for shift in [
                egui::vec2(-offset, 0.0),
                egui::vec2(offset, 0.0),
                egui::vec2(0.0, -offset),
                egui::vec2(0.0, offset),
            ] {
                painter.galley(centre + shift, Arc::clone(galley), color);
            }
        }
        painter.galley(centre, Arc::clone(galley), color);
    } else {
        painter.galley(at, Arc::clone(galley), color);
    }
}

/// Where a capital's ink sits in a line of `font`: the distance from the top of the line to the
/// top of the ink, and the ink's height. A line reserves room no capital reaches, so laying out
/// on the line rather than the ink leaves text riding high.
pub(super) fn cap_band(ui: &egui::Ui, font: egui::FontId) -> (f32, f32) {
    ink_band(ui, font, 'H')
}

/// Where one character's ink sits in a line of `font`, as [`cap_band`] measures a capital. A
/// symbol is artwork with no baseline of its own, so it is laid out on its ink.
pub(super) fn ink_band(ui: &egui::Ui, font: egui::FontId, glyph: char) -> (f32, f32) {
    let galley =
        ui.fonts_mut(|fonts| fonts.layout_no_wrap(glyph.to_string(), font, egui::Color32::WHITE));
    galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first())
        .map_or((0.0, galley.size().y), |glyph| {
            (glyph.pos.y + glyph.uv_rect.offset.y, glyph.uv_rect.size.y)
        })
}

/// The face and size a card's name is set in.
pub(super) fn title_font(ui: &egui::Ui, size: f32) -> egui::FontId {
    game_font(ui, GameFace::Title, size)
}

/// The face and size body text on a card or tooltip is set in.
pub(super) fn text_font(ui: &egui::Ui, size: f32) -> egui::FontId {
    game_font(ui, GameFace::Text, size)
}

/// The face and size a large figure is set in.
pub(super) fn figure_font(ui: &egui::Ui, size: f32) -> egui::FontId {
    game_font(ui, GameFace::Figure, size)
}

/// The game's symbol face at `size`, or `None` when the install's symbol face is not loaded, so
/// a glyph never draws as whatever icon shares its codepoint.
pub(super) fn symbol_font(ui: &egui::Ui, size: f32) -> Option<egui::FontId> {
    super::ui::game_face_installed(ui.ctx(), GameFace::Symbol)
        .then(|| game_font(ui, GameFace::Symbol, size))
}
