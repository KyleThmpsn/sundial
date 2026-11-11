//! The behavior picker drawn: its toolbar, the result status, the list and the detail pane.
use super::rows::{Filters, search_words};
use super::*;

/// Room for Show All on the toolbar line.
const SHOW_ALL_WIDTH: f32 = 90.0;
/// Room for the result count, which the list draws under this toolbar.
const COUNT_WIDTH: f32 = 115.0;

impl Purpose {
    pub(super) fn title(self) -> &'static str {
        match self {
            Purpose::Trigger => "Choose a Trigger",
            Purpose::Condition => "Choose a Condition",
            Purpose::Action => "Choose an Action",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Purpose::Trigger => "Use Trigger",
            Purpose::Condition => "Use Condition",
            Purpose::Action => "Add Action",
        }
    }
}

/// What the toolbar leaves for the list.
pub(super) struct Toolbar {
    pub(super) filters: Filters,
    /// Whether the picker opened or a choice or the query changed this frame.
    pub(super) reset: bool,
    /// The slot the result count fills once the results are known.
    pub(super) count: egui::Rect,
}

/// The search box and the filters, each starting from the choice the picker last kept.
///
/// The search box takes a line of its own at the picker's width, since four filters and the
/// search never shared one line in any window, and a query is what a reader types most. The
/// filters follow on a line that wraps.
pub(super) fn toolbar(
    ui: &mut egui::Ui,
    query: &mut String,
    opened: bool,
    purpose: Purpose,
) -> Toolbar {
    let mut filters = Filters {
        query: String::new(),
        stock: remembered(ui, stock_state(purpose), &StockUse::ALL),
        detail: remembered(ui, detail_state(purpose), &Detail::ALL),
        order: remembered(ui, order_state(purpose), &Order::ALL),
        category: remembered(ui, category_state(purpose), &Category::choices(purpose)),
        show_all: false,
    };
    let mut reset = opened;
    reset |= controls::search_line(ui, |ui, width| {
        sundial::ui::catalog::search(ui, query, opened, width, "Search Behaviors")
    });
    let (changed, count) = ui
        .horizontal_wrapped(|ui| filter_row(ui, purpose, &mut filters))
        .inner;
    reset |= changed;
    filters.query = search_words(query);
    Toolbar {
        filters,
        reset,
        count,
    }
}

/// A combo box places itself at the cursor without asking the wrapping layout, so one that
/// would run past the edge starts the next line instead. The remaining width on the line is
/// `available_rect_before_wrap`: `available_width` reports the whole row inside a wrapping
/// layout.
fn fit(ui: &mut egui::Ui, width: f32) {
    if ui.available_rect_before_wrap().width() < width + ui.spacing().item_spacing.x {
        ui.end_row();
    }
}

/// The filters, Show All and the result count's slot, on a line that wraps. Returns whether a
/// choice changed, and the slot.
fn filter_row(ui: &mut egui::Ui, purpose: Purpose, filters: &mut Filters) -> (bool, egui::Rect) {
    let mut changed = choose(ui, purpose, filters);
    let id = show_all_state(purpose);
    filters.show_all = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    fit(ui, SHOW_ALL_WIDTH);
    changed |= ui
        .checkbox(&mut filters.show_all, "Show All")
        .on_hover_text("Include unnamed behaviors.")
        .changed();
    ui.data_mut(|data| data.insert_temp(id, filters.show_all));
    fit(ui, COUNT_WIDTH);
    let count = ui
        .allocate_space(egui::vec2(COUNT_WIDTH, ui.spacing().interact_size.y))
        .1;
    (changed, count)
}

