//! The home page's item browser, laid out as Dawn's Loadout Studio lays out its own: a tab per
//! shelf, one row of filters, and the shelf's items as cards banded in their rarity's colour,
//! grouped by type under ruled headings.

use std::{collections::HashSet, sync::Arc};

use eframe::egui;

use crate::{
    app::{
        inspector::{
            look,
            requests::{self, AddDestination},
        },
        item_art::{self, Scale},
        ui::GameFace,
    },
    catalog::{BrowseEntry, Catalog, ItemRarity, Shelf},
    hash::format_hash_hex,
};

use super::super::super::state::{
    Browse, BrowseIndex, BrowseKey, BrowseResults, BrowseSort, BrowseTab,
};
use super::{Row, Target, result_row};

/// Card geometry, authored against 16 px body text as Dawn's is.
const CARD_HEIGHT: f32 = 52.0;
const CARD_MINIMUM_WIDTH: f32 = 285.0;
const CARD_COLUMN_GAP: f32 = 8.0;
const CARD_ROW_GAP: f32 = 3.0;
const CARD_TEXT_GAP: f32 = 8.0;
const CARD_TEXT_RIGHT_INSET: f32 = 10.0;
/// The name is all capitals, so its line's descender room already parts it from the detail.
const CARD_LINE_GAP: f32 = -1.0;
const CARD_NAME_SCALE: f32 = 1.8;
const CARD_DETAIL_SCALE: f32 = 1.1;
const CARD_NAME_WEIGHT: f32 = 0.8;
/// Separator between the halves of a card's detail line.
const DETAIL_SEPARATOR: &str = "  /  ";
/// Tabs: their height, widest width, the rail under the open one, and the capitals' spacing.
const TAB_HEIGHT: f32 = 34.0;
const TAB_MAXIMUM_WIDTH: f32 = 110.0;
const TAB_RAIL_INSET: f32 = 10.0;
const TAB_RAIL_HEIGHT: f32 = 2.0;
const TAB_HOVER_ALPHA: f32 = 0.45;
const TRACKING: f32 = 1.6;
/// Section headings: the room around the label, the gap before the count, and the rule's alpha.
const HEADING_PADDING: f32 = 8.0;
const HEADING_COUNT_GAP: f32 = 8.0;
/// The gap between a folding heading's chevron and its label.
const HEADING_CHEVRON_GAP: f32 = 8.0;
const HEADING_LABEL_ALPHA: f32 = 0.85;
const HEADING_RULE_ALPHA: f32 = 0.18;
const RULE_SPACING: f32 = 3.0;
const ROW_SPACING: f32 = 4.0;
/// Filter widths, and the least the search keeps when they crowd it.
const TYPE_FILTER_WIDTH: f32 = 200.0;
const RARITY_FILTER_WIDTH: f32 = 124.0;
const CLASS_FILTER_WIDTH: f32 = 124.0;
const SORT_WIDTH: f32 = 140.0;
const SEARCH_MINIMUM_WIDTH: f32 = 160.0;
/// Picker: the chevron in the field, the list's height, and the rail on the chosen row.
const CHEVRON_WIDTH: f32 = 8.0;
const CHEVRON_HEIGHT: f32 = 4.0;
const CHEVRON_INSET: f32 = 12.0;
const CHEVRON_ALPHA: f32 = 0.7;
const PICKER_LIST_HEIGHT: f32 = 420.0;
const PICKER_RAIL_WIDTH: f32 = 2.0;
const PICKER_ROW_INDENT: f32 = 8.0;
/// Definition rows keep the search rows' own height.
const DEFINITION_ROW_HEIGHT: f32 = 34.0;
const UNTYPED: &str = "Other";
const CLASSES: [&str; 3] = ["Titan", "Hunter", "Warlock"];

/// The shelf the cards come from. The Recent tab keeps the first shelf's counts current.
const fn shelf(tab: BrowseTab) -> Shelf {
    match tab {
        BrowseTab::Shelf(shelf) => shelf,
        BrowseTab::Recent => Shelf::Weapons,
    }
}

/// The catalog's browsable entries, built once per catalog.
pub(super) fn index(browse: &mut Browse, catalog: &Catalog) -> Arc<BrowseIndex> {
    let address = std::ptr::from_ref(catalog) as usize;
    if let Some(index) = browse
        .index
        .as_ref()
        .filter(|index| index.catalog == address)
    {
        return Arc::clone(index);
    }
    let entries = catalog.browse_entries();
    let text = entries
        .iter()
        .map(|entry| {
            format!(
                "{} {} {}",
                entry.name,
                entry.type_name,
                format_hash_hex(entry.hash)
            )
            .to_lowercase()
        })
        .collect();
    let index = Arc::new(BrowseIndex {
        catalog: address,
        entries,
        text,
    });
    browse.index = Some(Arc::clone(&index));
    index
}

