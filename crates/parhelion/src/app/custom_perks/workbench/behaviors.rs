//! Behavior selection. Sources are provenance, never the required navigation path.
use super::*;
use sundial::{
    investment::discovery::{behaviors as native, conditions::Family as ConditionFamily},
    package_authoring::sandbox_perk::{
        nodes,
        program::{Action, NativeNode, Program, Trigger},
    },
};

pub(super) enum Selection {
    Trigger(Trigger),
    Condition(NativeNode),
    Action(Action),
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
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum Family {
    Condition(ConditionFamily),
    Effect(u8, Option<String>),
}

/// The family a standard trigger belongs to. A native trigger carries its own node and so
/// has none of its own, which the picker shows by not offering it among the standard rows.
fn trigger_family(trigger: Trigger) -> Option<ConditionFamily> {
    use sundial::package_authoring::sandbox_perk::activation::PerkActivation as Kill;
    let kill = match trigger {
        Trigger::Always => return Some(ConditionFamily::Kind(0)),
        Trigger::Equipped => return Some(ConditionFamily::Kind(14)),
        Trigger::Drawn => return Some(ConditionFamily::Kind(16)),
        Trigger::WeaponKill => Kill::WeaponKill,
        Trigger::PrecisionKill => Kill::PrecisionWeaponKill,
        Trigger::MeleeKill => Kill::MeleeKill,
        Trigger::GrenadeKill => Kill::GrenadeKill,
        Trigger::AnyKill => Kill::AnyKill,
        Trigger::Native => return None,
    };
    Some(ConditionFamily::kill(kill.labels(), kill.requires_weapon()))
}

fn effect_family(kind: u8, bytes: &[u8]) -> Family {
    Family::Effect(
        kind,
        (kind == 8).then(|| program::native_action_label(kind, bytes)),
    )
}

fn action_family(action: &Action) -> Family {
    let kind = match action {
        Action::Attach { .. } => 1,
        Action::Spawn { .. } => 3,
        Action::Pattern { .. } => 26,
        Action::ExtendTimers { .. } => 32,
        Action::Property { .. } => 10,
        Action::AdjustComponent { .. } => 8,
        Action::UpdateAccumulator { .. } => 42,
        Action::AbilityProperty { .. } => 7,
        Action::TransmatContext { .. } => 47,
        Action::OverrideHostKey { .. } => 35,
        Action::SetDamageType { .. } => 6,
        Action::WeaponReferenceCount { .. } => 30,
        Action::AddRounds { .. } => 14,
        Action::AddFraction { .. } => 15,
        Action::Native { node } => return effect_family(node.kind, &node.bytes),
    };
    effect_family(kind, &[])
}

/// Whether the game's own perks configure this behavior, counted from the installed perk
/// data. This is an engine fact about the installation, not a statement about the
/// workbench: a behavior the stock perks never use is still authorable.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum StockUse {
    #[default]
    Any,
    /// At least one stock perk carries a configuration of this node kind.
    Used,
    /// The engine accepts this node kind, but no installed stock perk configures it.
    Unused,
}

impl StockUse {
    const ALL: [Self; 3] = [Self::Any, Self::Used, Self::Unused];

    fn label(self) -> &'static str {
        match self {
            Self::Any => "Any Stock Use",
            Self::Used => "Used by Stock Perks",
            Self::Unused => "Unused by Stock Perks",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Any => "Every behavior, whether or not the game's perks use it.",
            Self::Used => "Node kinds at least one installed stock perk configures.",
            Self::Unused => "Node kinds the engine accepts that no stock perk configures.",
        }
    }
}

/// Whether a row is described in the words a player uses, or only by the engine's own
/// traced name. This says what the workbench can explain, not what the engine can do:
/// a technical row is fully authorable, it just has no plain sentence yet.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Detail {
    /// Only behaviors with a name and description in the words a player uses.
    #[default]
    Standard,
    /// Everything, including the ones named only by their traced engine operation.
    Advanced,
}

impl Detail {
    const ALL: [Self; 2] = [Self::Standard, Self::Advanced];

    fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Advanced => "Advanced",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Standard => "Behaviors this workbench can describe in game terms.",
            Self::Advanced => {
                "Also show behaviors named only by the engine operation they were traced to."
            }
        }
    }
}

/// What a behavior is about, so a long list can be narrowed to one concern. A category is
/// read off the node kind's plain title (a kill, a reload, an ability, ammo), so it says
/// no more than the title does, and a kind without a plain title reads as Other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Category {
    #[default]
    Any,
    KillsAndDamage,
    WeaponHandling,
    EffectsAndSpawns,
    Abilities,
    Ammo,
    WeaponSettings,
    GameEvents,
    States,
    EventLabels,
    TimersAndCounters,
    Scripts,
    Timing,
    Other,
}

impl Category {
    /// Every category in the order the Suggested sort presents them.
    const ALL: [Self; 14] = [
        Self::Any,
        Self::KillsAndDamage,
        Self::WeaponHandling,
        Self::EffectsAndSpawns,
        Self::Abilities,
        Self::Ammo,
        Self::WeaponSettings,
        Self::GameEvents,
        Self::States,
        Self::EventLabels,
        Self::TimersAndCounters,
        Self::Scripts,
        Self::Timing,
        Self::Other,
    ];

