//! Behavior selection. Sources are provenance, never the required navigation path.
use super::*;
use sundial::{
    investment::discovery::{behaviors as native, conditions::Family as ConditionFamily},
    package_authoring::sandbox_perk::{
        nodes,
        program::{Action, NativeNode, Program, Trigger},
    },
};

mod recipes;

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
    match action {
        Action::Native { node } => effect_family(node.kind, &node.bytes),
        typed => effect_family(typed.kind(), &[]),
    }
}

/// A recipe's row in Add Action, refused with `reason` when the effect has no room for it.
fn recipe_row(recipe: recipes::Recipe, reason: &'static str) -> Row {
    Row {
        family: Family::Effect(recipe.kind, Some(recipe.title.to_owned())),
        enabled: reason.is_empty(),
        reason,
        title: recipe.title.to_owned(),
        detail: recipe.detail.to_owned(),
        search: format!("{} {}", recipe.title, recipe.detail),
        uses: 0,
        choice: Choice::Recipe(recipe.nodes),
    }
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
            Self::Any => "Every behavior.",
            Self::Used => "Behaviors a stock perk configures.",
            Self::Unused => "Behaviors no stock perk configures.",
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
            Self::Standard => "Behaviors with plain names.",
            Self::Advanced => "Also behaviors named by their engine operation.",
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
    /// Conditions that combine other conditions rather than reading the game.
    Logic,
    Other,
}

impl Category {
    /// Every category in the order the filter lists them.
    const ALL: [Self; 15] = [
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
        Self::Logic,
        Self::Other,
    ];

    /// The categories a purpose's rows can fall in, so the filter offers only those.
    fn choices(purpose: Purpose) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|category| match purpose {
                // Actions reach three categories a reader would look in for them: damage
                // taken, the signal an action sends, and the named state it holds.
                Purpose::Action => {
                    !matches!(category, Self::WeaponHandling | Self::Timing | Self::Logic)
                }
                Purpose::Trigger | Purpose::Condition => !matches!(
                    category,
                    Self::EffectsAndSpawns
                        | Self::WeaponSettings
                        | Self::EventLabels
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
            Self::Logic => "Logic",
            Self::Other => "Other",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Any => "Every category.",
            Self::KillsAndDamage => "Kills, damage dealt and damage taken.",
            Self::WeaponHandling => {
                "Equipping, drawing, holstering, reloading, aiming, crouching, sliding, sprinting and firing."
            }
            Self::EffectsAndSpawns => "Attaching effects and spawning objects, orbs and effects.",
            Self::Abilities => "Grenade, melee, class and Super abilities and finishers.",
            Self::Ammo => "Ammo pickup, ammo drops, the magazine and reserves.",
            Self::WeaponSettings => {
                "Damage type, firing mode, radar range, projectile and named properties."
            }
            Self::GameEvents => {
                "Game events and signals the engine raises, and the action that sends one."
            }
            Self::States => {
                "Player, weapon and subclass states the game compares, and the names an effect holds."
            }
            Self::EventLabels => "Labels and values on the event that started the effect.",
            Self::TimersAndCounters => {
                "Running timers, the effect's counter and the trigger that fires when it is reached."
            }
            Self::Scripts => "Scripts the game's own perks run, such as Charged with Light.",
            Self::Timing => "Always, and after a delay.",
            Self::Logic => "Requiring more than one thing at once.",
            Self::Other => "Behaviors with no plain title yet.",
        }
    }

    /// The category of a row's family, from its node kind.
    fn of(family: &Family) -> Self {
        match family {
            Family::Condition(ConditionFamily::Kill { .. }) => Self::KillsAndDamage,
            Family::Condition(
                ConditionFamily::Kind(kind) | ConditionFamily::Named { kind, .. },
            ) => match kind {
                2 | 4 | 5 => Self::KillsAndDamage,
                13..=19 | 22..=25 | 27 => Self::WeaponHandling,
                8..=11 | 42 => Self::Abilities,
                6 => Self::Ammo,
                12 | 29 | 30 => Self::GameEvents,
                20 | 35 => Self::States,
                31 => Self::Logic,
                // Reads back the name Remember a Target by Name stored.
                38 => Self::States,
                // The effect's counter is a counter first, filed beside the action that sets it.
                26 => Self::TimersAndCounters,
                0 | 1 => Self::Timing,
                _ => Self::Other,
            },
            Family::Effect(kind, _) => match kind {
                1..=5 => Self::EffectsAndSpawns,
                7 | 8 => Self::Abilities,
                11 | 13..=16 => Self::Ammo,
                // Named properties, the transmat effect and the named value adjustments
                // all set what the weapon or item carries.
                6 | 10 | 18 | 20 | 25 | 26 | 28 | 29 | 30 | 35 | 47 | 53 => Self::WeaponSettings,
                // Damage the wearer takes and damage the wearer deals.
                33 | 40 => Self::KillsAndDamage,
                37 | 54 => Self::EventLabels,
                // The signal an action sends belongs beside the conditions that hear it,
                // and the named target and value beside the states that read them.
                43 => Self::GameEvents,
                49 | 52 => Self::States,
                32 | 41 | 42 => Self::TimersAndCounters,
                48 => Self::Scripts,
                _ => Self::Other,
            },
        }
    }
}

