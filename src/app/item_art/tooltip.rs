//! The game's own item tooltip, as Dawn's Loadout Studio draws it: a rarity band carrying the
//! icon, the name and the type, the description, the leading figure, the stat bars, then one row
//! per perk. A catalog item is shown with the plugs it is granted with.

use std::{collections::BTreeMap, sync::Arc};

use eframe::egui;

use crate::{
    catalog::{
        Catalog, ItemDamageType, ItemRarity, ItemWeaponAmmoType,
        interpolate_investment_stat_display, item_type_label,
    },
    hash::parse_hash_hex,
};

use super::{Scale, band_color, band_text, bold, cap_band, shout, single_line, wrapped};
use crate::app::ui::GameFace;

/// The tooltip is a fixed column, as the game's is, so stats and perks line up down it. It runs
/// a little wider than Dawn's 330, so fewer names wrap.
const WIDTH: f32 = 360.0;
/// Padding inside the body. The band ignores it, so the icon sits flush in the frame.
const PADDING: f32 = 8.0;
/// Height of the rarity band, which is also the edge of the square icon struck into it.
const BAND_HEIGHT: f32 = 52.0;
/// Name and type sizes against body text, and the extra weight the name is struck with.
const NAME_SCALE: f32 = 2.05;
const TYPE_SCALE: f32 = 1.35;
const NAME_WEIGHT: f32 = 1.15;
/// Gap between the name's capitals and the type's.
const BAND_TYPE_GAP: f32 = 7.0;
/// The leading figure: its size and weight, the room its box keeps above the digits, and the
/// lines its row is set on, each as a share of the figure's box.
const FIGURE_SCALE: f32 = 2.60;
const FIGURE_WEIGHT: f32 = 2.0;
const FIGURE_TOP_TRIM: f32 = 7.0;
const FIGURE_RULE_GAP: f32 = 7.0;
const FIGURE_MIDDLE: f32 = 0.50;
const TEXT_ANCHOR: f32 = 0.576;
const FIGURE_RULE_HEIGHT: f32 = 0.58;
const FIGURE_ROW_HEIGHT: f32 = 0.74;
/// Gap after the damage type glyph that leads the figure.
const FIGURE_GLYPH_GAP: f32 = 1.5;
/// The symbol face fills its em where a digit does not, so a glyph is set smaller to match.
const ELEMENT_GLYPH_SCALE: f32 = 0.50;
/// The energy lockup: its damage type glyph, then its capacity, bold in the type's own colour.
/// It is the figure drawn small, so the glyph takes the same share of the figure beside it.
const ENERGY_VALUE_SCALE: f32 = 1.31;
const ENERGY_VALUE_WEIGHT: f32 = 0.9;
const ENERGY_GLYPH_GAP: f32 = 0.0;
/// The ammunition mark, which already carries its class colour. Its artwork keeps a margin, deeper
/// under the mark than over it, so it needs almost no gap before the word and drops a little to
/// sit level with the row.
const AMMO_MARK_EXTENT: f32 = 30.0;
const AMMO_WORD_GAP: f32 = 1.0;
const AMMO_MARK_DROP: f32 = 0.06;
/// The stats a weapon's and armor's figures are named for, Power and Defense.
const POWER_STAT_HASH: u64 = 0x735C_F023;
const DEFENSE_STAT_HASH: u64 = 0xE854_FA8E;
/// Stat rows: a right-aligned name column, then the bar, then the value.
const STAT_NAME_WIDTH: f32 = 124.0;
const STAT_VALUE_WIDTH: f32 = 34.0;
const STAT_BAR_HEIGHT: f32 = 10.0;
const STAT_COLUMN_GAP: f32 = 8.0;
const STAT_ROW_GAP: f32 = -2.0;
const STAT_BLOCK_PADDING_SCALE: f32 = 0.7;
const DEFAULT_STAT_CEILING: i32 = 100;
const TOTAL_LABEL: &str = "Total";
/// Perk rows: a round badge holding the icon, then the name, with a rule between rows.
const PERK_ICON: f32 = 22.0;
const PERK_BADGE_PADDING: f32 = 3.0;
const PERK_ROW_HEIGHT: f32 = 30.0;
/// Gap between the blocks of the body.
const BLOCK_GAP: f32 = 4.0;
/// How far the tooltip sits from the pointer.
const POINTER_OFFSET: f32 = 16.0;
/// Corner radius of the frame and the band's top corners.
const ROUNDING: u8 = 2;
/// The game's near-black tooltip with white-on-black rules, bars and badges.
const BODY: egui::Color32 = egui::Color32::from_rgba_premultiplied(22, 22, 25, 247);
const TEXT: egui::Color32 = egui::Color32::from_rgb(235, 240, 247);