/// The four filters, each kept for the next frame under its own slot. A combo takes the width
/// of its selected text, so each is drawn inside an allocation of the shared filter width and
/// truncates to it, with the full reading on hover. A row of equal controls reads as one
/// toolbar. Returns whether a choice changed.
fn choose(ui: &mut egui::Ui, purpose: Purpose, filters: &mut Filters) -> bool {
    const WIDTH: f32 = guidance::FILTER_WIDTH;
    let categories = Category::choices(purpose);
    let category_text = filters.category.label();
    let mut changed = controls::filter(
        ui,
        ("behavior-category", "Category"),
        WIDTH,
        (category_text, &format!("Category: {category_text}")),
        &mut filters.category,
        categories
            .iter()
            .map(|choice| (*choice, choice.label(), choice.hint())),
    );
    fit(ui, WIDTH);
    let detail_text = filters.detail.label();
    changed |= controls::filter(
        ui,
        ("behavior-detail", "Detail"),
        WIDTH,
        (detail_text, &format!("Detail: {detail_text}")),
        &mut filters.detail,
        Detail::ALL.map(|choice| (choice, choice.label(), choice.hint())),
    );
    fit(ui, WIDTH);
    let stock_text = filters.stock.label();
    changed |= controls::filter(
        ui,
        ("behavior-stock", "Stock Use"),
        WIDTH,
        (stock_text, &format!("Stock Use: {stock_text}")),
        &mut filters.stock,
        StockUse::ALL.map(|choice| (choice, choice.label(), choice.hint())),
    );
    fit(ui, WIDTH);
    let order_text = format!("Sort: {}", filters.order.label());
    changed |= controls::filter(
        ui,
        ("behavior-order", "Sort Order"),
        WIDTH,
        (&order_text, &order_text),
        &mut filters.order,
        Order::ALL.map(|choice| (choice, choice.label(), choice.hint())),
    );
    ui.data_mut(|data| {
        data.insert_temp(
            stock_state(purpose),
            index_of(&StockUse::ALL, &filters.stock),
        );
        data.insert_temp(
            detail_state(purpose),
            index_of(&Detail::ALL, &filters.detail),
        );
        data.insert_temp(order_state(purpose), index_of(&Order::ALL, &filters.order));
        data.insert_temp(
            category_state(purpose),
            index_of(&categories, &filters.category),
        );
    });
    changed
}

/// The result count in the toolbar's slot, or why there are no results yet. A failed read
/// shows its reason and a way to try again. Returns whether Retry was clicked.
pub(super) fn status(
    ui: &mut egui::Ui,
    loaded: Option<&Loaded>,
    discovery: &discovery::Discovery,
    count: egui::Rect,
    results: usize,
) -> bool {
    let text = match loaded {
        None if discovery.packages().is_none() => "No Game Content".to_owned(),
        None => format!("{results} · Reading…"),
        Some(Err(_)) => "Read Failed".to_owned(),
        _ => sundial::ui::catalog::result_count(results),
    };
    let response = sundial::ui::catalog::toolbar_status(ui, count, text);
    match loaded {
        Some(Err(error)) => return failed(ui, response, error),
        Some(Ok(catalog)) if !catalog.errors.is_empty() => {
            response.on_hover_text(format!(
                "{} action resources could not be read.\n{}",
                catalog.errors.len(),
                catalog.errors.join("\n")
            ));
        }
        _ => {}
    }
    false
}

/// The reason and a way to try again, where the count would be.
fn failed(ui: &mut egui::Ui, response: egui::Response, error: &str) -> bool {
    response.on_hover_text(error);
    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                .truncate(),
        );
        ui.button("Retry").clicked()
    })
    .inner
}

/// What the list draws besides its groups.
pub(super) struct List<'a> {
    pub(super) purpose: Purpose,
    /// The picker button's text, which is the title of the behavior in use.
    pub(super) label: &'a str,
    pub(super) opened: bool,
    pub(super) reset: bool,
    pub(super) catalog: Option<&'a native::Catalog>,
    pub(super) names: &'a BTreeMap<u16, String>,
}

/// The behaviors and the detail pane of the selected one. Returns what the pane chose.
pub(super) fn list(ui: &mut egui::Ui, groups: &[Group], list: &List) -> Option<Selection> {
    let keys = groups
        .iter()
        .map(|group| egui::Id::new((list.purpose as u8, group.family)).value())
        .collect::<Vec<_>>();
    // The picker opens on the condition in use. Its button carries the same title the row does,
    // since the rows are named as the cards name them.
    let select = list
        .opened
        .then(|| groups.iter().position(|group| group.title == list.label))
        .flatten()
        .map(|index| keys[index]);
    controls::browser_list(ui, &keys, list.reset, select).draw_body_activating(
        ui,
        |ui, index, selected| draw_row(ui, &groups[index], selected),
        |ui, index, activated| details(ui, &groups[index], list, activated),
        0,
    )
}

