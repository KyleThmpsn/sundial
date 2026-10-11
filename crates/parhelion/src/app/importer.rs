use super::*;
use crate::app::style;
use parhelion_import::d2_mot::service::{self, ScanProgress, Settings, Weapon};

fn item_kind(item: &Weapon) -> ItemKind {
    match item.family() {
        service::Family::Weapon => ItemKind::Weapon,
        service::Family::Armor => ItemKind::Armor,
        service::Family::GhostShell => ItemKind::GhostShell,
        service::Family::Ship => ItemKind::Ship,
        service::Family::Sparrow => ItemKind::Sparrow,
        service::Family::Shader => ItemKind::Shader,
        service::Family::Emblem => ItemKind::Emblem,
    }
}
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

mod batch;
mod browser;
mod filters;
mod icons;
mod models;
mod perks;
#[cfg(test)]
mod tests;

pub(super) struct Importer {
    pub open: bool,
    pub enabled: bool,
    settings: Settings,
    weapons: Vec<Weapon>,
    selected: BTreeSet<u32>,
    browser: browser::Browser,
    view_path: Option<PathBuf>,
    icons: icons::Icons,
    read_requested: bool,
    catalog_target: PathBuf,
    models: models::Picker,
    perks: perks::Picker,
    receiver: Option<Receiver<Event>>,
    scan_progress: Option<ScanProgress>,
    scan_started: Option<Instant>,
    scan_error: Option<String>,
    import_started: Option<Instant>,
    importing: Option<Progress>,
    notice: String,
    outcome: Option<Outcome>,
    cancel: Arc<AtomicBool>,
    /// (donor count, weapon count) the no-donor set was computed for.
    donor_stamp: (usize, usize),
}

/// A running import: one slot per worker thread.
struct Progress {
    total: usize,
    done: usize,
    /// Weapon name and current step for each busy worker.
    slots: Vec<Option<(String, String)>>,
}

/// Result of the last import run.
struct Outcome {
    added: usize,
    failures: Vec<String>,
    show_failures: bool,
    cancelled: bool,
}

enum Event {
    ScanProgress(ScanProgress),
    Scanned(Result<Vec<Weapon>, String>),
    /// A step message for a worker slot; slot 0 when no import is running.
    Progress(usize, String),
    /// A worker slot started (`Some`) or finished (`None`) a weapon.
    Slot(usize, Option<String>),
    Done(usize),
    ModelPrepared(Box<models::Prepared>),
    PerkPrepared(Box<perks::Prepared>),
    Imported {
        paths: Vec<PathBuf>,
        errors: Vec<String>,
        cancelled: bool,
    },
}

fn data_root() -> Result<PathBuf, String> {
    sundial::package_authoring::parhelion_data_directory()
        .ok_or_else(|| "Could not locate Sundial’s data folder".into())
}

fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

impl Default for Importer {
    fn default() -> Self {
        let loaded =
            data_root().and_then(|root| match std::fs::read(root.join("d2-importer.json")) {
                Ok(bytes) => {
                    serde_json::from_slice::<Settings>(&bytes).map_err(|error| error.to_string())
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(Settings::default())
                }
                Err(error) => Err(error.to_string()),
            });
        let (settings, notice) = match loaded {
            Ok(settings) => (settings, String::new()),
            Err(error) => (Settings::default(), error),
        };
        let mut browser = browser::Browser::default();
        let view_path = data_root().ok().map(|root| root.join("importer-view.json"));
        if let Some(path) = &view_path {
            browser.apply_view(browser::View::load(path));
        }
        Self {
            open: false,
            enabled: settings.enabled,
            settings,
            weapons: Vec::new(),
            selected: BTreeSet::new(),
            browser,
            view_path,
            icons: icons::Icons::default(),
            read_requested: false,
            catalog_target: PathBuf::new(),
            models: models::Picker::default(),
            perks: perks::Picker::default(),
            receiver: None,
            scan_progress: None,
            scan_started: None,
            scan_error: None,
            import_started: None,
            importing: None,
            notice,
            outcome: None,
            cancel: Arc::new(AtomicBool::new(false)),
            donor_stamp: (usize::MAX, usize::MAX),
        }
    }
}

impl Importer {
    pub fn busy(&self) -> bool {
        self.receiver.is_some()
    }