fn white(alpha: f32) -> egui::Color32 {
    egui::Color32::from_white_alpha((alpha * 255.0).round() as u8)
}

fn border() -> egui::Color32 {
    white(0.12)
}

fn muted() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 140)
}

/// A damage type's glyph in the game's symbol face, and the colour the game tints it and the
/// figure beside it with. Kinetic has neither.
const fn element_mark(element: ItemDamageType) -> Option<(char, egui::Color32)> {
    match element {
        ItemDamageType::Kinetic => None,
        ItemDamageType::Arc => Some(('\u{E143}', egui::Color32::from_rgb(120, 209, 245))),
        ItemDamageType::Solar => Some(('\u{E140}', egui::Color32::from_rgb(245, 140, 56))),
        ItemDamageType::Void => Some(('\u{E144}', egui::Color32::from_rgb(179, 125, 237))),
    }
}

/// Shows the tooltip beside the pointer while `response` is hovered.
pub(in crate::app) fn on_hover(
    response: egui::Response,
    catalog: &Catalog,
    hash: u64,
) -> egui::Response {
    let menu_open =
        response.context_menu_opened() || response.ctx.memory(|memory| memory.any_popup_open());
    if response.hovered() && !response.is_pointer_button_down_on() && !menu_open {
        show(&response.ctx, catalog, hash);
    }
    response
}

/// Places the tooltip right of and below the pointer, or on the side with room for it.
fn show(ctx: &egui::Context, catalog: &Catalog, hash: u64) {
    let Some(pointer) = ctx.pointer_hover_pos() else {
        return;
    };
    let id = egui::Id::new("item_art_tooltip");
    let size = ctx
        .memory(|memory| memory.area_rect(id))
        .map_or(egui::Vec2::ZERO, |rect| rect.size());
    let screen = ctx.screen_rect();
    let mut at = pointer + egui::Vec2::splat(POINTER_OFFSET);
    if at.x + size.x > screen.right() {
        at.x = (pointer.x - POINTER_OFFSET - size.x).max(screen.left());
    }
    if at.y + size.y > screen.bottom() {
        at.y = (screen.bottom() - size.y).max(screen.top());
    }
    egui::Area::new(id)
        .order(egui::Order::Tooltip)
        .fixed_pos(at)
        .interactable(false)
        .show(ctx, |ui| {
            let scale = Scale::of(ui);
            egui::Frame::NONE
                .fill(BODY)
                .stroke(egui::Stroke::new(1.0, border()))
                .corner_radius(ROUNDING)
                .inner_margin(egui::Margin::same(scale.px(PADDING).round() as i8))
                .show(ui, |ui| draw(ui, catalog, hash, scale));
        });
}

/// The inside edges of the frame, which the band, rules and panels span.
#[derive(Clone, Copy)]
struct Span {
    left: f32,
    right: f32,
    top: f32,
}

/// The tooltip's body inside its frame.
fn draw(ui: &mut egui::Ui, catalog: &Catalog, hash: u64, scale: Scale) {
    let width = scale.px(WIDTH);
    ui.set_width(width);
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    let padding = scale.px(PADDING).round();
    let content = ui.max_rect();
    let span = Span {
        left: content.left() - padding + 1.0,
        right: content.left() + width + padding - 1.0,
        top: content.top() - padding + 1.0,
    };
    let item = Item::read(catalog, hash);
    draw_band(ui, catalog, &item, span, scale);
    let continues = !item.perks.is_empty();
    let shows_stats = item
        .title
        .consumed
        .map_or(!item.stats.is_empty(), |_| item.stats.len() > 1);
    if let Some(description) = &item.description {
        gap(ui, scale);
        let galley = wrapped(ui, description, body_font(ui, scale), muted(), width);
        paint_block(ui, galley, width);
        if item.title.shown || shows_stats || continues {
            gap(ui, scale);
            rule(ui, span);
        }
    }
    if item.title.shown {
        gap(ui, scale);
        draw_figure(ui, catalog, &item.title, scale, width);
    }
    if shows_stats {
        gap(ui, scale);
        draw_stats(ui, &item, scale, width);
    }
    for (index, perk) in item.perks.iter().enumerate() {
        if index > 0 {
            rule(ui, span);
        }
        draw_perk(ui, catalog, perk, span, scale, width);
    }
}

