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
}

pub(super) struct Picker {
    target: Target,
    choices: Vec<Choice>,
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
        library: Option<&RecipeLibrary>,
        weapon: &WeaponRecipe,
    ) {
        let mut warnings = if self.initialized {
            Vec::new()
        } else {
            self.initialize();
            self.error.clone().into_iter().collect()
        };
        warnings.extend(self.scan_library());
        let (templates, template_warnings) = templates::load(library, weapon);
        warnings.extend(template_warnings);
        warnings.sort();
        warnings.dedup();
        let choices = choices::collect(
            &self.entries,
            &self.documents,
            templates.iter().map(|template| {
                (
                    templates::from_variant(&template.variant, catalog),
                    format!("Weapon Recipe · {}", template.weapon),
                )
            }),
            |recipe| self.perk_issue(recipe),
        );
        self.picker = Some(Picker {
            target,
            choices,
            query: String::new(),
            error: None,
            warnings,
        });
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
        let action = view::show(ctx, &mut picker, donor, catalog, stale.as_deref());
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
                let mut document = Document::new(PerkRecipe::new(), None);
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
