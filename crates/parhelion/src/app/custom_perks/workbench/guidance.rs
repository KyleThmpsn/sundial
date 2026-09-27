//! Discovery and plain-language guidance derived from the behavior actually being authored.
use super::*;
use sundial::package_authoring::sandbox_perk::{
    dependencies::Behavior,
    nodes::{CONDITIONS, EFFECTS, condition_title, effect_title},
    program::{Action, KeyCatalog, Program, Trigger},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Purpose {
    #[default]
    All,
    Projectiles,
    Patterns,
    Timers,
    Properties,
    Ammunition,
}

impl Purpose {
    const ALL: [Self; 6] = [
        Self::All,
        Self::Projectiles,
        Self::Patterns,
        Self::Timers,
        Self::Properties,
        Self::Ammunition,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All Behavior",
            Self::Projectiles => "Projectiles and Emitters",
            Self::Patterns => "Weapon Patterns",
            Self::Timers => "Timed Effects",
            Self::Properties => "Properties and Stats",
            Self::Ammunition => "Ammo",
        }
    }

    #[cfg(test)]
    pub(super) fn allows(self, behavior: Option<&Behavior>) -> bool {
        self == Self::All || self.allows_search(behavior.map(behavior_search).as_deref())
    }

    /// Whether a behavior passes, given its `behavior_search` text, or `None` when there is
    /// no behavior.
    pub(super) fn allows_search(self, search: Option<&str>) -> bool {
        if self == Self::All {
            return true;
        }
        let Some(search) = search else {
            return false;
        };
        let text = search.to_lowercase();
        let words: &[&str] = match self {
            Self::All => &[],
            Self::Projectiles => &["entity", "projectile", "emitter", "spawn"],
            Self::Patterns => &["pattern"],
            Self::Timers => &["timer", "cooldown", "duration"],
            Self::Properties => &["property", "modifier", "numeric", "stat"],
            Self::Ammunition => &["ammunition", "magazine", "reserves", "round"],
        };
        words.iter().any(|word| text.contains(word))
    }
}

/// Include the displayed digest, the names the workbench gives each node kind and the
/// registered operation names in discovery search.
pub(super) fn behavior_search(behavior: &Behavior) -> String {
    let mut text = behavior.headline.clone();
    for section in &behavior.details {
        for line in &section.lines {
            text.push(' ');
            text.push_str(&line.text);
            for field in &line.fields {
                text.push(' ');
                text.push_str(field);
            }
        }
    }
    for node in EFFECTS
        .iter()
        .filter(|node| behavior.effect_kinds.contains(&node.kind))
    {
        text.push(' ');
        text.push_str(effect_title(node.kind));
        text.push(' ');
        text.push_str(node.name);
        text.push(' ');
        text.push_str(node.summary);
    }
    for node in CONDITIONS
        .iter()
        .filter(|node| behavior.condition_kinds.contains(&node.kind))
    {
        text.push(' ');
        text.push_str(condition_title(node.kind));
        text.push(' ');
        text.push_str(node.name);
        text.push(' ');
        text.push_str(node.summary);
    }
    text
}

/// Add from Perk opens on every stock effect, since each can be added and the ones that read
/// as programs then edit in place.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum EditingFilter {
    #[default]
    All,
    Programs,
    Stock,
}

impl EditingFilter {
    const ALL: [Self; 3] = [Self::All, Self::Programs, Self::Stock];

    /// The same words the effect cards use for their support badge.
    fn label(self) -> &'static str {
        match self {
            Self::All => "Any Support",
            Self::Programs => "Editable as a Program",
            Self::Stock => "Stock Only",
        }
    }

    pub(super) fn allows(self, behavior: Option<&Behavior>) -> bool {
        match self {
            Self::All => true,
            Self::Programs => behavior.is_some_and(|value| value.editable),
            Self::Stock => behavior.is_some_and(|value| !value.editable),
        }
    }
}

/// How the stock effect results are ordered. Order changes presentation only. It never
/// hides an effect and never changes which effects can be copied.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum EffectOrder {
    /// Effects whose own name matches the query first, then the famous perks in the order
    /// `EFFECT_LEAD` gives, then what weapons carry, then armor, then abilities, with
    /// cosmetic sources last.
    #[default]
    Suggested,
    Name,
    Kind,
}

impl EffectOrder {
    pub(super) const ALL: [Self; 3] = [Self::Suggested, Self::Name, Self::Kind];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Suggested => "Suggested",
            Self::Name => "Name",
            Self::Kind => "Item Type",
        }
    }

    pub(super) fn hint(self) -> &'static str {
        match self {
            Self::Suggested => "Search matches, then famous perks, then weapon perks.",
            Self::Name => "Every result by name.",
            Self::Kind => "Grouped by the item type that carries the effect, then by name.",
        }
    }
}

