//! Behavior selection. Sources are provenance, never the required navigation path.
use super::*;
use sundial::{
    investment::discovery::{behaviors as native, conditions::Family as ConditionFamily},
    package_authoring::sandbox_perk::{
        nodes,
        program::{Action, NativeNode, Program, Trigger},
    },
};

mod lead;
mod recipes;
mod rows;
#[cfg(test)]
mod tests;
mod words;

use lead::*;
pub(super) use rows::counts;
use rows::*;
use words::*;

pub(super) enum Selection {
    Trigger(Trigger),
    Condition(NativeNode),
    Action(Action),
    /// Several actions added together, in order, as one stock behavior.
    Actions(Vec<NativeNode>),
    /// Open the chosen asset's components in the editor.
    Components,
}
type Loaded = Result<native::Catalog, String>;

#[derive(Clone, Copy, PartialEq)]
enum Purpose {
    Trigger,
    Condition,
    Action,
}

enum Choice {
    Trigger(Trigger),
    Action(Action),
    Condition(usize),
    Effect(usize),
    Kind(u8),
    /// A condition the workbench composes from a stock template, such as a compiled
    /// comparison on an engine variable.
    Native(NativeNode),
    /// Actions the stock perks always add together (see `recipes`).
    Recipe(Vec<NativeNode>),
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum Family {
    Condition(ConditionFamily),
    Effect(u8, Option<String>),
}

/// Stable slots for the picker's filter and sort choice.
///
/// A child `Ui` inside a wrapped row has its own id, so `make_persistent_id` there does not
/// address the slot the surrounding code reads. These ids do not depend on which `Ui` is in
/// hand, so the choice survives the frame that set it.
fn stock_state(purpose: Purpose) -> egui::Id {
    egui::Id::new(("behavior-stock", purpose as u8))
}

fn detail_state(purpose: Purpose) -> egui::Id {
    egui::Id::new(("behavior-detail", purpose as u8))
}

fn order_state(purpose: Purpose) -> egui::Id {
    egui::Id::new(("behavior-order", purpose as u8))
}

fn category_state(purpose: Purpose) -> egui::Id {
    egui::Id::new(("behavior-category", purpose as u8))
}

fn show_all_state(purpose: Purpose) -> egui::Id {
    egui::Id::new(("behavior-show-all", purpose as u8))
}

fn index_of<T: PartialEq>(all: &[T], value: &T) -> u8 {
    all.iter().position(|choice| choice == value).unwrap_or(0) as u8
}

struct Row {
    family: Family,
    enabled: bool,
    /// Why the row cannot be used, shown when it is disabled. Empty when it is enabled.
    reason: &'static str,
    title: String,
    detail: String,
    search: String,
    /// How many times the installed stock perks that weapons, armor and abilities carry use
    /// this configuration in the picker's role. A row the workbench offers itself has none of
    /// its own.
    uses: usize,
    choice: Choice,
}

/// The most actions one program can hold, which `Program::validate` also enforces. The
/// picker disables every action row at the cap so the limit is met before the build, not
/// reported after it.
const ACTION_LIMIT: usize = 16;

/// Groups presentation only. Each configuration retains its complete native record.
struct Group<'a> {
    family: &'a Family,
    title: &'a str,
    rows: Vec<&'a Row>,
    /// Where the group's first row arrived, which is the workbench's own order for its offers.
    arrival: usize,
}

#[derive(Default)]
pub(super) struct Picker {
    packages: PathBuf,
    query: String,
    loaded: Option<Loaded>,
    pending: Option<Receiver<Loaded>>,
    /// Where each stock perk is carried. Suggested counts only perks a weapon, armor piece or
    /// ability carries, so one only a Ship's transmat effect or other vehicle socket holds does
    /// not lead.
    carried: Option<std::sync::Arc<sundial::investment::IngredientCatalog>>,
}

impl Picker {
    /// The installation's ingredient catalog, whose carried perks Suggested counts.
    pub fn count_carried(
        &mut self,
        ingredients: Option<std::sync::Arc<sundial::investment::IngredientCatalog>>,
    ) {
        self.carried = ingredients;
    }

