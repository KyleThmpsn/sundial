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
pub(super) use recipes::WEAPON_PROPERTIES_GRAPH;
mod rows;
mod view;
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

/// The choice of `all` the picker last kept under `id`, the first when it kept none.
fn remembered<T: Copy>(ui: &egui::Ui, id: egui::Id, all: &[T]) -> T {
    let index = ui.data(|data| data.get_temp::<u8>(id).unwrap_or_default());
    all[usize::from(index).min(all.len() - 1)]
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
const ACTION_LIMIT: usize = 64;

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
    /// Change Weapon Properties' attachment with its rows read, or why it is not ready, set
    /// by the canvas before the Add Action picker draws. None while no card has asked.
    pub(super) weapon_properties:
        Option<Result<sundial::package_authoring::sandbox_perk::program::Asset, &'static str>>,
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
    /// together. Change Weapon Properties' row takes `weapon_properties` as the canvas left it.
    pub fn draw_action_selection(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
        names: &BTreeMap<u16, String>,
        labels: &BTreeMap<u32, String>,
        program: &Program,
        keys: &sundial::package_authoring::sandbox_perk::program::properties::KeyCatalog,
    ) -> Option<Selection> {
        let weapon_properties = self
            .weapon_properties
            .clone()
            .unwrap_or(Err("Its rows are read on the effect's card."));
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
            rows.push(weapon_properties_row(program, weapon_properties.clone()));
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

    /// Lists the behaviors for `purpose` and returns the one its detail pane uses. Each frame
    /// draws the toolbar, collects the rows its filters leave, then draws the status and the
    /// list.
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
        // Retry after a failed read starts the read again on the next frame.
        let mut retry = false;
        let picked = pickers::browser_with_toolbar(
            ui,
            ("native-behaviors", purpose as u8),
            label,
            purpose.title(),
            &mut self.query,
            |ui, query, opened, _| {
                let toolbar = view::toolbar(ui, query, opened, purpose);
                let show_all = toolbar.filters.show_all;
                let inputs = rows::Inputs {
                    catalog: self.loaded.as_ref().and_then(|loaded| loaded.as_ref().ok()),
                    carried: self.carried.as_deref().map(|data| &data.sources),
                    names,
                    labels,
                };
                let native = rows::native_rows(&inputs, purpose, show_all, blocked);
                let basic = rows::basic_rows(basic(), inputs.catalog, show_all);
                let groups = rows::groups(&basic, &native, &toolbar.filters, purpose);
                ui.separator();
                retry |= view::status(
                    ui,
                    self.loaded.as_ref(),
                    discovery,
                    toolbar.count,
                    groups.len(),
                );
                let list = view::List {
                    purpose,
                    label,
                    opened,
                    reset: toolbar.reset,
                    catalog: inputs.catalog,
                    names,
                };
                view::list(ui, &groups, &list)
            },
        );
        if retry {
            self.loaded = None;
            self.pending = None;
        }
        picked
    }
}
