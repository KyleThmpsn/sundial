//! Read-only engine kinds and their decoded stock uses. Editing actions belong to the host.
use crate::{
    investment::{
        PerkSources,
        discovery::kinds::{Family, users},
    },
    sandbox_perk::{
        dependencies,
        nodes::{NodeKind, Support},
    },
};
use eframe::egui;

#[derive(Clone, Copy)]
pub enum Source<'a> {
    Loading,
    Unavailable,
    Ready(&'a dependencies::Index),
}
impl<'a> Source<'a> {
    fn index(self) -> Option<&'a dependencies::Index> {
        match self {
            Self::Ready(index) => Some(index),
            _ => None,
        }
    }
}
#[derive(Clone, Copy)]
pub enum UseLocation {
    Menu,
    Footer,
}
/// Optional host actions. A read-only catalog supplies None.
pub type UseActions<'a> = dyn FnMut(&mut egui::Ui, Option<u16>, UseLocation) + 'a;

#[derive(Default)]
pub struct Kinds {
    query: String,
    pub selected: Option<(Family, u8)>,
    authorable_only: bool,
    installed_only: bool,
    family: Option<Family>,
    use_query: String,
    selected_use: Option<u16>,
    sort: (UseColumn, bool),
    /// Whether the host has a Perk Effects tab to open examples on.
    pub reference_links: bool,
    /// Scroll the list to the selected kind on the next draw.
    reveal: bool,
    /// Stock use counts for the index they were counted from.
    counts: Option<Counts>,
    /// The selected kind's stock uses after the search, in table order.
    uses: UseRows,
}

/// An index's address, its perk list's address and its perk count.
type Identity = (usize, usize, usize);

fn identity(index: &dependencies::Index) -> Identity {
    (
        std::ptr::from_ref(index) as usize,
        index.perks.as_ptr() as usize,
        index.perks.len(),
    )
}

/// Stock use counts per kind, counted once per index.
struct Counts {
    index: Identity,
    /// Stock uses by kind number, effects then conditions.
    uses: [Vec<usize>; 2],
    /// Stock perks whose action was not fully decoded.
    unreadable: usize,
}

impl Counts {
    fn new(index: &dependencies::Index) -> Self {
        Self {
            index: identity(index),
            uses: Family::ALL.map(|family| {
                let mut uses = vec![0; 256];
                for node in family.nodes() {
                    uses[usize::from(node.kind)] = users(&index.perks, family, node.kind).len();
                }
                uses
            }),
            unreadable: index
                .perks
                .iter()
                .filter(|perk| {
                    perk.error.is_some() || (perk.action.is_some() && perk.behavior.is_none())
                })
                .count(),
        }
    }

    fn uses(&self, family: Family, kind: u8) -> usize {
        self.uses[family as usize][usize::from(kind)]
    }
}

/// What a kind's stock uses were read from: the index, the kind and the source names.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct UsesSource {
    index: Identity,
    kind: (usize, u8),
    sources: usize,
}

/// The selected kind's stock uses after the search, in table order.
#[derive(Default)]
struct UseRows {
    /// The source and search the rows were built from.
    built: Option<(UsesSource, String)>,
    /// The order the rows are in.
    sorted: Option<(UseColumn, bool)>,
    /// Stock uses before the search.
    total: usize,
    rows: Vec<StockUse>,
}

impl UseRows {
    /// Rebuilds the rows when the source or search changed, then applies the order.
    fn refresh(
        &mut self,
        source: UsesSource,
        query: &str,
        sort: (UseColumn, bool),
        rows: impl FnOnce() -> Vec<StockUse>,
    ) {
        if self
            .built
            .as_ref()
            .is_none_or(|(built, search)| *built != source || search != query)
        {
            let mut rows = rows();
            self.total = rows.len();
            rows.retain(|row| matches_use(row, query));
            self.rows = rows;
            self.built = Some((source, query.to_owned()));
            self.sorted = None;
        }
        self.sort(sort);
    }

    fn sort(&mut self, sort: (UseColumn, bool)) {
        if self.sorted != Some(sort) {
            sort_uses(&mut self.rows, sort.0, sort.1);
            self.sorted = Some(sort);
        }
    }
}

