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

pub(super) fn filters(
    ui: &mut egui::Ui,
    purpose: &mut Purpose,
    editing: &mut EditingFilter,
    width: f32,
) -> bool {
    let before = (*purpose, *editing);
    egui::ComboBox::from_id_salt("behavior-purpose")
        .width(width)
        .truncate()
        .selected_text(purpose.label())
        .show_ui(ui, |ui| {
            for value in Purpose::ALL {
                ui.selectable_value(purpose, value, value.label());
            }
        });
    egui::ComboBox::from_id_salt("behavior-editing")
        .width(width)
        .truncate()
        .selected_text(editing.label())
        .show_ui(ui, |ui| {
            for value in EditingFilter::ALL {
                ui.selectable_value(editing, value, value.label());
            }
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
    pub(super) fn draw_starting_points(
        &mut self,
        ui: &mut egui::Ui,
        choices: &[WeaponSandboxPerkChoice],
    ) {
        egui::CollapsingHeader::new("Getting Started")
            .default_open(self.documents.get(self.selected).is_some_and(|document| document.recipe.effects.is_empty()))
            .show(ui, |ui| {
                ui.label("A perk is a set of effects. Each effect says when it starts, what it does and when it ends.");
                for step in [
                    "1. Copy an example below, or add an existing behavior.",
                    "2. Change what it does, then Save Perk.",
                    "3. Pick a socket under Weapon Sockets and Apply to Weapon.",
                ] {
                    crate::app::style::hint(ui, step);
                }
                ui.add_space(4.0);
                egui::Grid::new("getting-started-examples")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        for (source, explanation) in [
                            ("Micro-Missile", "A projectile pattern with movement properties you can change."),
                            ("Dragonfly", "An effect that starts on a precision kill."),
                            ("Cluster Bomb", "Spawned projectiles that live on their own after the effect ends."),
                        ] {
                            let choice = choices.iter().find(|choice| choice.representative_name.eq_ignore_ascii_case(source));
                            if ui.add_enabled(choice.is_some(), egui::Button::new(format!("Copy {source}")))
                                .on_disabled_hover_text("This stock behavior is not available in the loaded catalog.").clicked()
                                && let Some(choice) = choice
                            {
                                self.copy_behavior(choice);
                            }
                            ui.label(explanation);
                            ui.end_row();
                        }
                    });
            });
    }

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