/// How the behavior results are ordered. Order changes presentation only. Name and Kind are
/// engine facts, Stock Use counts what the installed stock perks carry, and Suggested leads
/// with the common choices before counting the same way.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Order {
    /// The common choices first, in the order `TRIGGER_LEAD`, `ACTION_LEAD` and
    /// `CONDITION_LEAD` give, then what the stock perks that weapons, armor and abilities carry
    /// use most, with
    /// kills by any filter and ability energy for any ability pooled, and only their triggers
    /// counted in the trigger picker. Equal counts keep the workbench's own offers first, in
    /// the order it lists them, then go by name.
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
            Self::Suggested => "Common choices first, then the most used by the game's perks.",
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

fn show_all_state(purpose: Purpose) -> egui::Id {
    egui::Id::new(("behavior-show-all", purpose as u8))
}

/// The family and name of a bare node kind, read from the node its row places, so the row
/// joins the configurations that read alike and carries the name its card will.
fn bare_kind(purpose: Purpose, kind: &nodes::NodeKind) -> (Family, String) {
    if purpose == Purpose::Action {
        return Action::native(kind.kind).map_or_else(
            || {
                (
                    effect_family(kind.kind, &[]),
                    nodes::effect_title(kind.kind).to_owned(),
                )
            },
            |action| (action_family(&action), program::action_title(&action)),
        );
    }
    NativeNode::condition(kind.kind)
        .and_then(|node| {
            sundial::package_authoring::sandbox_perk::action::decode_condition_node(&node.bytes)
                .ok()
        })
        .map_or_else(
            || {
                (
                    Family::Condition(ConditionFamily::Kind(kind.kind)),
                    nodes::condition_title(kind.kind).to_owned(),
                )
            },
            |condition| {
                (
                    Family::Condition(ConditionFamily::of(&condition)),
                    program::decoded_condition_title(&condition),
                )
            },
        )
}

/// A bare kind's sentence. One the engine alone names also gives its number, since that name
/// can be shared by several kinds.
fn bare_kind_summary(purpose: Purpose, kind: &nodes::NodeKind) -> String {
    let plain = if purpose == Purpose::Action {
        program::plain_action_summary(kind.kind)
    } else {
        program::plain_condition_summary(kind.kind)
    };
    plain.map_or_else(
        || format!("{} Kind {}.", kind.summary, kind.kind),
        str::to_owned,
    )
}

/// Names condition kinds carried in the picker before every view shared one, kept searchable
/// so a remembered name still finds its row.
fn former_title(kind: u8) -> &'static str {
    match kind {
        0 => "On Perk Activation",
        1 => "After a Timer",
        2 => "On a Kill",
        14 => "When the Weapon Is Equipped",
        15 => "When the Weapon Is Unequipped",
        16 => "When the Weapon Is Drawn",
        17 => "When the Weapon Is Holstered",
        26 => "Counter Reaches Threshold Accumulator",
        31 => "All Requirements Met",
        35 => "Predicate and Nested Condition Pass",
        _ => "",
    }
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
            uses: 0,
            choice: Choice::Native(node),
        })
        .collect()
}

/// Whether a stock configuration is listed without Show All. A named one is, unless its name
/// is only the hash of a state nobody has named. An unnamed one is listed under its kind's
/// plain title when the kind has one, as the ammo pickup, ability and game event
/// configurations are. Kills stay behind Show All, since they group by label and each
/// unrecognized label would add another row titled On a Kill.
fn offered_by_default(condition: &sundial::investment::discovery::conditions::Condition) -> bool {
    match &condition.name {
        // The reading `summary::unnamed_state_description` gives a state known only by hash.
        Some(_) => !condition.description.contains("the unnamed state 0x"),
        None => condition.kind != 2 && program::plain_condition_title(condition.kind).is_some(),
    }
}

/// A stock configuration's sentence. A plain one replaces the decoded one where the kind has
/// it, so the row reads the way the guided actions do. An empty state check reads Always or
/// Never, and kind 0 reads Always too, so it says which it is.
fn stock_detail(condition: &sundial::investment::discovery::conditions::Condition) -> String {
    match (condition.kind, condition.description.as_str()) {
        (20, "Always") => {
            "A state check with nothing to check. Always-active stock perks start on it.".into()
        }
        (20, "Never") => {
            "The same empty check inverted. Always-active stock perks end on it.".into()
        }
        (kind, description) => program::plain_condition_summary(kind)
            .map_or_else(|| description.to_owned(), str::to_owned),
    }
}