/// A column of the Stock Uses table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum UseColumn {
    #[default]
    Effect,
    Name,
    Description,
}

impl UseColumn {
    const ALL: [Self; 3] = [Self::Effect, Self::Name, Self::Description];

    fn label(self) -> &'static str {
        match self {
            Self::Effect => "Effect",
            Self::Name => "Referenced By",
            Self::Description => "Decoded Behavior",
        }
    }
}

/// One row of the Stock Uses table.
struct StockUse {
    index: u16,
    name: String,
    description: String,
}

fn usage_rows(perks: &[&dependencies::Perk], sources: &PerkSources) -> Vec<StockUse> {
    perks
        .iter()
        .filter_map(|perk| {
            let index = u16::try_from(perk.index).ok()?;
            let behavior = perk.behavior.as_ref()?;
            Some(StockUse {
                index,
                name: sources.summary(perk.index),
                description: behavior.headline.clone(),
            })
        })
        .collect()
}

/// Sorts one kind's rows by the chosen column. The effect column sorts by index, the others
/// by their text, and the index breaks every tie so the order is stable.
fn sort_uses(rows: &mut [StockUse], column: UseColumn, descending: bool) {
    rows.sort_by_cached_key(|row| {
        let text = match column {
            UseColumn::Effect => String::new(),
            UseColumn::Name => row.name.to_lowercase(),
            UseColumn::Description => row.description.to_lowercase(),
        };
        (text, row.index)
    });
    if descending {
        rows.reverse();
    }
}

impl Kinds {
    /// Selected stock configuration, separate from the selected kind.
    pub fn selected_stock_use(&self) -> Option<u16> {
        self.selected_use
    }
    /// Show one kind, clearing any filter that would hide it.
    pub fn open(&mut self, family: Family, kind: u8) {
        self.query.clear();
        self.authorable_only = false;
        self.installed_only = false;
        self.family = None;
        self.select(family, kind);
        self.reveal = true;
    }