    fn save(&mut self) -> Result<(), String> {
        self.settings.enabled = self.enabled;
        let root = data_root()?;
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(&self.settings).map_err(|error| error.to_string())?;
        sundial::package_authoring::replace_authoring_file(&root.join("d2-importer.json"), &bytes)
            .map_err(|error| error.to_string())
    }

    /// Selected weapons that have a donor to convert with.
    fn selected_weapons(&self) -> impl Iterator<Item = &Weapon> {
        self.weapons.iter().filter(|weapon| {
            self.selected.contains(&weapon.hash) && self.browser.importable(weapon)
        })
    }
}

impl PackageAuthoringApp {
    pub(super) fn draw_importer_preference(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                !self.importer.busy(),
                egui::Checkbox::new(&mut self.importer.enabled, "D2 Importer"),
            )
            .on_hover_text("Browse modern Destiny 2 items and import them into the recipe library.")
            .changed()
        {
            if !self.importer.enabled {
                self.importer.open = false;
            }
            if let Err(error) = self.importer.save() {
                self.log.push(LogEntry::error(error));
            }
        }
    }

    pub(super) fn draw_importer(&mut self, ctx: &egui::Context) {
        self.poll_importer();
        if self.importer.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let closed = !self.importer.open || !self.importer.enabled;
        // A pending search is written when this window or Parhelion closes.
        if closed || ctx.input(|input| input.viewport().close_requested()) {
            self.importer.save_view();
        }
        if closed {
            return;
        }
        self.importer.icons.poll(ctx);
        let stamp = (
            self.donor_summaries.len() + self.gear_donors.values().map(Vec::len).sum::<usize>(),
            self.importer.weapons.len(),
        );
        if self.importer.donor_stamp != stamp {
            self.importer.donor_stamp = stamp;
            self.importer.browser.no_donor = self.importer_no_donor();
        }
        if !self.importer.busy() && self.importer.catalog_target != self.packages {
            self.importer.read_requested = false;
            self.importer.weapons.clear();
            self.importer.selected.clear();
            self.importer.browser.dirty = true;
            self.importer.browser.reset_scroll = true;
        }
        if !self.importer.read_requested
            && !self.importer.busy()
            && self.importer.settings.modern_packages.is_some()
        {
            self.scan_importer(ctx, false);
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("d2-importer"),
            egui::ViewportBuilder::default()
                .with_title("D2 Importer")
                .with_inner_size([980.0, 740.0])
                .with_min_inner_size([680.0, 520.0]),
            |ui, class| {
                if ui.input(|input| input.viewport().close_requested()) {
                    self.importer.open = false;
                    self.importer.read_requested = false;
                }
                if class == egui::ViewportClass::EmbeddedWindow {
                    let mut open = self.importer.open;
                    egui::Window::new("D2 Importer")
                        .open(&mut open)
                        .default_size([980.0, 740.0])
                        .resizable(true)
                        .show(ui, |ui| self.draw_importer_contents(ui));
                    self.importer.open = open;
                    if !open {
                        self.importer.read_requested = false;
                    }
                } else {
                    egui::CentralPanel::default().show(ui, |ui| self.draw_importer_contents(ui));
                }
            },
        );
    }

    fn poll_importer(&mut self) {
        loop {
            let event = match self
                .importer
                .receiver
                .as_ref()
                .map(|receiver| receiver.try_recv())
            {
                Some(Ok(event)) => event,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.importer.receiver = None;
                    self.importer.scan_progress = None;
                    self.importer.scan_started = None;
                    self.importer.importing = None;
                    self.importer.import_started = None;
                    self.importer.notice = "Import worker stopped unexpectedly.".into();
                    break;
                }
                _ => break,
            };
            match event {
                Event::PerkPrepared(prepared) => {
                    self.importer.receiver = None;
                    self.importer.import_started = None;
                    self.importer.importing = None;
                    self.finish_imported_perk(*prepared);
                }
                Event::ModelPrepared(prepared) => {
                    self.importer.receiver = None;
                    self.importer.import_started = None;
                    self.importer.importing = None;
                    self.finish_imported_model(*prepared);
                }
                Event::ScanProgress(progress) => self.importer.scan_progress = Some(progress),
                Event::Progress(slot, message) => {
                    match self
                        .importer
                        .importing
                        .as_mut()
                        .and_then(|progress| progress.slots.get_mut(slot))
                    {
                        Some(Some((_, step))) => *step = message,
                        _ => self.importer.notice = message,
                    }
                }
                Event::Slot(slot, name) => {
                    if let Some(entry) = self
                        .importer
                        .importing
                        .as_mut()
                        .and_then(|progress| progress.slots.get_mut(slot))
                    {
                        *entry = name.map(|name| (name, String::new()));
                    }
                }
                Event::Done(done) => {
                    if let Some(progress) = self.importer.importing.as_mut() {
                        progress.done = done;
                    }
                }
                Event::Scanned(result) => {
                    self.importer.receiver = None;
                    self.importer.scan_progress = None;
                    self.importer.scan_started = None;
                    match result {
                        Ok(mut weapons) => {
                            let installed: BTreeSet<_> = self
                                .donor_summaries
                                .iter()
                                .chain(self.gear_donors.values().flatten())
                                .map(|donor| donor.hash)
                                .collect();
                            for weapon in &mut weapons {
                                weapon.dummy |=
                                    sundial::package_authoring::is_dummy_item(weapon.hash);
                                weapon.present_in_native |= installed.contains(&weapon.hash)
                                    || service::destination_hash(weapon.hash)
                                        .is_ok_and(|hash| installed.contains(&hash));
                            }
                            self.importer.browser.dirty = true;
                            self.importer.browser.reset_scroll = true;
                            self.importer.browser.anchor = None;
                            self.importer.scan_error = None;
                            self.importer.notice.clear();
                            self.importer.weapons = weapons;
                            self.importer.selected.clear();
                        }
                        Err(error) => {
                            self.importer.scan_error = Some(error);
                            self.importer.notice.clear();
                        }
                    }
                }
                Event::Imported {
                    paths,
                    errors,
                    cancelled,
                } => {
                    self.importer.cancel.store(false, Ordering::Relaxed);
                    self.importer.receiver = None;
                    self.importer.importing = None;
                    self.importer.import_started = None;
                    self.importer.notice.clear();
                    self.importer.outcome = Some(Outcome {
                        added: paths.len(),
                        show_failures: paths.is_empty(),
                        failures: errors,
                        cancelled,
                    });
                    if !paths.is_empty() {
                        self.importer.selected.clear();
                        self.refresh_recipe_library();
                        self.reveal_library_entries(paths);
                    }
                }
            }
        }
    }

    /// Weapons the conversion would turn down before extracting anything: no installed native
    /// weapon of their type, no base for their type and no profile donor present. Empty until
    /// the donor catalog loads.
    fn importer_no_donor(&self) -> BTreeSet<u32> {
        if self.donor_summaries.is_empty() && self.gear_donors.values().all(Vec::is_empty) {
            return BTreeSet::new();
        }
        let types: BTreeSet<&str> = self
            .donor_summaries
            .iter()
            .map(|donor| donor.type_name.as_str())
            .collect();
        let donors: BTreeSet<u32> = self
            .donor_summaries
            .iter()
            .map(|donor| donor.hash)
            .collect();
        self.importer
            .weapons
            .iter()
            .filter(|weapon| {
                if weapon.is_shader() {
                    return self.gear_donors_for(ItemKind::Shader).is_empty();
                }
                if weapon.family() == service::Family::Emblem {
                    return self.gear_donors_for(ItemKind::Emblem).is_empty();
                }
                if weapon.family().is_model_gear() {
                    return !self.gear_donors_for(item_kind(weapon)).iter().any(|donor| {
                        weapon.accepts_gear_donor(
                            donor.bucket_hash,
                            self.catalog
                                .as_ref()
                                .and_then(|catalog| catalog.item_class_type(donor.hash)),
                        )
                    });
                }
                !types.contains(weapon.weapon_type.as_str())
                    && !service::profile_donor(weapon.hash)
                        .or_else(|| service::type_base(&weapon.weapon_type))
                        .is_some_and(|hash| donors.contains(&hash))
            })
            .map(|weapon| weapon.hash)
            .collect()
    }

    fn importer_idle(&self) -> bool {
        !self.importer.busy() && self.build_receiver.is_none() && self.install_receiver.is_none()
    }

    fn draw_importer_contents(&mut self, ui: &mut egui::Ui) {
        style::workbench_style(ui);
        let idle = self.importer_idle();
        self.importer.browser.refresh(&self.importer.weapons);
        egui::Panel::top("d2-importer-header")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ui, |ui| self.draw_importer_header(ui, idle));
        if !self.importer.weapons.is_empty()
            || self.importer.busy()
            || self.importer.outcome.is_some()
        {
            egui::Panel::bottom("d2-importer-actions")
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)))
                .show(ui, |ui| self.draw_importer_footer(ui, idle));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 6)))
            .show(ui, |ui| {
                if self.importer.settings.modern_packages.is_none() {
                    self.draw_importer_welcome(ui, idle);
                } else if let Some(error) = self
                    .importer
                    .scan_error
                    .clone()
                    .filter(|_| self.importer.weapons.is_empty())
                {
                    self.draw_importer_scan_error(ui, idle, &error);
                } else if self.importer.scan_progress.is_some() {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Reading Item Catalog").heading().weak());
                    });
                } else {
                    self.draw_importer_toolbar(ui);
                    ui.add_space(2.0);
                    ui.separator();
                    self.draw_importer_browser(ui, idle);
                }
            });
        self.draw_modern_perk_window(ui.ctx());
    }

    fn draw_importer_header(&mut self, ui: &mut egui::Ui, idle: bool) {
        ui.horizontal(|ui| {
            match &self.importer.settings.modern_packages {
                Some(path) => {
                    let shown = path.display().to_string();
                    ui.add(egui::Label::new(&shown).truncate());
                }
                None => {
                    ui.label(egui::RichText::new("Choose a Source Folder").weak());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(idle, |ui| {
                    let has_source = self.importer.settings.modern_packages.is_some();
                    self.draw_modern_perk_button(ui, has_source);
                    if ui
                        .add_enabled(has_source, egui::Button::new("Refresh"))
                        .clicked()
                    {
                        self.scan_importer(ui.ctx(), true);
                    }
                    if ui
                        .button(if has_source {
                            "Change Folder…"
                        } else {
                            "Choose Folder…"
                        })
                        .clicked()
                    {
                        self.choose_modern_folder(ui.ctx());
                    }
                });
            });
        });
        let summary = self.importer_catalog_summary();
        let blockers = self.importer_blockers();
        if !summary.is_empty() || (!blockers.is_empty() && !self.importer.weapons.is_empty()) {
            ui.horizontal_wrapped(|ui| {
                if !summary.is_empty() {
                    ui.label(egui::RichText::new(summary).weak());
                }
                if !blockers.is_empty() && !self.importer.weapons.is_empty() {
                    ui.label(
                        egui::RichText::new("Blocked:")
                            .color(ui.visuals().warn_fg_color)
                            .strong(),
                    );
                    ui.label(blockers.join(" "));
                }
            });
        }
        if !self.importer.notice.is_empty() && !self.importer.busy() {
            ui.colored_label(ui.visuals().error_fg_color, &self.importer.notice);
        }
        // A failed refresh keeps the earlier catalog usable.
        if let Some(error) = self
            .importer
            .scan_error
            .clone()
            .filter(|_| !self.importer.weapons.is_empty())
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("Refresh failed:")
                        .color(ui.visuals().error_fg_color)
                        .strong(),
                );
                ui.label(error);
                if ui
                    .add_enabled(idle, egui::Button::new("Try Again"))
                    .clicked()
                {
                    self.scan_importer(ui.ctx(), true);
                }
            });
        }
    }

    fn importer_catalog_summary(&self) -> String {
        let weapons = &self.importer.weapons;
        if weapons.is_empty() {
            return String::new();
        }
        let installed = weapons
            .iter()
            .filter(|weapon| weapon.present_in_native)
            .count();
        let count = ItemKind::ALL
            .into_iter()
            .filter_map(|kind| {
                let count = weapons
                    .iter()
                    .filter(|item| item_kind(item) == kind)
                    .count();
                (count > 0).then(|| {
                    format!(
                        "{count} {}",
                        if count == 1 {
                            kind.noun().to_owned()
                        } else {
                            kind.plural().to_lowercase()
                        }
                    )
                })
            })
            .collect::<Vec<_>>()
            .join(" · ");
        format!("{count} · {installed} installed")
    }

    fn importer_blockers(&self) -> Vec<&'static str> {
        let mut blockers = Vec::new();
        if self.recipe_library.is_none() {
            blockers.push("Recipe library unavailable.");
        }
        if self.donor_summaries.is_empty() && self.gear_donors.values().all(Vec::is_empty) {
            blockers.push("Catalog loading.");
        }
        if self.build_receiver.is_some() || self.install_receiver.is_some() {
            blockers.push("Build in progress.");
        }
        blockers
    }

    fn draw_importer_welcome(&mut self, ui: &mut egui::Ui, idle: bool) {
        ui.add_space(ui.available_height() * 0.25);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("Choose a Source Folder").heading().weak());
            style::hint(ui, "Choose your modern Destiny 2 installation or its packages folder.");
            style::hint(ui, "Imports become editable recipes in your library. Build and install them when you are ready.");
            ui.add_space(8.0);
            if ui
                .add_enabled(idle, style::primary(ui, "Choose Folder…"))
                .clicked()
            {
                self.choose_modern_folder(ui.ctx());
            }
        });
    }

    fn draw_importer_scan_error(&mut self, ui: &mut egui::Ui, idle: bool, error: &str) {
        ui.add_space(ui.available_height() * 0.25);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("Catalog read failed")
                    .heading()
                    .color(ui.visuals().error_fg_color),
            );
            ui.add(egui::Label::new(error).wrap());
            ui.add_space(8.0);
            if ui
                .add_enabled(idle, style::primary(ui, "Try Again"))
                .clicked()
            {
                self.scan_importer(ui.ctx(), true);
            }
            if ui
                .add_enabled(idle, egui::Button::new("Change Folder…"))
                .clicked()
            {
                self.choose_modern_folder(ui.ctx());
            }
        });
    }

    fn draw_importer_footer(&mut self, ui: &mut egui::Ui, idle: bool) {
        if self.importer.busy() {
            self.draw_importer_activity(ui);
            ui.add_space(6.0);
        } else if self.importer.outcome.is_some() {
            self.draw_importer_outcome(ui);
            ui.add_space(6.0);
        }
        let shown = self.importer.browser.visible.len();
        let selected = self.importer.selected.len();
        let importable = self.importer.selected_weapons().count();
        let has_catalog = !self.importer.weapons.is_empty();
        let blockers = self.importer_blockers();
        let mut import = false;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(idle && has_catalog, |ui| {
                ui.label(format!("{shown} shown · {selected} selected"));
                if ui
                    .add_enabled(shown > 0, egui::Button::new("Select Shown"))
                    .clicked()
                {
                    let hashes: Vec<_> = self
                        .importer
                        .browser
                        .visible
                        .iter()
                        .map(|&index| &self.importer.weapons[index])
                        .filter(|weapon| self.importer.browser.importable(weapon))
                        .map(|weapon| weapon.hash)
                        .collect();
                    self.importer.selected.extend(hashes);
                }
                if ui
                    .add_enabled(selected > 0, egui::Button::new("Clear Selection"))
                    .clicked()
                {
                    self.importer.selected.clear();
                    self.importer.browser.anchor = None;
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = match importable {
                    0 => "Import".to_owned(),
                    count => format!(
                        "Import {count} {}",
                        ItemKind::count_noun(
                            self.importer.selected_weapons().map(item_kind),
                            count
                        )
                    ),
                };
                let ready = idle && importable > 0 && blockers.is_empty();
                let response = ui.add_enabled(ready, style::primary(ui, &label));
                if response.clicked() {
                    import = true;
                }
                let reason = if let Some(blocker) = blockers.first().filter(|_| !ready) {
                    (*blocker).to_owned()
                } else if selected > importable {
                    format!("{} skipped: no donor.", selected - importable)
                } else {
                    String::new()
                };
                if !reason.is_empty() {
                    response.on_disabled_hover_text(&reason);
                    style::hint(ui, &reason);
                }
            });
        });
        if import {
            self.import_selected(ui.ctx());
        }
    }

    fn draw_importer_activity(&mut self, ui: &mut egui::Ui) {
        style::block(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if let Some(progress) = self.importer.scan_progress {
                let elapsed = self
                    .importer
                    .scan_started
                    .map(|started| started.elapsed().as_secs());
                match progress {
                    ScanProgress::ReadingItems {
                        completed,
                        total,
                        weapons,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Reading Items").strong());
                            if let Some(elapsed) = elapsed {
                                ui.label(egui::RichText::new(clock(elapsed)).weak());
                            }
                        });
                        ui.add(
                            egui::ProgressBar::new(completed as f32 / total.max(1) as f32)
                                .text(format!("{completed} / {total} · {weapons} items")),
                        );
                    }
                    phase => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(match phase {
                                ScanProgress::CheckingCache => "Checking Cache…",
                                ScanProgress::LoadingCachedItems => "Loading Cached Items…",
                                ScanProgress::OpeningModernPackages => "Opening Packages…",
                                ScanProgress::OpeningNativePackages => "Matching Native Items…",
                                ScanProgress::SavingCatalog => "Saving Catalog…",
                                ScanProgress::ReadingItems { .. } => unreachable!(),
                            });
                            if let Some(elapsed) = elapsed {
                                ui.label(egui::RichText::new(clock(elapsed)).weak());
                            }
                        });
                    }
                }
                return;
            }
            let elapsed = self
                .importer
                .import_started
                .map(|started| started.elapsed().as_secs());
            if let Some(progress) = &self.importer.importing {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Importing").strong());
                    ui.label(
                        egui::RichText::new(format!("{} / {}", progress.done, progress.total))
                            .weak(),
                    );
                    if let Some(elapsed) = elapsed {
                        ui.label(egui::RichText::new(clock(elapsed)).weak());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.importer.cancel.load(Ordering::Relaxed) {
                            ui.label(egui::RichText::new("Cancelling…").weak());
                        } else if ui.button("Cancel").clicked() {
                            self.importer.cancel.store(true, Ordering::Relaxed);
                        }
                    });
                });
                ui.add(egui::ProgressBar::new(
                    progress.done as f32 / progress.total.max(1) as f32,
                ));
                for (name, step) in progress.slots.iter().flatten() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(name);
                        ui.label(egui::RichText::new(step).weak());
                    });
                }
                return;
            }
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(if self.importer.notice.is_empty() {
                    "Working…"
                } else {
                    &self.importer.notice
                });
            });
        });
    }

    fn draw_importer_outcome(&mut self, ui: &mut egui::Ui) {
        let mut dismiss = false;
        style::block(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let Some(outcome) = &mut self.importer.outcome else {
                return;
            };
            ui.horizontal(|ui| {
                if outcome.cancelled {
                    ui.label(egui::RichText::new("Cancelled.").strong());
                }
                if outcome.added > 0 {
                    ui.label(
                        egui::RichText::new(format!(
                            "Added {}.",
                            plural(outcome.added, "recipe", "recipes")
                        ))
                        .color(style::success_color(ui.visuals()))
                        .strong(),
                    );
                }
                if !outcome.failures.is_empty() {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} failed.",
                            plural(outcome.failures.len(), "item", "items")
                        ))
                        .color(ui.visuals().error_fg_color)
                        .strong(),
                    );
                    ui.toggle_value(&mut outcome.show_failures, "Details");
                }
                if outcome.added == 0 && outcome.failures.is_empty() && !outcome.cancelled {
                    ui.label("Nothing imported.");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    dismiss = ui.button("Dismiss").clicked();
                });
            });
            if outcome.show_failures && !outcome.failures.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("d2-importer-failures")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for failure in &outcome.failures {
                            ui.add(egui::Label::new(failure).wrap());
                        }
                    });
            }
        });
        if dismiss {
            self.importer.outcome = None;
        }
    }

    fn choose_modern_folder(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Modern Destiny 2 Build")
            .pick_folder()
        else {
            return;
        };
        match service::packages_path(&path) {
            Ok(path) => {
                self.importer.settings.modern_packages = Some(path);
                self.importer.weapons.clear();
                self.importer.browser.dirty = true;
                self.importer.browser.reset_scroll = true;
                self.importer.browser.anchor = None;
                self.importer.selected.clear();
                self.importer.scan_error = None;
                self.importer.outcome = None;
                match self.importer.save() {
                    Ok(()) => self.scan_importer(ctx, false),
                    Err(error) => self.importer.notice = error,
                }
            }
            Err(error) => self.importer.notice = error.to_string(),
        }
    }

    fn scan_importer(&mut self, ctx: &egui::Context, refresh: bool) {
        let Some(modern) = self.importer.settings.modern_packages.clone() else {
            return;
        };
        let Ok(root) = data_root() else { return };
        let native = PathBuf::from(&self.packages);
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.read_requested = true;
        self.importer.catalog_target = self.packages.clone();
        self.importer.icons = icons::Icons::default();
        self.importer.scan_progress = Some(ScanProgress::CheckingCache);
        self.importer.scan_started = Some(Instant::now());
        self.importer.scan_error = None;
        self.importer.outcome = None;
        self.importer.notice.clear();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = service::scan_cached(
                &modern,
                &native,
                &root.join("importer/catalog"),
                refresh,
                |progress| {
                    let _ = sender.send(Event::ScanProgress(progress));
                    ctx.request_repaint();
                },
            )
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::Scanned(result));
            ctx.request_repaint();
        });
    }

    fn import_selected(&mut self, ctx: &egui::Context) {
        let existing: Vec<_> = self
            .recipe_entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.identity_hash))
            .collect();
        let Some(modern) = self.importer.settings.modern_packages.clone() else {
            return;
        };
        let Some(library) = self.recipe_library.clone() else {
            return;
        };
        let Ok(root) = data_root() else { return };
        let native = PathBuf::from(&self.packages);
        let selected: Arc<Vec<Weapon>> =
            Arc::new(self.importer.selected_weapons().cloned().collect());
        let risky_donors = self
            .catalog
            .as_ref()
            .map(|catalog| catalog.items_with_unrelated_collection_conditions())
            .unwrap_or_default();
        // Weapons Parhelion installed are not donors: their definitions are authored, and
        // checking them only slows every import.
        let stock = |hash: u32| {
            self.catalog
                .as_ref()
                .and_then(|catalog| catalog.item_definition_tag(hash))
                .is_none_or(crate::package_profile::is_stock_item_definition)
        };
        let gear = [ItemKind::Armor,ItemKind::GhostShell,ItemKind::Ship,ItemKind::Sparrow].into_iter()
                    .flat_map(|kind|self.gear_donors_for(kind)).filter(|donor|stock(donor.hash))
                    .map(|donor|serde_json::json!({"hash":donor.hash,"name":donor.name,"bucket_hash":donor.bucket_hash,
                        "class_type":self.catalog.as_ref().and_then(|catalog|catalog.item_class_type(donor.hash)),
                        "collection_backed":donor.collection_backed})).collect::<Vec<_>>();
        let donors = Arc::new(
            serde_json::json!({"weapons":self.donor_summaries.iter().filter(|donor| stock(donor.hash)).map(|donor| serde_json::json!({"hash":donor.hash,"name":donor.name,"weapon_type":donor.type_name,"present_in_native":true,"bucket_hash":donor.bucket_hash,"collection_backed":donor.collection_backed,"unrelated_collection_condition":risky_donors.contains(&donor.hash)})).collect::<Vec<_>>(),
                "shaders":self.gear_donors_for(ItemKind::Shader).iter().filter(|donor| stock(donor.hash)).map(|donor| serde_json::json!({"hash":donor.hash,"name":donor.name})).collect::<Vec<_>>(),
                "emblems":self.gear_donors_for(ItemKind::Emblem).iter().filter(|donor| stock(donor.hash)).map(|donor| serde_json::json!({"hash":donor.hash,"name":donor.name})).collect::<Vec<_>>(),
                "gear":gear}),
        );
        let workers = import_workers(selected.len());
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.outcome = None;
        self.importer.import_started = Some(Instant::now());
        self.importer.importing = Some(Progress {
            total: selected.len(),
            done: 0,
            slots: vec![None; workers],
        });
        self.importer.notice.clear();
        self.importer.cancel.store(false, Ordering::Relaxed);
        let cancel = self.importer.cancel.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let mut paths = Vec::new();
            let mut errors = Vec::new();
            let mut done = 0;
            let report = batch::run(
                selected.len(),
                workers,
                &cancel,
                |slot, index| {
                    let weapon = &selected[index];
                    let _ = sender.send(Event::Slot(slot, Some(weapon.name.clone())));
                    ctx.request_repaint();
                    let result = parhelion_import::cancellation::run(cancel.clone(), || {
                        let folder = service::model_directory(
                            &root.join("models"),
                            &weapon.name,
                            weapon.hash,
                        )?;
                        service::prepare_with_progress(
                            weapon,
                            &modern,
                            &native,
                            &donors,
                            &folder,
                            &mut |message| {
                                let _ = sender.send(Event::Progress(slot, message));
                                ctx.request_repaint();
                            },
                        )
                    });
                    let result = match result {
                        Ok(path) => Ok(Some(path)),
                        Err(error) if parhelion_import::cancellation::is_cancelled(&error) => {
                            Ok(None)
                        }
                        Err(error) => Err(format!("{error:#}")),
                    };
                    let _ = sender.send(Event::Slot(slot, None));
                    result
                },
                |index, result| {
                    let weapon = &selected[index];
                    let saved = result.and_then(|recipe| {
                        if cancel.load(Ordering::Relaxed) {
                            return Ok(None);
                        }
                        recipe
                            .map(|recipe| save_imported_recipe(&library, &existing, &recipe))
                            .transpose()
                    });
                    match saved {
                        Ok(Some(path)) => {
                            paths.push(path);
                        }
                        Ok(None) => {}
                        Err(error) => errors.push(format!("{}: {error}", weapon.name)),
                    }
                    done += 1;
                    let _ = sender.send(Event::Done(done));
                    ctx.request_repaint();
                },
            );
            debug_assert_eq!(done, report.completed);
            let _ = sender.send(Event::Imported {
                paths,
                errors,
                cancelled: report.cancelled,
            });
            ctx.request_repaint();
        });
    }
}

