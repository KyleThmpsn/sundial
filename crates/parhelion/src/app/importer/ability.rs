//! Source jump choices are prepared off the UI thread and applied to the unchanged draft.
use super::*;
use crate::subclass::{import, layout};
use parhelion_import::d1_roi::{AbilityChoice, Source as RoiSource};

// Keep the entry point hidden until ability preparation is ready for use.
const AVAILABLE: bool = false;

pub(super) struct Picker {
    open: bool,
    pending: bool,
    choices: Vec<AbilityChoice>,
    selected: Option<usize>,
    entry: u8,
    loaded: Option<(PathBuf, PathBuf)>,
    notice: String,
    failed: bool,
}

impl Default for Picker {
    fn default() -> Self {
        Self {
            open: false,
            pending: false,
            choices: Vec::new(),
            selected: None,
            entry: layout::MOVEMENT[0],
            loaded: None,
            notice: String::new(),
            failed: false,
        }
    }
}

impl Picker {
    pub(super) fn close(&mut self) -> bool {
        self.open = false;
        self.pending
    }

    pub(super) fn worker_stopped(&mut self) {
        if self.pending {
            self.pending = false;
            self.notice = "The ability import worker stopped. Read the choices again.".into();
            self.failed = true;
        }
    }
}

pub(super) struct Listed {
    source_packages: PathBuf,
    native_packages: PathBuf,
    result: Result<Vec<AbilityChoice>, String>,
}

pub(super) struct Prepared {
    source_packages: PathBuf,
    native_packages: PathBuf,
    result: Result<import::Prepared, String>,
}

impl PackageAuthoringApp {
    pub(super) fn draw_roi_ability_button(&mut self, ui: &mut egui::Ui, has_source: bool) {
        if !AVAILABLE {
            return;
        }
        let enabled = has_source && self.recipe.kind == ItemKind::Subclass && self.importer_idle();
        if ui
            .add_enabled(enabled, egui::Button::new("Import D1 Ability…"))
            .on_disabled_hover_text("Open a subclass draft and choose the D1 packages folder")
            .clicked()
        {
            self.importer.abilities = Picker {
                open: true,
                ..Picker::default()
            };
            self.read_roi_abilities(ui.ctx());
        }
    }