/// Whether a stock configuration already checks this row's compared variable as a plain
/// state, the way "While Super Active" checks super_active. The stock form is listed, and the
/// comparison waits behind Show All.
fn stock_state_covers(row: &Row, states: &BTreeSet<&str>) -> bool {
    use sundial::package_authoring::sandbox_perk::action::native::{Graph, predicate};
    let Choice::Native(node) = &row.choice else {
        return false;
    };
    if node.kind != 20 {
        return false;
    }
    let Ok(graph) = Graph::read(&node.bytes, 0, 0x80803DCE) else {
        return false;
    };
    let [comparison] = &predicate::comparisons(&graph, 0)[..] else {
        return false;
    };
    let state = format!("While {}", predicate::plain_variable(&comparison.name));
    states.contains(state.as_str())
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

    /// Whether the workbench offers this row itself: a standard trigger, a guided action or
    /// comparison, or a promoted condition kind. Suggested puts these first among behaviors
    /// the stock perks use equally.
    fn curated(&self) -> bool {
        match self.choice {
            Choice::Trigger(_) | Choice::Action(_) | Choice::Native(_) | Choice::Recipe(_) => true,
            Choice::Kind(kind) => {
                matches!(self.family, Family::Condition(_))
                    && program::PROMOTED_NATIVE_CONDITIONS.contains(&kind)
            }
            Choice::Condition(_) | Choice::Effect(_) => false,
        }
    }
}

/// Groups presentation only. Each configuration retains its complete native record.
struct Group<'a> {
    family: &'a Family,
    title: &'a str,
    rows: Vec<&'a Row>,
    /// Where the group's first row arrived, which is the workbench's own order for its offers.
    arrival: usize,
}

impl Group<'_> {
    /// How many times the installed stock perks use this behavior, over every configuration.
    fn uses(&self) -> usize {
        self.rows.iter().map(|row| row.uses).sum()
    }

    /// The group's place among the workbench's own offers, or last when it has none.
    fn curated_rank(&self) -> usize {
        if self.rows.iter().any(|row| row.curated()) {
            self.arrival
        } else {
            usize::MAX
        }
    }
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
            Choice::Recipe(_) => egui::Id::new(("recipe", &self.title)).value(),
        }
    }
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

/// Whether Suggested counts a stock perk. Once the installation is read, only a perk a weapon,
/// armor piece or ability carries counts. Vehicle and Ghost Shell sockets hold cosmetic plugs,
/// such as a Ship's transmat effect. The asset picker counts the perks referencing an asset
/// the same way.
pub(super) fn counts(
    carried: Option<&BTreeMap<u16, BTreeSet<sundial::investment::IngredientSource>>>,
    perk: u16,
) -> bool {
    use sundial::investment::IngredientSource::{Ability, Armor, Weapon};
    let Some(map) = carried else {
        return true;
    };
    map.get(&perk)
        .into_iter()
        .flatten()
        .any(|source| matches!(source, Weapon | Armor | Ability))
}