    /// The categories a purpose's rows can fall in, so the filter offers only those.
    fn choices(purpose: Purpose) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|category| match purpose {
                Purpose::Action => !matches!(
                    category,
                    Self::KillsAndDamage
                        | Self::WeaponHandling
                        | Self::GameEvents
                        | Self::States
                        | Self::Timing
                ),
                Purpose::Trigger | Purpose::Condition => !matches!(
                    category,
                    Self::EffectsAndSpawns
                        | Self::WeaponSettings
                        | Self::EventLabels
                        | Self::TimersAndCounters
                        | Self::Scripts
                ),
            })
            .collect()
    }

    fn label(self) -> &'static str {
        match self {
            Self::Any => "Any Category",
            Self::KillsAndDamage => "Kills and Damage",
            Self::WeaponHandling => "Weapon Handling",
            Self::EffectsAndSpawns => "Effects and Spawns",
            Self::Abilities => "Abilities",
            Self::Ammo => "Ammo",
            Self::WeaponSettings => "Weapon Settings",
            Self::GameEvents => "Game Events",
            Self::States => "States",
            Self::EventLabels => "Event Labels and Values",
            Self::TimersAndCounters => "Timers and Counters",
            Self::Scripts => "Game Scripts",
            Self::Timing => "Timing",
            Self::Other => "Other",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Any => "Every category.",
            Self::KillsAndDamage => "Kills, damage dealt and damage taken.",
            Self::WeaponHandling => {
                "Equipping, drawing, holstering, reloading, aiming, crouching and firing."
            }
            Self::EffectsAndSpawns => "Attaching effects and spawning objects, orbs and effects.",
            Self::Abilities => "Grenade, melee, class and super abilities and finishers.",
            Self::Ammo => "Ammo pickup, ammo drops, the magazine and reserves.",
            Self::WeaponSettings => {
                "Damage type, firing mode, radar range, projectile and named properties."
            }
            Self::GameEvents => "Game events and signals the engine raises.",
            Self::States => "Player, weapon and subclass states the game compares.",
            Self::EventLabels => "Labels and values on the event that started the effect.",
            Self::TimersAndCounters => "Running timers and the effect's counter.",
            Self::Scripts => "Scripts the game's own perks run, such as Charged with Light.",
            Self::Timing => "Always, and after a delay.",
            Self::Other => "Behaviors with no plain title yet.",
        }
    }

    /// Where the Suggested order places the category.
    fn rank(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(Self::ALL.len())
    }

    /// The category of a row's family, from its node kind.
    fn of(family: &Family) -> Self {
        match family {
            Family::Condition(ConditionFamily::Kill { .. }) => Self::KillsAndDamage,
            Family::Condition(
                ConditionFamily::Kind(kind) | ConditionFamily::Named { kind, .. },
            ) => match kind {
                2 | 4 | 5 => Self::KillsAndDamage,
                14..=17 | 19 | 22 | 23 | 27 => Self::WeaponHandling,
                8 | 9 | 42 => Self::Abilities,
                6 => Self::Ammo,
                12 | 29 | 30 => Self::GameEvents,
                20 | 26 | 31 | 35 => Self::States,
                0 | 1 => Self::Timing,
                _ => Self::Other,
            },
            Family::Effect(kind, _) => match kind {
                1..=5 => Self::EffectsAndSpawns,
                7 | 8 => Self::Abilities,
                11 | 13..=16 => Self::Ammo,
                // Named properties, the transmat effect and the named value adjustments
                // all set what the weapon or item carries.
                6 | 10 | 18 | 26 | 35 | 47 | 53 => Self::WeaponSettings,
                37 | 40 | 54 => Self::EventLabels,
                32 | 42 => Self::TimersAndCounters,
                48 => Self::Scripts,
                _ => Self::Other,
            },
        }
    }
}

/// How the behavior results are ordered. Order changes presentation only. Every order but
/// Suggested is an engine fact: the node kind number, or how much the stock perks use it.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Order {
    /// The order the workbench offers them in, its own suggestion rather than engine data.
    #[default]
    Suggested,
    Name,
    Kind,
    /// Most configured by the installed stock perks first.
    StockUse,
}

impl Order {
    const ALL: [Self; 4] = [Self::Suggested, Self::Name, Self::Kind, Self::StockUse];

    fn label(self) -> &'static str {
        match self {
            Self::Suggested => "Suggested",
            Self::Name => "Name",
            Self::Kind => "Kind Number",
            Self::StockUse => "Stock Use",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Suggested => {
                "The workbench's own suggestion: by category, with guided behaviors first in each."
            }
            Self::Name => "Every behavior by name.",
            Self::Kind => "By the engine's node kind number, then by name.",
            Self::StockUse => "What the game's own perks configure most, counted first.",
        }
    }
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

/// The title of a bare node kind offered under Advanced. A condition kind with a plain name
/// reads as that name; everything else keeps its number and traced name.
fn bare_kind_title(purpose: Purpose, kind: &nodes::NodeKind) -> String {
    if purpose == Purpose::Action {
        return format!("{:02}: {}", kind.kind, kind.name);
    }
    program::plain_condition_title(kind.kind)
        .map_or_else(|| format!("{:02}: {}", kind.kind, kind.name), str::to_owned)
}

fn bare_kind_summary(purpose: Purpose, kind: &nodes::NodeKind) -> String {
    if purpose == Purpose::Action {
        return kind.summary.to_owned();
    }
    program::plain_condition_summary(kind.kind)
        .unwrap_or(kind.summary)
        .to_owned()
}

/// One guided row per engine variable the stock perks compare, each a general predicate
/// composed from the stock template with that variable's own comparison. The variable
/// names are the client's, so every row reads in the game's terms.
fn comparison_rows() -> Vec<Row> {
    program::compiled_comparisons()
        .into_iter()
        .map(|(title, detail, node)| Row {
            family: Family::Condition(ConditionFamily::Named {
                kind: node.kind,
                name: title.clone(),
            }),
            enabled: true,
            reason: "",
            search: format!("{title} {detail}"),
            title,
            detail,
            choice: Choice::Native(node),
        })
        .collect()
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
    choice: Choice,
}

