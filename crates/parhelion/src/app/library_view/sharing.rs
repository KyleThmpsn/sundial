use super::*;
use state::TransferResult;

impl PackageAuthoringApp {
    pub(super) fn report_library_error(&mut self, error: String) {
        self.log.push(LogEntry::error(&error));
        self.library_state.notice = Some("Recipe action could not be completed.".into());
        self.library_state.errors = vec![error];
    }

    pub(super) fn import_library_files(&mut self, ctx: &egui::Context) {
        let Some(library) = self.recipe_library.clone() else {
            return;
        };
        let Some(paths) = rfd::FileDialog::new()
            .set_title("Import Recipes Or Bundles")
            .add_filter("Parhelion Recipes And Bundles", &["json"])
            .pick_files()
        else {
            return;
        };
        self.library_state.notice = Some("Importing recipes…".into());
        self.library_state.highlighted.clear();
        self.library_state.reveal = None;
        self.library_state.errors.clear();
        let ctx = ctx.clone();
        self.library_state.job = Some(thread::spawn(move || {
            let report = library.import_files(&paths);
            ctx.request_repaint();
            TransferResult::Imported(report)
        }));
    }

    pub(super) fn export_library_bundle(&mut self, ctx: &egui::Context, paths: BTreeSet<PathBuf>) {
        let Some(library) = self.recipe_library.clone() else {
            return;
        };
        if paths.is_empty() {
            return;
        }
        let current = self.recipe_path.clone().filter(|path| paths.contains(path));
        if current.is_some()
            && let Some((_, error)) = &self.invalid_weapon_name
        {
            self.report_library_error(error.clone());
            return;
        }
        let current = current.map(|path| (path, self.recipe.clone()));
        let Some(destination) = rfd::FileDialog::new()
            .set_title("Export Recipe Bundle")
            .add_filter("Parhelion Recipe Bundle", &["json"])
            .set_file_name("recipes.parhelion-bundle.json")
            .save_file()
        else {
            return;
        };
        self.library_state.notice = Some(format!("Exporting {} recipes…", paths.len()));
        self.library_state.errors.clear();
        let ctx = ctx.clone();
        self.library_state.job = Some(thread::spawn(move || {
            let result = (|| {
                let recipes = paths
                    .iter()
                    .map(|path| {
                        if let Some((current_path, recipe)) = &current
                            && current_path == path
                        {
                            return Ok(recipe.clone());
                        }
                        WeaponRecipe::load_json(path)
                            .map_err(|error| format!("{}: {error}", path.display()))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                library.export_bundle(&recipes, &destination)?;
                Ok((destination, recipes.len()))
            })();
            ctx.request_repaint();
            TransferResult::Exported(result)
        }));
    }

    pub(super) fn draw_library_notice(&mut self, ui: &mut egui::Ui) {
        let Some(message) = &self.library_state.notice else {
            return;
        };
        let mut dismiss = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !self.library_state.busy() {
                dismiss = ui.small_button("Dismiss").clicked();
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if self.library_state.busy() {
                    ui.spinner();
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
                ui.add(egui::Label::new(message).wrap());
            });
        });
        if !self.library_state.errors.is_empty() {
            egui::CollapsingHeader::new(format!("Details ({})", self.library_state.errors.len()))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(120.0)
                        .show(ui, |ui| {
                            for error in &self.library_state.errors {
                                ui.colored_label(ui.visuals().error_fg_color, error);
                            }
                        });
                });
        }
        if dismiss {
            self.library_state.notice = None;
            self.library_state.errors.clear();
            self.library_state.highlighted.clear();
        }
    }
}