fn group_rows<'a>(
    rows: impl Iterator<Item = &'a Row>,
    query: &str,
    stock: StockUse,
    detail: Detail,
    category: Category,
    order: Order,
    leads: &[Lead],
) -> Vec<Group<'a>> {
    let mut result = Vec::<Group<'a>>::new();
    let mut groups = BTreeMap::new();
    for row in rows {
        let index = *groups.entry(&row.family).or_insert_with(|| {
            let arrival = result.len();
            result.push(Group {
                family: &row.family,
                title: &row.title,
                rows: Vec::new(),
                arrival,
            });
            arrival
        });
        result[index].rows.push(row);
    }
    // A behavior's configurations list the workbench's own first, so the default stays the
    // one it offers, then the stock ones the installed perks use most.
    for group in &mut result {
        group
            .rows
            .sort_by_key(|row| (!row.curated(), std::cmp::Reverse(row.uses)));
    }
    // A behavior split into variants, kills by filter or energy by ability, ranks by its
    // variants' combined uses, counted before any filter so a search keeps the order.
    let mut pooled = BTreeMap::<u8, usize>::new();
    for group in &result {
        if let Some(kind) = pool(group.family) {
            *pooled.entry(kind).or_default() += group.uses();
        }
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
    // A search also finds a behavior by the everyday words for what it does.
    result.retain(|group| {
        let goals = goal_words(group.family, group.title);
        group.rows.iter().any(|row| {
            pickers::matches(query, &row.search)
                || pickers::matches(query, &format!("{} {goals}", row.search))
        })
    });
    // A stable secondary key on the title keeps a chosen row in place while the query
    // changes. Suggested leads with the common choices in their own order, then the rest by
    // how often the stock perks use them. Equal counts, which is every count before the
    // installed perks are read, keep the workbench's own offers first in the order it lists
    // them.
    match order {
        Order::Suggested => result.sort_by_cached_key(|group| {
            let uses = pool(group.family)
                .and_then(|kind| pooled.get(&kind).copied())
                .unwrap_or_else(|| group.uses());
            (
                lead_rank(leads, group.family),
                std::cmp::Reverse(uses),
                std::cmp::Reverse(group.uses()),
                group.curated_rank(),
                group.title.to_lowercase(),
            )
        }),
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
    // A search that names a behavior by its title leads with it, ahead of the rows the
    // everyday words also reach: "swap" finds On Weapon Swap before While Equipped and While
    // Drawn, which the word also describes. The sort is stable, so the order chosen above
    // holds on either side.
    if !query.trim().is_empty() {
        result.sort_by_key(|group| !pickers::matches(query, group.title));
    }
    result
}

/// A choice Suggested puts ahead of the rest.
#[derive(Clone, Copy)]
enum Lead {
    /// A kill trigger preset, whose kill filter is its own group.
    Kill(Trigger),
    /// Every group of a node kind.
    Kind(u8),
    /// A recipe, by its title.
    Recipe(&'static str),
}

/// The triggers most weapon perks start from, in the order Suggested lists them.
const TRIGGER_LEAD: [Lead; 13] = [
    Lead::Kill(Trigger::WeaponKill),
    Lead::Kill(Trigger::PrecisionKill),
    // While Equipped, While Drawn and Always Active.
    Lead::Kind(14),
    Lead::Kind(16),
    Lead::Kind(0),
    // On Dealing Damage, On Reloading and On Aiming Down Sights.
    Lead::Kind(4),
    Lead::Kind(19),
    Lead::Kind(23),
    Lead::Kill(Trigger::AnyKill),
    // On Taking Damage.
    Lead::Kind(5),
    Lead::Kill(Trigger::MeleeKill),
    Lead::Kill(Trigger::GrenadeKill),
    // On Picking Up Ammo.
    Lead::Kind(6),
];

/// The actions most weapon perks are built from, in the order Suggested lists them.
const ACTION_LEAD: [Lead; 14] = [
    // Change a Weapon or Ability Stat, Change Fired Projectile, Change Outgoing Damage and
    // Reload from Reserves.
    Lead::Kind(10),
    Lead::Kind(26),
    Lead::Kind(40),
    Lead::Kind(16),
    // Adjust Ammo by Capacity, Adjust Ammo and Change Ability Energy for every ability.
    Lead::Kind(15),
    Lead::Kind(14),
    Lead::Kind(8),
    // Spawn an Object or Effect, Attach an Effect and Generate Orbs of Light.
    Lead::Kind(3),
    Lead::Kind(1),
    Lead::Kind(5),
    Lead::Recipe(recipes::FULL_AUTO),
    // Change Damage Type, Change Incoming Damage and Extend Timers.
    Lead::Kind(6),
    Lead::Kind(33),
    Lead::Kind(32),
];

impl Lead {
    fn matches(self, family: &Family) -> bool {
        match (self, family) {
            (Self::Kill(trigger), Family::Condition(condition)) => {
                trigger_family(trigger).as_ref() == Some(condition)
            }
            (Self::Recipe(title), Family::Effect(_, label)) => label.as_deref() == Some(title),
            (Self::Kind(kind), _) => family.kind() == Some(kind),
            _ => false,
        }
    }
}

/// The conditions endings and requirements are most often built from, in the order Suggested
/// lists them. State checks come last because each state is a group of its own.
const CONDITION_LEAD: [Lead; 13] = [
    // Always, After a Delay, On Holster, On Unequip and On Reloading.
    Lead::Kind(0),
    Lead::Kind(1),
    Lead::Kind(17),
    Lead::Kind(15),
    Lead::Kind(19),
    Lead::Kill(Trigger::WeaponKill),
    Lead::Kill(Trigger::AnyKill),
    // On Taking Damage, On Dealing Damage, On Firing This Weapon, On Aiming Down Sights and
    // On Sprinting.
    Lead::Kind(5),
    Lead::Kind(4),
    Lead::Kind(27),
    Lead::Kind(23),
    Lead::Kind(25),
    // State Check.
    Lead::Kind(20),
];

impl Purpose {
    /// The common choices Suggested puts first in this picker. Choose a Condition serves
    /// endings and requirements, so it leads with what those use rather than the everyday
    /// triggers.
    fn leads(self) -> &'static [Lead] {
        match self {
            Self::Trigger => &TRIGGER_LEAD,
            Self::Action => &ACTION_LEAD,
            Self::Condition => &CONDITION_LEAD,
        }
    }
}

/// Where Suggested places a group: its place among the picker's common choices, or after them
/// all.
fn lead_rank(leads: &[Lead], family: &Family) -> usize {
    leads
        .iter()
        .position(|lead| lead.matches(family))
        .unwrap_or(usize::MAX)
}

/// The behavior a group is a variant of, whose uses Suggested pools: a kill with any filter,
/// or ability energy for any ability. Every other group counts its own uses.
fn pool(family: &Family) -> Option<u8> {
    match family {
        Family::Condition(ConditionFamily::Kill { .. } | ConditionFamily::Kind(2)) => Some(2),
        Family::Effect(8, _) => Some(8),
        _ => None,
    }
}

/// A row's detail, or nothing when it only repeats the title.
fn distinct_detail<'a>(detail: &'a str, title: &str) -> &'a str {
    if detail == title { "" } else { detail }
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