/// The most actions one program can hold, which `Program::validate` also enforces. The
/// picker disables every action row at the cap so the limit is met before the build, not
/// reported after it.
const ACTION_LIMIT: usize = 16;

/// Why this action cannot be added to this program, or an empty string when it can. Every
/// rule here mirrors one the compiler or `Program::validate` enforces, so the picker refuses
/// exactly what a build would refuse.
fn action_reason(program: &Program, action: &Action) -> &'static str {
    if program.actions.len() >= ACTION_LIMIT {
        return "This effect already holds the most actions a program can run.";
    }
    match action {
        Action::Pattern { .. }
            if program
                .actions
                .iter()
                .any(|current| matches!(current, Action::Pattern { .. })) =>
        {
            "This effect already changes the fired projectile."
        }
        Action::ExtendTimers { .. } if !program.has_kill_trigger() => {
            "Extend Timers needs a kill trigger, since it extends the timers a kill started."
        }
        _ => "",
    }
}

impl Family {
    /// The engine's node kind number, when this family names one.
    fn kind(&self) -> Option<u8> {
        match self {
            Self::Effect(kind, _) => Some(*kind),
            Self::Condition(ConditionFamily::Kind(kind) | ConditionFamily::Named { kind, .. }) => {
                Some(*kind)
            }
            Self::Condition(ConditionFamily::Kill { .. }) => None,
        }
    }
}

impl Row {
    /// A row stands for a node kind the stock perks configure unless it came from the bare
    /// kind list, which is exactly the set with no stock configuration.
    fn configured_by_stock_perks(&self) -> bool {
        !matches!(self.choice, Choice::Kind(_))
    }

    /// Whether the workbench can describe this row in the words a player uses. Triggers and
    /// the composed actions are written that way by hand; a native effect qualifies when
    /// its kind has a plain title.
    fn plain_language(&self) -> bool {
        match (&self.choice, &self.family) {
            (Choice::Trigger(_) | Choice::Action(_) | Choice::Native(_), _) => true,
            (_, Family::Effect(kind, _)) => program::plain_action_title(*kind).is_some(),
            // A general predicate row is named only when its compiled comparison carries an
            // engine variable name, such as Nearby Enemies or Subclass Is Arc, so that name
            // is the game's own and the row reads in plain terms.
            (_, Family::Condition(ConditionFamily::Named { kind: 20 | 35, .. })) => true,
            (
                _,
                Family::Condition(
                    ConditionFamily::Kind(kind) | ConditionFamily::Named { kind, .. },
                ),
            ) => program::plain_condition_title(*kind).is_some(),
            // A kill family is the guided trigger set, which is written in game terms.
            (_, Family::Condition(ConditionFamily::Kill { .. })) => true,
        }
    }
}

/// Groups presentation only. Each configuration retains its complete native record.
struct Group<'a> {
    family: &'a Family,
    title: &'a str,
    rows: Vec<&'a Row>,
}

impl Row {
    fn key(&self) -> u64 {
        match self.choice {
            Choice::Trigger(trigger) => 1_000_000 + trigger as u64,
            Choice::Action(_) => egui::Id::new(&self.title).value(),
            Choice::Condition(index) => index as u64,
            Choice::Effect(index) => 2_000_000 + index as u64,
            Choice::Kind(kind) => 3_000_000 + u64::from(kind),
            Choice::Native(_) => egui::Id::new(("native", &self.title)).value(),
        }
    }
}

#[derive(Default)]
pub(super) struct Picker {
    packages: PathBuf,
    query: String,
    loaded: Option<Loaded>,
    pending: Option<Receiver<Loaded>>,
}

