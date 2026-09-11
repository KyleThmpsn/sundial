use super::*;

impl PackageAuthoringApp {
    pub(super) fn draw_recipe_restore(&mut self, ctx: &egui::Context, busy: bool) {
        let Some(preview) = &self.library_state.restore else {
            return;
        };
        let has_edits = self.recipe_dirty && self.recipe_path.as_ref() == Some(&preview.path);
        let mut restore = false;
        let mut cancel = false;
        let response = egui::Modal::new("restore_library_recipe".into()).show(ctx, |ui| {
            ui.set_width(380.0);
            workbench_style(ui);
            ui.heading("Restore This Recipe?");
            ui.strong(&preview.name);
            ui.label(
                "Your saved recipe will be backed up, then replaced with its bundled default.",
            );
            if has_edits {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Save or discard this recipe's open edits first.",
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                restore = ui
                    .add_enabled(!busy && !has_edits, egui::Button::new("Back Up & Restore"))
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if restore {
            let preview = self.library_state.restore.take().unwrap();
            let result = self
                .recipe_library
                .as_ref()
                .ok_or("Recipe library is unavailable".into())
                .and_then(|library| library.restore_recipe(&preview));
            match result {
                Ok(backup) => {
                    self.refresh_recipe_library();
                    if self.recipe_path.as_ref() == Some(&preview.path) {
                        self.open_recipe_path(&preview.path);
                    }
                    let message = match backup {
                        Some(path) => {
                            format!("Restored {}. Backup: {}", preview.name, path.display())
                        }
                        None => format!("{} already matches its default.", preview.name),
                    };
                    self.log.push(LogEntry::info(&message));
                    self.library_state.notice =
                        Some(format!("{} restored to its default.", preview.name));
                    self.library_state.errors.clear();
                }
                Err(error) => self.report_library_error(error),
            }
        } else if cancel {
            self.library_state.restore = None;
        }
    }
}