/// Rebuilds the cards when the search or a filter changed. Returns whether it rebuilt.
pub(super) fn refresh(browse: &mut Browse, index: &BrowseIndex, query: &str) -> bool {
    let shelf = shelf(browse.tab);
    let key = BrowseKey {
        catalog: index.catalog,
        query: query.trim().to_owned(),
        shelf,
        type_name: browse.type_name.clone(),
        rarity: browse.rarity,
        class: browse.class,
        sort: browse.sort,
        dummy_items: browse.dummy_items,
    };
    if browse.results.key.as_ref() == Some(&key) {
        return false;
    }
    let words = key
        .query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let mut results = BrowseResults::default();
    for (position, entry) in index.entries.iter().enumerate() {
        // Whether the shelf has class items decides the Class filter, whatever narrows the cards.
        results.has_classes |= entry.on_shelf(shelf) && entry.class.is_some();
        // Dummy items stay off the shelves, but a search reaches every named one.
        let hidden = entry.internal && !browse.dummy_items && (words.is_empty() || entry.unnamed);
        if hidden
            || !words
                .iter()
                .all(|word| index.text[position].contains(word.as_str()))
        {
            continue;
        }
        for (count, candidate) in results.shelf_matches.iter_mut().zip(Shelf::ALL) {
            *count += usize::from(entry.on_shelf(candidate));
        }
        if !entry.on_shelf(shelf)
            || browse.rarity.is_some_and(|rarity| entry.rarity != rarity)
            || browse
                .class
                .is_some_and(|class| entry.class.is_some_and(|own| own != class))
        {
            continue;
        }
        // Types are counted before the type filter, so the Type list offers every type.
        results.total += 1;
        *results
            .types
            .entry(type_label(entry).to_owned())
            .or_default() += 1;
        if browse
            .type_name
            .as_deref()
            .is_none_or(|type_name| type_name == type_label(entry))
        {
            results.cards.push(position);
        }
    }
    // Entries are in name order, so a stable sort keeps names in order within each key.
    match browse.sort {
        BrowseSort::Type => results.cards.sort_by_cached_key(|position| {
            let entry = &index.entries[*position];
            (entry.type_name.is_empty(), entry.type_name.to_lowercase())
        }),
        BrowseSort::Name => {}
        BrowseSort::Rarity => results
            .cards
            .sort_by_key(|position| std::cmp::Reverse(index.entries[*position].rarity)),
    }
    results.key = Some(key);
    browse.results = results;
    true
}

fn type_label(entry: &BrowseEntry) -> &str {
    if entry.type_name.is_empty() {
        UNTYPED
    } else {
        &entry.type_name
    }
}

/// The shelf tabs in spaced capitals, each with its match count while a search is active, and
/// Recent. Other counts the matching definitions that are not items too. The open tab is
/// underlined, and a tab under the pointer faintly.
pub(super) fn tabs(ui: &mut egui::Ui, browse: &mut Browse, searching: bool, recent: usize) {
    let scale = Scale::of(ui);
    let matches = browse.results.shelf_matches;
    let definitions = browse.definitions.hits.len();
    let mut tabs = Shelf::ALL
        .into_iter()
        .zip(matches)
        .map(|(shelf, count)| {
            let count = count
                + if shelf == Shelf::Other {
                    definitions
                } else {
                    0
                };
            (
                BrowseTab::Shelf(shelf),
                shelf.label(),
                searching.then_some(count),
            )
        })
        .collect::<Vec<_>>();
    if recent > 0 {
        tabs.push((BrowseTab::Recent, "Recent", Some(recent)));
    }
    let mut chosen = browse.tab;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (tab, label, count) in tabs {
            if tab_button(ui, scale, label, count, browse.tab == tab).clicked() {
                chosen = tab;
            }
        }
    });
    if chosen != browse.tab {
        browse.tab = chosen;
        browse.type_name = None;
    }
}

