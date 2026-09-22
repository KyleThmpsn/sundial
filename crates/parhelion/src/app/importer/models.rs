use super::*;
use parhelion_import::GraphReference;

#[derive(Clone)]
struct Model {
    recipe: WeaponRecipe,
    source_hash: Option<u32>,
}

#[derive(Default)]
pub(super) struct Picker {
    open: bool,
    native_tab: bool,
    query: String,
    native_query: String,
    choices: Vec<Model>,
    selected: Option<usize>,
    donor: Option<u32>,
    loading: Option<Receiver<Result<Vec<Model>, String>>>,
    notice: String,
}

pub(super) struct Prepared {
    baseline: WeaponRecipe,
    packages: PathBuf,
    result: Result<(GraphReference, WeaponDonorReference), String>,
}

fn source_hash(recipe: &WeaponRecipe) -> Option<u32> {
    recipe
        .namespace
        .strip_prefix("parhelion.bulk.")
        .and_then(|hash| u32::from_str_radix(hash, 16).ok())
        .or_else(|| {
            let graph = recipe.overrides.imported_graph.as_ref()?;
            let bytes = std::fs::read(graph.directory.join("asset-graph.json")).ok()?;
            let graph: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            u32::try_from(graph["source_item"].as_u64()?).ok()
        })
}

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_imported_model_picker(&mut self, ui: &mut egui::Ui) {
        if !self.importer.enabled {
            self.importer.models.open = false;
            return;
        }
        if ui.button("Choose Donor Model…").clicked() {
            self.importer.models.open = true;
            self.importer.models.selected = None;
            self.importer.models.donor = None;
            self.importer.models.notice.clear();
            let paths: Vec<_> = self
                .recipe_entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
            let (sender, receiver) = mpsc::channel();
            self.importer.models.loading = Some(receiver);
            let ctx = ui.ctx().clone();
            thread::spawn(move || {
                let mut choices = Vec::new();
                for path in paths {
                    if let Ok(recipe) = WeaponRecipe::load_json(&path)
                        && recipe.overrides.imported_graph.is_some()
                    {
                        choices.push(Model {
                            source_hash: source_hash(&recipe),
                            recipe,
                        });
                    }
                }
                choices.sort_by_key(|model| model.recipe.name.to_lowercase());
                let _ = sender.send(Ok(choices));
                ctx.request_repaint();
            });
        }
        self.draw_model_window(ui.ctx());
    }

    fn draw_model_window(&mut self, ctx: &egui::Context) {
        if !self.importer.models.open {
            return;
        }
        if let Some(receiver) = &self.importer.models.loading {
            match receiver.try_recv() {
                Ok(Ok(choices)) => {
                    self.importer.models.choices = choices;
                    self.importer.models.loading = None;
                }
                Ok(Err(error)) => {
                    self.importer.models.notice = error;
                    self.importer.models.loading = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.importer.models.notice = "Model list failed to load.".into();
                    self.importer.models.loading = None;
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        let gameplay = self.current_donor();
        let compatible: BTreeSet<_> = gameplay
            .as_ref()
            .and_then(|gameplay| {
                authored_inventory_slot(&self.recipe.overrides, &gameplay.summary).map(|target| {
                    self.donor_summaries
                        .iter()
                        .filter(|donor| {
                            presentation_donor_candidate_is_compatible(
                                donor,
                                &gameplay.summary,
                                target,
                            )
                        })
                        .map(|donor| donor.hash)
                        .collect()
                })
            })
            .unwrap_or_default();
        let idle = !self.importer.busy()
            && self.build_receiver.is_none()
            && self.install_receiver.is_none();
        let mut open = true;
        let mut apply = false;
        egui::Window::new("Choose Donor Model")
            .id(egui::Id::new("d2-model-donor"))
            .open(&mut open)
            .default_width(650.0)
            .default_height(500.0)
            .show(ctx, |ui| {
                crate::app::style::workbench_style(ui);
                ui.add_enabled_ui(idle, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.importer.models.native_tab,
                            false,
                            "Imported Models",
                        );
                        ui.selectable_value(
                            &mut self.importer.models.native_tab,
                            true,
                            "Native Conversion Donor",
                        );
                    });
                    ui.separator();
                    if self.importer.models.native_tab {
                        ui.label("Reconverts the model with this donor.");
                        let current = self.importer.models.donor;
                        let label = current
                            .and_then(|hash| {
                                self.donor_summaries.iter().find(|donor| donor.hash == hash)
                            })
                            .map_or("Keep Model’s Existing Donor", |donor| donor.name.as_str());
                        if let Some(catalog) = &self.catalog {
                            let selection = catalog.draw_weapon_donor_header_picker(
                                ui,
                                "d2-conversion-donor",
                                &mut self.importer.models.native_query,
                                self.donor_summaries
                                    .iter()
                                    .filter(|donor| compatible.contains(&donor.hash)),
                                WeaponDonorPickerOptions {
                                    selected_hash: current,
                                    selected_label: label,
                                    header_label: None,
                                    action_label: "Choose Native Donor",
                                    selected_icon_override: None,
                                    secondary_action_label: None,
                                    row_detail: None,
                                    clear: Some(WeaponDonorPickerClearChoice {
                                        label: "Keep Model’s Existing Donor",
                                        tooltip: "No reconversion.",
                                        selected: current.is_none(),
                                    }),
                                },
                            );
                            match selection {
                                Some(WeaponDonorPickerAction::Select(hash)) => {
                                    self.importer.models.donor = Some(hash)
                                }
                                Some(WeaponDonorPickerAction::Clear) => {
                                    self.importer.models.donor = None
                                }
                                _ => {}
                            }
                        } else {
                            ui.label("Catalog loading.");
                        }
                    } else {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.importer.models.query)
                                .hint_text("Search")
                                .desired_width(f32::INFINITY),
                        );
                        if self.importer.models.loading.is_some() {
                            ui.spinner();
                        }
                        let query = self.importer.models.query.to_lowercase();
                        let visible: Vec<_> = self
                            .importer
                            .models
                            .choices
                            .iter()
                            .enumerate()
                            .filter_map(|(index, model)| {
                                model
                                    .recipe
                                    .name
                                    .to_lowercase()
                                    .contains(&query)
                                    .then_some(index)
                            })
                            .collect();
                        egui::ScrollArea::vertical()
                            .id_salt("imported-model-choices")
                            .max_height(280.0)
                            .show_rows(ui, 24.0, visible.len(), |ui, rows| {
                                for row in rows {
                                    let index = visible[row];
                                    if ui
                                        .add_sized(
                                            [ui.available_width(), 24.0],
                                            egui::Button::new(
                                                &self.importer.models.choices[index].recipe.name,
                                            )
                                            .selected(self.importer.models.selected == Some(index)),
                                        )
                                        .clicked()
                                    {
                                        self.importer.models.selected = Some(index);
                                    }
                                }
                            });
                        if self.importer.models.loading.is_none()
                            && self.importer.models.choices.is_empty()
                        {
                            ui.label("No imported models.");
                        }
                    }
                    ui.separator();
                    let model = self
                        .importer
                        .models
                        .selected
                        .and_then(|index| self.importer.models.choices.get(index));
                    let recipe = model.map(|model| &model.recipe).unwrap_or(&self.recipe);
                    let carrier = self.importer.models.donor.or_else(|| {
                        recipe
                            .presentation_donor
                            .as_ref()
                            .unwrap_or(&recipe.donor)
                            .item_hash
                            .parse_u32()
                            .ok()
                    });
                    ui.label(format!(
                        "Model: {}",
                        if recipe.overrides.imported_graph.is_some() {
                            recipe.name.as_str()
                        } else {
                            "Choose a model"
                        }
                    ));
                    let ready = recipe.overrides.imported_graph.is_some()
                        && carrier.is_some_and(|hash| compatible.contains(&hash));
                    if !ready && recipe.overrides.imported_graph.is_some() {
                        ui.label("Donor incompatible with this weapon’s gameplay base.");
                    }
                    apply = ui
                        .add_enabled(ready, egui::Button::new("Apply Model"))
                        .clicked();
                });
                if self.importer.busy() {
                    ui.spinner();
                    ui.label(&self.importer.notice);
                    if let Some(started) = self.importer.import_started {
                        let seconds = started.elapsed().as_secs();
                        ui.label(format!("Elapsed: {}:{:02}", seconds / 60, seconds % 60));
                    }
                }
                if !self.importer.models.notice.is_empty() {
                    ui.label(&self.importer.models.notice);
                }
            });
        self.importer.models.open = open;
        if apply {
            self.apply_imported_model(ctx);
        }
    }

    fn apply_imported_model(&mut self, ctx: &egui::Context) {
        let model = self
            .importer
            .models
            .selected
            .and_then(|index| self.importer.models.choices.get(index))
            .cloned()
            .unwrap_or_else(|| Model {
                source_hash: source_hash(&self.recipe),
                recipe: self.recipe.clone(),
            });
        let baseline = self.recipe.clone();
        let native = PathBuf::from(&self.packages);
        let modern = self.importer.settings.modern_packages.clone();
        let donor = self
            .importer
            .models
            .donor
            .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
            .cloned();
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.import_started = Some(Instant::now());
        self.importer.notice = "Preparing model…".into();
        self.importer.models.notice.clear();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = (|| -> Result<(GraphReference, WeaponDonorReference), String> {
                let root = data_root()?;
                let mut recipe = model.recipe;
                if let Some(donor) = donor {
                    let modern = modern.as_ref().ok_or("No Destiny 2 folder chosen")?;
                    let source = model
                        .source_hash
                        .ok_or("No source identity for reconversion")?;
                    let mut progress = |message| {
                        let _ = sender.send(Event::Progress(0, message));
                        ctx.request_repaint();
                    };
                    progress("Reading catalog…".into());
                    let weapons = service::scan_cached(
                        modern,
                        &native,
                        &root.join("importer/catalog"),
                        false,
                        |_| {},
                    )
                    .map_err(|error| format!("{error:#}"))?;
                    let weapon = weapons
                        .iter()
                        .find(|weapon| weapon.hash == source)
                        .ok_or("Source weapon missing from this build")?;
                    let folder =
                        service::model_directory(&root.join("models"), &weapon.name, source)
                            .map_err(|error| error.to_string())?;
                    let donors = serde_json::json!({"weapons":[{"hash":donor.hash,"name":donor.name,"weapon_type":donor.type_name,"present_in_native":true}]});
                    let path = service::prepare_with_progress(
                        weapon,
                        modern,
                        &native,
                        &donors,
                        &folder,
                        &mut progress,
                    )
                    .map_err(|error| format!("{error:#}"))?;
                    recipe = WeaponRecipe::load_json(&path).map_err(|error| error.to_string())?;
                }
                let graph = recipe
                    .overrides
                    .imported_graph
                    .as_ref()
                    .ok_or("No imported model")?;
                let target = baseline
                    .identity
                    .item_hash
                    .parse_u32()
                    .map_err(|error| error.to_string())?;
                let source = recipe
                    .identity
                    .item_hash
                    .parse_u32()
                    .map_err(|error| error.to_string())?;
                let folder = service::model_directory(&root.join("models"), &baseline.name, target)
                    .map_err(|error| error.to_string())?;
                let folder = service::reserve_assets(&folder, "appearance")
                    .map_err(|error| error.to_string())?
                    .join("graph");
                let _ = sender.send(Event::Progress(0, "Copying model…".into()));
                ctx.request_repaint();
                let mut graph = graph
                    .copy_model(source, target, &folder)
                    .map_err(|error| format!("{error:#}"))?;
                if let Some(source) = model.source_hash {
                    let path = folder.join("asset-graph.json");
                    let mut document: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(&path).map_err(|error| error.to_string())?,
                    )
                    .map_err(|error| error.to_string())?;
                    document["source_item"] = source.into();
                    std::fs::write(
                        path,
                        serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?,
                    )
                    .map_err(|error| error.to_string())?;
                    graph =
                        GraphReference::new(&folder, target).map_err(|error| error.to_string())?;
                }
                Ok((graph, recipe.presentation_donor.unwrap_or(recipe.donor)))
            })();
            let _ = sender.send(Event::ModelPrepared(Box::new(Prepared {
                baseline,
                packages: native,
                result,
            })));
            ctx.request_repaint();
        });
    }

    pub(super) fn finish_imported_model(&mut self, prepared: Prepared) {
        match prepared.result {
            Ok((graph, donor))
                if self.recipe == prepared.baseline && self.packages == prepared.packages =>
            {
                self.recipe.set_presentation_donor(Some(donor));
                self.recipe.overrides.imported_graph = Some(graph);
                self.recipe_dirty = true;
                self.invalidate_results();
                self.importer.models.notice = "Model applied.".into();
            }
            Ok(_) => {
                self.importer.models.notice = "Recipe changed while preparing. Apply again.".into()
            }
            Err(error) => self.importer.models.notice = error,
        }
        self.importer.notice = self.importer.models.notice.clone();
    }
}