/// Real installed examples help explain a behavior without inventing gameplay claims. A
/// recipe's examples are the perks that carry one of its actions exactly.
fn stock_examples(
    group: &Group<'_>,
    catalog: &native::Catalog,
    names: &BTreeMap<u16, String>,
) -> Vec<String> {
    group
        .rows
        .iter()
        .flat_map(|row| match &row.choice {
            Choice::Condition(index) => catalog.conditions[*index]
                .sources
                .iter()
                .collect::<Vec<_>>(),
            Choice::Effect(index) => catalog.effects[*index].sources.iter().collect(),
            Choice::Recipe(nodes) => catalog
                .effects
                .iter()
                .filter(|effect| {
                    nodes
                        .iter()
                        .any(|node| node.kind == effect.kind && node.bytes == effect.bytes)
                })
                .flat_map(|effect| &effect.sources)
                .collect(),
            _ => Vec::new(),
        })
        .filter_map(|source| names.get(&source.perk))
        .filter(|name| !unnamed_ability(name))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The stock perks behind a row, by name, so a search for a perk finds what it does.
fn perk_names(sources: &[native::Source], names: &BTreeMap<u16, String>) -> String {
    sources
        .iter()
        .filter_map(|source| names.get(&source.perk))
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Everyday words for what a behavior does, searched beside its own text, so a search for the
/// goal finds it: "headshot" finds precision kills and "stacks" the counter.
fn goal_words(family: &Family, title: &str) -> String {
    let words = match family {
        Family::Condition(ConditionFamily::Kill { .. }) => KILL_WORDS,
        Family::Condition(_) => family.kind().map_or("", condition_words),
        Family::Effect(kind, _) => action_words(*kind),
    };
    if title.to_lowercase().contains("precision") {
        format!("{words} headshot headshots crit critical")
    } else {
        words.to_owned()
    }
}

const KILL_WORDS: &str = "kill kills killing defeat defeated final blow";

fn condition_words(kind: u8) -> &'static str {
    match kind {
        0 => "passive always permanent constant",
        1 => "timer delay wait seconds",
        2 => KILL_WORDS,
        4 => "hit hits shoot shooting shot damage dealt",
        5 => "hurt damaged hit shield broken",
        6 => "ammo brick pickup scavenger",
        8..=10 => "grenade melee super class ability dodge barricade rift cast",
        14 | 16 => "passive equip equipped draw drawn ready swap switch",
        15 | 17 => "unequip holster stow swap switch",
        19 => "reload reloading reloaded",
        20 | 35 => "while state status",
        22 => "crouch crouching",
        23 => "ads aim aiming scope zoom sights",
        24 => "slide sliding",
        25 => "sprint sprinting run running",
        26 => "stack stacks stacking count counter times in a row after",
        27 => "shoot shooting fire firing shot trigger",
        29 | 30 => "signal",
        31 => "and both while requirement requirements",
        42 => "finisher execute",
        _ => "",
    }
}

fn action_words(kind: u8) -> &'static str {
    match kind {
        1 | 2 => "buff debuff status aura explode explosion blast",
        3 => "spawn explode explosion blast",
        4 => "debuff target",
        5 => "orb orbs power light masterwork",
        6 => "element elemental solar arc void kinetic",
        7 | 8 => "grenade melee super class ability energy cooldown refund recharge",
        10 => "stat stats buff boost range stability handling reload speed aim assist zoom",
        11 | 13 => "ammo drop finder brick special heavy primary",
        14 | 15 => "ammo rounds magazine mag refill return reserves",
        16 => "reload refill magazine mag instant",
        18 | 20 => "radar minimap",
        25 | 26 => "projectile rocket bullet pattern",
        28 | 29 | 35 => "full auto trigger charge firing mode",
        30 => "tracking homing lock on rocket",
        32 => "extend duration timer longer",
        33 => "damage resistance reduction resist defense",
        37 | 54 => "label champion stagger overload pierce unstoppable barrier",
        40 => "damage bonus buff more damage multiplier",
        41 | 42 => "stack stacks counter count",
        43 => "signal",
        49 => "mark marked remember",
        _ => "",
    }
}