fn tab_button(
    ui: &mut egui::Ui,
    scale: Scale,
    label: &str,
    count: Option<usize>,
    active: bool,
) -> egui::Response {
    let font = item_art::text_font(ui, scale.body);
    let text = ui.visuals().strong_text_color();
    let muted = look::muted(ui);
    let label_galley = item_art::spaced_capitals(ui, label, font.clone(), text, scale.px(TRACKING));
    let count_galley = count.map(|count| {
        ui.fonts(|fonts| fonts.layout_no_wrap(count.to_string(), font.clone(), muted))
    });
    let count_width = count_galley
        .as_ref()
        .map_or(0.0, |galley| galley.size().x + scale.px(HEADING_COUNT_GAP));
    let content = label_galley.size().x + count_width;
    let width = scale
        .px(TAB_MAXIMUM_WIDTH)
        .max(content + scale.px(TAB_RAIL_INSET) * 2.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, scale.px(TAB_HEIGHT)),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, label)
    });
    let lit = active || response.hovered();
    let color = if lit { text } else { muted };
    let left = rect.center().x - content * 0.5;
    let top = rect.center().y - item_art::line_height(ui, &font) * 0.5;
    let painter = ui.painter();
    painter.galley(egui::pos2(left, top), label_galley.clone(), color);
    if let Some(galley) = count_galley {
        painter.galley(
            egui::pos2(
                left + label_galley.size().x + scale.px(HEADING_COUNT_GAP),
                top,
            ),
            galley,
            muted,
        );
    }
    if lit {
        let alpha = if active { 1.0 } else { TAB_HOVER_ALPHA };
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(
                    rect.left() + scale.px(TAB_RAIL_INSET),
                    rect.bottom() - scale.px(TAB_RAIL_HEIGHT),
                ),
                egui::pos2(rect.right() - scale.px(TAB_RAIL_INSET), rect.bottom()),
            ),
            0.0,
            text.gamma_multiply(alpha),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The search, then the Type, Rarity, Class and Sort pickers, Dummy Items, and Reset once
/// something narrows the cards. The search takes what the rest leave. Returns the search
/// field's response and whether Reset was pressed.
pub(super) fn filter_row(
    ui: &mut egui::Ui,
    browse: &mut Browse,
    narrowed: bool,
    search: impl FnOnce(&mut egui::Ui, f32, f32) -> egui::Response,
) -> (egui::Response, bool) {
    let scale = Scale::of(ui);
    let filters = browse.tab != BrowseTab::Recent;
    let height = ui.spacing().interact_size.y.max(scale.px(30.0));
    let mut reset = false;
    let response = ui
        .horizontal_wrapped(|ui| {
            let spacing = ui.spacing().item_spacing.x;
            let padding = ui.spacing().button_padding.x * 2.0;
            let trailing = if filters {
                // A checkbox sets its label in the button font and pads it as a button does, so
                // measured that way every item stays on the search's line.
                let checkbox = ui.spacing().icon_width
                    + ui.spacing().icon_spacing
                    + text_width(ui, egui::TextStyle::Button, "Dummy Items")
                    + padding;
                let reset_width = if narrowed {
                    text_width(ui, egui::TextStyle::Button, "Reset") + padding + spacing
                } else {
                    0.0
                };
                let class = if browse.results.has_classes {
                    scale.px(CLASS_FILTER_WIDTH) + spacing
                } else {
                    0.0
                };
                scale.px(TYPE_FILTER_WIDTH + RARITY_FILTER_WIDTH + SORT_WIDTH)
                    + class
                    + checkbox
                    + reset_width
                    + spacing * 4.0
            } else {
                0.0
            };
            // Whole pixels, so rounding never pushes the last item onto a line of its own.
            let width = (ui.available_width() - trailing - 1.0)
                .floor()
                .max(scale.px(SEARCH_MINIMUM_WIDTH));
            let response = search(ui, width, height);
            if filters {
                type_filter(ui, browse, scale, height);
                rarity_filter(ui, browse, scale, height);
                if browse.results.has_classes {
                    class_filter(ui, browse, scale, height);
                }
                sort_order(ui, browse, scale, height);
                ui.checkbox(&mut browse.dummy_items, "Dummy Items");
                if narrowed && ui.button("Reset").clicked() {
                    reset = true;
                }
            }
            response
        })
        .inner;
    (response, reset)
}

fn text_width(ui: &egui::Ui, style: egui::TextStyle, text: &str) -> f32 {
    let font = style.resolve(ui.style());
    ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
            .size()
            .x
    })
}

