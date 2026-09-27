//! Locate authoring failures using the same validators that gate saving and attachment.
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Location {
    pub document: String,
    pub effect: u16,
    pub action: Option<usize>,
    pub native: Option<sundial::package_authoring::sandbox_perk::program::NativeIssue>,
}

pub(super) struct Issue {
    pub message: String,
    pub location: Option<Location>,
    /// Whether it keeps Apply to Weapon and the socket picker closed. A warning names a likely
    /// mistake the game still runs, so it is shown but never blocks.
    pub blocking: bool,
}

impl Workbench {
    /// The perk's first problem, or its first warning when it has no problem.
    pub(super) fn validation_issue(&self, recipe: &PerkRecipe) -> Option<Issue> {
        if let Some(message) = self.perk_issue(recipe) {
            return Some(self.locate(recipe, message, true));
        }
        let message = perk_warning(recipe)?;
        Some(self.locate(recipe, message, false))
    }

    /// The effect, action and native field an issue belongs to, so Show Problem can open it.
    fn locate(&self, recipe: &PerkRecipe, message: String, blocking: bool) -> Issue {
        // Document-level failures have no effect to open. Never guess from error wording.
        let mut document = recipe.clone();
        document.effects.clear();
        if blocking && document.validate().is_err() {
            return Issue {
                message,
                location: None,
                blocking,
            };
        }
        for (position, effect) in recipe.effects.iter().enumerate() {
            let flagged = if blocking {
                document.effects = vec![effect.clone()];
                document.validate().is_err()
                    || self
                        .discovery
                        .perk_issue(effect.source_perk_index)
                        .is_some()
                    || counter_issue(effect.program.as_ref()).is_some()
                    || choice_issue(effect.program.as_ref()).is_some()
            } else {
                ending_issue(effect.program.as_ref()).is_some()
            };
            if !flagged {
                continue;
            }
            // Every effect edits in the node design, so a problem is revealed there.
            let native = effect.program.as_ref().and_then(|program| {
                if let Some(native) = &program.native {
                    native.authoring_issue().ok().flatten()
                } else {
                    sundial::package_authoring::sandbox_perk::program::native_draft(program)
                        .ok()
                        .and_then(|native| native.authoring_issue().ok().flatten())
                }
            });
            let action = effect.program.as_ref().and_then(|program| {
                if program.native.is_some() {
                    return None;
                }
                let mut one = program.clone();
                one.actions.clear();
                if one.validate().is_err() {
                    return None;
                }
                program
                    .actions
                    .iter()
                    .enumerate()
                    .find_map(|(index, action)| {
                        one.actions = vec![action.clone()];
                        one.validate().is_err().then_some(index)
                    })
            });
            let context = action.map_or_else(
                || format!("Effect {}", position + 1),
                |index| format!("Effect {}, Action {}", position + 1, index + 1),
            );
            return Issue {
                message: format!("{context}: {message}"),
                location: Some(Location {
                    document: recipe.id.clone(),
                    effect: effect.source_perk_index,
                    action,
                    native,
                }),
                blocking,
            };
        }
        Issue {
            message,
            location: None,
            blocking,
        }
    }

    pub(super) fn finish_reveal(&mut self, target: Option<Location>, response: &egui::Response) {
        if let Some(target) = target {
            let unhandled_action = self.reveal_action.take().is_some();
            if target.native.is_none() && (target.action.is_none() || unhandled_action) {
                response.scroll_to_me(Some(egui::Align::Min));
            }
            self.reveal_problem = None;
        }
    }

    pub(super) fn draw_issue(&mut self, ui: &mut egui::Ui, issue: &Issue) {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, &issue.message);
            if issue.location.is_some()
                && ui
                    .add_enabled(
                        self.editor.is_none(),
                        egui::Button::new("Show Problem").small(),
                    )
                    .clicked()
            {
                self.reveal_problem = issue.location.clone();
                self.page = Page::Effects;
            }
        });
    }
}

/// Triggers the stock perks leave running with no ending: Always, After a Delay, On Equip and
/// On Draw turn an effect on for as long as the perk is applied. Any other trigger with no
/// ending starts the effect once and never lets it start again, which no installed stock perk
/// does.
const LASTING_TRIGGERS: [u8; 4] = [0, 1, 14, 16];

/// Whether a behavior group starts on something that can happen again but never ends.
pub(super) fn endless(
    group: &sundial::package_authoring::sandbox_perk::action::DecodedGroup,
) -> bool {
    event_fired(group) && group.removal.is_empty()
}

/// Whether a behavior group starts on something that can happen again, rather than turning on
/// for as long as the perk is applied.
pub(super) fn event_fired(
    group: &sundial::package_authoring::sandbox_perk::action::DecodedGroup,
) -> bool {
    !group.activation.is_empty()
        && !group
            .activation
            .iter()
            .any(|condition| LASTING_TRIGGERS.contains(&condition.kind))
}