/// A perk known only by its ability list and entry reads "Ability 1 / 19" (see the investment
/// ingredients), which names nothing a reader could look up, so it is no example.
fn unnamed_ability(name: &str) -> bool {
    name.strip_prefix("Ability ")
        .and_then(|rest| rest.split_once(" / "))
        .is_some_and(|(list, entry)| list.parse::<u32>().is_ok() && entry.parse::<u32>().is_ok())
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

/// The actions Add Action adds for the recipe with this title, for tests that author perks
/// the way a user picks from the action picker.
#[cfg(test)]
pub(super) fn recipe_actions(
    title: &str,
    catalog: &native::Catalog,
    names: &BTreeMap<u16, String>,
) -> Option<Vec<NativeNode>> {
    recipes::all()
        .into_iter()
        .chain(recipes::adopted(catalog, names))
        .find(|recipe| recipe.title == title)
        .map(|recipe| recipe.nodes)
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
                uses: 0,
                choice: Choice::Trigger(Trigger::WeaponKill),
            },
            Row {
                family,
                enabled: true,
                reason: "",
                title: "A Kill from This Weapon".into(),
                detail: String::new(),
                search: "a kill from this weapon with extra restrictions".into(),
                uses: 0,
                choice: Choice::Condition(42),
            },
            Row {
                family: Family::Condition(trigger_family(Trigger::AnyKill).unwrap()),
                enabled: true,
                reason: "",
                title: "On Any Credited Kill".into(),
                detail: String::new(),
                search: "on any credited kill".into(),
                uses: 0,
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
            Purpose::Trigger.leads(),
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
            Purpose::Trigger.leads(),
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
            uses: 0,
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
                Purpose::Action.leads(),
            )
            .iter()
            .map(|group| group.title.to_owned())
            .collect::<Vec<_>>()
        };
        // Suggested puts the common actions, here the stat change and the spawn, ahead of the
        // rest.
        let suggested = titles(StockUse::Any, Order::Suggested);
        assert_eq!(suggested.len(), 3);
        assert_eq!(suggested[2], "Zero Native Kind");
        // Only the bare kind list carries node kinds no stock perk configures.
        assert_eq!(
            titles(StockUse::Unused, Order::Suggested),
            ["Zero Native Kind"]
        );
        let mut used = titles(StockUse::Used, Order::Suggested);
        used.sort();
        assert_eq!(used, ["Alpha Stock Effect", "Spawn an Effect"]);
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
            Purpose::Action.leads(),
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
                Order::Suggested,
                Purpose::Action.leads()
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
            // Kind 27's traced evidence leaves its gameplay feature unresolved, so it has
            // no plain title and reads only as the operation it was traced to.
            row("Host Activation Counter", 27, Choice::Effect(2)),
        ];
        let titles = |detail| {
            group_rows(
                rows.iter(),
                "",
                StockUse::Any,
                detail,
                Category::Any,
                Order::Suggested,
                Purpose::Action.leads(),
            )
            .iter()
            .map(|group| group.title.to_owned())
            .collect::<Vec<_>>()
        };
        assert_eq!(titles(Detail::Standard), ["Spawn an Object or Effect"]);
        assert_eq!(
            titles(Detail::Advanced),
            ["Spawn an Object or Effect", "Host Activation Counter"]
        );
        // Hiding is per group, so a plain group keeps every configuration it holds.
        let plain = group_rows(
            rows.iter(),
            "",
            StockUse::Any,
            Detail::Standard,
            Category::Any,
            Order::Suggested,
            Purpose::Action.leads(),
        );
        assert_eq!(plain[0].rows.len(), 2);
    }

    #[test]
    fn the_category_filter_narrows_to_one_concern_and_suggested_leads_with_the_common_choices() {
        let condition = |title: &str, kind: u8, uses: usize| Row {
            family: Family::Condition(ConditionFamily::Kind(kind)),
            enabled: true,
            reason: "",
            title: title.into(),
            detail: String::new(),
            search: title.to_lowercase(),
            uses,
            choice: Choice::Kind(kind),
        };
        let mut rows = comparison_rows();
        rows.push(condition("On Reloading", 19, 0));
        rows.push(condition("On a Kill", 2, 3));
        rows.push(condition("On Picking Up Ammo", 6, 1));
        let titles = |category, order| {
            group_rows(
                rows.iter(),
                "",
                StockUse::Any,
                Detail::Standard,
                category,
                order,
                Purpose::Trigger.leads(),
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
        // Suggested leads with the common triggers, here reload and ammo pickup, then the rest
        // by how often the stock perks use them: the kill, then the unused compared states in
        // the order the workbench lists them.
        let suggested = titles(Category::Any, Order::Suggested);
        let mut common = suggested[..2].to_vec();
        common.sort();
        assert_eq!(common, ["On Picking Up Ammo", "On Reloading"]);
        assert_eq!(suggested[2], "On a Kill");
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

    /// A configuration named only by a state hash waits behind Show All. One the stock perks
    /// never name is offered under its kind's plain title, except a kill, and an empty state
    /// check says how it differs from kind 0's Always.
    #[test]
    fn default_stock_configurations_read_in_plain_terms() {
        use sundial::investment::discovery::conditions;
        use sundial::package_authoring::sandbox_perk::program::native_draft;
        let state = |key: u32| {
            let mut node = NativeNode::condition(20).unwrap();
            node.bytes[0x38] = 0;
            node.bytes[0x81] = 0;
            node.bytes[0xD4..0xD8].copy_from_slice(&key.to_le_bytes());
            let program = Program {
                trigger: Trigger::Native,
                native_trigger: Some(node),
                actions: vec![Action::add_rounds(1)],
                ..Program::default()
            };
            let payload = native_draft(&program).unwrap().graph.emit().unwrap();
            conditions::from_payload(&payload)
                .unwrap()
                .into_iter()
                .find(|condition| condition.kind == 20)
                .unwrap()
        };
        let hashed = state(0x1234_5678);
        assert!(hashed.name.is_some(), "{}", hashed.description);
        assert!(!offered_by_default(&hashed), "{}", hashed.description);
        let empty = state(0x811C_9DC5);
        assert!(offered_by_default(&empty));
        assert_eq!(empty.name.as_deref(), Some("Always"));
        assert_ne!(
            stock_detail(&empty),
            program::plain_condition_summary(0).unwrap()
        );
        let unnamed = |kind| conditions::Condition {
            family: ConditionFamily::Kind(kind),
            name: None,
            description: String::new(),
            source: String::new(),
            kind,
            requirements: Vec::new(),
            details: Vec::new(),
            bytes: Vec::new(),
        };
        assert!(offered_by_default(&unnamed(6)));
        assert!(offered_by_default(&unnamed(12)));
        assert!(!offered_by_default(&unnamed(2)));
        assert!(!offered_by_default(&unnamed(32)));
    }

    /// A stock "While Super Active" covers the super_active comparison only. Super Recently
    /// Active compares a different variable, and a numeric state such as nearby enemies reads
    /// as a requirement, not "While", so its comparison stays.
    #[test]
    fn a_stock_state_hides_only_the_comparison_of_its_own_variable() {
        let states = [
            "While Super Active",
            "Meets the Nearby Enemy Count requirement",
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
        let hidden = comparison_rows()
            .into_iter()
            .filter(|row| stock_state_covers(row, &states))
            .map(|row| row.title)
            .collect::<Vec<_>>();
        assert_eq!(hidden, ["Super Active = 1"]);
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
            Purpose::Trigger.leads(),
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
        assert!(!actions.is_empty());
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

    /// A promoted condition is offered to every author, not only one who found Show All, so it
    /// has to read like a guided row rather than an engine kind with a number in front of it.
    /// Both halves matter: the title is what a reader scans for, and the summary is what tells
    /// them it is the one they want.
    #[test]
    fn every_promoted_condition_reads_as_a_plain_row() {
        for kind in program::PROMOTED_NATIVE_CONDITIONS {
            let node = nodes::CONDITIONS
                .iter()
                .find(|node| node.kind == kind)
                .unwrap_or_else(|| panic!("condition kind {kind} is not an engine kind"));
            assert_eq!(
                node.support,
                nodes::Support::Authorable,
                "condition kind {kind} is promoted but cannot be authored"
            );
            let title = program::plain_condition_title(kind);
            assert!(
                title.is_some(),
                "condition kind {kind} is promoted without a plain title"
            );
            assert!(
                program::plain_condition_summary(kind).is_some(),
                "condition kind {kind} is promoted without a plain summary"
            );
            // The picker builds its row from these, so a promoted kind never shows "31: ...".
            assert_eq!(
                super::bare_kind(Purpose::Condition, node).1,
                title.unwrap_or_default()
            );
            // Offering a kind whose node cannot be built would leave the row doing nothing,
            // which is the failure the promoted actions are already guarded against.
            assert!(
                NativeNode::condition(kind).is_some(),
                "condition kind {kind} is promoted but builds no node"
            );
        }
    }

    /// The same promise on the effects side: an effect written up for a reader has to be
    /// reachable without Show All, through a stock configuration, a guided action, or the
    /// promoted list.
    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn every_effect_written_up_for_a_reader_is_offered_without_show_all() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let catalog = sundial::investment::discovery::behaviors::discover(&packages).unwrap();
        let configured = catalog
            .effects
            .iter()
            .map(|effect| effect.kind)
            .collect::<std::collections::BTreeSet<_>>();
        let guided = program::common_actions(&Default::default(), &Default::default())
            .into_iter()
            .map(|(_, _, action)| action.kind())
            .collect::<std::collections::BTreeSet<_>>();
        let hidden = nodes::EFFECTS
            .iter()
            .filter(|node| node.support == nodes::Support::Authorable)
            .filter(|node| program::plain_action_title(node.kind).is_some())
            .filter(|node| {
                !configured.contains(&node.kind)
                    && !guided.contains(&node.kind)
                    && !program::PROMOTED_NATIVE_ACTIONS.contains(&node.kind)
            })
            .map(|node| {
                format!(
                    "{}: {}",
                    node.kind,
                    program::plain_action_title(node.kind).unwrap_or(node.name)
                )
            })
            .collect::<Vec<_>>();
        assert!(
            hidden.is_empty(),
            "effects written up but reachable only behind Show All: {hidden:#?}"
        );
    }

    /// Offering a condition is a promise that a perk using it builds.
    ///
    /// Constructing the node is only the first half. The compiler writes the whole program into
    /// a package, and a condition whose record it refuses turns a row someone chose into a build
    /// that fails with an engine message. Every condition the picker offers is compiled here
    /// against the installed packages, so the promise is checked rather than assumed.
    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn every_offered_condition_compiles_into_a_program() {
        use sundial::package_authoring::open_shadowkeep_package_manager;
        use sundial::package_authoring::sandbox_perk::program::{
            Action, Position, Program, Trigger, compile,
        };
        let packages =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let manager = open_shadowkeep_package_manager(&packages).unwrap();
        let failed = nodes::CONDITIONS
            .iter()
            .filter(|node| node.support == nodes::Support::Authorable)
            .filter_map(|node| {
                let Some(condition) = NativeNode::condition(node.kind) else {
                    return Some(format!("{}: {} builds no node", node.kind, node.name));
                };
                let program = Program {
                    trigger: Trigger::Native,
                    native_trigger: Some(condition),
                    actions: vec![Action::generate_orb(Position::Event)],
                    ..Program::default()
                };
                compile(&manager, &program)
                    .err()
                    .map(|error| format!("{}: {} -> {error}", node.kind, node.name))
            })
            .collect::<Vec<_>>();
        assert!(
            failed.is_empty(),
            "conditions that do not build: {failed:#?}"
        );
    }

    /// Every condition written up for a reader has to be reachable without Show All.
    ///
    /// A plain title and summary exist for exactly the kinds meant to be offered to an author.
    /// A kind reaches the picker one of two ways: a stock configuration carrying a recognized
    /// name, or promotion of the bare kind. A kind with neither is written up and then hidden
    /// behind a switch described as unnamed configurations and empty kinds, which is how the
    /// effect's counter and requiring two things at once were both lost.
    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn every_condition_written_up_for_a_reader_is_offered_without_show_all() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let catalog = sundial::investment::discovery::behaviors::discover(&packages).unwrap();
        let named = catalog
            .conditions
            .iter()
            .filter(|entry| entry.condition.name.is_some())
            .map(|entry| entry.condition.kind)
            .collect::<std::collections::BTreeSet<_>>();
        let hidden = (0..=u8::MAX)
            .filter(|kind| program::plain_condition_title(*kind).is_some())
            .filter(|kind| {
                !named.contains(kind) && !program::PROMOTED_NATIVE_CONDITIONS.contains(kind)
            })
            .map(|kind| {
                format!(
                    "{kind}: {}",
                    program::plain_condition_title(kind).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>();
        assert!(
            hidden.is_empty(),
            "written up but reachable only behind Show All: {hidden:#?}"
        );
    }

    /// Requiring two things at once is a basic thing to want, and it was reachable only behind
    /// Show All, which is described as unnamed configurations and empty kinds.
    ///
    /// Being in the list is half of being found. A picker row is matched as a substring of the
    /// text it is built from, so the words an author would type have to appear in it: they will
    /// reach for "and" or "both" long before they reach for the engine's "subgroup".
    #[test]
    fn requiring_two_things_at_once_is_offered_and_searchable_without_show_all() {
        assert!(program::PROMOTED_NATIVE_CONDITIONS.contains(&31));
        let node = nodes::CONDITIONS
            .iter()
            .find(|node| node.kind == 31)
            .expect("the all-requirements condition is an engine kind");
        let title = super::bare_kind(Purpose::Condition, node).1;
        let detail = super::bare_kind_summary(Purpose::Condition, node);
        assert_eq!(title, "When All Requirements Are Met");
        let search = format!(
            "{:02} {} {} {title} {detail}",
            node.kind, node.name, node.summary
        );
        for query in [
            "and",
            "both",
            "all",
            "requirement",
            "every",
            "requirements are met",
        ] {
            assert!(
                crate::app::pickers::matches(query, &search),
                "a search for {query:?} does not reach the all-requirements condition"
            );
        }
    }

    #[test]
    fn every_guided_action_is_offered_under_the_name_its_card_carries() {
        let program = Program::default();
        for (title, _, action) in program::common_actions(&program, &Default::default()) {
            assert_eq!(
                title,
                program::action_title(&action),
                "effect kind {} is offered under a name its card does not carry",
                action.kind()
            );
        }
    }

    /// A plain name is a promise that the workbench can place the thing it names. A kind
    /// named without a buildable record would offer a row that produces nothing.
    #[test]
    fn every_plain_name_belongs_to_a_kind_the_workbench_can_place() {
        use sundial::package_authoring::sandbox_perk::action::native::fields;
        use sundial::package_authoring::sandbox_perk::nodes;
        use sundial::package_authoring::sandbox_perk::program::NativeNode;
        for kind in 0..=u8::MAX {
            assert_eq!(
                program::plain_action_title(kind).is_some(),
                program::plain_action_summary(kind).is_some(),
                "effect kind {kind} has an incomplete description"
            );
            assert_eq!(
                program::plain_condition_title(kind).is_some(),
                program::plain_condition_summary(kind).is_some(),
                "condition kind {kind} has an incomplete description"
            );
            if let Some(title) = program::plain_action_title(kind) {
                let node = NativeNode::effect(kind)
                    .unwrap_or_else(|| panic!("{title} (effect kind {kind}) builds no record"));
                assert_eq!(node.kind, kind);
                assert!(!node.bytes.is_empty(), "{title} builds an empty record");
                let class = nodes::effect(kind).unwrap().class;
                assert!(
                    fields::describe(class)
                        .unwrap()
                        .iter()
                        .any(|field| field.editable),
                    "{title} has no editable field"
                );
            }
            if let Some(title) = program::plain_condition_title(kind) {
                let node = NativeNode::condition(kind)
                    .unwrap_or_else(|| panic!("{title} (condition kind {kind}) builds no record"));
                assert_eq!(node.kind, kind);
                assert!(!node.bytes.is_empty(), "{title} builds an empty record");
            }
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
            &[],
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
                &[],
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
            &[],
        )
        .iter()
        .map(|group| group.title.to_owned())
        .collect::<Vec<_>>();
        assert_eq!(ordered, ["Spawn an Effect", "A Kill"]);
    }
}