fn type_filter(ui: &mut egui::Ui, browse: &mut Browse, scale: Scale, height: f32) {
    let total = browse.results.total;
    let selected = browse.type_name.as_deref().map_or_else(
        || look::tab_text(ui, "All Types", Some(total)),
        |type_name| {
            let count = browse.results.types.get(type_name).copied().unwrap_or(0);
            look::tab_text(ui, type_name, Some(count))
        },
    );
    let types = &browse.results.types;
    let mut choice = browse.type_name.clone();
    picker(
        ui,
        "definition_browse_type",
        scale.px(TYPE_FILTER_WIDTH),
        height,
        selected,
        |ui| {
            if picker_row(
                ui,
                look::tab_text(ui, "All Types", Some(total)),
                choice.is_none(),
            ) {
                choice = None;
            }
            for (type_name, count) in types {
                let chosen = choice.as_deref() == Some(type_name.as_str());
                if picker_row(ui, look::tab_text(ui, type_name, Some(*count)), chosen) {
                    choice = Some(type_name.clone());
                }
            }
        },
    );
    browse.type_name = choice;
}

fn rarity_filter(ui: &mut egui::Ui, browse: &mut Browse, scale: Scale, height: f32) {
    let selected = browse.rarity.map_or("All Rarities", ItemRarity::label);
    picker(
        ui,
        "definition_browse_rarity",
        scale.px(RARITY_FILTER_WIDTH),
        height,
        selected.into(),
        |ui| {
            if picker_row(ui, "All Rarities".into(), browse.rarity.is_none()) {
                browse.rarity = None;
            }
            for rarity in ItemRarity::ALL {
                if picker_row(ui, rarity.label().into(), browse.rarity == Some(rarity)) {
                    browse.rarity = Some(rarity);
                }
            }
        },
    );
}

fn class_filter(ui: &mut egui::Ui, browse: &mut Browse, scale: Scale, height: f32) {
    let selected = browse
        .class
        .and_then(|class| CLASSES.get(usize::from(class)).copied())
        .unwrap_or("All Classes");
    picker(
        ui,
        "definition_browse_class",
        scale.px(CLASS_FILTER_WIDTH),
        height,
        selected.into(),
        |ui| {
            if picker_row(ui, "All Classes".into(), browse.class.is_none()) {
                browse.class = None;
            }
            for (class, label) in (0_u8..).zip(CLASSES) {
                if picker_row(ui, label.into(), browse.class == Some(class)) {
                    browse.class = Some(class);
                }
            }
        },
    );
}

fn sort_order(ui: &mut egui::Ui, browse: &mut Browse, scale: Scale, height: f32) {
    picker(
        ui,
        "definition_browse_sort",
        scale.px(SORT_WIDTH),
        height,
        browse.sort.label().into(),
        |ui| {
            for sort in BrowseSort::ALL {
                if picker_row(ui, sort.label().into(), browse.sort == sort) {
                    browse.sort = sort;
                }
            }
        },
    );
}

/// A field showing the choice, with a chevron in it, that opens its list of rows beneath it.
fn picker(
    ui: &mut egui::Ui,
    id: &'static str,
    width: f32,
    height: f32,
    selected: egui::WidgetText,
    rows: impl FnOnce(&mut egui::Ui),
) {
    let scale = Scale::of(ui);
    let popup_id = egui::Id::new(id);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let open = ui.memory(|memory| memory.is_popup_open(popup_id));
    let visuals = ui.visuals();
    let stroke = if open || response.hovered() {
        visuals.widgets.hovered.bg_stroke
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    let text_color = visuals.strong_text_color();
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(4),
        visuals.extreme_bg_color,
        stroke,
        egui::StrokeKind::Inside,
    );
    let chevron_x = rect.right() - scale.px(CHEVRON_INSET);
    let galley = selected.into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (chevron_x - scale.px(CHEVRON_WIDTH) - rect.left() - 8.0 - 6.0).max(0.0),
        egui::TextStyle::Body,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y * 0.5),
        galley,
        text_color,
    );
    let half = scale.px(CHEVRON_WIDTH) * 0.5;
    let rise = scale.px(CHEVRON_HEIGHT) * 0.5;
    let centre = rect.center().y;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(chevron_x - half, centre - rise),
            egui::pos2(chevron_x + half, centre - rise),
            egui::pos2(chevron_x, centre + rise),
        ],
        text_color.gamma_multiply(CHEVRON_ALPHA),
        egui::Stroke::NONE,
    ));
    if response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }
    egui::popup_below_widget(
        ui,
        popup_id,
        &response.on_hover_cursor(egui::CursorIcon::PointingHand),
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing.y = 1.0;
            egui::ScrollArea::vertical()
                .max_height(scale.px(PICKER_LIST_HEIGHT))
                .show(ui, rows);
        },
    );
}

