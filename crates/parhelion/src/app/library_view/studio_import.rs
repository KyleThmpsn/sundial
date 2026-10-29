use super::*;

impl PackageAuthoringApp {
    /// Imports a Sunrise Studio project's items as recipes. Not built yet.
    pub(super) fn draw_studio_import(&mut self, ctx: &egui::Context) {
        if !self.library_state.studio_import {
            return;
        }
        let mut close = false;
        let response = egui::Modal::new("import_studio_project".into()).show(ctx, |ui| {
            ui.set_width(320.0);
            workbench_style(ui);
            ui.heading("Import Studio Project");
            ui.label("Work in progress.");
            ui.add_space(8.0);
            close = ui.button("Close").clicked();
        });
        if close || response.should_close() {
            self.library_state.studio_import = false;
        }
    }
}