/// The perks Suggested lists first, by their stock names: famous weapon perks, then exotic
/// weapon perks.
pub(super) const EFFECT_LEAD: &[&str] = &[
    "Rampage",
    "Outlaw",
    "Kill Clip",
    "Firefly",
    "Explosive Payload",
    "Dragonfly",
    "Headseeker",
    "Swashbuckler",
    "Demolitionist",
    "Multikill Clip",
    "Feeding Frenzy",
    "Triple Tap",
    "Fourth Time's the Charm",
    "Subsistence",
    "Rapid Hit",
    "Vorpal Weapon",
    "Desperado",
    "Moving Target",
    "Snapshot Sights",
    "Quickdraw",
    "Timed Payload",
    "Surrounded",
    "Opening Shot",
    "Box Breathing",
    "Killing Wind",
    "Full Court",
    "Auto-Loading Holster",
    "Lightning Rounds",
    "Reign Havoc",
    "White Nail",
    "Arc Conductor",
    "Cosmology",
    "Repulsor Force",
    "The Corruption Spreads",
    "Memento Mori",
    "Release the Wolves",
    "Payday",
    "Pyrotoxin Rounds",
    "Judgment",
    "Lightning Rod",
    "Poison Arrows",
    "Sun Blast",
    "Sunburn",
    "String of Curses",
    "Honed Edge",
    "Compounding Force",
    "Conserve Momentum",
    "Transmutation",
    "The Fundamentals",
    "Arc Traps",
    "Last Stand",
    "MIDA Radar",
    "Rat Pack",
];

/// Where Suggested places a stock effect: the famous perks in their order, then by what carries
/// it. A weapon perk comes before an armor perk, then an ability's, then one only a Sparrow,
/// Ship or Ghost Shell carries, then one no item is known to carry.
pub(super) fn effect_rank(
    name: &str,
    sources: Option<&BTreeSet<sundial::investment::IngredientSource>>,
) -> (usize, u8) {
    use sundial::investment::IngredientSource::{Ability, Armor, Weapon};
    let lead = EFFECT_LEAD
        .iter()
        .position(|lead| lead.eq_ignore_ascii_case(name.trim()))
        .unwrap_or(usize::MAX);
    let carrier = sources
        .into_iter()
        .flatten()
        .map(|source| match source {
            Weapon => 0,
            Armor => 1,
            Ability => 2,
            _ => 3,
        })
        .min()
        .unwrap_or(4);
    (lead, carrier)
}

/// The comparable key for one stock effect result. Every order ends with the name, so
/// results never reshuffle between frames. `rank` is [`effect_rank`], which only Suggested
/// reads.
pub(super) fn effect_sort_key(
    order: EffectOrder,
    item_type: &str,
    name: &str,
    direct_match: bool,
    rank: (usize, u8),
) -> (bool, (usize, u8), String, String) {
    let name = name.to_owned();
    match order {
        EffectOrder::Suggested => (!direct_match, rank, String::new(), name),
        EffectOrder::Name => (false, (0, 0), String::new(), name),
        EffectOrder::Kind => (false, (0, 0), item_type.to_lowercase(), name),
    }
}

/// Width of one filter in a browser toolbar.
///
/// A combo takes the width of its selected text, and `ComboBox::width` only sets a floor,
/// so a filter is drawn inside an allocation of this width and truncates to it, with the
/// full reading on hover. This is the width at which the everyday choices still read in
/// full. A toolbar that places these filters reserves the same number for each of them.
pub(super) const FILTER_WIDTH: f32 = 150.0;

pub(super) fn filters(
    ui: &mut egui::Ui,
    purpose: &mut Purpose,
    editing: &mut EditingFilter,
    width: f32,
) -> bool {
    // A caller may offer more room than the shared width, never less: below it these
    // choices elide to a few letters and the toolbar stops saying what it filters by.
    let width = width.max(FILTER_WIDTH);
    let before = (*purpose, *editing);
    let purpose_text = purpose.label();
    controls::sized(ui, width, |ui| {
        egui::ComboBox::from_id_salt("behavior-purpose")
            .width(width)
            .truncate()
            .selected_text(purpose_text)
            .show_ui(ui, |ui| {
                for value in Purpose::ALL {
                    ui.selectable_value(purpose, value, value.label());
                }
            })
            .response
            .on_hover_text(format!("Behavior Purpose: {purpose_text}"));
        pickers::name_combo(ui, "behavior-purpose", "Behavior Purpose");
    });
    let editing_text = editing.label();
    controls::sized(ui, width, |ui| {
        egui::ComboBox::from_id_salt("behavior-editing")
            .width(width)
            .truncate()
            .selected_text(editing_text)
            .show_ui(ui, |ui| {
                for value in EditingFilter::ALL {
                    ui.selectable_value(editing, value, value.label());
                }
            })
            .response
            .on_hover_text(format!("Editing Support: {editing_text}"));
        pickers::name_combo(ui, "behavior-editing", "Editing Support");
    });
    before != (*purpose, *editing)
}