/// One row of a picker's list. The chosen row carries a rail at its left edge.
fn picker_row(ui: &mut egui::Ui, text: egui::WidgetText, selected: bool) -> bool {
    let height = ui.spacing().interact_size.y + 4.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            text.text(),
        )
    });
    let visuals = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    } else if selected {
        ui.painter()
            .rect_filled(rect, 0.0, visuals.widgets.inactive.weak_bg_fill);
    }
    let color = visuals.strong_text_color();
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(PICKER_RAIL_WIDTH, rect.height())),
            0.0,
            color,
        );
    }
    let galley = text.into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        rect.width() - PICKER_ROW_INDENT * 2.0,
        egui::TextStyle::Body,
    );
    ui.painter().galley(
        egui::pos2(
            rect.left() + PICKER_ROW_INDENT,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    response.clicked()
}

/// One line of the result list, laid out before any is drawn so only visible lines draw.
enum Line {
    Group { start: usize, count: usize },
    Cards { start: usize, end: usize },
    Heading { count: usize },
    Definition(usize),
}

/// Heights and widths of the result list at this UI's size.
#[derive(Clone, Copy)]
struct Metrics {
    scale: Scale,
    card_height: f32,
    column_gap: f32,
    /// A heading's label, its padding and the rule spacing under it.
    heading: f32,
    /// The space above a heading that follows other lines.
    heading_gap: f32,
}

impl Metrics {
    fn of(ui: &egui::Ui) -> Self {
        let scale = Scale::of(ui);
        let font = item_art::text_font(ui, scale.body);
        let line = item_art::line_height(ui, &font);
        Self {
            scale,
            card_height: scale.px(CARD_HEIGHT).floor(),
            column_gap: scale.px(CARD_COLUMN_GAP),
            heading: line + scale.px(HEADING_PADDING) + scale.px(RULE_SPACING),
            heading_gap: scale.px(ROW_SPACING),
        }
    }

    fn card_pitch(self) -> f32 {
        self.card_height + self.scale.px(CARD_ROW_GAP).floor()
    }
}

/// The cards, grouped by type under the Type order, then other definitions matching the
/// search. Returns what was clicked.
pub(super) fn results(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    base_id: egui::Id,
    browse: &mut Browse,
    index: &BrowseIndex,
    definitions: &[Row<'_>],
    rebuilt: bool,
) -> Option<Target> {
    let shelf = shelf(browse.tab);
    let cards = &browse.results.cards;
    if cards.is_empty() && definitions.is_empty() {
        look::empty_state(ui, "No Matching Items");
        return None;
    }
    let grouped = browse.sort == BrowseSort::Type && browse.type_name.is_none();
    let metrics = Metrics::of(ui);
    let mut area = egui::ScrollArea::vertical()
        .id_salt(base_id.with("browse_results"))
        .auto_shrink([false, false]);
    if rebuilt {
        area = area.vertical_scroll_offset(0.0);
    }
    let mut click = None;
    area.show_viewport(ui, |ui, viewport| {
        // Measured inside the area, so a solid scroll bar takes its width from the cards.
        let width = ui.available_width();
        let minimum = metrics.scale.px(CARD_MINIMUM_WIDTH);
        let gap = metrics.column_gap;
        let columns = (((width + gap) / (minimum + gap)).floor() as usize).max(1);
        let grid = Grid {
            catalog,
            index,
            cards,
            definitions,
            shelf,
            folded: &browse.folded,
            width,
            card_width: ((width - gap * (columns - 1) as f32) / columns as f32).max(0.0),
            metrics,
        };
        let lines = lines(
            index,
            cards,
            definitions.len(),
            columns,
            grouped,
            metrics,
            |label| grid.is_folded(label),
        );
        ui.set_height(lines.iter().map(|(_, height)| height).sum::<f32>());
        let origin = ui.min_rect().min;
        let mut top = 0.0;
        for (line, height) in &lines {
            let line_top = top;
            top += height;
            if line_top + height >= viewport.min.y && line_top <= viewport.max.y {
                let at = origin + egui::vec2(0.0, line_top);
                if let Some(clicked) = grid.draw(ui, line, at, *height) {
                    click = Some(clicked);
                }
            }
        }
    });
    match click {
        Some(Click::Open(target)) => Some(target),
        Some(Click::Fold(label)) => {
            let key = (shelf, label);
            if !browse.folded.remove(&key) {
                browse.folded.insert(key);
            }
            None
        }
        None => None,
    }
}

/// What a click in the result list asks for.
enum Click {
    Open(Target),
    Fold(String),
}

/// What the result list draws from, and its measured widths.
struct Grid<'a> {
    catalog: &'a Catalog,
    index: &'a BrowseIndex,
    cards: &'a [usize],
    definitions: &'a [Row<'a>],
    shelf: Shelf,
    folded: &'a HashSet<(Shelf, String)>,
    width: f32,
    card_width: f32,
    metrics: Metrics,
}

