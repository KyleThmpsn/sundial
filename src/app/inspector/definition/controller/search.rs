//! Definition search: the toolbar field with its dropdown of hits, and the home page, which
//! browses the catalog's items by shelf and lists recently opened definitions.

mod browse;

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

use eframe::egui;

use crate::{
    app::inspector::{look, metadata::parse_hash_text},
    catalog::{Catalog, DefinitionSearchHit, Shelf},
    hash::format_hash_hex,
};

use super::super::state::{
    Browse, BrowseTab, DefinitionSearch, HOME, HashInspectionState, SearchResults,
};
use super::{
    HashInspectorAction, InspectorWindow, apply_navigation, definition_title, inspector_base_id,
    navigation_buttons, navigation_hashes, navigation_input, search_shortcut,
    show_inspector_window, toolbar,
};

/// Hits kept per query. The home page lists them all.
const RESULT_LIMIT: usize = 200;
const DROPDOWN_LIMIT: usize = 12;
const DROPDOWN_WIDTH: f32 = 560.0;
const DROPDOWN_ROW_HEIGHT: f32 = 28.0;
const TOOLBAR_FIELD_WIDTH: f32 = 280.0;
const HOME_ROW_HEIGHT: f32 = 34.0;
/// The widest kind chip, so names line up.
const WIDEST_KIND: &str = "Presentation Node";
const KIND_COLUMN_GAP: f32 = 4.0;
/// The search hint. The field also opens a pasted hash.
const HINT: &str = "Search…";

/// Where a result row leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Hash(u64),
    History(usize),
}

/// One row of hits or recent definitions.
struct Row<'a> {
    target: Target,
    hash: u64,
    kind: &'a str,
    name: Cow<'a, str>,
    detail: Cow<'a, str>,
    /// The item whose icon leads the row.
    icon: Option<u64>,
}

#[derive(Default)]
struct Keys {
    moved: bool,
    enter: bool,
}

/// The toolbar's search field. Typing opens a dropdown of hits under it, arrow keys move the
/// highlight, Enter opens it or a pasted hash, and Escape closes the dropdown.
pub(super) fn toolbar_search(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    base_id: egui::Id,
    search: &mut DefinitionSearch,
    action: &mut HashInspectorAction,
) {
    let field_id = base_id.with("toolbar_search");
    let popup_id = base_id.with("toolbar_search_results");
    let keys = if ui.memory(|memory| memory.has_focus(field_id)) {
        let rows = row_count(&search.results, DROPDOWN_LIMIT);
        navigation_keys(ui, &mut search.results.highlighted, rows)
    } else {
        Keys::default()
    };
    let response = ui.add(
        egui::TextEdit::singleline(&mut search.query)
            .id(field_id)
            .hint_text(HINT)
            .desired_width(TOOLBAR_FIELD_WIDTH),
    );
    if std::mem::take(&mut search.focus) {
        response.request_focus();
    }
    refresh(
        &mut search.results,
        catalog,
        &search.query,
        Catalog::search_definitions,
    );
    if search.query.trim().is_empty() {
        close_popup(ui, popup_id);
    } else if response.has_focus() && (response.changed() || response.gained_focus() || keys.moved)
    {
        egui::Popup::open_id(ui, popup_id);
    }
    let rows = search_rows(
        catalog,
        &search.results.hits,
        search.results.hash,
        DROPDOWN_LIMIT,
    );
    let highlighted = search.results.highlighted;
    let clicked = crate::ui::dropdown(&response, popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(response.rect.width());
            dropdown(ui, catalog, &rows, highlighted)
        })
        .and_then(|popup| popup.inner);
    let chosen = clicked.or_else(|| {
        keys.enter
            .then(|| chosen_target(&rows, highlighted, &search.query))
            .flatten()
    });
    if let Some(Target::Hash(hash)) = chosen {
        action.open_hash = Some(hash);
        close_popup(ui, popup_id);
        ui.memory_mut(|memory| memory.surrender_focus(field_id));
    }
}