/// What the tooltip shows of one item.
struct Item {
    name: String,
    type_name: String,
    rarity: ItemRarity,
    hash: u64,
    description: Option<String>,
    title: Title,
    stats: Vec<StatRow>,
    /// The value a full bar stands for.
    ceiling: i32,
    armor: bool,
    perks: Vec<Perk>,
}

/// The figure a tooltip leads with, and the word beside it.
#[derive(Default)]
struct Title {
    shown: bool,
    value: i32,
    label: String,
    /// Stat row the title took, which the block under it leaves out.
    consumed: Option<usize>,
    /// The damage type that leads the figure and tints it.
    element: Option<ItemDamageType>,
    /// The energy armor carries, set between the rule and the label.
    energy: Option<Energy>,
    /// The ammunition a weapon draws, whose mark leads the label.
    ammo: Option<ItemWeaponAmmoType>,
}

/// The energy fitted to a piece of armor, and the damage type that holds it.
struct Energy {
    capacity: i32,
    element: Option<ItemDamageType>,
}

struct StatRow {
    name: String,
    value: i32,
    numeric: bool,
}

struct Perk {
    hash: u64,
    name: String,
    type_name: String,
}

impl Item {
    fn read(catalog: &Catalog, hash: u64) -> Self {
        let definition = catalog.item(hash);
        let plugs = definition
            .into_iter()
            .flat_map(|item| &item.default_plugs)
            .filter_map(|plug| plug.as_deref().and_then(parse_hash_hex))
            .collect::<Vec<_>>();
        let armor = definition.is_some_and(|item| item_type_label(item) == "Armor");
        let gear = armor || definition.is_some_and(|item| item_type_label(item) == "Weapon");
        let stats = stat_rows(catalog, hash, &plugs, armor);
        let title = if gear {
            power_title(catalog, hash, &plugs, armor)
        } else {
            title_of(catalog, hash, &stats)
        };
        Self {
            name: catalog.display_name(hash).unwrap_or_default().to_owned(),
            type_name: definition
                .map(|item| item.type_name.trim())
                .or_else(|| catalog.plug_type_name(hash))
                .or_else(|| catalog.package_item_type_name(hash))
                .unwrap_or_default()
                .to_owned(),
            rarity: catalog.item_rarity(hash),
            hash,
            description: catalog
                .description(hash)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned),
            title,
            stats,
            ceiling: catalog
                .item_stat_group(hash)
                .map(|group| group.maximum_value)
                .filter(|maximum| *maximum > 0)
                .unwrap_or(DEFAULT_STAT_CEILING),
            armor,
            perks: plugs
                .iter()
                .filter_map(|plug| listed_perk(catalog, *plug))
                .collect(),
        }
    }
}

