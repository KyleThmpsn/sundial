//! Discovery and plain-language guidance derived from the behavior actually being authored.
use super::*;
use sundial::package_authoring::sandbox_perk::{
    dependencies::Behavior,
    nodes::{CONDITIONS, EFFECTS},
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
            Self::Ammunition => "Ammunition",
        }
    }

    pub(super) fn allows(self, behavior: Option<&Behavior>) -> bool {
        if self == Self::All {
            return true;
        }
        let Some(behavior) = behavior else {
            return false;
        };
        let text = behavior_search(behavior).to_lowercase();
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

/// Include the displayed digest and registered operation names in discovery search.
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
        .chain(
            CONDITIONS
                .iter()
                .filter(|node| behavior.condition_kinds.contains(&node.kind)),
        )
    {
        text.push(' ');
        text.push_str(node.name);
        text.push(' ');
        text.push_str(node.summary);
    }
    text
}

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
    /// Effects whose own name matches the query come first.
    #[default]
    BestMatch,
    Name,
    Kind,
}

impl EffectOrder {
    pub(super) const ALL: [Self; 3] = [Self::BestMatch, Self::Name, Self::Kind];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::BestMatch => "Best Match",
            Self::Name => "Name",
            Self::Kind => "Item Type",
        }
    }

    pub(super) fn hint(self) -> &'static str {
        match self {
            Self::BestMatch => "Effects whose own name matches the search come first.",
            Self::Name => "Every result by name.",
            Self::Kind => "Grouped by the item type that carries the effect, then by name.",
        }
    }
}

/// The comparable key for one stock effect result. Every order ends with the name, so
/// results never reshuffle between frames.
pub(super) fn effect_sort_key(
    order: EffectOrder,
    item_type: &str,
    name: &str,
    direct_match: bool,
) -> (bool, String, String) {
    let name = name.to_owned();
    match order {
        EffectOrder::BestMatch => (!direct_match, String::new(), name),
        EffectOrder::Name => (false, String::new(), name),
        EffectOrder::Kind => (false, item_type.to_lowercase(), name),
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
            .map(|decoded| {
                sundial::package_authoring::sandbox_perk::action::ActionSummary::new(&decoded)
                    .render()
            })
            .unwrap_or_else(|error| format!("Complete program needs attention: {error}"));
    }
    let mut sentences = vec![match (program.trigger, &program.native_trigger) {
        (Trigger::Always, _) => "Actions run when the perk is applied.".into(),
        (Trigger::Native, Some(node)) => {
            format!(
                "Actions start when {}.",
                program::native_condition_text(node)
            )
        }
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
                Action::Attach { .. } => format!("Attach {name} to the weapon"),
                Action::Pattern { .. } => format!("Fire {name}"),
                _ => program::action_text(action, keys),
            })
            .unwrap_or_else(|| program::action_text(action, keys));
        sentences.push(format!(
            "{}.",
            if program.trigger.is_event() {
                text.replace("at the triggering event", "at the defeated enemy")
                    .replace("at the Event Position", "at the defeated enemy")
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
        recipe.name = format!("Custom Effect {}", choice.perk_index);
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