impl Grid<'_> {
    fn is_folded(&self, label: &str) -> bool {
        self.folded.contains(&(self.shelf, label.to_owned()))
    }

    /// Draws one line whose top is at `at`. A heading sits at the bottom of its line, under
    /// the gap that separates it from the line above.
    fn draw(&self, ui: &mut egui::Ui, line: &Line, at: egui::Pos2, height: f32) -> Option<Click> {
        let heading_rect = || {
            egui::Rect::from_min_size(
                at + egui::vec2(0.0, height - self.metrics.heading),
                egui::vec2(self.width, self.metrics.heading),
            )
        };
        match *line {
            Line::Group { start, count } => {
                let label = type_label(&self.index.entries[self.cards[start]]);
                let folded = self.is_folded(label);
                section_heading(ui, heading_rect(), self.metrics, label, count, Some(folded))
                    .clicked()
                    .then(|| Click::Fold(label.to_owned()))
            }
            Line::Cards { start, end } => self.card_row(ui, at, start, end),
            Line::Heading { count } => {
                section_heading(ui, heading_rect(), self.metrics, "Definitions", count, None);
                None
            }
            Line::Definition(row) => {
                let rect =
                    egui::Rect::from_min_size(at, egui::vec2(self.width, DEFINITION_ROW_HEIGHT));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                let row = &self.definitions[row];
                result_row(&mut child, self.catalog, row, false, DEFINITION_ROW_HEIGHT)
                    .clicked()
                    .then_some(Click::Open(row.target))
            }
        }
    }

    fn card_row(
        &self,
        ui: &mut egui::Ui,
        at: egui::Pos2,
        start: usize,
        end: usize,
    ) -> Option<Click> {
        let mut click = None;
        for (column, position) in self.cards[start..end].iter().enumerate() {
            let rect = egui::Rect::from_min_size(
                at + egui::vec2(
                    column as f32 * (self.card_width + self.metrics.column_gap),
                    0.0,
                ),
                egui::vec2(self.card_width, self.metrics.card_height),
            );
            let entry = &self.index.entries[*position];
            if card(ui, self.catalog, entry, rect, self.metrics.scale) {
                click = Some(Click::Open(Target::Hash(entry.hash)));
            }
        }
        click
    }
}

/// The lines in order, each with its height. Folded groups keep their heading.
fn lines(
    index: &BrowseIndex,
    cards: &[usize],
    definitions: usize,
    columns: usize,
    grouped: bool,
    metrics: Metrics,
    folded: impl Fn(&str) -> bool,
) -> Vec<(Line, f32)> {
    let mut lines = Vec::new();
    let push_cards = |lines: &mut Vec<(Line, f32)>, start: usize, end: usize| {
        let mut row = start;
        while row < end {
            let row_end = (row + columns).min(end);
            lines.push((
                Line::Cards {
                    start: row,
                    end: row_end,
                },
                metrics.card_pitch(),
            ));
            row = row_end;
        }
    };
    if grouped {
        let mut start = 0;
        while start < cards.len() {
            let label = type_label(&index.entries[cards[start]]);
            let end = start
                + cards[start..]
                    .iter()
                    .take_while(|position| type_label(&index.entries[**position]) == label)
                    .count();
            let gap = if start == 0 { 0.0 } else { metrics.heading_gap };
            lines.push((
                Line::Group {
                    start,
                    count: end - start,
                },
                metrics.heading + gap,
            ));
            if !folded(label) {
                push_cards(&mut lines, start, end);
            }
            start = end;
        }
    } else {
        push_cards(&mut lines, 0, cards.len());
    }
    if definitions > 0 {
        let gap = if cards.is_empty() {
            0.0
        } else {
            metrics.heading_gap
        };
        lines.push((Line::Heading { count: definitions }, metrics.heading + gap));
        for row in 0..definitions {
            lines.push((Line::Definition(row), DEFINITION_ROW_HEIGHT + 2.0));
        }
    }
    lines
}

