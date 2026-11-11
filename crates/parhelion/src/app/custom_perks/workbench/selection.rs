//! Choose portable perk recipes for the exact socket choice that opened the picker.
use super::{
    attachment::{Change, Target},
    *,
};

mod choices;
#[cfg(test)]
mod tests;
mod view;

struct Choice {
    recipe: PerkRecipe,
    source: String,
    issue: Option<String>,
    /// A likely mistake that leaves the perk selectable, shown under the row.
    warning: Option<String>,
    /// Whether it was imported from a saved item recipe, which Show Imported Perks lists.
    imported: bool,
    /// Whether it is built on an ornament, which Show Ornaments lists.
    ornament: bool,
    /// The name, description and source, lowercase, which the search matches.
    search: String,
}

pub(super) struct Picker {
    target: Target,
    choices: Vec<Choice>,
    /// Whether the saved recipes' perks are still being read. They join the list when they land.
    pending: bool,
    query: String,
    error: Option<String>,
    warnings: Vec<String>,
}

enum Action {
    Use(usize),
    Create,
    Cancel,
}

impl Workbench {
    pub(super) fn open_picker(
        &mut self,
        target: Target,
        catalog: &InvestmentCatalog,
        weapon: &WeaponRecipe,
    ) {
        let mut warnings = if self.initialized {
            Vec::new()
        } else {
            self.initialize();
            self.error.clone().into_iter().collect()
        };
        warnings.extend(self.scan_library());
        warnings.sort();
        warnings.dedup();
        self.picker = Some(Picker {
            target,
            choices: Vec::new(),
            pending: true,
            query: String::new(),
            error: None,
            warnings,
        });
        self.fill_picker(weapon, catalog);
    }

    /// Lists what Select Custom Perk offers: Custom Perks, open drafts, the perks of the open
    /// recipe `weapon` and, once they are read, those of saved recipes.
    pub(super) fn fill_picker(&mut self, weapon: &WeaponRecipe, catalog: &InvestmentCatalog) {
        let Some(mut picker) = self.picker.take() else {
            return;
        };
        let drafts = crate::perk::library::saved::SavedPerks::of_draft(weapon, catalog);
        let saved = self
            .authored_templates
            .iter()
            .flatten()
            .filter(|template| !template.draft);
        let mut choices = choices::collect(
            &self.entries,
            &self.documents,
            saved.chain(&drafts).map(|template| {
                (
                    template.perk.clone(),
                    format!("{} Recipe · {}", template.kind.label(), template.weapon),
                    !template.draft,
                )
            }),
            (&self.imported_perks, catalog),
            |recipe| self.perk_issue(recipe),
        );
        for choice in choices.iter_mut().filter(|choice| choice.issue.is_none()) {
            choice.warning = validation::perk_warning(&choice.recipe, self.branding.runtime());
        }
        picker.choices = choices;
        picker.pending = self.authored_templates.is_none();
        self.picker = Some(picker);
    }

    pub(super) fn show_picker(
        &mut self,
        ctx: &egui::Context,
        weapon: &mut WeaponRecipe,
        donor: &WeaponDonor,
        catalog: &InvestmentCatalog,
    ) -> bool {
        let Some(mut picker) = self.picker.take() else {
            return false;
        };
        let stale = picker.target.check(weapon, donor).err();
        let mut imported = self
            .show_imported_perks
            .unwrap_or(cfg!(feature = "d2-model-importer"));
        let action = view::show(
            ctx,
            &mut picker,
            (donor, catalog),
            stale.as_deref(),
            (&mut imported, &mut self.show_ornaments),
        );
        self.show_imported_perks = Some(imported);
        match action {
            Some(Action::Use(index)) => {
                let change = Change {
                    target: picker.target.clone(),
                    perk: Some(picker.choices[index].recipe.clone()),
                };
                match change.apply(weapon, donor) {
                    Ok(()) => return true,
                    Err(error) => picker.error = Some(error),
                }
            }
            Some(Action::Create) => {
                let mut recipe = PerkRecipe::new();
                if let Some(plug) = picker.target.gear_plug() {
                    recipe.template_plug = plug.into();
                }
                let mut document = Document::new(recipe, None);
                document.target = Some(picker.target);
                self.add_document(document);
                self.page = Page::Basics;
                self.open = true;
                return false;
            }
            Some(Action::Cancel) => return false,
            None => {}
        }
        self.picker = Some(picker);
        false
    }
}
