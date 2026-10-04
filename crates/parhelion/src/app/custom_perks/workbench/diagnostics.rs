use super::*;
use crate::perk::preflight;

pub(super) struct Cache {
    recipe: PerkRecipe,
    kind: crate::ItemKind,
    discovery: Option<Arc<sundial::package_authoring::sandbox_perk::dependencies::Index>>,
    issues: Vec<validation::Issue>,
}

impl Workbench {
    pub(super) fn selected_diagnostics(&mut self) -> Vec<validation::Issue> {
        let Some(document) = self.documents.get(self.selected) else {
            return Vec::new();
        };
        let recipe = &document.recipe;
        let index = self.discovery.data.as_ref().map(|data| &data.perks);
        let same_index = |a: Option<&Arc<_>>, b: Option<&Arc<_>>| match (a, b) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if let Some(cache) = &self.diagnostic_cache
            && cache.recipe == *recipe
            && cache.kind == self.item_kind
            && same_index(cache.discovery.as_ref(), index)
        {
            return cache.issues.clone();
        }
        let location = |position: usize| validation::Location {
            document: recipe.id.clone(),
            effect: recipe.effects[position].source_perk_index,
            action: None,
            native: None,
        };
        let mut issues = preflight::check(recipe, self.item_kind)
            .into_iter()
            .map(|issue| validation::Issue {
                message: issue.effect.map_or(issue.message.clone(), |index| {
                    format!("Effect {}: {}", index + 1, issue.message)
                }),
                location: issue.effect.map(location),
                blocking: issue.blocking,
            })
            .collect::<Vec<_>>();
        for (position, effect) in recipe.effects.iter().enumerate() {
            if let Some(program) = &effect.program
                && let Ok(native) =
                    sundial::package_authoring::sandbox_perk::program::native_draft(program)
                && let Ok(failures) = native.authoring_issues()
            {
                for failure in failures {
                    let mut target = location(position);
                    target.native = Some(failure.clone());
                    issues.push(validation::Issue {
                        message: format!(
                            "Effect {}, Behavior {}, Action {}: {}",
                            position + 1,
                            failure.group + 1,
                            failure.action + 1,
                            failure.message
                        ),
                        location: Some(target),
                        blocking: true,
                    });
                }
            }
            for message in [
                self.discovery
                    .perk_issue(effect.source_perk_index)
                    .map(str::to_owned),
                validation::counter_issue(effect.program.as_ref()),
                validation::choice_issue(effect.program.as_ref()),
            ]
            .into_iter()
            .flatten()
            {
                let issue = validation::Issue {
                    message: format!("Effect {}: {message}", position + 1),
                    location: Some(location(position)),
                    blocking: true,
                };
                if !issues
                    .iter()
                    .any(|existing| existing.message == issue.message)
                {
                    issues.push(issue);
                }
            }
        }
        self.diagnostic_cache = Some(Cache {
            recipe: recipe.clone(),
            kind: self.item_kind,
            discovery: index.cloned(),
            issues: issues.clone(),
        });
        issues
    }

    pub(super) fn show_diagnostics(&mut self, ctx: &egui::Context) {
        if !self.diagnostics_open {
            return;
        }
        let Some(document) = self.documents.get(self.selected) else {
            return;
        };
        let recipe = document.recipe.clone();
        let issues = self.selected_diagnostics();
        let mut open = true;
        let mut merge = false;
        egui::Window::new("Perk Diagnostics").id(egui::Id::new("perk-diagnostics"))
            .open(&mut open).default_width(700.0).default_height(560.0).vscroll(true)
            .show(ctx, |ui| {
                ui.heading(&recipe.name);
                ui.label(format!("Runtime Budget: {} of {} Effects Active", recipe.effects.len().min(crate::perk::SANDBOX_PERK_CAPACITY), recipe.effects.len()));
                ui.weak(format!("Destination: {}", self.item_kind.label()));
                ui.label("Blocking problems prevent attachment. Warnings describe runtime limits or behavior that still needs verification.");
                if issues.is_empty() { ui.label("No authoring problems found. Gameplay has not been verified by these checks."); }
                for (index, issue) in issues.iter().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.separator();
                        ui.colored_label(if issue.blocking { ui.visuals().error_fg_color } else { ui.visuals().warn_fg_color }, &issue.message);
                        if let Some(location) = &issue.location
                            && let Some(position) = recipe.effects.iter().position(|effect| effect.source_perk_index == location.effect)
                            && ui.add_enabled(self.editor.is_none(), egui::Button::new(format!("Open Effect {}", position + 1))).clicked() {
                            self.reveal_problem = Some(location.clone());
                            self.open = true;
                            self.page = Page::Effects;
                        }
                    });
                }
                ui.separator();
                ui.heading("Consolidate Compatible Actions");
                ui.label("Combine actions only when their trigger and lifetime match. Moved actions run after the destination's actions. Separate events, policies, component edits, and shared state keep their own effects.");
                if recipe.effects.len() >= 2 {
                    self.consolidation.0 = self.consolidation.0.min(recipe.effects.len() - 1);
                    self.consolidation.1 = self.consolidation.1.min(recipe.effects.len() - 1);
                    ui.horizontal(|ui| {
                        for (id, label, selected) in [("merge-into", "Into", &mut self.consolidation.0), ("merge-from", "From", &mut self.consolidation.1)] {
                            egui::ComboBox::from_id_salt(id).selected_text(format!("{label} Effect {}", *selected + 1)).show_ui(ui, |ui| {
                                for index in 0..recipe.effects.len() { ui.selectable_value(selected, index, format!("Effect {}", index + 1)); }
                            });
                        }
                    });
                    let mut preview = recipe.clone();
                    let result = preflight::consolidate(&mut preview, self.consolidation.0, self.consolidation.1);
                    merge = ui.add_enabled(result.is_ok() && self.editor.is_none(), egui::Button::new("Consolidate Actions")).clicked();
                    if let Err(reason) = result { ui.weak(reason); }
                }
            });
        self.diagnostics_open = open;
        if merge {
            let mut changed = recipe.clone();
            match preflight::consolidate(&mut changed, self.consolidation.0, self.consolidation.1) {
                Ok(()) => {
                    let document = &mut self.documents[self.selected];
                    document.history.record_step(recipe);
                    document.recipe = changed;
                    document.modified = Some(SystemTime::now());
                    self.persist_drafts();
                }
                Err(error) => self.error = Some(error),
            }
        }
    }
}