/// Every stat the item shows, in the order the game shows them: the stats its group scales, the
/// barred ones first, from the item and the plugs it is granted with. Armor always shows the six
/// character stats, an empty one as an empty bar.
fn stat_rows(catalog: &Catalog, hash: u64, plugs: &[u64], armor: bool) -> Vec<StatRow> {
    let mut totals = BTreeMap::<u16, i32>::new();
    for source in std::iter::once(hash).chain(plugs.iter().copied()) {
        for stat in catalog
            .item_package_metadata(source)
            .into_iter()
            .flat_map(|metadata| &metadata.investment_stats)
        {
            *totals.entry(stat.definition_index).or_default() += stat.value;
        }
    }
    if armor {
        for row in catalog.character_stat_rows().into_iter().flatten() {
            totals.entry(row).or_default();
        }
    }
    let name = |row: u16| {
        catalog
            .item_stat_definition(row)
            .map(|definition| definition.name.trim())
            .filter(|name| !name.is_empty())
    };
    let Some(group) = catalog.item_stat_group(hash) else {
        return totals
            .iter()
            .filter_map(|(row, value)| {
                Some(StatRow {
                    name: name(*row)?.to_owned(),
                    value: *value,
                    numeric: false,
                })
            })
            .collect();
    };
    let mut rows = group
        .scaled_stats
        .iter()
        .filter_map(|scaled| {
            let value = *totals.get(&scaled.definition_index)?;
            Some(StatRow {
                name: name(scaled.definition_index)?.to_owned(),
                value: interpolate_investment_stat_display(
                    &scaled.display_interpolation,
                    scaled.is_linear,
                    value,
                )
                .unwrap_or(value),
                numeric: scaled.display_as_numeric,
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.numeric);
    rows
}

/// Anything other than gear is titled by the first stat it declares, which is how a Sparrow
/// reads as its speed.
fn title_of(catalog: &Catalog, hash: u64, stats: &[StatRow]) -> Title {
    let Some(declared) = catalog
        .item_package_metadata(hash)
        .and_then(|metadata| metadata.investment_stats.first())
        .and_then(|stat| catalog.item_stat_definition(stat.definition_index))
        .map(|definition| definition.name.trim())
    else {
        return Title::default();
    };
    stats
        .iter()
        .position(|row| row.name == declared)
        .map_or_else(Title::default, |index| Title {
            shown: true,
            value: stats[index].value,
            label: shout(&stats[index].name),
            consumed: Some(index),
            ..Title::default()
        })
}

/// Gear is titled by its Power, which the game words as the ammunition a weapon draws and as
/// the energy armor carries, else by the stat its figure is named for. A catalog entry has no
/// Power of its own, so it shows the Power the item is given when it is added to a character.
fn power_title(catalog: &Catalog, hash: u64, plugs: &[u64], armor: bool) -> Title {
    let level = catalog.inventory_metadata(hash).map_or(0, |metadata| {
        crate::app::item_editor::new_inventory_item_level(
            metadata.native_bucket_id,
            catalog.item_power_cap(hash),
        )
    });
    if level <= 0 {
        return Title::default();
    }
    let value =
        i32::try_from(crate::app::item_editor::displayed_item_power(level)).unwrap_or(i32::MAX);
    let mut title = Title {
        shown: true,
        value,
        element: catalog.item_damage_type(hash),
        ..Title::default()
    };
    title.ammo = catalog
        .item_package_metadata(hash)
        .and_then(|metadata| metadata.weapon_ammo_type);
    if let Some(ammo) = title.ammo {
        title.label = shout(ammo.label());
        return title;
    }
    if armor {
        title.energy = energy_of(catalog, hash, plugs);
        if title.energy.is_some() {
            title.label = "ENERGY".to_owned();
            return title;
        }
    }
    let named = if armor {
        DEFENSE_STAT_HASH
    } else {
        POWER_STAT_HASH
    };
    title.label = catalog
        .item_stat_definition_by_hash(named)
        .map(|definition| definition.name.trim())
        .filter(|name| !name.is_empty())
        .map_or_else(|| "POWER".to_owned(), shout);
    title
}

/// The energy fitted to a piece of armor, from the item and the plugs it is granted with. The
/// capacity is a stat of its own, named for the damage type that holds it, which the armor's stat
/// group does not scale, so it never reaches the stat block.
fn energy_of(catalog: &Catalog, hash: u64, plugs: &[u64]) -> Option<Energy> {
    let mut totals = BTreeMap::<u16, i32>::new();
    for source in std::iter::once(hash).chain(plugs.iter().copied()) {
        for stat in catalog
            .item_package_metadata(source)
            .into_iter()
            .flat_map(|metadata| &metadata.investment_stats)
        {
            *totals.entry(stat.definition_index).or_default() += stat.value;
        }
    }
    totals.into_iter().find_map(|(row, capacity)| {
        let name = catalog.item_stat_definition(row)?.name.to_lowercase();
        (capacity > 0 && name.contains("energy capacity")).then(|| Energy {
            capacity,
            element: [
                ("arc", ItemDamageType::Arc),
                ("solar", ItemDamageType::Solar),
                ("void", ItemDamageType::Void),
            ]
            .into_iter()
            .find_map(|(word, element)| name.contains(word).then_some(element)),
        })
    })
}

/// A plug the game lists as a perk: named by the game, and not an empty or default plug.
fn listed_perk(catalog: &Catalog, hash: u64) -> Option<Perk> {
    let name = catalog.display_name(hash)?.trim();
    if name.is_empty()
        || crate::unnamed_plugs::contains(hash)
        || name.starts_with("Empty ")
        || name.starts_with("Default ")
    {
        return None;
    }
    Some(Perk {
        hash,
        name: name.to_owned(),
        type_name: catalog.plug_type_name(hash).unwrap_or_default().to_owned(),
    })
}

fn body_font(ui: &egui::Ui, scale: Scale) -> egui::FontId {
    super::text_font(ui, scale.body)
}

fn gap(ui: &mut egui::Ui, scale: Scale) {
    ui.add_space(scale.px(BLOCK_GAP));
}

/// A divider across the whole frame at the cursor.
fn rule(ui: &egui::Ui, span: Span) {
    let y = ui.cursor().top();
    ui.painter().hline(
        span.left..=span.right,
        y,
        egui::Stroke::new(1.0, white(0.10)),
    );
}

/// Paints a galley at the cursor and advances past it.
fn paint_block(ui: &mut egui::Ui, galley: Arc<egui::Galley>, width: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, galley.size().y), egui::Sense::hover());
    ui.painter().galley(rect.min, galley, muted());
}

/// A name wrapped to `width`, one galley a line. The face's own line height keeps descender room
/// that capitals leave empty, so the band sets the lines closer itself.
fn name_lines(
    ui: &egui::Ui,
    name: &str,
    font: &egui::FontId,
    color: egui::Color32,
    width: f32,
) -> Vec<Arc<egui::Galley>> {
    wrapped(ui, name, font.clone(), color, width)
        .rows
        .iter()
        .map(|row| {
            let line = row.glyphs.iter().map(|glyph| glyph.chr).collect::<String>();
            ui.fonts(|fonts| fonts.layout_no_wrap(line.trim().to_owned(), font.clone(), color))
        })
        .collect()
}

/// The rarity band: the icon flush in the corner, the name in capitals beside it, the type
/// under the name and the tier at the far right of that line. A name too long for its line wraps,
/// and the band grows under it while the icon keeps its size in the corner.
fn draw_band(ui: &mut egui::Ui, catalog: &Catalog, item: &Item, span: Span, scale: Scale) {
    let height = scale.px(BAND_HEIGHT);
    let padding = scale.px(PADDING);
    let origin = egui::pos2(span.left, span.top);
    let text_left = origin.x + height + padding;
    let name_font = super::title_font(ui, scale.body * NAME_SCALE);
    let name_color = band_text(item.rarity, false);
    let names = name_lines(
        ui,
        &shout(&item.name),
        &name_font,
        name_color,
        span.right - padding - text_left,
    );
    let (name_cap_top, name_cap) = cap_band(ui, name_font);
    // A wrapped name's lines are parted by the gap that parts the name from the type.
    let type_gap = scale.px(BAND_TYPE_GAP);
    let leading = name_cap + type_gap;
    let grown = leading * names.len().saturating_sub(1) as f32;
    let band = egui::Rect::from_min_max(origin, egui::pos2(span.right, span.top + height + grown));
    ui.painter().rect_filled(
        band,
        egui::CornerRadius {
            nw: ROUNDING,
            ne: ROUNDING,
            sw: 0,
            se: 0,
        },
        band_color(item.rarity),
    );
    let icon = egui::Rect::from_min_size(origin, egui::Vec2::splat(height));
    super::icon(ui, catalog, item.hash, item.rarity, icon);

    let type_font = super::text_font(ui, scale.body * TYPE_SCALE);
    let (type_cap_top, type_cap) = cap_band(ui, type_font.clone());
    let tier = (item.rarity != ItemRarity::Unknown).then(|| item.rarity.label());
    let muted = band_text(item.rarity, true);
    let tier = tier.map(|tier| {
        ui.fonts(|fonts| fonts.layout_no_wrap(tier.to_owned(), type_font.clone(), muted))
    });
    let tier_width = tier.as_ref().map_or(0.0, |galley| galley.size().x);
    let second_line = !item.type_name.is_empty() || tier.is_some();
    // The pair is centred on the ink it puts on the band: the name's capitals, the gap and the
    // type's capitals. Each line then backs off by the room its own box keeps above them. The
    // lines a wrapped name adds sit between, on the band's growth.
    let ink = if second_line {
        name_cap + type_gap + type_cap
    } else {
        name_cap
    };
    let ink_top = origin.y + (height - ink).max(0.0) * 0.5;
    let name_top = ink_top - name_cap_top;
    let type_top = ink_top + grown + name_cap + type_gap - type_cap_top;

    let weight = super::strike(ui, GameFace::Title, scale.px(NAME_WEIGHT));
    let painter = ui.painter();
    for (index, line) in names.iter().enumerate() {
        bold(
            painter,
            egui::pos2(text_left, name_top + leading * index as f32),
            line,
            name_color,
            weight,
        );
    }
    if !item.type_name.is_empty() {
        let type_line = single_line(
            ui,
            &item.type_name,
            type_font,
            muted,
            span.right - padding - text_left - tier_width - padding,
        );
        painter.galley(egui::pos2(text_left, type_top), type_line, muted);
    }
    if let Some(tier) = tier {
        painter.galley(
            egui::pos2(span.right - padding - tier_width, type_top),
            tier,
            muted,
        );
    }
    // The cursor started one padding below the frame; this carries it under the band.
    ui.allocate_exact_size(
        egui::vec2(ui.available_width(), (height + grown - padding).max(0.0)),
        egui::Sense::hover(),
    );
}

/// The lines the figure's row sets its content on: the middle of the figure's digits, which
/// artwork centres its ink on, and the line every line of type hangs from, near the figure's
/// baseline.
#[derive(Clone, Copy)]
struct Row {
    centre: f32,
    anchor: f32,
}

/// The leading figure and everything the game sets beside it on one row: the damage type's
/// glyph, the figure in its colour, a rule, the energy armor carries, then a weapon's ammunition
/// mark and word, or the label anything else carries.
fn draw_figure(ui: &mut egui::Ui, catalog: &Catalog, title: &Title, scale: Scale, width: f32) {
    let trim = scale.px(FIGURE_TOP_TRIM);
    let top = ui.cursor().top() - trim;
    let mut x = ui.cursor().left();
    let size = scale.body * FIGURE_SCALE;
    let figure_font = super::figure_font(ui, size);
    let box_height = super::line_height(ui, &figure_font);
    let row = Row {
        centre: top + box_height * FIGURE_MIDDLE,
        anchor: top + box_height * TEXT_ANCHOR,
    };
    let mark = title.element.and_then(element_mark);
    let tint = mark.map_or(TEXT, |(_, tint)| tint);
    if let Some(mark) = mark
        && let Some(advance) = draw_element(ui, mark, size * ELEMENT_GLYPH_SCALE, x, row.centre)
    {
        x += advance + scale.px(FIGURE_GLYPH_GAP);
    }
    let figure = ui.fonts(|fonts| fonts.layout_no_wrap(title.value.to_string(), figure_font, tint));
    let weight = super::strike(ui, GameFace::Figure, scale.px(FIGURE_WEIGHT));
    bold(ui.painter(), egui::pos2(x, top), &figure, tint, weight);
    x += figure.size().x + weight + scale.px(FIGURE_RULE_GAP);
    let rule_half = box_height * FIGURE_RULE_HEIGHT * 0.5;
    ui.painter().vline(
        x,
        (row.centre - rule_half)..=(row.centre + rule_half),
        egui::Stroke::new(1.0, white(0.30)),
    );
    x += 1.0 + scale.px(FIGURE_RULE_GAP);
    if let Some(energy) = &title.energy {
        x = draw_energy(ui, energy, row, x, scale);
    }
    draw_ammunition(ui, catalog, title, row, x, scale);
    ui.allocate_exact_size(
        egui::vec2(width, (box_height * FIGURE_ROW_HEIGHT - trim).max(0.0)),
        egui::Sense::hover(),
    );
}

/// Paints a damage type's glyph at `x` with its ink centred on `line`, and returns its width.
/// Nothing is drawn without the install's symbol face.
fn draw_element(
    ui: &egui::Ui,
    (glyph, tint): (char, egui::Color32),
    size: f32,
    x: f32,
    line: f32,
) -> Option<f32> {
    let font = super::symbol_font(ui, size)?;
    let (ink_top, ink_height) = super::ink_band(ui, font.clone(), glyph);
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap(glyph.to_string(), font, tint));
    let width = galley.size().x;
    ui.painter().galley(
        egui::pos2(x, line - ink_top - ink_height * 0.5),
        galley,
        tint,
    );
    Some(width)
}