/// Conversions are package-read and shader-compile heavy; a few at once is the sweet spot.
fn import_workers(weapons: usize) -> usize {
    thread::available_parallelism()
        .map_or(1, |cores| (cores.get() / 4).clamp(1, 3))
        .min(weapons.max(1))
}

/// Merges a converted recipe into an existing library entry for the same item, keeping the
/// authored gameplay and narrative edits, or saves it as a new entry.
fn save_imported_recipe(
    library: &RecipeLibrary,
    existing: &[(PathBuf, u32)],
    recipe: &Path,
) -> Result<PathBuf, String> {
    let recipe = WeaponRecipe::load_json(recipe).map_err(|error| error.to_string())?;
    let identity = recipe.identity.item_hash.parse_u32().unwrap_or_default();
    for (path, _) in existing.iter().filter(|(_, hash)| *hash == identity) {
        let Ok(baseline) = WeaponRecipe::load_json(path) else {
            continue;
        };
        if baseline.identity.item_hash != recipe.identity.item_hash {
            continue;
        }
        if baseline.kind != recipe.kind {
            return Err("Reimport would change this recipe's item kind".into());
        }
        if !recipe.kind.is_weapon()
            && baseline.donor.item_hash != recipe.donor.item_hash
            && (baseline
                .overrides
                .socket_columns
                .iter()
                .any(Option::is_some)
                || !baseline.overrides.socket_plug_variants.is_empty())
        {
            return Err("Reimport selected a different native template. Save the prepared import as a separate recipe to preserve this recipe's edited sockets.".into());
        }
        let mut updated = baseline.clone();
        updated.overrides.imported_graph = recipe.overrides.imported_graph.clone();
        if updated.icon_donor.is_none() && updated.overrides.icon_edit.imported_image.is_none() {
            updated.overrides.icon_edit.imported_image =
                recipe.overrides.icon_edit.imported_image.clone();
        }
        if let (Some(next), Some(previous)) = (
            &mut updated.overrides.imported_graph,
            &baseline.overrides.imported_graph,
        ) {
            next.attachments = previous.attachments.clone();
        }
        if !recipe.kind.is_weapon() {
            updated.donor = recipe.donor.clone();
            updated.presentation_donor = None;
            if updated.overrides.rarity.is_none() {
                updated.overrides.rarity = recipe.overrides.rarity;
            }
            if updated.overrides.nameplate.is_none() {
                updated.overrides.nameplate = recipe.overrides.nameplate.clone();
            }
        } else {
            updated.presentation_donor = recipe
                .presentation_donor
                .clone()
                .or_else(|| Some(recipe.donor.clone()));
        }
        library.save_existing_if_unchanged(path, &baseline, &updated)?;
        return Ok(path.clone());
    }
    library.save_new(&recipe)
}