/// A behavior's row: its title over its first configuration's sentence.
fn draw_row(ui: &mut egui::Ui, group: &Group, selected: bool) -> egui::Response {
    sundial::investment::draw_asset_choice_row(
        ui,
        group.title,
        distinct_detail(&group.rows[0].detail, group.title),
        selected,
    )
}

/// The selected behavior: its configuration, what it does, stock examples and the command
/// that uses it.
fn details(ui: &mut egui::Ui, group: &Group, list: &List, activated: bool) -> Option<Selection> {
    ui.heading(group.title);
    let row = configuration(ui, group, list.catalog);
    ui.add_space(4.0);
    ui.add(egui::Label::new(distinct_detail(&row.detail, group.title)).wrap());
    if let Some(catalog) = list.catalog {
        examples(ui, group, catalog, list.names);
    }
    ui.add_space(4.0);
    // A double-click uses the row's selected configuration, as the button does, and is refused
    // where the button is.
    let used = controls::command(
        ui,
        row.enabled,
        list.purpose.command(),
        Some(row.reason),
        activated,
    );
    // A refused row says why where the button is, since a tooltip never opens inside the
    // picker's own popup.
    if !row.enabled {
        ui.add(egui::Label::new(egui::RichText::new(row.reason).weak()).wrap());
    }
    chosen(ui, row, list, used)
}

/// Installed perks that use the behavior, the first three by name.
fn examples(
    ui: &mut egui::Ui,
    group: &Group,
    catalog: &native::Catalog,
    names: &BTreeMap<u16, String>,
) {
    let examples = stock_examples(group, catalog, names);
    if examples.is_empty() {
        return;
    }
    let text = format!(
        "Stock examples: {}",
        examples
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
    ui.add(egui::Label::new(egui::RichText::new(&text).weak()).wrap())
        .on_hover_text(format!("{text}\nInstalled perks that use this behavior. Their exact settings can differ from the selected configuration."));
}

/// What a used row selects. A stock configuration also shows its requirements and technical
/// details, used or not.
fn chosen(ui: &mut egui::Ui, row: &Row, list: &List, used: bool) -> Option<Selection> {
    match &row.choice {
        Choice::Condition(index) => list
            .catalog
            .and_then(|catalog| condition(ui, &catalog.conditions[*index], list.names, used)),
        Choice::Effect(index) => list
            .catalog
            .and_then(|catalog| effect(ui, &catalog.effects[*index], list.names, used)),
        _ if !used => None,
        Choice::Trigger(trigger) => Some(Selection::Trigger(*trigger)),
        Choice::Action(action) => Some(Selection::Action(action.clone())),
        Choice::Kind(kind) if list.purpose == Purpose::Action => {
            Action::native(*kind).map(Selection::Action)
        }
        Choice::Kind(kind) => NativeNode::condition(*kind).map(Selection::Condition),
        Choice::Native(node) => Some(Selection::Condition(node.clone())),
        Choice::Recipe(nodes) => Some(Selection::Actions(nodes.clone())),
    }
}

fn condition(
    ui: &mut egui::Ui,
    entry: &native::Condition,
    names: &BTreeMap<u16, String>,
    used: bool,
) -> Option<Selection> {
    for requirement in &entry.condition.requirements {
        sundial::investment::draw_authoring_info_icon(ui, requirement);
    }
    technical(ui, &entry.sources, names, &entry.condition.details);
    used.then(|| {
        Selection::Condition(NativeNode {
            kind: entry.condition.kind,
            bytes: entry.condition.bytes.clone(),
        })
    })
}

fn effect(
    ui: &mut egui::Ui,
    entry: &native::Effect,
    names: &BTreeMap<u16, String>,
    used: bool,
) -> Option<Selection> {
    technical(ui, &entry.sources, names, &entry.details);
    used.then(|| {
        Selection::Action(Action::Native {
            node: NativeNode {
                kind: entry.kind,
                bytes: entry.bytes.clone(),
            },
        })
    })
}
