//! Activity recipe documents keep their source settings and their own save baseline.
use super::*;

pub(super) struct Editor {
    pub(super) recipe: crate::map::MapRecipe,
    baseline: crate::map::MapRecipe,
    original: Vec<u8>,
    path: PathBuf,
    error: Option<String>,
    pending: Option<Action>,
    images: image_files::ImageFiles<()>,
    preview: Option<(u64, egui::TextureHandle)>,
    choices: Option<Vec<ImageChoice>>,
    choices_job: Option<Receiver<Result<Vec<crate::map::ImageChoice>, String>>>,
    choose_image: bool,
    image_query: String,
    blank_rows: Option<Result<usize, String>>,
    blank_rows_job: Option<Receiver<Result<usize, String>>>,
}

enum Action {
    Close,
    Open(PathBuf),
    CloseApp,
}

struct ImageChoice {
    name: String,
    preview: Result<egui::TextureHandle, String>,
}

impl Editor {
    pub(super) fn dirty(&self) -> bool {
        self.recipe != self.baseline || self.images.busy()
    }

    fn load(path: &Path) -> Result<Self, String> {
        let original = std::fs::read(path).map_err(|error| error.to_string())?;
        let recipe: crate::map::MapRecipe =
            serde_json::from_slice(&original).map_err(|error| error.to_string())?;
        recipe.validate().map_err(|error| error.to_string())?;
        Ok(Self {
            baseline: recipe.clone(),
            recipe,
            original,
            path: path.to_owned(),
            error: None,
            pending: None,
            images: Default::default(),
            preview: None,
            choices: None,
            choices_job: None,
            choose_image: false,
            image_query: String::new(),
            blank_rows: None,
            blank_rows_job: None,
        })
    }

