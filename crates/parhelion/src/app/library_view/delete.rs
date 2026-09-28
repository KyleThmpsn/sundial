use super::*;

impl PackageAuthoringApp {
    pub(super) fn draw_recipe_delete(&mut self, ctx: &egui::Context, busy: bool) {
        let Some(preview) = &self.library_state.delete else {
            return;
        };
        let has_edits = self.recipe_dirty && self.recipe_path.as_ref() == Some(&preview.path);
        let mut delete = false;
        let mut cancel = false;
        let response = egui::Modal::new("delete_library_recipe".into()).show(ctx, |ui| {
            ui.set_width(380.0);
            workbench_style(ui);
            ui.heading("Delete This Recipe?");
            ui.strong(&preview.name);
            if has_edits {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Save or discard this recipe's open edits first.",
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                delete = ui
                    .add_enabled(!busy && !has_edits, egui::Button::new("Back Up & Delete"))
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if delete {
            let preview = self.library_state.delete.take().unwrap();
            let result = self
                .recipe_library
                .as_ref()
                .ok_or("Recipe library is unavailable".into())
                .and_then(|library| library.delete_recipe(&preview));
            match result {
                Ok(backup) => {
                    // An open build selection must not save the deleted file back into it.
                    if let Some(draft) = &mut self.build_selection_draft {
                        draft.remove(&preview.path);
                    }
                    self.refresh_recipe_library();
                    // The open recipe no longer has a file, so the editor starts a new one.
                    if self.recipe_path.as_ref() == Some(&preview.path) {
                        self.start_new_recipe(ItemKind::Weapon);
                    }
                    self.log.push(LogEntry::info(format!(
                        "Deleted {}. Backup: {}",
                        preview.name,
                        backup.display()
                    )));
                    self.library_state.notice = Some(format!("Deleted {}.", preview.name));
                    self.library_state.errors.clear();
                }
                Err(error) => self.report_library_error(error),
            }
        } else if cancel {
            self.library_state.delete = None;
        }
    }
}
