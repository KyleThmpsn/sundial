//! The rows the behavior pickers list, and how they are filtered, grouped and ordered.
use super::*;
use sundial::package_authoring::sandbox_perk::program::Asset;

/// The family a standard trigger belongs to. A native trigger carries its own node and so
/// has none of its own, which the picker shows by not offering it among the standard rows.
pub(super) fn trigger_family(trigger: Trigger) -> Option<ConditionFamily> {
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

pub(super) fn effect_family(kind: u8, bytes: &[u8]) -> Family {
    Family::Effect(
        kind,
        (kind == 8).then(|| program::native_action_label(kind, bytes)),
    )
}

pub(super) fn action_family(action: &Action) -> Family {
    match action {
        Action::Native { node } => effect_family(node.kind, &node.bytes),
        typed => effect_family(typed.kind(), &[]),
    }
}

/// A recipe's row in Add Action, refused with `reason` when the effect has no room for it.
pub(super) fn recipe_row(recipe: recipes::Recipe, reason: &'static str) -> Row {
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

/// Whether the effect already attaches `graph`, as a guided action or a native node.
fn attaches(program: &Program, graph: u32) -> bool {
    program.actions.iter().any(|action| match action {
        Action::Attach { asset, .. } => asset.graph == graph,
        Action::Native { node } => {
            node.kind == 1
                && node
                    .bytes
                    .get(16..20)
                    .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
                    .map(u32::from_le_bytes)
                    == Some(graph)
        }
        _ => false,
    })
}

/// Change Weapon Properties' row in Add Action: modifier rows attached to the weapon itself,
/// every one neutral until set. `asset` is that attachment once its rows are read, or why it
/// is not ready. One effect holds one, since an effect keeps its edits of an asset by the
/// asset.
pub(super) fn weapon_properties_row(program: &Program, asset: Result<Asset, &'static str>) -> Row {
    use sundial::package_authoring::sandbox_perk::program::AttachmentTarget;
    let title = recipes::WEAPON_PROPERTIES;
    let detail = "Changes bullets per shot, firing speed, spread width, damage, magazine size and more. Rows start with no effect. Set simultaneous pellets with Pellets per Bullet under Gameplay's Barrel Settings.";
    let (reason, action) = if program.actions.len() >= ACTION_LIMIT {
        (
            "This effect already holds the most actions a program can run.",
            None,
        )
    } else if attaches(program, recipes::WEAPON_PROPERTIES_GRAPH) {
        ("This effect already changes weapon properties.", None)
    } else {
        match asset {
            Ok(asset) => (
                "",
                Some(Action::Attach {
                    asset,
                    mode: AttachmentTarget::ThisItem,
                    keys: [sundial::package_authoring::FNV1_EMPTY_HASH; 2],
                    float_bits: [0; 4],
                }),
            ),
            Err(reason) => (reason, None),
        }
    };
    Row {
        family: Family::Effect(1, Some(title.to_owned())),
        enabled: reason.is_empty(),
        reason,
        title: title.to_owned(),
        detail: detail.to_owned(),
        search: format!("{title} {detail}"),
        uses: 0,
        choice: Choice::Action(action.unwrap_or_else(|| Action::attach(Asset::default()))),
    }
}

/// Whether the game's own perks configure this behavior, counted from the installed perk
/// data. This is an engine fact about the installation, not a statement about the
/// workbench: a behavior the stock perks never use is still authorable.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum StockUse {
    #[default]
    Any,
    /// At least one stock perk carries a configuration of this node kind.
    Used,
    /// The engine accepts this node kind, but no installed stock perk configures it.
    Unused,
}

impl StockUse {
    pub(super) const ALL: [Self; 3] = [Self::Any, Self::Used, Self::Unused];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Any => "Any Stock Use",
            Self::Used => "Used by Stock Perks",
            Self::Unused => "Unused by Stock Perks",
        }
    }

    pub(super) fn hint(self) -> &'static str {
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
pub(super) enum Detail {
    /// Only behaviors with a name and description in the words a player uses.
    #[default]
    Standard,
    /// Everything, including the ones named only by their traced engine operation.
    Advanced,
}

