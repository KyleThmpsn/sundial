use super::*;
use crate::perk::verification::{Outcome, Record};

#[derive(Default)]
pub(super) struct Window {
    pub open: bool,
    record: Option<Record>,
    receiver: Option<Receiver<Result<Record, String>>>,
    worker: Option<thread::JoinHandle<()>>,
    error: Option<String>,
    saved: Option<PathBuf>,
}

impl Window {
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }

    pub fn poll(&mut self) {
        let result = self
            .receiver
            .as_ref()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(
                    "The verification reader stopped before finishing.".into(),
                )),
            });
        if let Some(result) = result {
            self.receiver = None;
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            match result {
                Ok(record) => {
                    self.record = Some(record);
                    self.error = None;
                    self.saved = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        recipe: &PerkRecipe,
        kind: crate::ItemKind,
        library: Option<&Path>,
    ) {
        if !self.open {
            return;
        }
        if self.record.is_none() {
            match Record::new(
                recipe,
                kind,
                sundial::package_authoring::sandbox_perk::nodes::CLIENT_BUILD,
            ) {
                Ok(record) => self.record = Some(record),
                Err(error) => self.error = Some(error),
            }
        }
        let mut open = true;
        egui::Window::new("Gameplay Verification").id(egui::Id::new("perk-verification"))
            .open(&mut open).default_width(720.0).default_height(620.0).vscroll(true).show(ctx, |ui| {
                if self.busy() { ui.horizontal(|ui| { ui.spinner(); ui.label("Checking staged recipes and package hashes…"); }); }
                ui.add_enabled_ui(!self.busy(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("New Record for Current Perk").clicked() {
                            match Record::new(recipe, kind, sundial::package_authoring::sandbox_perk::nodes::CLIENT_BUILD) {
                                Ok(record) => { self.record = Some(record); self.error = None; self.saved = None; }, Err(error) => self.error = Some(error),
                            }
                        }
                        if ui.button("Open Record…").clicked()
                            && let Some(path) = rfd::FileDialog::new().add_filter("Verification Record", &["json"]).pick_file() {
                            match Record::load(&path) { Ok(record) => { self.record = Some(record); self.saved = Some(path); self.error = None; }, Err(error) => self.error = Some(error) }
                        }
                    });
                    let Some(record) = self.record.as_mut() else { return; };
                    let current = record.matches(recipe) && record.destination_kind == kind;
                    ui.heading(&record.recipe.name);
                    ui.weak(format!("Recipe SHA-256: {}", record.recipe_sha256));
                    if !current { ui.colored_label(ui.visuals().warn_fg_color, "Recorded for an earlier configuration."); }
                    let before_build = (record.client_build.clone(), record.runtime_build.clone());
                    ui.add_enabled_ui(current, |ui| {
                        ui.horizontal(|ui| { ui.label("Client Build"); ui.text_edit_singleline(&mut record.client_build); });
                        ui.horizontal(|ui| { ui.label("Runtime Build"); ui.text_edit_singleline(&mut record.runtime_build).on_hover_text("Enter the Sunrise or Dawn version used for the gameplay test."); });
                        if before_build != (record.client_build.clone(), record.runtime_build.clone()) {
                            for observation in &mut record.observations { observation.outcome = Outcome::NotTested; observation.notes.clear(); }
                            self.saved = None;
                        }
                        if ui.button("Bind Staged Build…").clicked()
                            && let Some(run) = rfd::FileDialog::new().pick_folder() {
                            let mut snapshot = record.clone();
                            let repaint = ctx.clone();
                            let (sender, receiver) = mpsc::channel();
                            self.receiver = Some(receiver);
                            self.worker = Some(thread::spawn(move || {
                                let result = snapshot.bind_staged(&run).map(|()| snapshot);
                                let _ = sender.send(result);
                                repaint.request_repaint();
                            }));
                        }
                        ui.label(format!("{} Bound Packages · {} Destinations", record.packages.len(), record.destinations.len()));
                        let selected = record.selected_destination;
                        let name = selected.and_then(|index| record.destinations.get(index)).map_or_else(|| "Choose the Tested Destination".into(), |destination| format!("{} · {}", destination.namespace, destination.location));
                        egui::ComboBox::from_id_salt("verification-destination").width(ui.available_width()).selected_text(name).show_ui(ui, |ui| {
                            for (index, destination) in record.destinations.iter().enumerate() {
                                ui.selectable_value(&mut record.selected_destination, Some(index), format!("{} · {}", destination.namespace, destination.location));
                            }
                        });
                        if record.selected_destination != selected {
                            for observation in &mut record.observations { observation.outcome = Outcome::NotTested; observation.notes.clear(); }
                            self.saved = None;
                        }
                        let can_record = record.selected_destination.is_some() && !record.packages.is_empty() && !record.client_build.trim().is_empty() && !record.runtime_build.trim().is_empty();
                        if !can_record { ui.weak("Needs a staged build, a destination and both build versions."); }
                        ui.add_enabled_ui(can_record, |ui| {
                            for observation in &mut record.observations {
                                ui.separator();
                                ui.horizontal(|ui| {
                                    ui.strong(observation.check.label());
                                    egui::ComboBox::from_id_salt(observation.check.label()).selected_text(observation.outcome.label()).show_ui(ui, |ui| {
                                        for outcome in Outcome::ALL { if ui.selectable_value(&mut observation.outcome, outcome, outcome.label()).changed() { self.saved = None; } }
                                    });
                                });
                                ui.weak(observation.check.guidance());
                                if ui.add(egui::TextEdit::multiline(&mut observation.notes).desired_rows(2).desired_width(f32::INFINITY).hint_text("Test setup, measurements, result, and evidence location")).changed() { self.saved = None; }
                            }
                        });
                    });
                    ui.separator();
                    ui.strong(if record.gameplay_verified() { "All Gameplay Checks Recorded as Passed" } else { "Gameplay Verification Incomplete" });
                    if ui.button("Save Record").clicked() {
                        let directory = library.map(|root| root.join("verification")).or_else(|| rfd::FileDialog::new().pick_folder());
                        if let Some(directory) = directory {
                            match record.save(&directory) { Ok(path) => { self.saved = Some(path); self.error = None; }, Err(error) => self.error = Some(error) }
                        }
                    }
                });
                if let Some(error) = &self.error { ui.colored_label(ui.visuals().error_fg_color, error); }
                if let Some(path) = &self.saved { ui.weak(format!("Saved Record: {}", path.display())); }
            });
        self.open = open;
    }
}