/// A likely mistake the game still runs, the first one in the perk. It is shown wherever a
/// problem is, but it never keeps the perk from being applied or chosen.
pub(super) fn perk_warning(recipe: &PerkRecipe) -> Option<String> {
    recipe
        .effects
        .iter()
        .find_map(|effect| ending_issue(effect.program.as_ref()))
}

/// A behavior that never ends runs once. The compiler accepts it, since the game can run it,
/// so the workbench names it as a warning, in the status bar and under the trigger itself.
/// No installed stock perk does it, but an author may mean it. A guided effect is read in the
/// form it compiles to, since its ending comes from its trigger and duration.
pub(super) fn ending_issue(
    program: Option<&sundial::package_authoring::sandbox_perk::program::Program>,
) -> Option<String> {
    use sundial::package_authoring::sandbox_perk::{action, program::native_draft};
    let program = program?;
    let payload = match &program.native {
        Some(native) => native.graph.emit(),
        None => native_draft(program).ok()?.graph.emit(),
    }
    .ok()?;
    let groups = action::decode(&payload).ok()?.groups;
    let index = groups.iter().position(endless)?;
    let subject = match (groups.len(), index) {
        (1, _) => "This effect".to_owned(),
        (_, 0) => "The main behavior".to_owned(),
        (_, index) => format!("Behavior {}", index + 1),
    };
    Some(format!(
        "{subject} never ends, so it cannot start again. End it at once or set a Duration."
    ))
}

/// A counter with no contributing conditions never moves, so its effect can never fire.
/// The compiler accepts the bare node, since it is a valid node; the workbench is where it
/// becomes a named problem, in the status bar and beside the counter itself. A counter counts
/// the same way inside a requirement, a "while" check or an ending, so every one is checked.
pub(super) fn counter_issue(
    program: Option<&sundial::package_authoring::sandbox_perk::program::Program>,
) -> Option<String> {
    use sundial::package_authoring::sandbox_perk::{action, program::Trigger};
    let program = program?;
    let empty = if let Some(native) = &program.native {
        let decoded = action::decode(&native.graph.emit().ok()?).ok()?;
        decoded
            .conditions()
            .iter()
            .any(|condition| condition.kind == 26 && condition.children.is_empty())
    } else {
        program.trigger == Trigger::Native
            && program.native_trigger.as_ref().is_some_and(|node| {
                node.kind == 26
                    && action::decode_condition_node(&node.bytes)
                        .is_ok_and(|condition| condition.children.is_empty())
            })
    };
    empty.then(|| {
        "The counter has nothing to count. Add a contributing condition such as a kill.".to_owned()
    })
}

/// A key, tag or selection a node still needs, which compiles but never acts in game.
pub(super) fn choice_issue(
    program: Option<&sundial::package_authoring::sandbox_perk::program::Program>,
) -> Option<String> {
    let program = program?;
    match &program.native {
        Some(native) => super::program::native::unset_choice(&native.graph),
        None => sundial::package_authoring::sandbox_perk::program::native_draft(program)
            .ok()
            .and_then(|draft| super::program::native::unset_choice(&draft.graph)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::sandbox_perk::program::{
        Action, NativeGroup, NativeNode, Program, Trigger,
    };

    /// A kill that starts a behavior with no ending can never start it again, whether it is
    /// the effect's only behavior or a second one beside it. A duration ends it, and a trigger
    /// that turns the effect on for good, such as On Draw, is not a problem.
    #[test]
    fn a_repeatable_trigger_with_no_ending_is_named() {
        let program = |kind, duration_ms| Program {
            trigger: Trigger::Native,
            native_trigger: NativeNode::condition(kind),
            duration_ms,
            actions: vec![Action::add_rounds(1)],
            ..Program::default()
        };
        assert_eq!(
            ending_issue(Some(&program(2, 0))).as_deref(),
            Some(
                "This effect never ends, so it cannot start again. End it at once or set a Duration."
            )
        );
        assert!(ending_issue(Some(&program(2, 5_000))).is_none());
        assert!(ending_issue(Some(&program(16, 0))).is_none());
        let second = Program {
            trigger: Trigger::Drawn,
            actions: vec![Action::add_rounds(1)],
            additional_groups: vec![NativeGroup {
                activation: vec![NativeNode::condition(2).unwrap()],
                effects: vec![NativeNode::effect(16).unwrap()],
                ..NativeGroup::default()
            }],
            ..Program::default()
        };
        assert!(
            ending_issue(Some(&second))
                .unwrap()
                .starts_with("Behavior 2 never ends")
        );
    }
}