    pub(super) fn draw_roi_ability_window(&mut self, ctx: &egui::Context) {
        if !AVAILABLE
            || !self.importer.open
            || !self.importer.enabled
            || !self.importer.abilities.open
        {
            return;
        }
        let idle = self.importer_idle();
        let current = self
            .importer
            .settings
            .d1_roi
            .clone()
            .map(|source| (source, self.packages.clone()));
        if self.importer.abilities.loaded.is_some() && self.importer.abilities.loaded != current {
            self.importer.abilities.loaded = None;
            self.importer.abilities.choices.clear();
            self.importer.abilities.selected = None;
            self.importer.abilities.notice =
                "The package folder changed. Read the choices again.".into();
            self.importer.abilities.failed = true;
        }
        let mut open = true;
        let mut read = false;
        let mut start = false;
        egui::Window::new("Import D1 Ability")
            .open(&mut open)
            .default_width(530.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(
                    "Import D1 jump modifiers into a movement choice. The native jump controller \
                     and animations are retained.",
                );
                ui.add_space(6.0);
                if self.recipe.kind != ItemKind::Subclass {
                    ui.label("Open a subclass draft to import an ability.");
                }
                read = ui
                    .add_enabled(
                        idle && current.is_some(),
                        egui::Button::new("Read D1 Choices"),
                    )
                    .clicked();
                let picker = &mut self.importer.abilities;
                ui.add_enabled_ui(idle, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(230.0)
                        .show(ui, |ui| {
                            for (index, choice) in picker.choices.iter().enumerate() {
                                let response = ui.selectable_label(
                                    picker.selected == Some(index),
                                    format!("{} · {}", choice.name, choice.subclass_name),
                                );
                                if response.on_hover_text(&choice.description).clicked() {
                                    picker.selected = Some(index);
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("Destination");
                        egui::ComboBox::from_id_salt("d1-ability-destination")
                            .selected_text(crate::subclass::AbilitySlot::Movement.entry_label(picker.entry))
                            .show_ui(ui, |ui| {
                                for &entry in &layout::MOVEMENT {
                                    ui.selectable_value(
                                        &mut picker.entry,
                                        entry,
                                        crate::subclass::AbilitySlot::Movement.entry_label(entry),
                                    );
                                }
                            });
                    });
                });
                if let Some(choice) = picker.selected.and_then(|index| picker.choices.get(index)) {
                    ui.label(&choice.description);
                }
                ui.weak("Replaces the destination's ability settings. Save the draft to keep the import.");
                if !picker.notice.is_empty() {
                    if picker.failed {
                        ui.colored_label(ui.visuals().error_fg_color, &picker.notice);
                    } else {
                        ui.label(&picker.notice);
                    }
                }
                start = ui
                    .add_enabled(
                        idle
                            && self.recipe.kind == ItemKind::Subclass
                            && picker.loaded == current
                            && picker.selected.is_some(),
                        egui::Button::new("Import into Draft"),
                    )
                    .clicked();
                if !idle {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Reading and translating the ability…");
                    });
                }
            });
        self.importer.abilities.open = open;
        if !open && self.importer.abilities.pending {
            self.importer.cancel.store(true, Ordering::Relaxed);
        } else if read {
            self.read_roi_abilities(ctx);
        } else if start {
            self.start_roi_ability(ctx);
        }
    }

    fn read_roi_abilities(&mut self, ctx: &egui::Context) {
        if !self.importer_idle() {
            return;
        }
        let Some(source_packages) = self.importer.settings.d1_roi.clone() else {
            return;
        };
        let native_packages = self.packages.clone();
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.abilities.pending = true;
        self.importer.import_started = Some(Instant::now());
        self.importer.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.importer.cancel.clone();
        self.importer.abilities.loaded = None;
        self.importer.abilities.choices.clear();
        self.importer.abilities.selected = None;
        self.importer.abilities.notice.clear();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = parhelion_import::cancellation::run(cancel, || {
                RoiSource::open(&source_packages, |_, _| {})?.ability_choices()
            })
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::AbilitiesListed(Box::new(Listed {
                source_packages,
                native_packages,
                result,
            })));
            ctx.request_repaint();
        });
    }

    fn start_roi_ability(&mut self, ctx: &egui::Context) {
        if !self.importer_idle() || self.recipe.kind != ItemKind::Subclass {
            return;
        }
        let Some(source_packages) = self.importer.settings.d1_roi.clone() else {
            return;
        };
        let Some(choice) = self
            .importer
            .abilities
            .selected
            .and_then(|index| self.importer.abilities.choices.get(index).cloned())
        else {
            return;
        };
        let native_packages = self.packages.clone();
        if self.importer.abilities.loaded.as_ref()
            != Some(&(source_packages.clone(), native_packages.clone()))
        {
            return;
        }
        // The subclass is the ability's source item, so its conversions sit with every other
        // import's.
        let output = data_root().and_then(|root| {
            service::item_directory(
                &root.join(service::IMPORTS),
                super::Source::D1RiseOfIron.folder(),
                choice.subclass,
            )
            .and_then(|item| service::reserve_assets(&item, "ability"))
            .map_err(|error| error.to_string())
        });
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                self.importer.abilities.notice = error;
                self.importer.abilities.failed = true;
                return;
            }
        };
        let recipe = self.recipe.clone();
        let entry = self.importer.abilities.entry;
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.abilities.pending = true;
        self.importer.import_started = Some(Instant::now());
        self.importer.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.importer.cancel.clone();
        self.importer.abilities.notice.clear();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = parhelion_import::cancellation::run(cancel, || {
                import::prepare(&import::Request {
                    source_packages: &source_packages,
                    native_packages: &native_packages,
                    output: &output,
                    recipe: &recipe,
                    choice: &choice,
                    entry,
                })
                .map_err(anyhow::Error::msg)
            })
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::AbilityPrepared(Box::new(Prepared {
                source_packages,
                native_packages,
                result,
            })));
            ctx.request_repaint();
        });
    }

    pub(super) fn finish_roi_abilities(&mut self, listed: Listed) {
        self.importer.abilities.pending = false;
        if !self.importer.open || !self.importer.enabled || !self.importer.abilities.open {
            return;
        }
        let result = if self.packages != listed.native_packages
            || self.importer.settings.d1_roi.as_ref() != Some(&listed.source_packages)
        {
            Err("The package folder changed. Read the choices again.".into())
        } else {
            listed.result
        };
        match result {
            Ok(choices) => {
                self.importer.abilities.notice = if choices.is_empty() {
                    "No supported D1 jump choices were found.".into()
                } else {
                    String::new()
                };
                self.importer.abilities.failed = false;
                self.importer.abilities.loaded =
                    Some((listed.source_packages, listed.native_packages));
                self.importer.abilities.choices = choices;
            }
            Err(error) => {
                self.importer.abilities.notice = error;
                self.importer.abilities.failed = true;
            }
        }
    }

    pub(super) fn finish_roi_ability(&mut self, prepared: Prepared) {
        self.importer.abilities.pending = false;
        if !self.importer.open || !self.importer.enabled || !self.importer.abilities.open {
            return;
        }
        let result = if self.packages != prepared.native_packages
            || self.importer.settings.d1_roi.as_ref() != Some(&prepared.source_packages)
        {
            Err("The package folder changed during import. Prepare again.".into())
        } else {
            prepared
                .result
                .and_then(|candidate| candidate.apply(&mut self.recipe))
        };
        match result {
            Ok(()) => {
                self.recipe_dirty = true;
                self.invalidate_results();
                self.importer.abilities.notice =
                    "D1 jump modifiers imported into the draft. Save to keep the change.".into();
                self.importer.abilities.failed = false;
            }
            Err(error) => {
                self.importer.abilities.notice = error;
                self.importer.abilities.failed = true;
            }
        }
    }
}