impl Picker {
    pub fn draw_trigger(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
        names: &BTreeMap<u16, String>,
        labels: &BTreeMap<u32, String>,
        label: &str,
        retained: bool,
    ) -> Option<Selection> {
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
                Some(Row {
                    family: Family::Condition(trigger_family(trigger)?),
                    enabled: true,
                    reason: "",
                    search: format!("{title} {detail}"),
                    title,
                    detail: detail.to_owned(),
                    choice: Choice::Trigger(trigger),
                })
            })
            .collect::<Vec<_>>();
        rows.extend(comparison_rows());
        self.draw_picker(
            ui,
            discovery,
            names,
            labels,
            &format!("{label} {}", egui_phosphor::regular::CARET_DOWN),
            Purpose::Trigger,
            rows,
            "",
        )
    }

    pub fn draw_condition(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
        names: &BTreeMap<u16, String>,
        labels: &BTreeMap<u32, String>,
    ) -> Option<NativeNode> {
        match self.draw_picker(
            ui,
            discovery,
            names,
            labels,
            "Change Condition…",
            Purpose::Condition,
            comparison_rows(),
            "",
        )? {
            Selection::Condition(node) => Some(node),
            _ => None,
        }
    }

    pub fn draw_action(
        &mut self,
        ui: &mut egui::Ui,
        discovery: &discovery::Discovery,
        names: &BTreeMap<u16, String>,
        labels: &BTreeMap<u32, String>,
        program: &Program,
        keys: &sundial::package_authoring::sandbox_perk::program::properties::KeyCatalog,
    ) -> Option<Action> {
        // At the cap every action row is refused, including the native kinds the picker
        // builds itself, so the limit is visible before a build rather than after one.
        let blocked = if program.actions.len() >= ACTION_LIMIT {
            "This effect already holds the most actions a program can run."
        } else {
            ""
        };
        let rows = program::common_actions(program, keys)
            .into_iter()
            .map(|(title, detail, action)| Row {
                family: action_family(&action),
                reason: action_reason(program, &action),
                enabled: action_reason(program, &action).is_empty(),
                title: title.to_owned(),
                detail: detail.to_owned(),
                search: format!("{title} {detail}"),
                choice: Choice::Action(action),
            })
            .collect();
        match self.draw_picker(
            ui,
            discovery,
            names,
            labels,
            "Add Action…",
            Purpose::Action,
            rows,
            blocked,
        )? {
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
        basic: Vec<Row>,
        blocked: &'static str,
    ) -> Option<Selection> {
        self.load(ui, discovery);
        let title = match purpose {
            Purpose::Trigger => "Choose a Trigger",
            Purpose::Condition => "Choose a Condition",
            Purpose::Action => "Choose an Action",
        };
        pickers::browser_with_toolbar(
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
                // Six controls do not fit one line in a narrow window. Wrapping keeps every
                // control usable instead of clipping the search box.
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
                    // The search box takes what the rest of the line leaves. Summing the
                    // same widths the controls use keeps that true when one of them changes,
                    // which a hand-added total does not.
                    let reserved = FILTER_WIDTH * 4.0
                        + SHOW_ALL_WIDTH
                        + COUNT_WIDTH
                        + ui.spacing().item_spacing.x * 6.0;
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
                    reset |= sundial::ui::catalog::search(
                        ui,
                        query,
                        opened,
                        (ui.available_width() - reserved).max(160.0),
                        "Search Behaviors",
                    );
                    fit(ui, FILTER_WIDTH);
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
                            .on_hover_text(format!(
                                "{category_text}\nShow one category of behavior, read off each row's plain title."
                            ));
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
                            .on_hover_text(format!(
                                "{detail_text}\nAdvanced adds the behaviors named only by their engine operation."
                            ));
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
                            .on_hover_text(format!(
                                "{stock_text}\nFilter by whether the game's own perks configure the behavior."
                            ));
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
                            .on_hover_text(format!(
                                "{order_text}\nOrder the results. Sorting never hides a behavior."
                            ));
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
                    let id = ui.make_persistent_id("show-all-behaviors");
                    show_all = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
                    fit(ui, SHOW_ALL_WIDTH);
                    reset |= ui
                        .checkbox(&mut show_all, "Show All")
                        .on_hover_text(
                            "Include unnamed native configurations and empty native kinds.",
                        )
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
                if let Some(Ok(catalog)) = &self.loaded {
                    if purpose == Purpose::Action {
                        for (index, effect) in catalog.effects.iter().enumerate() {
                            let title = program::native_action_label(effect.kind, &effect.bytes);
                            let detail = asset_names(&effect.description, labels);
                            native_rows.push(Row {
                                family: effect_family(effect.kind, &effect.bytes),
                                enabled: purpose != Purpose::Action || blocked.is_empty(),
                                reason: blocked,
                                search: format!("{title} {detail}"),
                                title,
                                detail,
                                choice: Choice::Effect(index),
                            });
                        }
                    } else {
                        for (index, entry) in catalog.conditions.iter().enumerate() {
                            let condition = &entry.condition;
                            let title = condition
                                .name
                                .clone()
                                .unwrap_or_else(|| program::native_condition_title(condition.kind));
                            // A plain sentence replaces the decoded one where the kind has
                            // it, so the row reads the way the guided actions do.
                            let detail = program::plain_condition_summary(condition.kind)
                                .map_or_else(|| condition.description.clone(), str::to_owned);
                            if !show_all && condition.name.is_none() {
                                continue;
                            }
                            native_rows.push(Row {
                                family: Family::Condition(condition.family.clone()),
                                enabled: true,
                                reason: "",
                                search: format!("{title} {detail}"),
                                title,
                                detail,
                                choice: Choice::Condition(index),
                            });
                        }
                    }
                }
                if show_all {
                    let kinds = if purpose == Purpose::Action {
                        nodes::EFFECTS.as_slice()
                    } else {
                        nodes::CONDITIONS.as_slice()
                    };
                    for kind in kinds.iter().filter(|kind| {
                        kind.support == nodes::Support::Authorable
                            && (purpose != Purpose::Action || kind.kind != 5)
                    }) {
                        native_rows.push(Row {
                            family: if purpose == Purpose::Action {
                                effect_family(kind.kind, &[])
                            } else {
                                Family::Condition(ConditionFamily::Kind(kind.kind))
                            },
                            enabled: purpose != Purpose::Action || blocked.is_empty(),
                            reason: blocked,
                            title: bare_kind_title(purpose, kind),
                            detail: bare_kind_summary(purpose, kind),
                            search: format!("{:02} {} {}", kind.kind, kind.name, kind.summary),
                            choice: Choice::Kind(kind.kind),
                        });
                    }
                }
                native_rows.sort_by_cached_key(|row| row.title.to_lowercase());
                let rows = group_rows(
                    basic.iter().chain(&native_rows),
                    &query,
                    stock,
                    detail,
                    category,
                    order,
                );
                ui.separator();
                let status = match &self.loaded {
                    None => format!("{} · Reading…", rows.len()),
                    Some(Err(_)) => "Read Failed".to_owned(),
                    _ if rows.len() == 1 => "1 Result".to_owned(),
                    _ => format!("{} Results", rows.len()),
                };
                let response = ui.put(result_rect, egui::Label::new(status));
                match &self.loaded {
                    Some(Err(error)) => {
                        response.on_hover_text(error);
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
                pickers::BrowserList {
                    keys: &keys,
                    height: (ui.available_height() - 4.0).max(110.0),
                    reset,
                    row_height: sundial::investment::authoring_choice_row_height(ui),
                }
                .draw_body(
                    ui,
                    |ui, index, selected| {
                        let group = &rows[index];
                        let detail = &group.rows[0].detail;
                        sundial::investment::draw_asset_choice_row(
                            ui,
                            group.title,
                            detail,
                            selected,
                        )
                    },
                    |ui, index| {
                        let group = &rows[index];
                        ui.heading(group.title);
                        let catalog = self.loaded.as_ref().and_then(|r| r.as_ref().ok());
                        let row = configuration(ui, group, catalog);
                        ui.add_space(4.0);
                        ui.add(egui::Label::new(&row.detail).wrap());
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
                        let use_it = ui
                            .add_enabled(row.enabled, crate::app::style::primary(ui, command))
                            .on_disabled_hover_text(row.reason)
                            .clicked();
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
                            Choice::Native(node) if use_it => {
                                selected = Some(Selection::Condition(node.clone()));
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
                )
            },
        )
    }
}

fn group_rows<'a>(
    rows: impl Iterator<Item = &'a Row>,
    query: &str,
    stock: StockUse,
    detail: Detail,
    category: Category,
    order: Order,
) -> Vec<Group<'a>> {
    let mut result = Vec::<Group<'a>>::new();
    let mut groups = BTreeMap::new();
    for row in rows {
        let index = *groups.entry(&row.family).or_insert_with(|| {
            result.push(Group {
                family: &row.family,
                title: &row.title,
                rows: Vec::new(),
            });
            result.len() - 1
        });
        result[index].rows.push(row);
    }
    // Searching a configuration keeps its whole behavior available, including
    // the default. Changing search terms must not silently change the selection.
    // A group reads as plain language when any of its rows does, so a named action keeps
    // its whole set of configurations rather than losing the ones read from stock perks.
    if detail == Detail::Standard {
        result.retain(|group| group.rows.iter().any(|row| row.plain_language()));
    }
    // A group is filtered as a whole. One group can hold both a guided row and the stock
    // configurations of the same node kind, so filtering row by row would split it.
    result.retain(|group| {
        let used = group.rows.iter().any(|row| row.configured_by_stock_perks());
        match stock {
            StockUse::Any => true,
            StockUse::Used => used,
            StockUse::Unused => !used,
        }
    });
    if category != Category::Any {
        result.retain(|group| Category::of(group.family) == category);
    }
    result.retain(|group| {
        group
            .rows
            .iter()
            .any(|row| pickers::matches(query, &row.search))
    });
    // A stable secondary key on the title keeps a chosen row in place while the query
    // changes. Suggested orders by category and, within one, keeps the order the rows
    // arrived in, guided behaviors first, so the event conditions lead the state ones.
    match order {
        Order::Suggested => {
            result.sort_by_cached_key(|group| Category::of(group.family).rank());
        }
        Order::Name => result.sort_by_cached_key(|group| group.title.to_lowercase()),
        Order::Kind => result.sort_by_cached_key(|group| {
            (
                group.family.kind().unwrap_or(u8::MAX),
                group.title.to_lowercase(),
            )
        }),
        Order::StockUse => result.sort_by_cached_key(|group| {
            let configurations = group
                .rows
                .iter()
                .filter(|row| row.configured_by_stock_perks())
                .count();
            (
                std::cmp::Reverse(configurations),
                group.title.to_lowercase(),
            )
        }),
    }
    result
}

fn configuration<'a>(
    ui: &mut egui::Ui,
    group: &Group<'a>,
    catalog: Option<&native::Catalog>,
) -> &'a Row {
    if group.rows.len() == 1 {
        return group.rows[0];
    }
    let id = ui.make_persistent_id(("behavior-configuration", group.family));
    let mut selected = ui
        .data(|data| data.get_temp::<u64>(id))
        .filter(|key| group.rows.iter().any(|row| row.key() == *key))
        .unwrap_or_else(|| group.rows[0].key());
    let details = group
        .rows
        .iter()
        .map(|row| match (&row.choice, catalog) {
            (Choice::Condition(index), Some(catalog)) => {
                catalog.conditions[*index].condition.details.as_slice()
            }
            (Choice::Effect(index), Some(catalog)) => catalog.effects[*index].details.as_slice(),
            _ => &[],
        })
        .collect::<Vec<_>>();
    let labels = group
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            if matches!(row.choice, Choice::Trigger(_) | Choice::Action(_)) {
                return "Custom Configuration".to_owned();
            }
            let changes = details[index]
                .iter()
                .filter(|value| {
                    !details
                        .iter()
                        .filter(|other| !other.is_empty())
                        .all(|other| other.contains(value))
                })
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ");
            if changes.is_empty() && group.rows.iter().any(|other| other.detail != row.detail) {
                format!("{} · {}", index + 1, row.detail)
            } else if changes.is_empty() {
                format!("Configuration {}", index + 1)
            } else {
                format!("{} · {changes}", index + 1)
            }
        })
        .collect::<Vec<_>>();
    let current = group
        .rows
        .iter()
        .position(|row| row.key() == selected)
        .unwrap_or(0);
    // A configuration is named by the values that tell it apart, so the reading runs long.
    // A combo takes the width of its selected text and `width` only sets a floor, so the
    // allocation is what holds it to the row. The whole reading stays on hover.
    let width = ui.available_width().min(340.0);
    let chosen = labels[current].clone();
    controls::sized(ui, width, |ui| {
        egui::ComboBox::from_id_salt(id.with("selector"))
            .width(width)
            .truncate()
            .selected_text(&chosen)
            .show_ui(ui, |ui| {
                ui.set_max_width(440.0);
                for (row, label) in group.rows.iter().zip(&labels) {
                    ui.selectable_value(&mut selected, row.key(), label);
                }
            })
            .response
            .on_hover_text(format!(
                "{chosen}\n{} configurations available. Selecting one keeps its values and restrictions, which you can edit after adding it.", group.rows.len()
            ));
        pickers::name_combo(ui, id.with("selector"), "Configuration");
    });
    ui.data_mut(|data| data.insert_temp(id, selected));
    group
        .rows
        .iter()
        .find(|row| row.key() == selected)
        .copied()
        .unwrap_or(group.rows[0])
}

