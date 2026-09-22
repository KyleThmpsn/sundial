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
}

impl Workbench {
    pub(super) fn validation_issue(&self, recipe: &PerkRecipe) -> Option<Issue> {
        let message = self.perk_issue(recipe)?;
        // Document-level failures have no effect to open. Never guess from error wording.
        let mut document = recipe.clone();
        document.effects.clear();
        if document.validate().is_err() {
            return Some(Issue {
                message,
                location: None,
            });
        }
        for (position, effect) in recipe.effects.iter().enumerate() {
            document.effects = vec![effect.clone()];
            if document.validate().is_ok()
                && self
                    .discovery
                    .perk_issue(effect.source_perk_index)
                    .is_none()
                && counter_issue(effect.program.as_ref()).is_none()
            {
                continue;
            }
            let native = effect
                .program
                .as_ref()
                .filter(|program| program::uses_complete_editor(program))
                .and_then(|program| {
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
            return Some(Issue {
                message: format!("{context}: {message}"),
                location: Some(Location {
                    document: recipe.id.clone(),
                    effect: effect.source_perk_index,
                    action,
                    native,
                }),
            });
        }
        Some(Issue {
            message,
            location: None,
        })
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

/// A counter with no contributing conditions never moves, so its effect can never fire.
/// The compiler accepts the bare node, since it is a valid node; the workbench is where it
/// becomes a named problem, in the status bar and beside the counter itself.
pub(super) fn counter_issue(
    program: Option<&sundial::package_authoring::sandbox_perk::program::Program>,
) -> Option<String> {
    use sundial::package_authoring::sandbox_perk::{action, program::Trigger};
    let program = program?;
    let empty = if let Some(native) = &program.native {
        let decoded = action::decode(&native.graph.emit().ok()?).ok()?;
        decoded.groups.iter().any(|group| {
            group
                .activation
                .iter()
                .any(|condition| condition.kind == 26 && condition.children.is_empty())
        })
    } else {
        program.trigger == Trigger::Native
            && program.native_trigger.as_ref().is_some_and(|node| {
                node.kind == 26
                    && action::decode_condition_node(&node.bytes)
                        .is_ok_and(|condition| condition.children.is_empty())
            })
    };
    empty.then(|| {
        "The counter has no contributing conditions, so it can never fire. Add a condition that counts, such as a kill."
            .to_owned()
    })
}