/// A heading in spaced capitals over a rule, with its count after it, as the game heads a
/// column. One that folds is clickable, and dims with its rule while folded.
fn section_heading(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    metrics: Metrics,
    label: &str,
    count: usize,
    folded: Option<bool>,
) -> egui::Response {
    let scale = metrics.scale;
    let sense = if folded.is_some() {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let response = ui.interact(rect, ui.id().with(("browse_heading", label)), sense);
    let open = folded != Some(true);
    if folded.is_some() {
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, label)
        });
    }
    let lit = open || response.hovered();
    let text = ui.visuals().strong_text_color();
    let muted = look::muted(ui);
    let color = if lit {
        text.gamma_multiply(HEADING_LABEL_ALPHA)
    } else {
        muted
    };
    let font = item_art::text_font(ui, scale.body);
    let label_galley =
        item_art::spaced_capitals(ui, label, font.clone(), color, scale.px(TRACKING));
    // The label hangs half the padding under the top, and the rule closes the padded line.
    let top = rect.top() + scale.px(HEADING_PADDING) * 0.5;
    let rule_y = rect.top() + item_art::line_height(ui, &font) + scale.px(HEADING_PADDING) - 1.0;
    let line = item_art::line_height(ui, &font);
    let count_galley = ui.fonts(|fonts| fonts.layout_no_wrap(count.to_string(), font, muted));
    let painter = ui.painter();
    // A heading that folds leads with a chevron, pointing down while open and right while folded.
    let label_left = if folded.is_some() {
        let half = scale.px(CHEVRON_WIDTH) * 0.5;
        let rise = scale.px(CHEVRON_HEIGHT) * 0.5;
        let (x, y) = (rect.left() + half, top + line * 0.5);
        let points = if open {
            vec![
                egui::pos2(x - half, y - rise),
                egui::pos2(x + half, y - rise),
                egui::pos2(x, y + rise),
            ]
        } else {
            vec![
                egui::pos2(x - rise, y - half),
                egui::pos2(x + rise, y),
                egui::pos2(x - rise, y + half),
            ]
        };
        painter.add(egui::Shape::convex_polygon(
            points,
            color.gamma_multiply(CHEVRON_ALPHA),
            egui::Stroke::NONE,
        ));
        rect.left() + half * 2.0 + scale.px(HEADING_CHEVRON_GAP)
    } else {
        rect.left()
    };
    let label_width = label_galley.size().x;
    painter.galley(egui::pos2(label_left, top), label_galley, color);
    painter.galley(
        egui::pos2(label_left + label_width + scale.px(HEADING_COUNT_GAP), top),
        count_galley,
        muted,
    );
    let rule_alpha = if open {
        HEADING_RULE_ALPHA
    } else {
        HEADING_RULE_ALPHA * 0.5
    };
    painter.hline(
        rect.x_range(),
        rule_y,
        egui::Stroke::new(1.0, text.gamma_multiply(rule_alpha)),
    );
    if folded.is_some() {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// One item as the game rows it: a band of its rarity's colour with the icon struck flush into
/// its left edge, the name in capitals in the title cut, and the type and slot muted under it.
/// Returns whether it was opened, by a click or from its menu.
fn card(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    entry: &BrowseEntry,
    rect: egui::Rect,
    scale: Scale,
) -> bool {
    let response = ui.interact(
        rect,
        ui.id().with(("browse_card", entry.hash)),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &entry.name));
    ui.painter()
        .rect_filled(rect, 0.0, item_art::band_color(entry.rarity));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_white_alpha(26));
    }
    let icon = egui::Rect::from_min_size(rect.min, egui::Vec2::splat(rect.height()));
    item_art::icon(ui, catalog, entry.hash, entry.rarity, icon);

    // The name and the detail are centred as a pair against the icon, on Dear ImGui's line
    // heights. The pair keeps its place when an item has no detail line.
    let text_left = icon.right() + scale.px(CARD_TEXT_GAP);
    let text_width = (rect.right() - scale.px(CARD_TEXT_RIGHT_INSET) - text_left).max(0.0);
    let name_color = item_art::band_text(entry.rarity, false);
    let detail_color = item_art::band_text(entry.rarity, true);
    let name_font = item_art::title_font(ui, scale.body * CARD_NAME_SCALE);
    let detail_font = item_art::text_font(ui, scale.body * CARD_DETAIL_SCALE);
    let name_height = item_art::line_height(ui, &name_font);
    let line_gap = scale.px(CARD_LINE_GAP);
    let top = rect.top()
        + (rect.height() - name_height - line_gap - item_art::line_height(ui, &detail_font)) * 0.5;
    let name = item_art::single_line(
        ui,
        &item_art::shout(&entry.name),
        name_font,
        name_color,
        text_width,
    );
    let detail = detail(entry);
    let detail = (!detail.is_empty())
        .then(|| item_art::single_line(ui, &detail, detail_font, detail_color, text_width));
    let weight = item_art::strike(ui, GameFace::Title, scale.px(CARD_NAME_WEIGHT));
    let painter = ui.painter();
    item_art::bold(
        painter,
        egui::pos2(text_left, top),
        &name,
        name_color,
        weight,
    );
    if let Some(detail) = detail {
        painter.galley(
            egui::pos2(text_left, top + name_height + line_gap),
            detail,
            detail_color,
        );
    }
    let response = item_art::tooltip::on_hover(response, catalog, entry.hash)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut opened = false;
    response.context_menu(|ui| opened = card_menu(ui, catalog, entry));
    response.clicked() || opened
}