    pub fn draw_trigger(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
        names: &BTreeMap<u16, String>,
        labels: &BTreeMap<u32, String>,
        label: &str,
        retained: bool,
    ) -> Option<Selection> {
        let rows = || {
            let mut rows = Trigger::ALL
                .into_iter()
                .filter(|trigger| *trigger != Trigger::Native)
                .filter_map(|trigger| {
                    let title = program::trigger_label(trigger, retained).to_owned();
                    let detail = match trigger {
                        Trigger::Always => "Starts when the perk is applied.",
                        Trigger::Equipped => "Starts when this weapon is equipped.",
                        Trigger::Drawn => "Starts when this weapon is drawn.",
                        _ => trigger.description(),
                    };
                    // The trigger's other reading stays searchable, so "On Draw" still finds
                    // the trigger while it reads "While Drawn".
                    let other = program::trigger_label(trigger, !retained);
                    Some(Row {
                        family: Family::Condition(trigger_family(trigger)?),
                        enabled: true,
                        reason: "",
                        search: format!("{title} {other} {detail}"),
                        title,
                        detail: detail.to_owned(),
                        uses: 0,
                        choice: Choice::Trigger(trigger),
                    })
                })
                .collect::<Vec<_>>();
            rows.extend(comparison_rows());
            rows
        };
        self.draw_picker(
            ui,
            discovery,
            names,
            labels,
            label,
            Purpose::Trigger,
            &rows,
            "",
        )
    }

    pub fn draw_condition_named(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
        names: &BTreeMap<u16, String>,
        labels: &BTreeMap<u32, String>,
        label: &str,
    ) -> Option<NativeNode> {
        match self.draw_picker(
            ui,
            discovery,
            names,
            labels,
            label,
            Purpose::Condition,
            &comparison_rows,
            "",
        )? {
            Selection::Condition(node) => Some(node),
            _ => None,
        }
    }

    /// The Add Action picker. It returns one action, or the several a stock behavior adds
    /// together.
    pub fn draw_action_selection(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
        names: &BTreeMap<u16, String>,
        labels: &BTreeMap<u32, String>,
        program: &Program,
        keys: &sundial::package_authoring::sandbox_perk::program::properties::KeyCatalog,
    ) -> Option<Selection> {
        // At the cap every action row is refused, including the native kinds the picker
        // builds itself, so the limit is visible before a build rather than after one.
        let blocked = if program.actions.len() >= ACTION_LIMIT {
            "This effect already holds the most actions a program can run."
        } else {
            ""
        };
        let rows = || {
            let mut rows = program::common_actions(program, keys)
                .into_iter()
                .map(|(title, detail, action)| Row {
                    family: action_family(&action),
                    reason: action_reason(program, &action),
                    enabled: action_reason(program, &action).is_empty(),
                    title: title.to_owned(),
                    detail: detail.to_owned(),
                    search: format!("{title} {detail}"),
                    uses: 0,
                    choice: Choice::Action(action),
                })
                .collect::<Vec<_>>();
            rows.extend(recipes::all().into_iter().map(|recipe| {
                let reason = if program.actions.len() + recipe.nodes.len() > ACTION_LIMIT {
                    "This effect has no room for these actions."
                } else {
                    ""
                };
                recipe_row(recipe, reason)
            }));
            rows
        };
        self.draw_picker(
            ui,
            discovery,
            names,
            labels,
            "Add Action…",
            Purpose::Action,
            &rows,
            blocked,
        )
    }

    /// One action from the Add Action picker.
    #[cfg(test)]
    pub fn draw_action(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
        names: &BTreeMap<u16, String>,
        labels: &BTreeMap<u32, String>,
        program: &Program,
        keys: &sundial::package_authoring::sandbox_perk::program::properties::KeyCatalog,
    ) -> Option<Action> {
        match self.draw_action_selection(ui, discovery, names, labels, program, keys)? {
            Selection::Action(action) => Some(action),
            _ => None,
        }
    }