/// Summarize routing and retained lifetime without inventing an asset's gameplay effect.
#[cfg(test)]
pub(super) fn summary(program: &Program, keys: Option<&KeyCatalog>) -> String {
    summary_with_assets(program, keys, None)
}

/// A native program's reading, with each object or effect it references named where the
/// catalog names it rather than by its tag.
pub(super) fn native_summary(
    decoded: &sundial::package_authoring::sandbox_perk::action::DecodedAction,
    labels: Option<&BTreeMap<u32, String>>,
) -> sundial::package_authoring::sandbox_perk::action::ActionSummary {
    let mut summary = sundial::package_authoring::sandbox_perk::action::ActionSummary::new(decoded);
    for line in summary
        .groups
        .iter_mut()
        .flat_map(|group| &mut group.effects)
    {
        if let Some((tag, label)) = line
            .asset
            .and_then(|tag| labels?.get(&tag).map(|label| (tag, label)))
        {
            line.text = line.text.replace(&format!("0x{tag:08X}"), label);
        }
    }
    summary
}

pub(super) fn summary_with_assets(
    program: &Program,
    keys: Option<&KeyCatalog>,
    labels: Option<&BTreeMap<u32, String>>,
) -> String {
    if let Some(native) = &program.native {
        return native
            .graph
            .emit()
            .and_then(|bytes| sundial::package_authoring::sandbox_perk::action::decode(&bytes))
            .map(|decoded| native_summary(&decoded, labels).render())
            .unwrap_or_else(|error| format!("Complete program needs attention: {error}"));
    }
    let mut sentences = vec![match (program.trigger, &program.native_trigger) {
        (Trigger::Always, _) => "Actions run when the perk is applied.".into(),
        (Trigger::Native, Some(node)) => format!("Trigger: {}.", program::condition_title(node)),
        _ => program.trigger.description().to_owned(),
    }];
    if program.trigger.is_event() && program.chance_permyriad != 10_000 {
        sentences.push(format!(
            "Each matching kill has a {}% activation chance.",
            f32::from(program.chance_permyriad) / 100.0
        ));
    }
    for action in &program.actions {
        let text = action
            .asset()
            .and_then(|asset| labels.and_then(|labels| labels.get(&asset.graph)))
            .map(|name| match action {
                Action::Spawn { position, .. } => format!(
                    "Create {name} {}",
                    match position {
                        sundial::package_authoring::sandbox_perk::program::Position::Owner =>
                            "at your position",
                        sundial::package_authoring::sandbox_perk::program::Position::Event =>
                            "at the triggering event",
                    }
                ),
                Action::Attach { .. } => format!("Attach {name}"),
                Action::Pattern { .. } => format!("Fire {name}"),
                _ => program::action_text(action, keys),
            })
            .unwrap_or_else(|| program::action_text(action, keys));
        sentences.push(format!(
            "{}.",
            if program.trigger.is_event() {
                text.replace("at the triggering event", "at the defeated enemy")
            } else {
                text
            }
        ));
    }
    if program.actions.is_empty() {
        sentences.push("Choose Add Action to give this effect behavior.".into());
    }
    let retained = program.actions.iter().any(Action::retained);
    if retained && let Some(removal) = program::removal_text(program, keys) {
        sentences.push(format!("Retained effects end: {removal}."));
    }
    if program.cooldown_ms != 0 && program.trigger.supports_cooldown() {
        sentences.push(if program.trigger == Trigger::Always {
            format!(
                "Repeat every {} seconds.",
                program.cooldown_ms as f32 / 1000.0
            )
        } else {
            format!("Cooldown: {} seconds.", program.cooldown_ms as f32 / 1000.0)
        });
    }
    sentences.join(" ")
}

impl Workbench {
    pub(super) fn copy_behavior(&mut self, choice: &WeaponSandboxPerkChoice) {
        let mut recipe = PerkRecipe::new();
        // The copy carries its perk's name. The effect number names nothing a reader knows.
        recipe.name = if choice.representative_name.starts_with("Ability ")
            || choice.representative_name.starts_with("Effect ")
        {
            format!("Custom Effect {}", choice.perk_index)
        } else {
            format!("Custom {}", choice.representative_name)
        };
        if !choice.representative_type_name.starts_with("Ability ·") {
            recipe.template_plug = choice.representative_hash.into();
        }
        recipe.description = self
            .discovery
            .behavior(choice.perk_index)
            .map(|behavior| behavior.headline.clone())
            .unwrap_or_default();
        recipe.effects.push(PerkRecipe::effect(choice.perk_index));
        self.add_document(Document::new(recipe, None));
        self.page = Page::Effects;
        self.open = true;
    }
}