/// The window with no definition open: the shelves of items and recent definitions.
pub(super) fn draw_search_window(
    ctx: &egui::Context,
    catalog: &Catalog,
    state: &mut HashInspectionState,
    viewport_salt: &'static str,
) {
    let default_size = *state
        .default_size
        .get_or_insert_with(|| egui::vec2(1_280.0, 880.0));
    let history = navigation_hashes(&state.history);
    let forward = navigation_hashes(&state.forward);
    let window = InspectorWindow {
        title: "Inspector",
        default_size,
        viewport_salt,
        focus: state.search.focus,
    };
    let base_id = inspector_base_id(viewport_salt);
    let (action, close_requested) = show_inspector_window(ctx, &window, |ui, action| {
        draw_home(
            ui,
            catalog,
            base_id,
            &history,
            &forward,
            &mut state.search,
            &mut state.browse,
            action,
        );
    });
    apply_navigation(state, action, close_requested);
}

#[allow(clippy::too_many_arguments)]
fn draw_home(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    base_id: egui::Id,
    history: &[u64],
    forward: &[u64],
    search: &mut DefinitionSearch,
    browse: &mut Browse,
    action: &mut HashInspectorAction,
) {
    navigation_input(ui, history, forward, action);
    search_shortcut(ui, search);
    crate::ui::model_preview::inspected_unavailable(ui);
    toolbar(ui, |ui| {
        navigation_buttons(ui, catalog, history, forward, action);
    });
    ui.separator();
    match home_contents(ui, catalog, base_id, history, search, browse) {
        Some(Target::Hash(hash)) => action.open_hash = Some(hash),
        Some(Target::History(index)) => action.history_index = Some(index),
        None => {}
    }
}

/// The shelf tabs, the filter row with its search, and the shelf's cards or the recent
/// definitions.
fn home_contents(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    base_id: egui::Id,
    history: &[u64],
    search: &mut DefinitionSearch,
    browse: &mut Browse,
) -> Option<Target> {
    let field_id = base_id.with("home_search");
    let focused = ui.memory(|memory| memory.has_focus(field_id));
    let index = browse::index(browse, catalog);
    let searching = !search.home_query.trim().is_empty();
    browse::tabs(ui, browse, searching, recent_count(history));
    ui.add_space(6.0);
    let recent_tab = browse.tab == BrowseTab::Recent;
    let recent = if recent_tab {
        let query = search.home_query.trim().to_lowercase();
        let mut rows = recent_rows(catalog, history, &mut search.kinds);
        rows.retain(|row| query.is_empty() || row.name.to_lowercase().contains(&query));
        rows
    } else {
        Vec::new()
    };
    let keys = if focused && recent_tab {
        navigation_keys(ui, &mut search.results.highlighted, recent.len())
    } else if focused && searching && ui.is_enabled() {
        Keys {
            moved: false,
            enter: ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)),
        }
    } else {
        Keys::default()
    };
    let rebuilt = browse::refresh(browse, &index, &search.home_query);
    if !recent_tab {
        refresh(
            &mut browse.definitions,
            catalog,
            &search.home_query,
            Catalog::search_non_item_definitions,
        );
    }
    let narrowed = browse.narrowed(&search.home_query);
    let (response, reset) = browse::filter_row(ui, browse, narrowed, |ui, width, height| {
        home_field(
            ui,
            field_id,
            &mut search.home_query,
            focused,
            (width, height),
        )
    });
    if std::mem::take(&mut search.focus) {
        response.request_focus();
    }
    if response.changed() {
        search.results.highlighted = 0;
        ui.ctx().request_repaint();
    }
    if reset {
        search.home_query.clear();
        browse.reset();
    }
    // A filter chosen in the row applies to this frame's cards.
    let rebuilt = browse::refresh(browse, &index, &search.home_query) || rebuilt;
    ui.add_space(8.0);

    if recent_tab {
        if recent.is_empty() {
            look::empty_state(ui, "No Recent Definitions");
            return None;
        }
        let highlighted = search.results.highlighted;
        let clicked = result_list(
            ui,
            catalog,
            base_id,
            &recent,
            highlighted,
            keys.moved,
            &mut search.list_view,
        );
        return clicked.or_else(|| {
            keys.enter
                .then(|| chosen_target(&recent, highlighted, &search.home_query))
                .flatten()
        });
    }
    // Set aside while the rows borrow it, since the cards take the rest of the state.
    let found = std::mem::take(&mut browse.definitions);
    // Words such as "ace" read as hex, so a typed hash that names nothing yields to cards.
    let typed_hash = found.hash.filter(|hash| {
        definition_title(catalog, *hash).is_some() || browse.results.cards.is_empty()
    });
    // Definitions that are not items have no shelf of their own, so they sit on Other. A typed
    // hash leads every shelf.
    let hits: &[DefinitionSearchHit] = if browse.tab == BrowseTab::Shelf(Shelf::Other) {
        &found.hits
    } else {
        &[]
    };
    let definitions = if searching {
        search_rows(catalog, hits, typed_hash, RESULT_LIMIT)
    } else {
        Vec::new()
    };
    let clicked = browse::results(ui, catalog, base_id, browse, &index, &definitions, rebuilt);
    let chosen = clicked.or_else(|| {
        (keys.enter && searching)
            .then(|| entered_target(typed_hash, browse, &index, &definitions))
            .flatten()
    });
    browse.definitions = found;
    chosen
}