    fn load(&mut self, ui: &egui::Ui, discovery: &discovery::Discovery) {
        let Some(packages) = discovery.packages() else {
            return;
        };
        if self.packages != packages {
            self.packages = packages.to_owned();
            self.loaded = None;
            self.pending = None;
        }
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(result) => {
                    self.loaded = Some(result);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.loaded = Some(Err("The behavior reader stopped before finishing.".into()));
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.loaded.is_none() && self.pending.is_none() {
            let packages = self.packages.clone();
            let ctx = ui.ctx().clone();
            let (sender, receiver) = mpsc::channel();
            self.pending = Some(receiver);
            thread::spawn(move || {
                let _ = sender.send(native::discover(&packages));
                ctx.request_repaint();
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_picker(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
        names: &BTreeMap<u16, String>,
        labels: &BTreeMap<u32, String>,
        label: &str,
        purpose: Purpose,
        // Built only while the picker is open.
        basic: &dyn Fn() -> Vec<Row>,
        blocked: &'static str,
    ) -> Option<Selection> {
        self.load(ui, discovery);
        let title = match purpose {
            Purpose::Trigger => "Choose a Trigger",
            Purpose::Condition => "Choose a Condition",
            Purpose::Action => "Choose an Action",
        };
        // Retry after a failed read starts the read again on the next frame.
        let mut retry = false;
        let picked = pickers::browser_with_toolbar(
            ui,
            ("native-behaviors", purpose as u8),
            label,
            title,
            &mut self.query,
            |ui, query, opened, _| {
                let mut show_all = false;
                let mut reset = opened;
                let mut result_rect = egui::Rect::NOTHING;
                let mut stock = StockUse::ALL[usize::from(ui.data(|data| {
                    data.get_temp::<u8>(stock_state(purpose))
                        .unwrap_or_default()
                }))
                .min(StockUse::ALL.len() - 1)];
                let mut detail = Detail::ALL[usize::from(ui.data(|data| {
                    data.get_temp::<u8>(detail_state(purpose))
                        .unwrap_or_default()
                }))
                .min(Detail::ALL.len() - 1)];
                let mut order = Order::ALL[usize::from(ui.data(|data| {
                    data.get_temp::<u8>(order_state(purpose))
                        .unwrap_or_default()
                }))
                .min(Order::ALL.len() - 1)];
                let categories = Category::choices(purpose);
                let mut category = categories[usize::from(ui.data(|data| {
                    data.get_temp::<u8>(category_state(purpose))
                        .unwrap_or_default()
                }))
                .min(categories.len() - 1)];
                // The search box takes a line of its own at the picker's width, since four
                // filters and the search never shared one line in any window, and a query
                // is what a reader types most. The filters follow on a line that wraps.
                ui.horizontal(|ui| {
                    reset |= sundial::ui::catalog::search(
                        ui,
                        query,
                        opened,
                        ui.available_width() - pickers::CLEAR_WIDTH,
                        "Search Behaviors",
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    // A combo takes the width of its selected text, so each filter below is
                    // drawn inside an allocation of this width and truncates to it, with the
                    // full reading on hover. One width serves all four: it is the width at
                    // which every everyday choice still reads in full, and a row of equal
                    // controls reads as one toolbar.
                    const FILTER_WIDTH: f32 = 150.0;
                    const SHOW_ALL_WIDTH: f32 = 90.0;
                    // Room for the result count, which the list draws under this toolbar.
                    const COUNT_WIDTH: f32 = 115.0;
                    // A combo box places itself at the cursor without asking the wrapping
                    // layout, so one that would run past the edge starts the next line
                    // instead. The remaining width on the line is `available_rect_before_wrap`:
                    // `available_width` reports the whole row inside a wrapping layout.
                    let fit = |ui: &mut egui::Ui, width: f32| {
                        if ui.available_rect_before_wrap().width()
                            < width + ui.spacing().item_spacing.x
                        {
                            ui.end_row();
                        }
                    };
                    let category_before = category;
                    let category_text = category.label();
                    controls::sized(ui, FILTER_WIDTH, |ui| {
                        egui::ComboBox::from_id_salt("behavior-category")
                            .width(FILTER_WIDTH)
                            .truncate()
                            .selected_text(category_text)
                            .show_ui(ui, |ui| {
                                for choice in &categories {
                                    ui.selectable_value(&mut category, *choice, choice.label())
                                        .on_hover_text(choice.hint());
                                }
                            })
                            .response
                            .on_hover_text(format!("Category: {category_text}"));
                        pickers::name_combo(ui, "behavior-category", "Category");
                    });
                    fit(ui, FILTER_WIDTH);
                    let detail_before = detail;
                    let detail_text = detail.label();
                    controls::sized(ui, FILTER_WIDTH, |ui| {
                        egui::ComboBox::from_id_salt("behavior-detail")
                            .width(FILTER_WIDTH)
                            .truncate()
                            .selected_text(detail_text)
                            .show_ui(ui, |ui| {
                                for choice in Detail::ALL {
                                    ui.selectable_value(&mut detail, choice, choice.label())
                                        .on_hover_text(choice.hint());
                                }
                            })
                            .response
                            .on_hover_text(format!("Detail: {detail_text}"));
                        pickers::name_combo(ui, "behavior-detail", "Detail");
                    });
                    fit(ui, FILTER_WIDTH);
                    let before = stock;
                    let stock_text = stock.label();
                    controls::sized(ui, FILTER_WIDTH, |ui| {
                        egui::ComboBox::from_id_salt("behavior-stock")
                            .width(FILTER_WIDTH)
                            .truncate()
                            .selected_text(stock_text)
                            .show_ui(ui, |ui| {
                                for choice in StockUse::ALL {
                                    ui.selectable_value(&mut stock, choice, choice.label())
                                        .on_hover_text(choice.hint());
                                }
                            })
                            .response
                            .on_hover_text(format!("Stock Use: {stock_text}"));
                        pickers::name_combo(ui, "behavior-stock", "Stock Use");
                    });
                    fit(ui, FILTER_WIDTH);
                    let order_before = order;
                    let order_text = format!("Sort: {}", order.label());
                    controls::sized(ui, FILTER_WIDTH, |ui| {
                        egui::ComboBox::from_id_salt("behavior-order")
                            .width(FILTER_WIDTH)
                            .truncate()
                            .selected_text(order_text.clone())
                            .show_ui(ui, |ui| {
                                for choice in Order::ALL {
                                    ui.selectable_value(&mut order, choice, choice.label())
                                        .on_hover_text(choice.hint());
                                }
                            })
                            .response
                            .on_hover_text(order_text.clone());
                        pickers::name_combo(ui, "behavior-order", "Sort Order");
                    });
                    reset |= stock != before
                        || order != order_before
                        || detail != detail_before
                        || category != category_before;
                    ui.data_mut(|data| {
                        data.insert_temp(stock_state(purpose), index_of(&StockUse::ALL, &stock));
                        data.insert_temp(detail_state(purpose), index_of(&Detail::ALL, &detail));
                        data.insert_temp(order_state(purpose), index_of(&Order::ALL, &order));
                        data.insert_temp(category_state(purpose), index_of(&categories, &category));
                    });
                    let id = show_all_state(purpose);
                    show_all = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
                    fit(ui, SHOW_ALL_WIDTH);
                    reset |= ui
                        .checkbox(&mut show_all, "Show All")
                        .on_hover_text("Include unnamed behaviors.")
                        .changed();
                    ui.data_mut(|data| data.insert_temp(id, show_all));
                    fit(ui, COUNT_WIDTH);
                    result_rect = ui
                        .allocate_space(egui::vec2(COUNT_WIDTH, ui.spacing().interact_size.y))
                        .1;
                });
                let query = query
                    .trim()
                    .to_lowercase()
                    .replace("orb of power", "orb of light");
                let mut native_rows = Vec::new();
                // Suggested counts only the perks weapons, armor and abilities carry.
                let carried = self.carried.as_deref().map(|data| &data.sources);
                let counted = |perk: &u16| counts(carried, *perk);
                if let Some(Ok(catalog)) = &self.loaded {
                    if purpose == Purpose::Action {
                        for (index, effect) in catalog.effects.iter().enumerate() {
                            let title = program::native_action_label(effect.kind, &effect.bytes);
                            let detail = asset_names(&effect.description, labels);
                            native_rows.push(Row {
                                family: effect_family(effect.kind, &effect.bytes),
                                enabled: purpose != Purpose::Action || blocked.is_empty(),
                                reason: blocked,
                                search: format!(
                                    "{title} {detail} {}",
                                    perk_names(&effect.sources, names)
                                ),
                                title,
                                detail,
                                uses: effect
                                    .sources
                                    .iter()
                                    .filter(|source| counted(&source.perk))
                                    .count(),
                                choice: Choice::Effect(index),
                            });
                        }
                        // Stock perks' own buffs under the perk's name, such as Rampage's.
                        native_rows.extend(
                            recipes::adopted(catalog, names)
                                .into_iter()
                                .map(|recipe| recipe_row(recipe, blocked)),
                        );
                    } else {
                        for (index, entry) in catalog.conditions.iter().enumerate() {
                            let condition = &entry.condition;
                            // The catalog named each configuration as it decoded it, with the
                            // rule `program::condition_title` applies to a placed node, so the
                            // row and the card it becomes read alike.
                            let title = condition.name.clone().unwrap_or_else(|| {
                                nodes::condition_title(condition.kind).to_owned()
                            });
                            let detail = stock_detail(condition);
                            if !show_all && !offered_by_default(condition) {
                                continue;
                            }
                            // The trigger picker counts only the stock uses as a trigger, since
                            // most timers and signal keys end effects rather than start them.
                            let uses = entry
                                .sources
                                .iter()
                                .filter(|source| {
                                    purpose != Purpose::Trigger || source.role.ends_with("Trigger")
                                })
                                .filter(|source| counted(&source.perk))
                                .count();
                            native_rows.push(Row {
                                family: Family::Condition(condition.family.clone()),
                                enabled: true,
                                reason: "",
                                // The decoded reading and its fields name what sets one
                                // configuration apart from the next, such as the precision
                                // filter of a damage condition, which the plain sentence
                                // shared by every configuration of the kind does not.
                                search: format!(
                                    "{title} {detail} {} {} {} {}",
                                    condition.description,
                                    condition.details.join(" "),
                                    former_title(condition.kind),
                                    perk_names(&entry.sources, names)
                                ),
                                title,
                                detail,
                                uses,
                                choice: Choice::Condition(index),
                            });
                        }
                    }
                }
                {
                    let kinds = if purpose == Purpose::Action {
                        nodes::EFFECTS.as_slice()
                    } else {
                        nodes::CONDITIONS.as_slice()
                    };
                    // A promoted condition kind is offered like a guided row; the rest
                    // of the bare kinds stay behind Show All.
                    for kind in kinds.iter().filter(|kind| {
                        kind.support == nodes::Support::Authorable
                            && (purpose != Purpose::Action || kind.kind != 5)
                            && (show_all
                                || (purpose != Purpose::Action
                                    && program::PROMOTED_NATIVE_CONDITIONS.contains(&kind.kind)))
                    }) {
                        let (family, title) = bare_kind(purpose, kind);
                        let detail = bare_kind_summary(purpose, kind);
                        // The row is searched by what it shows as well as by the engine's
                        // own words and number, so "counter" finds the counter and
                        // "accumulator" still does.
                        let former = if purpose == Purpose::Action {
                            ""
                        } else {
                            former_title(kind.kind)
                        };
                        let search = format!(
                            "{:02} {} {} {title} {detail} {former}",
                            kind.kind, kind.name, kind.summary
                        );
                        native_rows.push(Row {
                            family,
                            enabled: purpose != Purpose::Action || blocked.is_empty(),
                            reason: blocked,
                            title,
                            detail,
                            search,
                            uses: 0,
                            choice: Choice::Kind(kind.kind),
                        });
                    }
                }
                native_rows.sort_by_cached_key(|row| row.title.to_lowercase());
                let mut basic = basic();
                if !show_all && let Some(Ok(catalog)) = &self.loaded {
                    let states = catalog
                        .conditions
                        .iter()
                        .filter(|entry| entry.condition.kind == 20)
                        .filter_map(|entry| entry.condition.name.as_deref())
                        .collect::<BTreeSet<_>>();
                    basic.retain(|row| !stock_state_covers(row, &states));
                }
                let rows = group_rows(
                    basic.iter().chain(&native_rows),
                    &query,
                    stock,
                    detail,
                    category,
                    order,
                    purpose.leads(),
                );
                ui.separator();
                let status = match &self.loaded {
                    None if discovery.packages().is_none() => "No Game Content".to_owned(),
                    None => format!("{} · Reading…", rows.len()),
                    Some(Err(_)) => "Read Failed".to_owned(),
                    _ if rows.len() == 1 => "1 Result".to_owned(),
                    _ => format!("{} Results", rows.len()),
                };
                let response = sundial::ui::catalog::toolbar_status(ui, result_rect, status);
                match &self.loaded {
                    Some(Err(error)) => {
                        response.on_hover_text(error);
                        // The reason and a way to try again, where the count would be.
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(error).color(ui.visuals().error_fg_color),
                                )
                                .truncate(),
                            )
                            .on_hover_text(error);
                            if ui.button("Retry").clicked() {
                                retry = true;
                            }
                        });
                    }
                    Some(Ok(catalog)) if !catalog.errors.is_empty() => {
                        response.on_hover_text(format!(
                            "{} action resources could not be read.\n{}",
                            catalog.errors.len(),
                            catalog.errors.join("\n")
                        ));
                    }
                    _ => {}
                }
                let keys = rows
                    .iter()
                    .map(|group| egui::Id::new((purpose as u8, group.family)).value())
                    .collect::<Vec<_>>();
                // The picker opens on the condition in use. Its button carries the same title
                // the row does, since the rows are named as the cards name them.
                let select = opened
                    .then(|| rows.iter().position(|group| group.title == label))
                    .flatten()
                    .map(|index| keys[index]);
                pickers::BrowserList {
                    keys: &keys,
                    height: (ui.available_height() - 4.0).max(110.0),
                    reset,
                    row_height: sundial::investment::authoring_choice_row_height(ui),
                    select,
                }
                .draw_body_activating(
                    ui,
                    |ui, index, selected| {
                        let group = &rows[index];
                        sundial::investment::draw_asset_choice_row(
                            ui,
                            group.title,
                            distinct_detail(&group.rows[0].detail, group.title),
                            selected,
                        )
                    },
                    |ui, index, activated| {
                        let group = &rows[index];
                        ui.heading(group.title);
                        let catalog = self.loaded.as_ref().and_then(|r| r.as_ref().ok());
                        let row = configuration(ui, group, catalog);
                        ui.add_space(4.0);
                        ui.add(egui::Label::new(distinct_detail(&row.detail, group.title)).wrap());
                        if let Some(catalog) = catalog {
                            let examples = stock_examples(group, catalog, names);
                            if !examples.is_empty() {
                                let text = format!("Stock examples: {}", examples.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&text).weak())
                                        .wrap(),
                                )
                                .on_hover_text(format!("{text}\nInstalled perks that use this behavior. Their exact settings can differ from the selected configuration."));
                            }
                        }
                        ui.add_space(4.0);
                        let command = match purpose {
                            Purpose::Trigger => "Use Trigger",
                            Purpose::Condition => "Use Condition",
                            Purpose::Action => "Add Action",
                        };
                        // A double-click uses the row's selected configuration, as the button
                        // does, and is refused where the button is.
                        let use_it = ui
                            .add_enabled(row.enabled, crate::app::style::primary(ui, command))
                            .on_disabled_hover_text(row.reason)
                            .clicked()
                            || (activated && row.enabled);
                        let catalog = self.loaded.as_ref().and_then(|result| result.as_ref().ok());
                        let mut selected = None;
                        match &row.choice {
                            Choice::Trigger(trigger) if use_it => {
                                selected = Some(Selection::Trigger(*trigger))
                            }
                            Choice::Action(action) if use_it => {
                                selected = Some(Selection::Action(action.clone()))
                            }
                            Choice::Kind(kind) if use_it => {
                                selected = if purpose == Purpose::Action {
                                    Action::native(*kind).map(Selection::Action)
                                } else {
                                    NativeNode::condition(*kind).map(Selection::Condition)
                                }
                            }
                            Choice::Native(node) => {
                                selected = use_it.then(|| Selection::Condition(node.clone()));
                            }
                            Choice::Recipe(nodes) => {
                                selected = use_it.then(|| Selection::Actions(nodes.clone()));
                            }
                            Choice::Condition(index) => {
                                if let Some(catalog) = catalog {
                                    let entry = &catalog.conditions[*index];
                                    if use_it {
                                        selected = Some(Selection::Condition(NativeNode {
                                            kind: entry.condition.kind,
                                            bytes: entry.condition.bytes.clone(),
                                        }));
                                    }
                                    for requirement in &entry.condition.requirements {
                                        sundial::investment::draw_authoring_info_icon(
                                            ui,
                                            requirement,
                                        );
                                    }
                                    technical(ui, &entry.sources, names, &entry.condition.details);
                                }
                            }
                            Choice::Effect(index) => {
                                if let Some(catalog) = catalog {
                                    let entry = &catalog.effects[*index];
                                    if use_it {
                                        selected = Some(Selection::Action(Action::Native {
                                            node: NativeNode {
                                                kind: entry.kind,
                                                bytes: entry.bytes.clone(),
                                            },
                                        }));
                                    }
                                    technical(ui, &entry.sources, names, &entry.details);
                                }
                            }
                            _ => {}
                        }
                        selected
                    },
                    0,
                )
            },
        );
        if retry {
            self.loaded = None;
            self.pending = None;
        }
        picked
    }
}