/// The energy armor carries beside its Power: the damage type's glyph, then the capacity, bold in
/// that type's colour. Returns where the row continues.
fn draw_energy(ui: &egui::Ui, energy: &Energy, row: Row, mut x: f32, scale: Scale) -> f32 {
    let size = scale.body * ENERGY_VALUE_SCALE;
    let font = super::figure_font(ui, size);
    let height = super::line_height(ui, &font);
    // The capacity is the figure this lockup is set around, so its own digits carry the line the
    // glyph centres on, rather than the Power figure's, which is far larger.
    let top = row.anchor - height * TEXT_ANCHOR;
    let middle = top + height * FIGURE_MIDDLE;
    let mark = energy.element.and_then(element_mark);
    let tint = mark.map_or(TEXT, |(_, tint)| tint);
    if let Some(mark) = mark
        && let Some(advance) = draw_element(ui, mark, size * ELEMENT_GLYPH_SCALE, x, middle)
    {
        x += advance + scale.px(ENERGY_GLYPH_GAP);
    }
    let capacity = ui.fonts(|fonts| fonts.layout_no_wrap(energy.capacity.to_string(), font, tint));
    let weight = super::strike(ui, GameFace::Figure, scale.px(ENERGY_VALUE_WEIGHT));
    bold(ui.painter(), egui::pos2(x, top), &capacity, tint, weight);
    x + capacity.size().x + weight + scale.px(FIGURE_RULE_GAP)
}