/// Enter opens a typed hash, else the first card, else the first other definition.
fn entered_target(
    hash: Option<u64>,
    browse: &Browse,
    index: &super::super::state::BrowseIndex,
    definitions: &[Row<'_>],
) -> Option<Target> {
    hash.map(Target::Hash)
        .or_else(|| {
            browse
                .results
                .cards
                .first()
                .map(|position| Target::Hash(index.entries[*position].hash))
        })
        .or_else(|| definitions.first().map(|row| row.target))
}

/// A rounded field with a search glyph, outlined while it has focus.
fn home_field(
    ui: &mut egui::Ui,
    id: egui::Id,
    query: &mut String,
    focused: bool,
    (width, height): (f32, f32),
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let visuals = ui.visuals();
    let stroke = if focused {
        visuals.selection.stroke
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(4),
        visuals.extreme_bg_color,
        stroke,
        egui::StrokeKind::Inside,
    );
    let mut field = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(8.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    field.spacing_mut().item_spacing.x = 6.0;
    field.label(
        egui::RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS).color(look::muted(ui)),
    );
    field.add(
        egui::TextEdit::singleline(query)
            .id(id)
            .frame(egui::Frame::NONE)
            .hint_text(HINT)
            .desired_width(f32::INFINITY),
    )
}

/// The home result rows. Only the rows in view are laid out, and the keyboard highlight is
/// scrolled into view when it moves.
fn result_list(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    base_id: egui::Id,
    rows: &[Row<'_>],
    highlighted: usize,
    reveal: bool,
    view: &mut (f32, f32),
) -> Option<Target> {
    ui.spacing_mut().item_spacing.y = 2.0;
    let step = HOME_ROW_HEIGHT + ui.spacing().item_spacing.y;
    let mut area = egui::ScrollArea::vertical()
        .id_salt(base_id.with("home_results"))
        .auto_shrink([false, false]);
    if reveal {
        let (offset, height) = *view;
        let top = highlighted as f32 * step;
        if top < offset {
            area = area.vertical_scroll_offset(top);
        } else if top + step > offset + height {
            area = area.vertical_scroll_offset(top + step - height);
        }
    }
    let output = area.show_rows(ui, HOME_ROW_HEIGHT, rows.len(), |ui, range| {
        let start = range.start;
        let mut chosen = None;
        for (offset, row) in rows[range].iter().enumerate() {
            let highlight = start + offset == highlighted;
            if result_row(ui, catalog, row, highlight, HOME_ROW_HEIGHT).clicked() {
                chosen = Some(row.target);
            }
        }
        chosen
    });
    *view = (output.state.offset.y, output.inner_rect.height());
    output.inner
}

fn dropdown(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    rows: &[Row<'_>],
    highlighted: usize,
) -> Option<Target> {
    ui.set_width(DROPDOWN_WIDTH);
    if rows.is_empty() {
        look::empty_state(ui, "No Matching Definitions");
        return None;
    }
    ui.spacing_mut().item_spacing.y = 1.0;
    let mut chosen = None;
    for (index, row) in rows.iter().enumerate() {
        if result_row(ui, catalog, row, index == highlighted, DROPDOWN_ROW_HEIGHT).clicked() {
            chosen = Some(row.target);
        }
    }
    chosen
}

/// One row: icon, kind chip, name and detail, and the hash at the right.
fn result_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    row: &Row<'_>,
    highlighted: bool,
    height: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let fill = if highlighted {
        ui.visuals().selection.bg_fill.gamma_multiply(0.4)
    } else if response.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(3), fill);
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(8.0, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    content.style_mut().interaction.selectable_labels = false;
    content.spacing_mut().item_spacing.x = 10.0;
    let muted = look::muted(&content);
    content.label(
        egui::RichText::new(format_hash_hex(row.hash))
            .monospace()
            .color(muted),
    );
    content.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        // Rows without an icon keep the slot, so kinds and names line up down the list.
        let (slot, _) =
            ui.allocate_exact_size(egui::Vec2::splat(height - 6.0), egui::Sense::hover());
        if let Some(icon) = row
            .icon
            .and_then(|hash| catalog.icon_texture(ui.ctx(), hash))
        {
            ui.painter()
                .rect_filled(slot, 0.0, crate::app::ui::package_icon_backdrop(ui));
            ui.painter().image(
                icon.id(),
                slot,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        let width = look::kind_chip_width(ui, WIDEST_KIND) + KIND_COLUMN_GAP;
        look::kind_chip_column(ui, row.kind, width);
        let text = name_and_detail(ui, &row.name, &row.detail);
        ui.add(egui::Label::new(text).truncate());
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The name in the Destiny font, with the detail muted after it at the same size.
fn name_and_detail(ui: &egui::Ui, name: &str, detail: &str) -> egui::text::LayoutJob {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let detail_font = egui::FontId::proportional(body.size);
    let mut job = egui::text::LayoutJob::default();
    job.append(
        name,
        0.0,
        egui::TextFormat::simple(
            crate::app::ui::destiny_font_id(ui, body),
            ui.visuals().strong_text_color(),
        ),
    );
    if !detail.is_empty() {
        job.append(
            detail,
            10.0,
            egui::TextFormat::simple(detail_font, look::muted(ui)),
        );
    }
    job
}

/// Arrow keys move the highlight and Enter picks it, before the text field sees them.
fn navigation_keys(ui: &mut egui::Ui, highlighted: &mut usize, rows: usize) -> Keys {
    if !ui.is_enabled() {
        return Keys::default();
    }
    let (down, up, enter) = ui.input_mut(|input| {
        (
            input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    if down {
        *highlighted = (*highlighted + 1).min(rows.saturating_sub(1));
    }
    if up {
        *highlighted = highlighted.saturating_sub(1);
    }
    Keys {
        moved: down || up,
        enter,
    }
}

fn close_popup(ui: &egui::Ui, popup_id: egui::Id) {
    if egui::Popup::is_id_open(ui, popup_id) {
        egui::Popup::close_id(ui, popup_id);
    }
}

/// The highlighted row, or the query itself when it reads as a hash.
fn chosen_target(rows: &[Row<'_>], highlighted: usize, query: &str) -> Option<Target> {
    rows.get(highlighted)
        .or_else(|| rows.first())
        .map(|row| row.target)
        .or_else(|| parse_hash_text(query).map(Target::Hash))
}

/// Recomputes the hits only when the query or the catalog changed since they were found.
fn refresh(
    results: &mut SearchResults,
    catalog: &Catalog,
    query: &str,
    search: fn(&Catalog, &str, usize) -> Vec<DefinitionSearchHit>,
) {
    let address = std::ptr::from_ref(catalog) as usize;
    let query = query.trim();
    if results
        .key
        .as_ref()
        .is_some_and(|(cached, text)| *cached == address && text == query)
    {
        return;
    }
    results.key = Some((address, query.to_owned()));
    results.hits = search(catalog, query, RESULT_LIMIT);
    results.hash = parse_hash_text(query)
        .filter(|hash| results.hits.is_empty() || definition_title(catalog, *hash).is_some());
    results.highlighted = 0;
}

fn row_count(results: &SearchResults, limit: usize) -> usize {
    usize::from(results.hash.is_some())
        + results
            .hits
            .iter()
            .filter(|hit| Some(hit.hash) != results.hash)
            .take(limit)
            .count()
}

/// A typed hash first, when it names a definition or nothing else matched, then the hits.
fn search_rows<'a>(
    catalog: &Catalog,
    hits: &'a [DefinitionSearchHit],
    hash: Option<u64>,
    limit: usize,
) -> Vec<Row<'a>> {
    let mut rows = Vec::with_capacity(limit + 1);
    if let Some(hash) = hash {
        rows.push(Row {
            target: Target::Hash(hash),
            hash,
            kind: "Hash",
            name: Cow::Owned(
                definition_title(catalog, hash).unwrap_or_else(|| "Open Hash".to_owned()),
            ),
            detail: Cow::Borrowed(""),
            icon: Some(hash),
        });
    }
    rows.extend(
        hits.iter()
            .filter(|hit| Some(hit.hash) != hash)
            .take(limit)
            .map(|hit| Row {
                target: Target::Hash(hit.hash),
                hash: hit.hash,
                kind: hit.kind,
                name: Cow::Borrowed(hit.name.as_str()),
                detail: Cow::Borrowed(hit.detail.as_str()),
                icon: hit.icon,
            }),
    );
    rows
}

fn recent_count(history: &[u64]) -> usize {
    history
        .iter()
        .filter(|hash| **hash != HOME)
        .collect::<HashSet<_>>()
        .len()
}

/// Opened definitions, most recent first and each once.
fn recent_rows(
    catalog: &Catalog,
    history: &[u64],
    kinds: &mut HashMap<u64, (&'static str, u64)>,
) -> Vec<Row<'static>> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for (index, &hash) in history.iter().enumerate().rev() {
        if hash == HOME || !seen.insert(hash) {
            continue;
        }
        let (kind, icon) = *kinds.entry(hash).or_insert_with(|| {
            let kind = quick_kind(catalog, hash);
            (kind, icon_hash(catalog, hash, kind))
        });
        rows.push(Row {
            target: Target::History(index),
            hash,
            kind,
            name: Cow::Owned(
                definition_title(catalog, hash).unwrap_or_else(|| format_hash_hex(hash)),
            ),
            detail: Cow::Owned(
                catalog
                    .package_item_type_name(hash)
                    .unwrap_or_default()
                    .to_owned(),
            ),
            icon: Some(icon),
        });
    }
    rows
}

/// The item whose icon stands for a definition: itself, or a collectible's item.
fn icon_hash(catalog: &Catalog, hash: u64, kind: &str) -> u64 {
    if kind != "Collectible" {
        return hash;
    }
    catalog
        .collectibles()
        .iter()
        .find(|collectible| collectible.hash == hash)
        .map_or(hash, |collectible| collectible.item_hash)
}

/// A kind label for a hash reached through history, trying the cheapest lookups first.
fn quick_kind(catalog: &Catalog, hash: u64) -> &'static str {
    let records = catalog.records().unwrap_or_default();
    if catalog.item_package_metadata(hash).is_some() || catalog.item(hash).is_some() {
        catalog.item_kind_label(hash)
    } else if catalog.item_stat_definition_by_hash(hash).is_some() {
        "Item Stat"
    } else if catalog.presentation_node(hash).is_some() {
        "Presentation Node"
    } else if catalog.items_for_bucket(hash).next().is_some() {
        "Inventory Bucket"
    } else if catalog
        .collectibles()
        .iter()
        .any(|entry| entry.hash == hash)
    {
        "Collectible"
    } else if records.iter().any(|record| record.hash == hash) {
        "Record"
    } else if catalog
        .progression_definitions()
        .iter()
        .any(|entry| entry.hash == hash)
    {
        "Progression"
    } else if catalog.objectives().iter().any(|entry| entry.hash == hash) {
        "Objective"
    } else if catalog
        .unlock_flag_definitions()
        .iter()
        .any(|entry| entry.hash == hash)
    {
        "Unlock Flag"
    } else if catalog
        .unlock_value_definitions()
        .iter()
        .any(|entry| entry.hash == hash)
    {
        "Unlock Value"
    } else {
        "Definition"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_search_leaves_navigation_input_for_enabled_controls() {
        let catalog = Catalog::for_test(vec![], Default::default());
        let mut receipts = Vec::new();
        for toolbar_mode in [false, true] {
            let ctx = egui::Context::default();
            let base = egui::Id::new(("disabled-definition-search", toolbar_mode));
            let field = base.with(if toolbar_mode {
                "toolbar_search"
            } else {
                "home_search"
            });
            let mut search = DefinitionSearch {
                query: "0x00000042".into(),
                ..Default::default()
            };
            let frame = |search: &mut DefinitionSearch, enabled, keys: bool| {
                let mut chosen = None;
                let mut retained = false;
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900.0, 520.0),
                        )),
                        events: if keys {
                            vec![egui::Key::ArrowDown, egui::Key::Enter]
                                .into_iter()
                                .map(|key| egui::Event::Key {
                                    key,
                                    physical_key: None,
                                    pressed: true,
                                    repeat: false,
                                    modifiers: egui::Modifiers::NONE,
                                })
                                .collect()
                        } else {
                            Vec::new()
                        },
                        ..Default::default()
                    },
                    |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            ui.memory_mut(|memory| memory.request_focus(field));
                            ui.add_enabled_ui(enabled, |ui| {
                                if toolbar_mode {
                                    let mut action = HashInspectorAction::default();
                                    toolbar_search(ui, &catalog, base, search, &mut action);
                                    chosen = action.open_hash.map(Target::Hash);
                                } else {
                                    let mut browse = Browse {
                                        tab: BrowseTab::Recent,
                                        ..Default::default()
                                    };
                                    chosen = home_contents(
                                        ui,
                                        &catalog,
                                        base,
                                        &[10, 20, 30],
                                        search,
                                        &mut browse,
                                    );
                                }
                            });
                            retained = ui.input(|input| {
                                input.key_pressed(egui::Key::ArrowDown)
                                    && input.key_pressed(egui::Key::Enter)
                            });
                        });
                    },
                );
                crate::test_support::capture::record(&output);
                (chosen, retained, output)
            };
            let _ = frame(&mut search, true, false);
            let (chosen, retained, output) = frame(&mut search, false, true);
            assert_eq!(chosen, None);
            assert!(retained);
            assert_eq!(search.results.highlighted, 0);
            crate::test_support::capture::write(
                &ctx,
                &output,
                if toolbar_mode {
                    "disabled-toolbar-search"
                } else {
                    "disabled-home-search"
                },
            );
            let (chosen, _, _) = frame(&mut search, true, true);
            assert_eq!(
                chosen,
                Some(if toolbar_mode {
                    Target::Hash(0x42)
                } else {
                    Target::History(1)
                })
            );
            receipts.push(
                serde_json::json!({"toolbar": toolbar_mode, "disabled_input_retained": retained,
                "enabled_action": format!("{chosen:?}")}),
            );
        }
        crate::test_support::artifact("definition-search-input.json", &serde_json::json!(receipts));
    }

    #[test]
    fn keyboard_highlight_stays_within_the_rows() {
        let ctx = egui::Context::default();
        let press = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let mut highlighted = 0;
        for _ in 0..3 {
            let mut keys = Keys::default();
            let _ = ctx.run_ui(
                egui::RawInput {
                    events: vec![press(egui::Key::ArrowDown), press(egui::Key::ArrowDown)],
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        keys = navigation_keys(ui, &mut highlighted, 3);
                    });
                },
            );
            assert!(keys.moved);
        }
        assert_eq!(highlighted, 2, "the highlight stops at the last row");
    }

    #[test]
    fn a_typed_hash_leads_the_rows_once() {
        let hits = vec![
            DefinitionSearchHit {
                hash: 0xACE,
                name: "Ace".into(),
                kind: "Item",
                detail: String::new(),
                icon: Some(0xACE),
            },
            DefinitionSearchHit {
                hash: 7,
                name: "Ace of Spades".into(),
                kind: "Item",
                detail: "Hand Cannon".into(),
                icon: Some(7),
            },
        ];
        let catalog = Catalog::for_test(vec![], Default::default());
        let rows = search_rows(&catalog, &hits, Some(0xACE), DROPDOWN_LIMIT);
        let targets = rows.iter().map(|row| row.target).collect::<Vec<_>>();
        assert_eq!(targets, [Target::Hash(0xACE), Target::Hash(7)]);
        assert_eq!(chosen_target(&[], 0, "0x0000000A"), Some(Target::Hash(10)));
        assert_eq!(chosen_target(&rows, 5, ""), Some(Target::Hash(0xACE)));
        assert_eq!(recent_count(&[HOME, 1, 2, 1]), 2);
    }
}