impl Detail {
    pub(super) const ALL: [Self; 2] = [Self::Standard, Self::Advanced];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Advanced => "Advanced",
        }
    }

    pub(super) fn hint(self) -> &'static str {
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
pub(super) enum Category {
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
    pub(super) fn choices(purpose: Purpose) -> Vec<Self> {
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

    pub(super) fn label(self) -> &'static str {
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

    pub(super) fn hint(self) -> &'static str {
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
    pub(super) fn of(family: &Family) -> Self {
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
pub(super) enum Order {
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
    pub(super) const ALL: [Self; 4] = [Self::Suggested, Self::Name, Self::Kind, Self::StockUse];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Suggested => "Suggested",
            Self::Name => "Name",
            Self::Kind => "Kind Number",
            Self::StockUse => "Stock Use",
        }
    }

    pub(super) fn hint(self) -> &'static str {
        match self {
            Self::Suggested => "Common choices first, then the most used by the game's perks.",
            Self::Name => "Every behavior by name.",
            Self::Kind => "By the engine's node kind number, then by name.",
            Self::StockUse => "What the game's own perks configure most, counted first.",
        }
    }
}

/// The family and name of a bare node kind, read from the node its row places, so the row
/// joins the configurations that read alike and carries the name its card will.
pub(super) fn bare_kind(purpose: Purpose, kind: &nodes::NodeKind) -> (Family, String) {
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
pub(super) fn bare_kind_summary(purpose: Purpose, kind: &nodes::NodeKind) -> String {
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
pub(super) fn former_title(kind: u8) -> &'static str {
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
pub(super) fn comparison_rows() -> Vec<Row> {
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
pub(super) fn offered_by_default(
    condition: &sundial::investment::discovery::conditions::Condition,
) -> bool {
    match &condition.name {
        // The reading `summary::unnamed_state_description` gives a state known only by hash.
        Some(_) => !condition.description.contains("the unnamed state 0x"),
        None => condition.kind != 2 && program::plain_condition_title(condition.kind).is_some(),
    }
}

/// A stock configuration's sentence. A plain one replaces the decoded one where the kind has
/// it, so the row reads the way the guided actions do. An empty state check reads Always or
/// Never, and kind 0 reads Always too, so it says which it is.
pub(super) fn stock_detail(
    condition: &sundial::investment::discovery::conditions::Condition,
) -> String {
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
pub(super) fn stock_state_covers(row: &Row, states: &BTreeSet<&str>) -> bool {
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

/// Why this action cannot be added to this program, or an empty string when it can. Every
/// rule here mirrors one the compiler or `Program::validate` enforces, so the picker refuses
/// exactly what a build would refuse.
pub(super) fn action_reason(program: &Program, action: &Action) -> &'static str {
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
    pub(super) fn kind(&self) -> Option<u8> {
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
    pub(super) fn configured_by_stock_perks(&self) -> bool {
        !matches!(self.choice, Choice::Kind(_))
    }

    /// Whether the workbench can describe this row in the words a player uses. Triggers and
    /// the composed actions are written that way by hand; a native effect qualifies when
    /// its kind has a plain title.
    pub(super) fn plain_language(&self) -> bool {
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
    pub(super) fn key(&self) -> u64 {
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

/// Whether Suggested counts a stock perk. Once the installation is read, only a perk a weapon,
/// armor piece or ability carries counts. Vehicle and Ghost Shell sockets hold cosmetic plugs,
/// such as a Ship's transmat effect. The asset picker counts the perks referencing an asset
/// the same way.
pub(in crate::app::custom_perks::workbench) fn counts(
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

pub(super) fn group_rows<'a>(
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