    fn save(&mut self, library: Option<&RecipeLibrary>) -> bool {
        let result = library
            .ok_or_else(|| "Activity library is unavailable".to_owned())
            .and_then(|library| library.save_map(&self.path, &self.recipe, &self.original));
        match result {
            Ok(bytes) => {
                self.original = bytes;
                self.baseline = self.recipe.clone();
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    /// `labels` are the destination labels of the build's other maps.
    fn fields(&mut self, ui: &mut egui::Ui, packages: &Path, labels: &BTreeSet<String>) {
        ui.label("Title");
        named_control(
            ui.text_edit_singleline(&mut self.recipe.title),
            "Activity Title",
        );
        ui.label("Internal Name");
        named_control(
            ui.text_edit_singleline(&mut self.recipe.name),
            "Activity Internal Name",
        );
        ui.weak("Use lowercase letters, digits and underscores.");
        ui.label("Description");
        named_control(
            ui.add(
                egui::TextEdit::multiline(&mut self.recipe.description)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            ),
            "Activity Description",
        );
        ui.label("Destination");
        let mut label = self.recipe.destination.clone().unwrap_or_default();
        if named_control(
            ui.add(egui::TextEdit::singleline(&mut label).hint_text("Donor Destination")),
            "Activity Destination",
        )
        .changed()
        {
            self.recipe.destination = (!label.trim().is_empty()).then_some(label);
        }
        self.draw_destination_rows(ui, packages, labels);
        ui.separator();
        ui.label("Vehicle Summoning");
        egui::ComboBox::from_id_salt("activity-vehicle-summoning")
            .selected_text(match self.recipe.options.vehicle_summoning {
                None => "Automatic",
                Some(true) => "Allowed",
                Some(false) => "Blocked",
            })
            .show_ui(ui, |ui| {
                for (value, label) in [
                    (None, "Automatic"),
                    (Some(true), "Allowed"),
                    (Some(false), "Blocked"),
                ] {
                    ui.selectable_value(&mut self.recipe.options.vehicle_summoning, value, label);
                }
            });
        ui.weak("Automatic allows summoning in mission_ activities and keeps the donor setting elsewhere. Combat and placement restrictions still apply.");
        ui.add_enabled_ui(self.recipe.source.is_some(), |ui| {
            ui.checkbox(&mut self.recipe.options.ambient_audio, "Ambient Audio");
            ui.checkbox(
                &mut self.recipe.options.death_boundaries,
                "Death Boundaries",
            );
        })
        .response
        .on_hover_text(
            "These switches control the source level's ambient sounds and death volumes.",
        );
        ui.separator();
        self.draw_image(ui, packages);
    }

    /// The destination rows still free once every label of the build takes one. Maps that share a
    /// label share its row.
    fn draw_destination_rows(
        &mut self,
        ui: &mut egui::Ui,
        packages: &Path,
        labels: &BTreeSet<String>,
    ) {
        if self.blank_rows.is_none() && self.blank_rows_job.is_none() {
            let (sender, receiver) = mpsc::channel();
            let packages = packages.to_owned();
            let context = ui.ctx().clone();
            let read = sundial::ui::model_preview::PackageRead::start();
            let spawned = thread::Builder::new()
                .name("destination-rows".into())
                .spawn(move || {
                    let _read = read;
                    let _ = sender.send(crate::map::blank_destination_rows(&packages));
                    context.request_repaint();
                });
            match spawned {
                Ok(_) => self.blank_rows_job = Some(receiver),
                Err(error) => self.blank_rows = Some(Err(error.to_string())),
            }
        }
        if let Some(receiver) = &self.blank_rows_job {
            match receiver.try_recv() {
                Ok(rows) => {
                    self.blank_rows = Some(rows);
                    self.blank_rows_job = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.blank_rows = Some(Err("Could not read destination rows".into()));
                    self.blank_rows_job = None;
                }
                Err(TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        let used = labels
            .iter()
            .map(String::as_str)
            .chain(self.recipe.destination.as_deref())
            .collect::<BTreeSet<_>>()
            .len();
        let total = crate::map::DESTINATION_ROWS;
        // One line in every state, so the fields below stay put when the read finishes.
        match &self.blank_rows {
            None => {
                ui.weak("Reading destination rows…");
            }
            Some(Err(error)) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Could not read destination rows.",
                )
                .on_hover_text(error);
            }
            Some(Ok(blank)) if used > *blank => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "This build needs {used} destination rows. {blank} of {total} are free."
                    ),
                );
            }
            Some(Ok(blank)) => {
                ui.weak(format!(
                    "{} of {total} destination rows are free.",
                    blank - used
                ));
            }
        }
    }

    fn poll_images(&mut self) {
        if let Some(((), image)) = self.images.poll() {
            match crate::presentation::Artwork::from_source(image.pixels().clone()) {
                Ok(image) => {
                    self.recipe.image = Some(image);
                    self.recipe.image_donor = None;
                    self.preview = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    fn draw_image(&mut self, ui: &mut egui::Ui, packages: &Path) {
        ui.strong("Activity Image");
        if !self.recipe.donor.starts_with("pvp_") {
            ui.weak("Only private-match activities take an image.");
            return;
        }
        if let Some(image) = &self.recipe.image {
            let key = image.fingerprint();
            if self.preview.as_ref().is_none_or(|(old, _)| *old != key) {
                let pixels = image.render(320, 180);
                self.preview = Some((
                    key,
                    ui.ctx().load_texture(
                        "activity-image",
                        egui::ColorImage::from_rgba_unmultiplied([320, 180], pixels.as_raw()),
                        egui::TextureOptions::LINEAR,
                    ),
                ));
            }
            if let Some((_, texture)) = &self.preview {
                ui.image((texture.id(), egui::vec2(240.0, 135.0)));
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.images.busy(), egui::Button::new("Upload Image…"))
                .clicked()
            {
                self.images.import(
                    (),
                    "Choose Activity Image".into(),
                    "activity-image",
                    ui.ctx().clone(),
                );
            }
            if ui
                .add_enabled(
                    !self.images.busy(),
                    egui::Button::new("Choose Activity Image…"),
                )
                .clicked()
            {
                self.choose_image = !self.choose_image;
                if self.choices.is_none() && self.choices_job.is_none() {
                    let (sender, receiver) = mpsc::channel();
                    self.choices_job = Some(receiver);
                    let packages = packages.to_owned();
                    let context = ui.ctx().clone();
                    let read = sundial::ui::model_preview::PackageRead::start();
                    let spawned =
                        thread::Builder::new()
                            .name("activity-images".into())
                            .spawn(move || {
                                let _read = read;
                                let _ = sender.send(crate::map::image_choices(&packages));
                                context.request_repaint();
                            });
                    if let Err(error) = spawned {
                        self.choices_job = None;
                        self.error = Some(error.to_string());
                    }
                }
            }
            if ui
                .add_enabled(
                    (self.recipe.image.is_some() || self.recipe.image_donor.is_some())
                        && !self.images.busy(),
                    egui::Button::new("Use Default Image"),
                )
                .clicked()
            {
                self.recipe.image = None;
                self.recipe.image_donor = None;
                self.preview = None;
            }
        });
        if let Some(receiver) = &self.choices_job {
            match receiver.try_recv() {
                Ok(Ok(choices)) => {
                    self.choices = Some(
                        choices
                            .into_iter()
                            .map(|choice| ImageChoice {
                                preview: choice.preview.map(|image| {
                                    ui.ctx().load_texture(
                                        format!("activity-image-{}", choice.name),
                                        egui::ColorImage::from_rgba_unmultiplied(
                                            [image.width() as usize, image.height() as usize],
                                            image.as_raw(),
                                        ),
                                        egui::TextureOptions::LINEAR,
                                    )
                                }),
                                name: choice.name,
                            })
                            .collect(),
                    );
                    self.choices_job = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.choices_job = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.error = Some("Could not load activity images".into());
                    self.choices_job = None;
                }
                Err(TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        if let Some(donor) = &self.recipe.image_donor {
            ui.label(format!("Artwork from {donor}"));
            if let Some(Ok(texture)) = self
                .choices
                .as_ref()
                .and_then(|choices| choices.iter().find(|choice| &choice.name == donor))
                .map(|choice| &choice.preview)
            {
                ui.image((texture.id(), egui::vec2(200.0, 112.0)));
            }
        }
        if self.choose_image {
            ui.add(
                egui::TextEdit::singleline(&mut self.image_query).hint_text("Search Activities…"),
            );
            if let Some(choices) = &self.choices {
                let query = self.image_query.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("activity-image-choices")
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for choice in choices
                            .iter()
                            .filter(|choice| choice.name.to_lowercase().contains(&query))
                        {
                            let selected = ui
                                .horizontal(|ui| {
                                    let image_clicked = match &choice.preview {
                                        Ok(texture) => ui
                                            .add(
                                                egui::Image::new((
                                                    texture.id(),
                                                    egui::vec2(96.0, 54.0),
                                                ))
                                                .sense(egui::Sense::click()),
                                            )
                                            .clicked(),
                                        Err(error) => {
                                            ui.weak("No Preview").on_hover_text(error);
                                            false
                                        }
                                    };
                                    ui.selectable_label(
                                        self.recipe.image_donor.as_ref() == Some(&choice.name),
                                        &choice.name,
                                    )
                                    .clicked()
                                        || image_clicked
                                })
                                .inner;
                            if selected {
                                self.recipe.image_donor = Some(choice.name.clone());
                                self.recipe.image = None;
                                self.preview = None;
                                self.choose_image = false;
                            }
                        }
                        if choices.is_empty() {
                            ui.label("No activity artwork found in these packages.");
                        }
                    });
            } else if self.choices_job.is_some() {
                ui.spinner();
            }
        }
        if let Some(Err(error)) = &self.images.outcome {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if self.images.busy() {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        ui.weak("Saved in the recipe and cropped for portrait and landscape.");
    }
}

impl PackageAuthoringApp {
    pub(super) fn open_activity(&mut self, path: &Path) {
        if let Some(editor) = &mut self.activity_editor {
            if editor.path == path {
                return;
            }
            if editor.dirty() || editor.images.busy() {
                editor.pending = Some(Action::Open(path.to_owned()));
                return;
            }
        }
        match Editor::load(path) {
            Ok(editor) => {
                self.activity_editor = Some(editor);
                self.library_open = false;
            }
            Err(error) => self
                .log
                .push(LogEntry::error(format!("Could not open activity: {error}"))),
        }
    }

    pub(super) fn confirm_activity_close(&mut self) {
        if let Some(editor) = &mut self.activity_editor {
            editor.pending = Some(Action::CloseApp);
        }
    }

    pub(super) fn draw_activity_editor(&mut self, ctx: &egui::Context) {
        let busy = self.has_background_work();
        let Some(editor) = &mut self.activity_editor else {
            return;
        };
        let labels = self
            .map_entries
            .iter()
            .filter(|map| map.path != editor.path && self.enabled_recipe_paths.contains(&map.path))
            .filter_map(|map| map.destination.clone())
            .collect::<BTreeSet<_>>();
        editor.poll_images();
        if editor.images.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let mut open = true;
        let mut close = false;
        let mut saved = false;
        let mut proceed = false;
        egui::Window::new("Activity Recipe")
            .id(egui::Id::new("activity-recipe"))
            .open(&mut open)
            .default_width(460.0)
            .resizable(true)
            .show(ctx, |ui| {
                workbench_style(ui);
                // Preview growth and scrolling must leave the save and conflict actions visible.
                let footer_id = ui.id().with("activity-footer-height");
                let footer_height = ctx
                    .data(|data| data.get_temp::<f32>(footer_id))
                    .unwrap_or(40.0);
                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - footer_height).clamp(0.0, 660.0))
                    .show(ui, |ui| {
                        ui.add_enabled_ui(!busy && editor.pending.is_none(), |ui| {
                            editor.fields(ui, &self.packages, &labels)
                        });
                    });
                let footer_top = ui.cursor().top();
                ui.separator();
                if let Some(error) = &editor.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                if editor.pending.is_some() {
                    ui.label("Save changes to this activity before continuing?");
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(
                                !busy && !editor.images.busy(),
                                egui::Button::new("Save and Continue"),
                            )
                            .clicked()
                        {
                            saved = editor.save(self.recipe_library.as_ref());
                            proceed = saved;
                        }
                        if ui
                            .add_enabled(
                                !editor.images.busy(),
                                egui::Button::new("Discard and Continue"),
                            )
                            .clicked()
                        {
                            proceed = true;
                        }
                        if ui.button("Keep Editing").clicked() {
                            editor.pending = None;
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !busy && !editor.images.busy(),
                                egui::Button::new("Save Activity"),
                            )
                            .clicked()
                        {
                            saved = editor.save(self.recipe_library.as_ref());
                        }
                        close = ui
                            .add_enabled(!editor.images.busy(), egui::Button::new("Close"))
                            .clicked();
                        if editor.dirty() {
                            ui.weak("Unsaved changes");
                        }
                    });
                }
                let footer_height = ui.cursor().top() - footer_top + ui.spacing().item_spacing.y;
                ctx.data_mut(|data| data.insert_temp(footer_id, footer_height));
            });
        if !open || close {
            if editor.dirty() || editor.images.busy() {
                editor.pending = Some(Action::Close);
            } else {
                editor.pending = Some(Action::Close);
                proceed = true;
            }
        }
        let action = proceed.then(|| editor.pending.take()).flatten();
        if saved {
            self.refresh_recipe_library();
        }
        if let Some(action) = action {
            self.activity_editor = None;
            match action {
                Action::Close => {}
                Action::Open(path) => self.open_activity(&path),
                Action::CloseApp => {
                    self.request_recipe_action(PendingRecipeAction::Close);
                }
            }
        }
    }
}