    /// Draws the tab. Returns an effect number the reader asked to open in Perk Effects.
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        sources: &PerkSources,
        source: Source<'_>,
        actions: &mut Option<&mut UseActions<'_>>,
    ) -> Option<u16> {
        ui.horizontal_wrapped(|ui| {
            super::search(ui, &mut self.query, false, 220.0, "Search Kinds");
            ui.checkbox(&mut self.authorable_only, "Authorable Only")
                .on_hover_text("Supported by the private perk editor");
            ui.add_enabled(
                source.index().is_some(),
                egui::Checkbox::new(&mut self.installed_only, "With Stock Uses"),
            );
        });
        self.refresh_counts(source.index());
        let unreadable = source
            .index()
            .and(self.counts.as_ref())
            .map_or(0, |counts| counts.unreadable);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.family, None, "All Kinds");
            for family in Family::ALL {
                ui.selectable_value(&mut self.family, Some(family), family.label());
            }
            if unreadable > 0 {
                ui.weak(format!(
                    "{unreadable} stock {} not decoded",
                    if unreadable == 1 { "perk" } else { "perks" }
                ));
            }
        });
        let rows = self.visible_kinds(source.index());
        if !rows
            .iter()
            .any(|(family, node, _)| self.selected == Some((*family, node.kind)))
        {
            if let Some((family, node, _)) = rows.first() {
                self.select(*family, node.kind);
            } else {
                self.selected = None;
                self.selected_use = None;
            }
        }
        ui.separator();
        let height = (ui.available_height() - 8.0).max(120.0);
        let narrow = ui.available_width() < 760.0;
        let list_width = if narrow {
            ui.available_width()
        } else {
            (ui.available_width() * 0.36).clamp(260.0, 380.0)
        };
        let list_height = if narrow { height * 0.36 } else { height };
        let layout = if narrow {
            egui::Layout::top_down(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Min)
        };
        let mut open = None;
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), layout, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(list_width, list_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_max_width(list_width);
                    egui::ScrollArea::vertical()
                        .id_salt("engine-catalog-kinds")
                        .max_height(list_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.draw_kinds(ui, &rows));
                },
            );
            ui.separator();
            let detail_height = if narrow {
                (height - list_height - 12.0).max(80.0)
            } else {
                height
            };
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), detail_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_max_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_salt((
                            "engine-catalog-details",
                            self.selected.map(|(family, kind)| (family.label(), kind)),
                        ))
                        .max_height(detail_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            open = self.draw_details(ui, sources, source, actions);
                        });
                },
            );
        });
        open
    }

    /// Counts each kind's stock uses once per index.
    fn refresh_counts(&mut self, index: Option<&dependencies::Index>) {
        if let Some(index) = index
            && self
                .counts
                .as_ref()
                .is_none_or(|counts| counts.index != identity(index))
        {
            self.counts = Some(Counts::new(index));
        }
    }

    fn visible_kinds(
        &mut self,
        index: Option<&dependencies::Index>,
    ) -> Vec<(Family, &'static NodeKind, Option<usize>)> {
        self.refresh_counts(index);
        let counts = index.and(self.counts.as_ref());
        let query = self.query.trim().to_lowercase();
        Family::ALL
            .into_iter()
            .filter(|family| self.family.is_none_or(|selected| selected == *family))
            .flat_map(|family| family.nodes().iter().map(move |node| (family, node)))
            .filter_map(|(family, node)| {
                let count = counts.map(|counts| counts.uses(family, node.kind));
                (matches_query(node, &query)
                    && (!self.authorable_only || node.support == Support::Authorable)
                    && (!self.installed_only || count.is_none_or(|count| count > 0)))
                .then_some((family, node, count))
            })
            .collect()
    }

    fn draw_kinds(
        &mut self,
        ui: &mut egui::Ui,
        rows: &[(Family, &'static NodeKind, Option<usize>)],
    ) {
        if rows.is_empty() {
            ui.strong("No Matching Kinds");
            ui.label("Clear the search or change a filter.");
            return;
        }
        for family in Family::ALL {
            let count = rows
                .iter()
                .filter(|(row_family, _, _)| *row_family == family)
                .count();
            if count == 0 {
                continue;
            }
            ui.strong(format!("{} · {count}", family.label()));
            let widths = table_widths(ui, &[40.0, 80.0]);
            table_header(ui, &widths, &["Kind", "Name", "Stock Uses"], None);
            for (_, node, count) in rows
                .iter()
                .filter(|(row_family, _, _)| *row_family == family)
            {
                let selected = self.selected == Some((family, node.kind));
                let response = table_row(
                    ui,
                    &widths,
                    selected,
                    &[
                        egui::RichText::new(node.kind.to_string()),
                        crate::ui_help::emphasized_text(ui, node.name),
                        egui::RichText::new(
                            count.map_or_else(|| "…".into(), |count| count.to_string()),
                        ),
                    ],
                )
                .on_hover_text(format!(
                    "{}\n{}\n{}",
                    node.name,
                    node.summary,
                    node.support.detail()
                ));
                if selected && self.reveal {
                    response.scroll_to_me_animation(
                        Some(egui::Align::Center),
                        egui::style::ScrollAnimation::none(),
                    );
                    self.reveal = false;
                }
                if response.clicked() {
                    self.select(family, node.kind);
                }
            }
            ui.add_space(6.0);
        }
    }

    fn select(&mut self, family: Family, kind: u8) {
        if self.selected != Some((family, kind)) {
            self.selected_use = None;
            self.use_query.clear();
        }
        self.selected = Some((family, kind));
    }

    fn draw_details(
        &mut self,
        ui: &mut egui::Ui,
        sources: &PerkSources,
        source: Source<'_>,
        actions: &mut Option<&mut UseActions<'_>>,
    ) -> Option<u16> {
        let (family, kind) = self.selected?;
        let node = family.nodes().iter().find(|node| node.kind == kind)?;
        ui.horizontal_wrapped(|ui| {
            ui.heading(node.name);
            ui.scope(|ui| {
                let body = egui::TextStyle::Body.resolve(ui.style());
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Small, body);
                super::support_badge(ui, node.support);
            });
        });
        ui.label(node.summary);
        ui.label(format!("Compiler: {}", node.support.label()));
        ui.label(format!("Semantics: {}", node.semantics()));
        ui.weak(format!(
            "Evidence: Client {} · {} Surveyed Occurrences",
            crate::sandbox_perk::nodes::CLIENT_BUILD,
            node.occurrences
        ));
        ui.weak("Gameplay: Unverified for the current perk and destination. Record results in the workbench.");
        egui::CollapsingHeader::new("Technical Details")
            .id_salt((family.label(), kind))
            .show(ui, |ui| {
                ui.label(format!(
                    "{} Kind {}",
                    family.label().trim_end_matches('s'),
                    node.kind
                ));
                if node.class != 0 {
                    ui.monospace(format!(
                        "Class 0x{:08X} · {} bytes",
                        node.class, node.struct_size
                    ));
                }
                ui.label(node.support.detail());
                ui.label(node.evidence);
            });
        ui.add_space(8.0);
        let Some(index) = source.index() else {
            ui.strong("Stock Uses");
            if matches!(source, Source::Loading) {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading native content…");
                });
            } else {
                ui.weak("Native Content Unavailable");
            }
            return None;
        };
        self.refresh_counts(Some(index));
        let count = self
            .counts
            .as_ref()
            .map_or(0, |counts| counts.uses(family, kind));
        ui.strong(match count {
            0 => "No Stock Uses".to_owned(),
            1 => "1 Stock Use".to_owned(),
            count => format!("{count} Stock Uses"),
        });
        if count == 0 {
            return None;
        }
        let uses = UsesSource {
            index: identity(index),
            kind: (family as usize, kind),
            sources: std::ptr::from_ref(sources) as usize,
        };
        let open = self.draw_uses(
            ui,
            uses,
            || usage_rows(&users(&index.perks, family, kind), sources),
            sources,
            actions,
        );
        if let Some(selected) = self.selected_use {
            super::draw_sources(ui, sources, usize::from(selected));
        }
        open
    }

    /// The Stock Uses table. Clicking a header sorts by that column and again reverses it.
    /// Returns an effect the reader asked to open in Perk Effects.
    fn draw_uses(
        &mut self,
        ui: &mut egui::Ui,
        source: UsesSource,
        rows: impl FnOnce() -> Vec<StockUse>,
        sources: &PerkSources,
        actions: &mut Option<&mut UseActions<'_>>,
    ) -> Option<u16> {
        let mut open = None;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 65.0).max(120.0);
            super::search(ui, &mut self.use_query, false, width, "Search Stock Uses");
        });
        self.uses.refresh(source, &self.use_query, self.sort, rows);
        let total = self.uses.total;
        self.selected_use = self
            .selected_use
            .filter(|selected| self.uses.rows.iter().any(|row| row.index == *selected));
        ui.horizontal_wrapped(|ui| {
            ui.weak(format!(
                "{} of {total} {}",
                self.uses.rows.len(),
                if total == 1 {
                    "stock use"
                } else {
                    "stock uses"
                }
            ));
            if self.reference_links
                && ui
                    .add_enabled(
                        self.selected_use.is_some(),
                        egui::Button::new("Open in Perk Effects"),
                    )
                    .clicked()
            {
                open = self.selected_use;
            }
            if let Some(actions) = actions.as_deref_mut() {
                actions(ui, self.selected_use, UseLocation::Footer);
            }
        });
        if self.uses.rows.is_empty() {
            ui.label("No Matching Stock Uses");
            return open;
        }
        if let Some(row) = self
            .uses
            .rows
            .iter()
            .find(|row| Some(row.index) == self.selected_use)
        {
            ui.strong(format!("Effect {} · {}", row.index, row.name));
            ui.label(&row.description);
        }
        let name_width = (ui.available_width() * 0.32).clamp(120.0, 220.0);
        let widths = table_widths(ui, &[72.0, name_width]);
        let (current, descending) = self.sort;
        let titles = UseColumn::ALL.map(|column| {
            if current == column {
                format!("{} {}", column.label(), if descending { "⬇" } else { "⬆" })
            } else {
                column.label().to_owned()
            }
        });
        let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
        if let Some(column) = table_header(
            ui,
            &widths,
            &titles,
            Some("Sort by this column. Click again to reverse."),
        ) {
            let column = UseColumn::ALL[column];
            self.sort = (column, current == column && !descending);
            self.uses.sort(self.sort);
        }
        // The table takes the panel; only the source list below it needs a line.
        let height = (ui.available_height() - 44.0).max(80.0);
        let row_height = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .id_salt("installed-examples")
            .max_height(height)
            .auto_shrink([false, false])
            .show_rows(ui, row_height, self.uses.rows.len(), |ui, range| {
                for row in &self.uses.rows[range] {
                    let index = row.index;
                    let selected = self.selected_use == Some(index);
                    let response = table_row(
                        ui,
                        &widths,
                        selected,
                        &[
                            egui::RichText::new(index.to_string()),
                            crate::ui_help::emphasized_text(ui, &row.name),
                            egui::RichText::new(&row.description).weak(),
                        ],
                    )
                    .on_hover_text(format!(
                        "{}\n{}",
                        sources.details(usize::from(index)),
                        row.description
                    ));
                    if response.clicked() {
                        self.selected_use = Some(index);
                    }
                    if self.reference_links && response.double_clicked() {
                        open = Some(index);
                    }
                    response.context_menu(|ui| {
                        if self.reference_links && ui.button("Open in Perk Effects").clicked() {
                            open = Some(index);
                            ui.close_menu();
                        }
                        if let Some(actions) = actions.as_deref_mut() {
                            actions(ui, Some(index), UseLocation::Menu);
                        }
                    });
                }
            });
        open
    }
}