/// What the game sets at the end of the figure's row: a weapon's ammunition mark and its word in
/// full white, or the muted label anything else carries.
fn draw_ammunition(
    ui: &egui::Ui,
    catalog: &Catalog,
    title: &Title,
    row: Row,
    mut x: f32,
    scale: Scale,
) {
    if let Some(texture) = title
        .ammo
        .and_then(|ammo| catalog.ammo_icon_texture(ui.ctx(), ammo))
    {
        let mark = scale.px(AMMO_MARK_EXTENT);
        egui::Image::new(&texture).paint_at(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(x, row.centre - mark * (0.5 - AMMO_MARK_DROP)),
                egui::Vec2::splat(mark),
            ),
        );
        x += mark + scale.px(AMMO_WORD_GAP);
    }
    let font = body_font(ui, scale);
    let top = row.anchor - super::line_height(ui, &font) * TEXT_ANCHOR;
    let color = if title.ammo.is_some() { TEXT } else { muted() };
    let label = ui.fonts(|fonts| fonts.layout_no_wrap(title.label.clone(), font, color));
    ui.painter().galley(egui::pos2(x, top), label, color);
}

/// The stat block: right-aligned names, white bars on a dark track, and the values. Armor signs
/// its values and sums its barred stats under them.
fn draw_stats(ui: &mut egui::Ui, item: &Item, scale: Scale, width: f32) {
    let font = body_font(ui, scale);
    let line = super::line_height(ui, &font);
    let row_height = line + scale.px(STAT_ROW_GAP);
    let name_width = scale.px(STAT_NAME_WIDTH);
    let column_gap = scale.px(STAT_COLUMN_GAP);
    let bar_width = width - name_width - scale.px(STAT_VALUE_WIDTH) - column_gap * 2.0;
    let bar_height = scale.px(STAT_BAR_HEIGHT);
    let block_padding = scale.px(PADDING) * STAT_BLOCK_PADDING_SCALE;
    let drawn =
        item.stats.len() - usize::from(item.title.consumed.is_some()) + usize::from(item.armor);
    let origin = ui.cursor().min;
    let bar_left = origin.x + name_width + column_gap;
    let mut top = origin.y + block_padding * 0.5;
    let mut sum = 0;
    let painter = ui.painter();
    let text = |at: egui::Pos2, value: &str, color: egui::Color32, right_aligned: bool| {
        let galley = ui.fonts(|fonts| fonts.layout_no_wrap(value.to_owned(), font.clone(), color));
        let x = if right_aligned {
            at.x - galley.size().x
        } else {
            at.x
        };
        painter.galley(egui::pos2(x, at.y), galley, color);
    };
    for (index, row) in item.stats.iter().enumerate() {
        if Some(index) == item.title.consumed {
            continue;
        }
        text(
            egui::pos2(origin.x + name_width, top),
            &row.name,
            muted(),
            true,
        );
        if !row.numeric {
            let bar_top = top + (line - bar_height) * 0.5;
            let share = (row.value as f32 / item.ceiling.max(1) as f32).clamp(0.0, 1.0);
            let track = egui::Rect::from_min_size(
                egui::pos2(bar_left, bar_top),
                egui::vec2(bar_width, bar_height),
            );
            painter.rect_filled(track, 0.0, white(0.14));
            painter.rect_filled(
                egui::Rect::from_min_size(track.min, egui::vec2(bar_width * share, bar_height)),
                0.0,
                egui::Color32::from_gray(237),
            );
            sum += row.value;
        }
        let value = if item.armor && row.value > 0 {
            format!("+{}", row.value)
        } else {
            row.value.to_string()
        };
        // A barred stat's value starts where its bar ends, a numeric one where the bar would.
        let value_x = if row.numeric {
            bar_left
        } else {
            bar_left + bar_width + column_gap
        };
        text(egui::pos2(value_x, top), &value, TEXT, false);
        top += row_height;
    }
    if item.armor {
        text(
            egui::pos2(origin.x + name_width, top),
            TOTAL_LABEL,
            muted(),
            true,
        );
        text(egui::pos2(bar_left, top), &sum.to_string(), TEXT, false);
    }
    ui.allocate_exact_size(
        egui::vec2(width, row_height * drawn as f32 + block_padding),
        egui::Sense::hover(),
    );
}