/// The card's menu: open it, add one to a character or the profile, preview its model, and copy
/// its name or hash. Returns whether Open was chosen.
fn card_menu(ui: &mut egui::Ui, catalog: &Catalog, entry: &BrowseEntry) -> bool {
    let open = ui.button("Open").clicked();
    add_menu(ui, catalog, entry);
    let preview = crate::app::item_editor::appearance::saved(catalog, entry.hash, None).is_some();
    if ui
        .add_enabled(preview, egui::Button::new("Model Preview"))
        .clicked()
    {
        crate::app::item_editor::request_model_preview(ui.ctx(), entry.hash, Default::default());
        ui.close_menu();
    }
    ui.separator();
    for (label, text) in [
        ("Copy Name", entry.name.clone()),
        ("Copy Hash (Hex)", format_hash_hex(entry.hash)),
        ("Copy Hash (Decimal)", entry.hash.to_string()),
    ] {
        if ui.button(label).clicked() {
            ui.ctx().copy_text(text);
            ui.close_menu();
        }
    }
    if open {
        ui.close_menu();
    }
    open
}

/// Add To, listing each character and the profile. A destination saving would refuse, such as a
/// character of another class, is listed but disabled, as is everything while the account
/// cannot be edited.
fn add_menu(ui: &mut egui::Ui, catalog: &Catalog, entry: &BrowseEntry) {
    requests::request_add_targets(ui.ctx());
    let targets = requests::add_targets(ui.ctx()).flatten();
    let fits = |class: Option<u8>, cross_class: bool| {
        class.is_some_and(|class| {
            catalog.fits_character_inventory(entry.hash, u64::from(class), cross_class)
        })
    };
    let to_profile = catalog
        .inventory_metadata(entry.hash)
        .is_some_and(|metadata| metadata.is_profile_items_candidate());
    let enabled = targets.as_ref().is_some_and(|targets| {
        to_profile
            || targets
                .characters
                .iter()
                .any(|class| fits(*class, targets.cross_class_subclasses))
    });
    ui.add_enabled_ui(enabled, |ui| {
        ui.menu_button("Add To", |ui| {
            let Some(targets) = targets else {
                return;
            };
            for (index, class) in targets.characters.iter().enumerate() {
                let label = class
                    .and_then(|class| CLASSES.get(usize::from(class)))
                    .map_or_else(
                        || format!("Character {}", index + 1),
                        |name| format!("Character {} · {name}", index + 1),
                    );
                let fits = fits(*class, targets.cross_class_subclasses);
                if ui.add_enabled(fits, egui::Button::new(label)).clicked() {
                    requests::request_add(ui.ctx(), entry.hash, AddDestination::Character(index));
                    ui.close_menu();
                }
            }
            if ui
                .add_enabled(to_profile, egui::Button::new("Profile"))
                .clicked()
            {
                requests::request_add(ui.ctx(), entry.hash, AddDestination::Profile);
                ui.close_menu();
            }
        });
    });
}

/// The type and the slot, or whichever of the two the item has.
fn detail(entry: &BrowseEntry) -> String {
    [Some(entry.type_name.as_str()), entry.slot]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(DETAIL_SEPARATOR)
}