/// Column widths for fixed columns followed by one column that takes the rest.
fn table_widths(ui: &egui::Ui, fixed: &[f32]) -> Vec<f32> {
    let gaps = ui.spacing().item_spacing.x * fixed.len() as f32;
    let rest = (ui.available_width() - fixed.iter().sum::<f32>() - gaps).max(80.0);
    let mut widths = fixed.to_vec();
    widths.insert(1, rest);
    widths
}

/// A header row aligned to the same columns. Returns the clicked column when sortable.
fn table_header(
    ui: &mut egui::Ui,
    widths: &[f32],
    labels: &[&str],
    sort_hint: Option<&str>,
) -> Option<usize> {
    let height = ui.spacing().interact_size.y;
    let gap = ui.spacing().item_spacing.x;
    let width = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    let sense = if sort_hint.is_some() {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    let mut clicked = None;
    if ui.is_rect_visible(rect) {
        let mut x = rect.left();
        for (index, (label, width)) in labels.iter().zip(widths).enumerate() {
            let cell =
                egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*width, height));
            let hovered = response.hovered()
                && ui
                    .input(|input| input.pointer.hover_pos())
                    .is_some_and(|pos| cell.contains(pos));
            let text = egui::RichText::new(*label).strong();
            let text = if hovered && sort_hint.is_some() {
                text.underline()
            } else {
                text
            };
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(cell.shrink2(egui::vec2(4.0, 0.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            )
            .add(egui::Label::new(text).truncate().selectable(false));
            if hovered && response.clicked() {
                clicked = Some(index);
            }
            x += width + gap;
        }
    }
    if let Some(hint) = sort_hint {
        response.on_hover_text(hint);
    }
    clicked
}

/// One selectable table row: a single highlight across every column, each cell truncated to
/// its column so nothing drifts.
fn table_row(
    ui: &mut egui::Ui,
    widths: &[f32],
    selected: bool,
    cells: &[egui::RichText],
) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    let gap = ui.spacing().item_spacing.x;
    let width = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            cells
                .iter()
                .map(|cell| cell.text())
                .collect::<Vec<_>>()
                .join(" · "),
        )
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() {
            ui.painter().rect(
                rect,
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let mut x = rect.left();
        for (cell, width) in cells.iter().zip(widths) {
            let cell_rect = egui::Rect::from_min_size(
                egui::pos2(x + 4.0, rect.top()),
                egui::vec2((width - 8.0).max(0.0), height),
            );
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(cell_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            )
            .add(egui::Label::new(cell.clone()).truncate().selectable(false));
            x += width + gap;
        }
    }
    response
}

fn matches_use(row: &StockUse, query: &str) -> bool {
    let text = format!("{} {} {}", row.index, row.name, row.description).to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}

/// Whether a kind matches every word of a search: its number, name or summary.
fn matches_query(node: &NodeKind, query: &str) -> bool {
    let text = format!("{} {} {}", node.kind, node.name, node.summary).to_lowercase();
    query.split_whitespace().all(|word| text.contains(word))
}

#[cfg(test)]
mod tests;