/// Real installed examples help explain a behavior without inventing gameplay claims.
fn stock_examples(
    group: &Group<'_>,
    catalog: &native::Catalog,
    names: &BTreeMap<u16, String>,
) -> Vec<String> {
    group
        .rows
        .iter()
        .flat_map(|row| match &row.choice {
            Choice::Condition(index) => catalog.conditions[*index].sources.as_slice(),
            Choice::Effect(index) => catalog.effects[*index].sources.as_slice(),
            _ => &[],
        })
        .filter_map(|source| names.get(&source.perk))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn asset_names(text: &str, labels: &BTreeMap<u32, String>) -> String {
    let mut result = text.to_owned();
    for word in text.split_whitespace() {
        if let Some(tag) = word
            .strip_prefix("0x")
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            && let Some(name) = labels.get(&tag)
        {
            result = result.replace(word, name);
        }
    }
    result
}

fn technical(
    ui: &mut egui::Ui,
    sources: &[native::Source],
    names: &BTreeMap<u16, String>,
    details: &[String],
) {
    egui::CollapsingHeader::new("Technical Details").show(ui, |ui| {
        for detail in details {
            ui.label(detail);
        }
        ui.separator();
        for source in sources {
            ui.label(format!(
                "{} · {}",
                names
                    .get(&source.perk)
                    .cloned()
                    .unwrap_or_else(|| format!("Effect {}", source.perk)),
                source.role
            ));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_share_one_entry_without_losing_configurations_or_weapon_restrictions() {
        let family = Family::Condition(trigger_family(Trigger::WeaponKill).unwrap());
        let rows = vec![
            Row {
                family: family.clone(),
                enabled: true,
                reason: "",
                title: "On Weapon Kill".into(),
                detail: String::new(),
                search: "on weapon kill".into(),
                choice: Choice::Trigger(Trigger::WeaponKill),
            },
            Row {
                family,
                enabled: true,
                reason: "",
                title: "A Kill from This Weapon".into(),
                detail: String::new(),
                search: "a kill from this weapon with extra restrictions".into(),
                choice: Choice::Condition(42),
            },
            Row {
                family: Family::Condition(trigger_family(Trigger::AnyKill).unwrap()),
                enabled: true,
                reason: "",
                title: "On Any Credited Kill".into(),
                detail: String::new(),
                search: "on any credited kill".into(),
                choice: Choice::Trigger(Trigger::AnyKill),
            },
        ];
        let groups = group_rows(
            rows.iter(),
            "",
            StockUse::Any,
            Detail::Advanced,
            Category::Any,
            Order::Suggested,
        );
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].rows.len(), 2);
        let filtered = group_rows(
            rows.iter(),
            "extra restrictions",
            StockUse::Any,
            Detail::Advanced,
            Category::Any,
            Order::Suggested,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].rows[0].key(), rows[0].key());
        assert_eq!(filtered[0].rows[1].key(), rows[1].key());
        assert_eq!(
            action_family(&Action::add_rounds(1)),
            effect_family(14, &[])
        );
        assert_eq!(
            action_family(&Action::add_fraction(0.5)),
            effect_family(15, &[])
        );
        assert_eq!(
            action_family(&Action::generate_orb(
                sundial::package_authoring::sandbox_perk::program::Position::Owner
            )),
            effect_family(5, &[])
        );
    }

    fn row(title: &str, kind: u8, choice: Choice) -> Row {
        Row {
            family: effect_family(kind, &[]),
            enabled: true,
            reason: "",
            title: title.into(),
            detail: String::new(),
            search: title.to_lowercase(),
            choice,
        }
    }

    #[test]
    fn the_stock_use_filter_splits_on_whether_the_game_configures_the_kind() {
        let rows = [
            row("Spawn an Effect", 3, Choice::Action(Action::add_rounds(1))),
            row("Zero Native Kind", 44, Choice::Kind(44)),
            row("Alpha Stock Effect", 10, Choice::Effect(7)),
        ];
        let titles = |stock, order| {
            group_rows(
                rows.iter(),
                "",
                stock,
                Detail::Advanced,
                Category::Any,
                order,
            )
            .iter()
            .map(|group| group.title.to_owned())
            .collect::<Vec<_>>()
        };
        // Suggested orders by category: the spawn (Effects and Spawns) before the named
        // property (Weapon Settings) before the bare kind with no plain title (Other).
        assert_eq!(
            titles(StockUse::Any, Order::Suggested),
            ["Spawn an Effect", "Alpha Stock Effect", "Zero Native Kind"]
        );
        // Only the bare kind list carries node kinds no stock perk configures.
        assert_eq!(
            titles(StockUse::Unused, Order::Suggested),
            ["Zero Native Kind"]
        );
        assert_eq!(
            titles(StockUse::Used, Order::Suggested),
            ["Spawn an Effect", "Alpha Stock Effect"]
        );
    }

    #[test]
    fn a_group_holding_both_a_guided_row_and_stock_configurations_is_never_split() {
        // One node kind can be offered as a named action and also decoded from the stock
        // perks that configure it. Both land in one group, so the filter keeps or drops the
        // group as a whole rather than removing rows from inside it.
        let rows = [
            row("Spawn an Effect", 3, Choice::Action(Action::add_rounds(1))),
            row("Spawn an Effect", 3, Choice::Effect(7)),
            row("Spawn an Effect", 3, Choice::Effect(9)),
        ];
        let groups = group_rows(
            rows.iter(),
            "",
            StockUse::Used,
            Detail::Advanced,
            Category::Any,
            Order::Suggested,
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].rows.len(),
            3,
            "every configuration stays available"
        );
        assert!(
            group_rows(
                rows.iter(),
                "",
                StockUse::Unused,
                Detail::Advanced,
                Category::Any,
                Order::Suggested
            )
            .is_empty()
        );
    }

    #[test]
    fn the_standard_list_hides_only_the_rows_named_by_their_engine_operation() {
        let rows = [
            // Kind 3 is a guided spawn, so both its rows read in game terms.
            row(
                "Spawn an Object or Effect",
                3,
                Choice::Action(Action::add_rounds(1)),
            ),
            row("Spawn an Object or Effect", 3, Choice::Effect(1)),
            // Kind 20's traced evidence leaves its counter unresolved, so it has no plain
            // title and reads only as the operation it was traced to.
            row("Host Reference Count", 20, Choice::Effect(2)),
        ];
        let titles = |detail| {
            group_rows(
                rows.iter(),
                "",
                StockUse::Any,
                detail,
                Category::Any,
                Order::Suggested,
            )
            .iter()
            .map(|group| group.title.to_owned())
            .collect::<Vec<_>>()
        };
        assert_eq!(titles(Detail::Standard), ["Spawn an Object or Effect"]);
        assert_eq!(
            titles(Detail::Advanced),
            ["Spawn an Object or Effect", "Host Reference Count"]
        );
        // Hiding is per group, so a plain group keeps every configuration it holds.
        let plain = group_rows(
            rows.iter(),
            "",
            StockUse::Any,
            Detail::Standard,
            Category::Any,
            Order::Suggested,
        );
        assert_eq!(plain[0].rows.len(), 2);
    }

    #[test]
    fn the_category_filter_narrows_to_one_concern_and_suggested_leads_with_events() {
        let condition = |title: &str, kind: u8| Row {
            family: Family::Condition(ConditionFamily::Kind(kind)),
            enabled: true,
            reason: "",
            title: title.into(),
            detail: String::new(),
            search: title.to_lowercase(),
            choice: Choice::Kind(kind),
        };
        let mut rows = comparison_rows();
        rows.push(condition("On Reloading", 19));
        rows.push(condition("On a Kill", 2));
        rows.push(condition("On Picking Up Ammo", 6));
        let titles = |category, order| {
            group_rows(
                rows.iter(),
                "",
                StockUse::Any,
                Detail::Standard,
                category,
                order,
            )
            .iter()
            .map(|group| group.title.to_owned())
            .collect::<Vec<_>>()
        };
        // Every compared variable is a state, and the filter keeps only those.
        let states = titles(Category::States, Order::Suggested);
        assert_eq!(states.len(), comparison_rows().len());
        assert_eq!(titles(Category::Ammo, Order::Name), ["On Picking Up Ammo"]);
        assert_eq!(titles(Category::KillsAndDamage, Order::Name), ["On a Kill"]);
        // Suggested puts the event conditions ahead of the compared states even though the
        // states arrived first, and keeps arrival order inside a category.
        let suggested = titles(Category::Any, Order::Suggested);
        assert_eq!(
            &suggested[..3],
            ["On a Kill", "On Reloading", "On Picking Up Ammo"]
        );
        assert_eq!(suggested[3..], states[..]);
        // Every condition and action kind with a plain title lands in a named category.
        for kind in 0..=44u8 {
            if program::plain_condition_title(kind).is_some() {
                let family = Family::Condition(ConditionFamily::Kind(kind));
                assert_ne!(Category::of(&family), Category::Other, "condition {kind}");
                assert!(Category::choices(Purpose::Condition).contains(&Category::of(&family)));
            }
        }
        for kind in 0..=54u8 {
            if program::plain_action_title(kind).is_some() {
                let family = effect_family(kind, &[]);
                assert_ne!(Category::of(&family), Category::Other, "action {kind}");
                assert!(Category::choices(Purpose::Action).contains(&Category::of(&family)));
            }
        }
    }

    #[test]
    fn every_compared_variable_is_a_standard_condition_that_validates() {
        use sundial::package_authoring::sandbox_perk::action::native::{self, predicate};
        let rows = comparison_rows();
        assert_eq!(rows.len(), predicate::VARIABLES.len());
        for row in &rows {
            assert!(row.plain_language(), "{}", row.title);
            assert!(row.configured_by_stock_perks(), "{}", row.title);
            let Choice::Native(node) = &row.choice else {
                panic!("{} is not a composed condition", row.title);
            };
            assert_eq!(node.kind, 20);
            let graph = native::Graph::read(&node.bytes, 0, 0x80803DCE).unwrap();
            graph.validate_node(true, 20).unwrap();
            let described = predicate::describe(&graph).unwrap();
            assert!(
                described.starts_with(&row.title),
                "{described} vs {}",
                row.title
            );
            assert!(!row.detail.is_empty());
        }
        // The rows read as Standard, so the default picker view offers them.
        let groups = group_rows(
            rows.iter(),
            "",
            StockUse::Any,
            Detail::Standard,
            Category::Any,
            Order::Suggested,
        );
        assert_eq!(groups.len(), rows.len());
        // The composed nodes are distinct per variable, not one template repeated.
        let distinct = rows
            .iter()
            .filter_map(|row| match &row.choice {
                Choice::Native(node) => Some(node.bytes.clone()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(distinct.len(), rows.len());
    }

    #[test]
    fn every_guided_action_reads_in_game_terms_and_carries_a_plain_sentence() {
        let program = Program::default();
        let actions = program::common_actions(&program, &Default::default());
        assert!(
            actions.len() >= 12,
            "expected the guided set to cover the kinds with plain descriptions, found {}",
            actions.len()
        );
        for (title, summary, action) in &actions {
            assert!(
                !title.trim().is_empty() && !summary.trim().is_empty(),
                "{title}"
            );
            // A guided entry never shows the engine's traced operation name.
            if let Action::Native { node } = action {
                assert_eq!(
                    program::plain_action_title(node.kind),
                    Some(*title),
                    "kind {} is offered as guided without a plain title",
                    node.kind
                );
            }
        }
        // Every promoted kind must actually produce an editable node. A kind whose record
        // cannot be built would be silently dropped, leaving a promise with nothing behind it.
        let missing = program::PROMOTED_NATIVE_ACTIONS
            .iter()
            .filter(|kind| {
                let title = program::plain_action_title(**kind);
                title.is_none() || !actions.iter().any(|(name, _, _)| Some(*name) == title)
            })
            .map(|kind| kind.to_string())
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "promoted kinds with no guided action: {missing:?}"
        );
    }

    /// A picker offer and the card it places are the same thing, so they read alike. They
    /// were written out separately and drifted: the picker said "Update Accumulator" and
    /// "Override a Host Key" where the card said "Set the Effect's Counter" and "Set a
    /// Weapon Firing Mode", so choosing a row produced a card the reader had not asked for.
    #[test]
    fn every_guided_action_is_offered_under_the_name_its_card_carries() {
        let program = Program::default();
        for (title, _, action) in program::common_actions(&program, &Default::default()) {
            assert_eq!(
                title,
                program::action_title(&action),
                "effect kind {} is offered under a name its card does not carry",
                program::action_kind(&action)
            );
        }
    }

    #[test]
    fn named_conditions_read_in_game_terms_and_pair_with_a_sentence() {
        // Every condition kind with a plain name must have a plain sentence, and the plain
        // name must replace the engine's traced one.
        for kind in 0..=u8::MAX {
            assert_eq!(
                program::plain_condition_title(kind).is_some(),
                program::plain_condition_summary(kind).is_some(),
                "condition kind {kind} has only one half of its plain description"
            );
            if let Some(title) = program::plain_condition_title(kind) {
                assert_eq!(program::native_condition_title(kind), title);
            }
        }
        // The conditions the stock perks rely on, and the ones their descriptions settled,
        // are covered.
        for kind in [
            0u8, 1, 2, 4, 5, 6, 8, 9, 12, 14, 15, 16, 17, 19, 22, 23, 26, 27, 29, 30, 31, 42,
        ] {
            assert!(
                program::plain_condition_title(kind).is_some(),
                "condition kind {kind} is common in stock perks but has no plain name"
            );
        }
        // An unresolved kind keeps the engine's traced name rather than gaining a guess.
        assert!(program::plain_condition_title(20).is_none());
        assert_eq!(program::native_condition_title(20), "General Predicate");
    }

    #[test]
    fn a_plain_title_and_a_plain_sentence_always_come_as_a_pair() {
        // A title with no sentence would show a name with nothing explaining it, and a
        // sentence with no title would never be reachable.
        for kind in 0..=u8::MAX {
            assert_eq!(
                program::plain_action_title(kind).is_some(),
                program::plain_action_summary(kind).is_some(),
                "effect kind {kind} has only one half of its plain description"
            );
        }
    }

    #[test]
    fn the_stock_use_order_counts_the_configurations_the_game_carries() {
        let rows = [
            row("One Configuration", 3, Choice::Effect(1)),
            row("Three Configurations", 10, Choice::Effect(2)),
            row("Three Configurations", 10, Choice::Effect(3)),
            row("Three Configurations", 10, Choice::Effect(4)),
            row("Two Configurations", 26, Choice::Effect(5)),
            row("Two Configurations", 26, Choice::Effect(6)),
        ];
        let titles = group_rows(
            rows.iter(),
            "",
            StockUse::Any,
            Detail::Advanced,
            Category::Any,
            Order::StockUse,
        )
        .iter()
        .map(|group| group.title.to_owned())
        .collect::<Vec<_>>();
        assert_eq!(
            titles,
            [
                "Three Configurations",
                "Two Configurations",
                "One Configuration"
            ]
        );
    }

    #[test]
    fn behavior_orders_sort_by_name_and_by_engine_kind_number() {
        let rows = [
            row("Spawn an Effect", 3, Choice::Action(Action::add_rounds(1))),
            row("Zero Native Kind", 44, Choice::Kind(44)),
            row("Alpha Stock Effect", 10, Choice::Effect(7)),
        ];
        let titles = |order| {
            group_rows(
                rows.iter(),
                "",
                StockUse::Any,
                Detail::Advanced,
                Category::Any,
                order,
            )
            .iter()
            .map(|group| group.title.to_owned())
            .collect::<Vec<_>>()
        };
        assert_eq!(
            titles(Order::Name),
            ["Alpha Stock Effect", "Spawn an Effect", "Zero Native Kind"]
        );
        assert_eq!(
            titles(Order::Kind),
            ["Spawn an Effect", "Alpha Stock Effect", "Zero Native Kind"]
        );
        // A kill family carries no kind number, so it sorts after every numbered kind
        // instead of being dropped.
        let kill = Row {
            family: Family::Condition(trigger_family(Trigger::WeaponKill).unwrap()),
            ..row("A Kill", 0, Choice::Trigger(Trigger::WeaponKill))
        };
        assert_eq!(kill.family.kind(), None);
        let with_kill = [
            row("Spawn an Effect", 3, Choice::Action(Action::add_rounds(1))),
            kill,
        ];
        let ordered = group_rows(
            with_kill.iter(),
            "",
            StockUse::Any,
            Detail::Advanced,
            Category::Any,
            Order::Kind,
        )
        .iter()
        .map(|group| group.title.to_owned())
        .collect::<Vec<_>>();
        assert_eq!(ordered, ["Spawn an Effect", "A Kill"]);
    }
}