/// One perk row: the round badge holding the icon, then the name. The intrinsic frame sits on a
/// panel of its own.
fn draw_perk(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    perk: &Perk,
    span: Span,
    scale: Scale,
    width: f32,
) {
    let row_height = scale.px(PERK_ROW_HEIGHT);
    let badge = scale.px(PERK_ICON) + scale.px(PERK_BADGE_PADDING) * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, row_height), egui::Sense::hover());
    let painter = ui.painter();
    let type_name = perk.type_name.to_lowercase();
    if type_name.contains("intrinsic") {
        painter.rect_filled(
            egui::Rect::from_x_y_ranges(span.left..=span.right, rect.y_range()),
            0.0,
            white(0.07),
        );
    }
    let centre = egui::pos2(rect.left() + badge * 0.5, rect.center().y);
    // A mod, a shader, an ornament and an intrinsic ship framed art of their own, so only a trait
    // gets the round badge behind it.
    if !["intrinsic", "mod", "shader", "ornament"]
        .iter()
        .any(|kind| type_name.contains(kind))
    {
        painter.circle_filled(centre, badge * 0.5, white(0.12));
    }
    if let Some(texture) = catalog.icon_texture(ui.ctx(), perk.hash) {
        let icon = badge * PERK_ICON / (PERK_ICON + PERK_BADGE_PADDING * 2.0);
        egui::Image::new(&texture).paint_at(
            ui,
            egui::Rect::from_center_size(centre, egui::Vec2::splat(icon)),
        );
    }
    let text_left = rect.left() + badge + scale.px(PADDING);
    let font = body_font(ui, scale);
    let name_top = rect.center().y - super::line_height(ui, &font) * 0.5;
    let name = single_line(ui, &perk.name, font, TEXT, rect.right() - text_left);
    ui.painter()
        .galley(egui::pos2(text_left, name_top), name, TEXT);
}
