//! A mod recipe's one perk: opened from the mod's page and applied as the mod itself.
use super::*;

impl Workbench {
    /// Opens the perk of the open mod recipe, or a new one for a mod that has none yet. A mod's
    /// perk is open in one document, which applying from keeps tied to the mod.
    pub(in crate::app::custom_perks::workbench) fn open_mod(
        &mut self,
        recipe: &WeaponRecipe,
        catalog: &InvestmentCatalog,
    ) {
        self.picker = None;
        self.initialize();
        self.message = None;
        self.message_path = None;
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.mod_item.as_deref() == Some(recipe.namespace.as_str()))
        {
            self.select_document(index);
        } else {
            let perk = recipe
                .overrides
                .socket_plug_variants
                .first()
                .map_or_else(PerkRecipe::new, |variant| {
                    templates::from_variant(variant, catalog)
                });
            let mut document = Document::new(perk, None);
            document.mod_item = Some(recipe.namespace.clone());
            self.add_document(document);
        }
        self.open = true;
    }

    /// The footer of a mod recipe, whose perk is the mod itself, so it has no destination to
    /// choose.
    pub(in crate::app::custom_perks::workbench) fn draw_mod_attachment(
        &mut self,
        ui: &mut egui::Ui,
    ) -> Option<PerkRecipe> {
        self.documents.get(self.selected)?;
        let issue = self.selected_issue();
        if let Some(issue) = &issue {
            self.draw_issue(ui, issue);
        }
        let edit_issue = self.edit_issue();
        let editing = self.editor.is_some();
        let recipe_unsaved = self.recipe_unsaved;
        let mut save_recipe = false;
        let document = self.documents.get(self.selected)?;
        let mut result = None;
        ui.add_enabled_ui(!editing, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(recipe_unsaved, egui::Button::new("Save Recipe"))
                        .on_disabled_hover_text("Recipe saved.")
                        .clicked()
                    {
                        save_recipe = true;
                    }
                    ui.separator();
                    // A warning is shown above but does not hold the perk back.
                    let blocking = issue.as_ref().filter(|issue| issue.blocking);
                    let held = edit_issue.or_else(|| blocking.map(|issue| issue.message.as_str()));
                    let apply = crate::app::style::primary(ui, "Apply to Mod");
                    let response = ui.add_enabled(held.is_none(), apply);
                    let response = match held {
                        Some(reason) => response.on_disabled_hover_text(reason),
                        None => response,
                    };
                    if response.clicked() {
                        result = Some(document.recipe.clone());
                    }
                });
            });
        });
        self.save_recipe_requested |= save_recipe;
        result
    }

    pub(in crate::app::custom_perks::workbench) fn applied_to_mod(
        &mut self,
        recipe: &WeaponRecipe,
    ) {
        if let Some(document) = self.documents.get_mut(self.selected) {
            document.mod_item = Some(recipe.namespace.clone());
        }
        self.error = None;
        self.message_path = None;
        self.message = Some("Applied to the mod.".into());
        self.persist_drafts();
    }
}
