use super::*;
use crate::app::style;
use crate::recipe_library::{self, Removed, SetAside, Status, Step};
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
        // A universal ornament is one plug offered in every armor piece of its class and slot, and
        // a weapon ornament one its stock weapon's shared ornament set offers.
        service::Family::Ornament | service::Family::WeaponOrnament => ItemKind::Mod,
    }
}
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

mod ability;
mod batch;
mod browser;
mod d1_roi;
mod filters;
mod icons;
mod melee;
mod models;
mod perks;
mod refresh;
mod sources;
mod stats;
#[cfg(test)]
mod tests;
mod tools;
use sources::{Origin, Shelf, Source};

/// A library recipe a reimport updates: the namespace that keeps its identity, and whether its
/// conversion brought ornaments, which the reimport brings again whatever the importer's choice.
#[derive(Clone)]
struct Saved {
    namespace: String,
    ornaments: bool,
}

pub(super) struct Importer {
    pub open: bool,
    pub enabled: bool,
    refresh: refresh::State,
    settings: Settings,
    /// The shown source. The others keep their listings in `shelves`.
    source: Source,
    shelves: BTreeMap<Source, Shelf>,
    /// Halo: Reach and Marathon sources of the shown rows, keyed like `weapons`.
    origins: BTreeMap<u32, Origin>,
    roi: Option<d1_roi::Inspected>,
    weapons: Vec<Weapon>,
    selected: BTreeSet<u32>,
    /// The library recipes a reimport updates, keyed by source item, for the next import.
    reimport: BTreeMap<u32, Saved>,
    browser: browser::Browser,
    view_path: Option<PathBuf>,
    icons: icons::Icons,
    read_requested: bool,
    catalog_target: PathBuf,
    models: models::Picker,
    perks: perks::Picker,
    abilities: ability::Picker,
    receiver: Option<Receiver<Event>>,
    scan_progress: Option<ScanProgress>,
    scan_started: Option<Instant>,
    scan_error: Option<String>,
    /// Rows the last read of the shown listing left out.
    scan_warning: Option<String>,
    import_started: Option<Instant>,
    importing: Option<Progress>,
    notice: String,
    outcome: Option<Outcome>,
    cancel: Arc<AtomicBool>,
    /// Set by an import that found the packages changed after the shown listing was read, so
    /// the listing is read again when the import finishes.
    relisted: Arc<AtomicBool>,
    /// (donor count, weapon count) the no-donor set was computed for.
    donor_stamp: (usize, usize),
    /// The shown source's outside tools. Cleared when a chosen path changes.
    tools: Option<tools::Checked>,
    /// Whether the Tools window is open.
    tools_open: bool,
    /// A running decompiler download or unpack.
    fetching: Option<Receiver<Result<(), String>>>,
    /// Whether this session's sweep of every item's earlier conversions has started.
    swept: bool,
    /// Whether the sweep is setting aside the conversions it removes.
    sweeping: bool,
    /// The sweep's removal of what it set aside, which runs on in the background.
    removal: Option<thread::JoinHandle<Removed>>,
    /// How far the sweep has got, through both.
    cleanup: Status,
}

/// A running import: one slot per worker thread.
struct Progress {
    total: usize,
    done: usize,
    /// Weapon name and current step for each busy worker.
    slots: Vec<Option<(String, String)>>,
    /// Whether every item is done and the batch is removing the conversions they replaced.
    finishing: bool,
}

/// Result of the last import run.
struct Outcome {
    added: usize,
    failures: Vec<String>,
    show_failures: bool,
    cancelled: bool,
}

/// A listing, for Halo: Reach and Marathon each row's source, and a warning about rows the read
/// left out.
type Listing = (Vec<Weapon>, BTreeMap<u32, Origin>, Option<String>);

enum Event {
    RoiInspected(Box<Result<d1_roi::Inspected, String>>),
    ScanProgress(ScanProgress),
    /// A listing, for Halo: Reach and Marathon each row's source, and a warning about rows the
    /// read left out.
    Scanned(Result<Listing, String>),
    /// A step message for a worker slot; slot 0 when no import is running.
    Progress(usize, String),
    /// A worker slot started (`Some`) or finished (`None`) a weapon.
    Slot(usize, Option<String>),
    Done(usize),
    /// Every item is done, and the batch removes the conversions they replaced.
    Finishing,
    ModelPrepared(Box<models::Prepared>),
    PerkPrepared(Box<perks::Prepared>),
    AbilitiesListed(Box<ability::Listed>),
    AbilityPrepared(Box<ability::Prepared>),
    Imported {
        paths: Vec<PathBuf>,
        errors: Vec<String>,
        cancelled: bool,
        /// The earlier conversions of the imported items that were removed, and the files the
        /// imported conversions share.
        removed: Option<String>,
    },
    /// The session's sweep set aside every item's unused conversions, for removal.
    ConversionsSetAside(Result<SetAside, String>),
}

/// The importer window's size when it first opens.
const DEFAULT_SIZE: [f32; 2] = [980.0, 860.0];

fn data_root() -> Result<PathBuf, String> {
    sundial::package_authoring::parhelion_data_directory()
        .ok_or_else(|| "Could not locate Sundial’s data folder".into())
}

/// The importer's settings, `importer/settings.json`. Earlier versions kept them at the data
/// folder's top as `d2-importer.json`, which moves there. Until a move succeeds the earlier file
/// stays in use, so a later save never writes defaults over the chosen folders.
fn settings_path(root: &Path) -> PathBuf {
    let path = root.join("importer").join("settings.json");
    let former = root.join("d2-importer.json");
    parhelion_import::cache::take_over(&former, &path);
    if former.is_file() && !path.exists() {
        former
    } else {
        path
    }
}

const MISSING_TOOLS: &str = "Missing tools.";
const CHECKING_TOOLS: &str = "Checking tools.";

fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A full-width bar in the warning tint, for what an importer window can't promise.
fn warning_bar(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.16))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .corner_radius(4)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add(egui::Label::new(text).wrap());
        });
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

impl Default for Importer {
    fn default() -> Self {
        let loaded = data_root().and_then(|root| match std::fs::read(settings_path(&root)) {
            Ok(bytes) => {
                serde_json::from_slice::<Settings>(&bytes).map_err(|error| error.to_string())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(error.to_string()),
        });
        let (settings, notice) = match loaded {
            Ok(settings) => (settings, String::new()),
            Err(error) => (Settings::default(), error),
        };
        // Map builds find vgmstream through the same chosen paths.
        parhelion_import::tools::configure(settings.tools(None));
        let mut browser = browser::Browser::default();
        let view_path = data_root()
            .ok()
            .map(|root| Source::D2Modern.view_path(&root));
        if let Some(path) = &view_path {
            browser.apply_view(browser::View::load(path));
        }
        Self {
            open: false,
            enabled: settings.enabled,
            refresh: refresh::State::default(),
            settings,
            source: Source::D2Modern,
            shelves: BTreeMap::new(),
            origins: BTreeMap::new(),
            roi: None,
            weapons: Vec::new(),
            selected: BTreeSet::new(),
            reimport: BTreeMap::new(),
            browser,
            view_path,
            icons: icons::Icons::default(),
            read_requested: false,
            catalog_target: PathBuf::new(),
            models: models::Picker::default(),
            perks: perks::Picker::default(),
            abilities: ability::Picker::default(),
            receiver: None,
            scan_progress: None,
            scan_started: None,
            scan_error: None,
            scan_warning: None,
            import_started: None,
            importing: None,
            notice,
            outcome: None,
            cancel: Arc::new(AtomicBool::new(false)),
            relisted: Arc::new(AtomicBool::new(false)),
            donor_stamp: (usize::MAX, usize::MAX),
            tools: None,
            tools_open: false,
            fetching: None,
            swept: false,
            sweeping: false,
            removal: None,
            cleanup: Status::default(),
        }
    }
}

impl Importer {
    pub fn busy(&self) -> bool {
        self.receiver.is_some()
    }

    /// Whether the importer is busy only setting aside the sweep's unused conversions.
    pub(super) fn setting_aside(&self) -> bool {
        self.sweeping
    }

    /// Whether the importer is busy only reading a source, which writes nothing outside the
    /// importer's own folder.
    pub(super) fn reading_source(&self) -> bool {
        self.scan_progress.is_some()
    }

    /// The sweep's label, with its step and how far it has got, and the lines its hover adds,
    /// while it runs.
    pub(super) fn cleanup_status(&self) -> Option<(String, Vec<String>)> {
        if !self.sweeping && self.removal.is_none() {
            return None;
        }
        let progress = self.cleanup.get();
        let mut details = Vec::new();
        if !progress.current.is_empty() {
            details.push(progress.current.clone());
        }
        let sizes: Vec<_> = [(progress.freed, "freed"), (progress.saved, "saved")]
            .into_iter()
            .filter(|(bytes, _)| *bytes > 0)
            .map(|(bytes, what)| format!("{} {what}", size(bytes)))
            .collect();
        if !sizes.is_empty() {
            details.push(sizes.join(" · "));
        }
        details.push(
            "Runs the first time Parhelion opens after Sundial starts. Stopping is safe, and the \
             next run skips assets already merged."
                .to_owned(),
        );
        Some((
            format!("Cleaning Up Imports · {}", cleanup_step(&progress)),
            details,
        ))
    }

    /// The importer's running background work, each with whether the importer window shows it
    /// and the lines its hover adds.
    pub(super) fn activities(&self) -> Vec<(String, bool, Vec<String>)> {
        let mut tasks = Vec::new();
        if let Some((label, details)) = self.cleanup_status() {
            tasks.push((label, false, details));
        }
        if let Some(progress) = self.scan_progress {
            let label = match progress {
                ScanProgress::ReadingItems {
                    completed, total, ..
                } => format!("Reading Source {completed} of {total}"),
                _ => "Reading Source…".to_owned(),
            };
            tasks.push((label, true, Vec::new()));
        }
        if let Some(progress) = &self.importing {
            let current = (progress.done + 1).min(progress.total);
            let label = if progress.finishing {
                "Cleaning Up Imports…".to_owned()
            } else {
                format!("Importing {current} of {}", progress.total)
            };
            tasks.push((label, true, Vec::new()));
        }
        tasks
    }

    fn save(&mut self) -> Result<(), String> {
        self.settings.enabled = self.enabled;
        let path = settings_path(&data_root()?);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(&self.settings).map_err(|error| error.to_string())?;
        sundial::package_authoring::replace_authoring_file(&path, &bytes)
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
            .on_hover_text(
                "Import items from Destiny 1, Destiny 2, Halo: Reach and Marathon as recipes.",
            )
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
        self.sweep_conversions(ctx);
        self.importer.refresh.observe(&self.recipe_entries);
        self.draw_import_refresh(ctx);
        if self.importer.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let closed = !self.importer.open || !self.importer.enabled;
        // A pending search is written when this window or Parhelion closes.
        if closed || ctx.input(|input| input.viewport().close_requested()) {
            self.importer.save_view();
        }
        if closed {
            if self.importer.abilities.close() {
                self.importer.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }
        self.check_importer_tools(ctx);
        self.importer.icons.poll(ctx);
        let stamp = (
            self.donor_summaries.len() + self.gear_donors.values().map(Vec::len).sum::<usize>(),
            self.importer.weapons.len(),
        );
        if self.importer.donor_stamp != stamp {
            self.importer.donor_stamp = stamp;
            self.importer.browser.no_donor = self.importer_no_donor();
            self.importer.browser.dirty = true;
        }
        if !self.importer.busy() && self.importer.catalog_target != self.packages {
            self.importer.read_requested = false;
            self.importer.weapons.clear();
            self.importer.scan_warning = None;
            self.importer.selected.clear();
            self.importer.browser.dirty = true;
            self.importer.browser.reset_scroll = true;
        }
        if !self.importer.read_requested
            && !self.importer.busy()
            && self.importer.folder().is_some()
        {
            self.scan_importer(ctx, false);
        }
        if !self.importer.busy() && self.importer.relisted.swap(false, Ordering::Relaxed) {
            // An import stored a listing of the changed packages. Show it beside the outcome.
            let outcome = self.importer.outcome.take();
            self.scan_importer(ctx, false);
            self.importer.outcome = outcome;
        }
        self.advance_import_refresh(ctx);
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("d2-importer"),
            egui::ViewportBuilder::default()
                .with_title("D2 Importer")
                .with_inner_size(DEFAULT_SIZE)
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
                        .default_size(DEFAULT_SIZE)
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

    /// The library a sweep reads, with the staging and package backup folders chosen in
    /// preferences, whose snapshots keep the conversions they name even outside the data folder.
    fn sweeping_library(&self) -> Option<RecipeLibrary> {
        let library = self.recipe_library.clone()?;
        Some(
            library.with_snapshot_roots(
                [self.staging.trim(), self.backup_root.trim()]
                    .into_iter()
                    .filter(|root| !root.is_empty())
                    .map(PathBuf::from),
            ),
        )
    }

    /// Once a session, every item's conversions that no saved recipe names go. Importer work
    /// waits while they are set aside, since a conversion in progress is not yet named, and their
    /// removal runs on in the background. Saved recipes can still be opened, since the sweep
    /// keeps whatever they name.
    fn sweep_conversions(&mut self, ctx: &egui::Context) {
        if let Some(removal) = self
            .importer
            .removal
            .take_if(|removal| removal.is_finished())
            && let Some(message) = removal.join().ok().as_ref().and_then(describe_removal)
        {
            self.log.push(LogEntry::info(message));
        }
        if self.importer.removal.is_some() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if self.importer.swept || !self.importer_idle() {
            return;
        }
        let Some(library) = self.sweeping_library() else {
            return;
        };
        self.importer.swept = true;
        self.importer.sweeping = true;
        self.importer.cleanup = Status::default();
        let status = self.importer.cleanup.clone();
        // Conversions reuse what the store kept from earlier imports, which earlier versions kept
        // in the `models` folder.
        if let Ok(root) = data_root() {
            let imports = root.join(service::IMPORTS);
            parhelion_import::store::relocate(&root.join("models"), &imports);
            parhelion_import::store::open(&imports);
        }
        let open: Vec<_> = serde_json::to_value(&self.recipe).into_iter().collect();
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let aside = library.set_aside_reporting(None, &open, status);
            let _ = sender.send(Event::ConversionsSetAside(aside));
            ctx.request_repaint();
        });
    }

    #[allow(clippy::cognitive_complexity)]
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
                    self.importer.abilities.worker_stopped();
                    self.importer.receiver = None;
                    self.importer.sweeping = false;
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
                Event::RoiInspected(result) => {
                    self.importer.receiver = None;
                    self.importer.scan_progress = None;
                    self.importer.scan_started = None;
                    self.importer.notice.clear();
                    match *result {
                        Ok(inspected) => {
                            self.importer.weapons = inspected.rows();
                            self.importer.browser.dirty = true;
                            self.importer.donor_stamp = (0, 0);
                            self.importer.roi = Some(inspected);
                        }
                        Err(error) => self.importer.scan_error = Some(error),
                    }
                }
                Event::AbilitiesListed(listed) => {
                    self.importer.receiver = None;
                    self.importer.import_started = None;
                    self.finish_roi_abilities(*listed);
                }
                Event::AbilityPrepared(prepared) => {
                    self.importer.receiver = None;
                    self.importer.import_started = None;
                    self.finish_roi_ability(*prepared);
                }
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
                Event::Finishing => {
                    if let Some(progress) = self.importer.importing.as_mut() {
                        progress.finishing = true;
                    }
                }
                Event::Scanned(result) => {
                    self.importer.receiver = None;
                    self.importer.scan_progress = None;
                    self.importer.scan_started = None;
                    match result {
                        Ok((mut weapons, origins, warning)) => {
                            let installed: BTreeSet<_> = self
                                .donor_summaries
                                .iter()
                                .chain(self.gear_donors.values().flatten())
                                .map(|donor| donor.hash)
                                .collect();
                            let modern = self.importer.source == Source::D2Modern;
                            for weapon in &mut weapons {
                                // Other sources key rows by their authored item already.
                                weapon.dummy |= modern
                                    && sundial::package_authoring::is_dummy_item(weapon.hash);
                                weapon.present_in_native |= installed.contains(&weapon.hash)
                                    || modern
                                        && service::destination_hash(weapon.hash, &weapon.name)
                                            .is_ok_and(|hash| installed.contains(&hash));
                            }
                            self.importer.origins = origins;
                            self.importer.browser.dirty = true;
                            self.importer.browser.reset_scroll = true;
                            self.importer.browser.anchor = None;
                            self.importer.scan_error = None;
                            self.importer.scan_warning = warning;
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
                    removed,
                } => {
                    if let Some(message) = removed {
                        self.log.push(LogEntry::info(message));
                    }
                    // The activity log keeps each failure after the outcome is dismissed.
                    if !errors.is_empty() {
                        self.log.push(LogEntry::info(format!(
                            "Import finished with {} added and {} failed.",
                            paths.len(),
                            errors.len()
                        )));
                    }
                    for error in &errors {
                        self.log.push(LogEntry::error(error.clone()));
                    }
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
                Event::ConversionsSetAside(result) => {
                    self.importer.receiver = None;
                    self.importer.sweeping = false;
                    self.importer.notice.clear();
                    match result {
                        Ok(aside) => {
                            self.importer.removal = Some(thread::spawn(move || aside.remove()));
                        }
                        Err(error) => eprintln!("Earlier conversions kept: {error}"),
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
        if !matches!(
            self.importer.source,
            Source::D2Modern | Source::D1RiseOfIron
        ) {
            let risky = self.risky_donors();
            return self
                .importer
                .weapons
                .iter()
                .filter(|weapon| {
                    self.importer
                        .origins
                        .get(&weapon.hash)
                        .is_none_or(|origin| self.foreign_donor(origin, &risky).is_none())
                })
                .map(|weapon| weapon.hash)
                .collect();
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
        let ornament_categories: BTreeSet<u32> = self
            .ornament_donors()
            .into_iter()
            .map(|(_, category)| category)
            .collect();
        let weapon_ornament_categories: BTreeSet<u32> = self
            .weapon_ornament_donors()
            .into_iter()
            .map(|ornament| ornament.plug_category)
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
                if weapon.family() == service::Family::Ornament {
                    return weapon
                        .plug_category
                        .is_none_or(|category| !ornament_categories.contains(&category));
                }
                if weapon.family() == service::Family::WeaponOrnament {
                    return weapon
                        .plug_category
                        .is_none_or(|category| !weapon_ornament_categories.contains(&category));
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
                if self.importer.source == Source::D1RiseOfIron {
                    return !self.donor_summaries.iter().any(|donor| {
                        donor.type_name == weapon.weapon_type
                            && self
                                .catalog
                                .as_ref()
                                .and_then(|catalog| catalog.item_definition_tag(donor.hash))
                                .is_some_and(crate::package_profile::is_stock_item_definition)
                    });
                }
                !types.contains(weapon.weapon_type.as_str())
                    && !service::type_base(&weapon.weapon_type)
                        .is_some_and(|hash| donors.contains(&hash))
            })
            .map(|weapon| weapon.hash)
            .collect()
    }

    /// The stock universal ornaments an imported one can be laid out on, with their plug
    /// categories. Plug sets offer them, and each category names a class and armor slot.
    fn ornament_donors(&self) -> Vec<(u32, u32)> {
        let Some(catalog) = self.catalog.as_ref() else {
            return Vec::new();
        };
        let mut donors = catalog
            .reusable_set_counts()
            .into_keys()
            .filter(|&hash| {
                catalog
                    .item_definition_tag(hash)
                    .is_none_or(crate::package_profile::is_stock_item_definition)
            })
            .filter_map(|hash| {
                let category = catalog.plug_category(hash)?;
                parhelion_import::tiger::universal_ornament::slot(category)?;
                Some((hash, category))
            })
            .collect::<Vec<_>>();
        donors.sort_unstable();
        donors
    }

    /// The stock weapon ornaments an imported one can be built on: ornaments a stock weapon's
    /// ornament socket offers from its shared plug set, with one art row each.
    fn weapon_ornament_donors(&self) -> Vec<sundial::investment::SharedWeaponOrnament> {
        let Some(catalog) = self.catalog.as_ref() else {
            return Vec::new();
        };
        let stock = |hash: u32| {
            catalog
                .item_definition_tag(hash)
                .is_none_or(crate::package_profile::is_stock_item_definition)
        };
        catalog
            .shared_weapon_ornaments()
            .into_iter()
            .filter(|ornament| {
                ornament.art_rows == 1 && stock(ornament.hash) && stock(ornament.weapon)
            })
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
                if self.importer.folder().is_none() {
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
        self.draw_roi_ability_window(ui.ctx());
        self.draw_importer_tools_window(ui.ctx());
    }

    fn draw_importer_header(&mut self, ui: &mut egui::Ui, idle: bool) {
        warning_bar(
            ui,
            "Imports are a work in progress. Some sources translate better than others, and some \
             items may not work as intended. Imports improve with each update.",
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            self.draw_source_tabs(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(idle, |ui| {
                    let has_source = self.importer.folder().is_some();
                    if self.importer.source == Source::D2Modern {
                        self.draw_modern_perk_button(ui, has_source);
                    }
                    if self.importer.source == Source::D1RiseOfIron {
                        self.draw_roi_ability_button(ui, has_source);
                    }
                    if ui
                        .add_enabled(has_source, egui::Button::new("Refresh"))
                        .clicked()
                    {
                        self.importer.tools = None;
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
                        self.choose_source_folder(ui.ctx());
                    }
                    self.draw_importer_tools_button(ui);
                    if self.importer.source == Source::D1RiseOfIron {
                        self.draw_roi_details(ui);
                    }
                });
            });
        });
        let summary = self.importer_catalog_summary();
        if let Some(path) = self.importer.folder() {
            let path = path.display().to_string();
            ui.horizontal(|ui| {
                if let Some((counts, kinds)) = summary {
                    let label = ui.label(egui::RichText::new(format!("{counts} ·")).weak());
                    if let Some(kinds) = kinds {
                        label.on_hover_text(kinds);
                    }
                }
                ui.add(egui::Label::new(egui::RichText::new(&path).weak()).truncate())
                    .on_hover_text(&path);
            });
        }
        self.draw_missing_tools(ui, idle);
        // Missing tools show above with their own lines, and a check in progress only in the footer.
        let blockers: Vec<_> = self
            .importer_blockers()
            .into_iter()
            .filter(|blocker| ![MISSING_TOOLS, CHECKING_TOOLS].contains(blocker))
            .collect();
        if !blockers.is_empty() && !self.importer.weapons.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("Blocked:")
                        .color(ui.visuals().warn_fg_color)
                        .strong(),
                );
                ui.label(blockers.join(" "));
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
        if let Some(warning) = &self.importer.scan_warning
            && !self.importer.weapons.is_empty()
        {
            style::warning(ui, warning);
        }
    }

    /// The catalog's item and installed counts, and with more than one kind, a line for each
    /// kind to show on hover.
    fn importer_catalog_summary(&self) -> Option<(String, Option<String>)> {
        let weapons = &self.importer.weapons;
        if weapons.is_empty() {
            return None;
        }
        let installed = weapons
            .iter()
            .filter(|weapon| weapon.present_in_native)
            .count();
        let kinds = ItemKind::ALL
            .into_iter()
            .filter_map(|kind| {
                let count = weapons
                    .iter()
                    .filter(|item| item_kind(item) == kind)
                    .count();
                (count > 0).then(|| {
                    // Reach vehicles import as Sparrows and ornaments as mods, but each lists as
                    // what it is.
                    let listed = match kind {
                        ItemKind::Sparrow if self.importer.source == Source::HaloReach => {
                            Some(("vehicle", "vehicles"))
                        }
                        ItemKind::Mod => Some(("ornament", "ornaments")),
                        _ => None,
                    };
                    format!(
                        "{count} {}",
                        match (count == 1, listed) {
                            (true, Some((one, _))) => one.to_owned(),
                            (false, Some((_, many))) => many.to_owned(),
                            (true, None) => kind.noun().to_owned(),
                            (false, None) => kind.plural().to_lowercase(),
                        }
                    )
                })
            })
            .collect::<Vec<_>>();
        let counts = match kinds.as_slice() {
            [kind] => format!("{kind} · {installed} installed"),
            _ => format!("{} items · {installed} installed", weapons.len()),
        };
        Some((counts, (kinds.len() > 1).then(|| kinds.join("\n"))))
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
        if self.importer.tools_checking() {
            blockers.push(CHECKING_TOOLS);
        } else if !self.importer.tools_ready() {
            blockers.push(MISSING_TOOLS);
        }
        blockers
    }

    fn draw_importer_welcome(&mut self, ui: &mut egui::Ui, idle: bool) {
        ui.add_space(ui.available_height() * 0.25);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("Choose a Source Folder")
                    .heading()
                    .weak(),
            );
            style::hint(ui, self.importer.source.welcome());
            style::hint(
                ui,
                if self.importer.source == Source::D1RiseOfIron {
                    "Only the Xbox One build is supported."
                } else {
                    "Imports become editable recipes in your library."
                },
            );
            ui.add_space(8.0);
            if ui
                .add_enabled(idle, style::primary(ui, "Choose Folder…"))
                .clicked()
            {
                self.choose_source_folder(ui.ctx());
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
                self.choose_source_folder(ui.ctx());
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
                    count if self.importer.source == Source::HaloReach => {
                        let vehicles = self
                            .importer
                            .selected_weapons()
                            .filter(|item| item_kind(item) == ItemKind::Sparrow)
                            .count();
                        format!(
                            "Import {count} {}",
                            match (vehicles, count == 1) {
                                (0, true) => "Weapon",
                                (0, false) => "Weapons",
                                (_, true) => "Vehicle",
                                (all, false) if all == count => "Vehicles",
                                _ => "Items",
                            }
                        )
                    }
                    count if self.importer.source == Source::Marathon => {
                        let weapons = self
                            .importer
                            .selected_weapons()
                            .filter(|item| filters::is_weapon(item))
                            .count();
                        format!(
                            "Import {count} {}",
                            match (weapons == count, count == 1) {
                                (true, true) => "Weapon",
                                (true, false) => "Weapons",
                                (false, true) => "Item",
                                (false, false) => "Items",
                            }
                        )
                    }
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
                            ui.label(egui::RichText::new(self.importer.reading_label()).strong());
                            if let Some(elapsed) = elapsed {
                                ui.label(egui::RichText::new(clock(elapsed)).weak());
                            }
                        });
                        // Other sources count their items only once the listing completes.
                        let text = if self.importer.source == Source::D2Modern {
                            format!("{completed} / {total} · {weapons} items")
                        } else {
                            format!("{completed} / {total}")
                        };
                        ui.add(
                            egui::ProgressBar::new(completed as f32 / total.max(1) as f32)
                                .text(text),
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
                let heading = if progress.finishing {
                    "Cleaning Up"
                } else {
                    "Importing"
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(heading).strong());
                    ui.label(
                        egui::RichText::new(format!("{} / {}", progress.done, progress.total))
                            .weak(),
                    );
                    if let Some(elapsed) = elapsed {
                        ui.label(egui::RichText::new(clock(elapsed)).weak());
                    }
                    // Every item is done once the batch cleans up, so there is nothing to cancel.
                    if !progress.finishing {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.importer.cancel.load(Ordering::Relaxed) {
                                ui.label(egui::RichText::new("Cancelling…").weak());
                            } else if ui.button("Cancel").clicked() {
                                self.importer.cancel.store(true, Ordering::Relaxed);
                            }
                        });
                    }
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
                if progress.finishing {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new("Removing replaced conversions…").weak());
                    });
                }
                return;
            }
            ui.horizontal(|ui| {
                ui.spinner();
                if let Some((label, details)) = self
                    .importer
                    .cleanup_status()
                    .filter(|_| self.importer.setting_aside())
                {
                    ui.label(label).on_hover_ui(|ui| {
                        for line in &details {
                            ui.weak(line);
                        }
                    });
                } else {
                    ui.label(if self.importer.notice.is_empty() {
                        "Working…"
                    } else {
                        &self.importer.notice
                    });
                }
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

    fn scan_importer(&mut self, ctx: &egui::Context, refresh: bool) {
        if self.importer.source == Source::D1RiseOfIron {
            self.scan_roi_source(ctx);
            return;
        }
        if self.importer.source != Source::D2Modern {
            self.scan_foreign(ctx, refresh);
            return;
        }
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
            .map(|weapons| (weapons, BTreeMap::new(), None))
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::Scanned(result));
            ctx.request_repaint();
        });
    }

    /// The namespace of the library recipe an earlier import of `source`'s item `hash` saved. An
    /// import of the item keeps it, as the review does, so it updates that recipe rather than
    /// adding a second item when the importer has named items another way since.
    fn saved_namespace(&self, source: Source, hash: u32) -> Option<String> {
        self.recipe_entries
            .iter()
            .find(|entry| {
                Source::of_namespace(&entry.namespace) == Some(source)
                    && Source::named_item(&entry.namespace) == Some(hash)
            })
            .map(|entry| entry.namespace.clone())
    }

    fn import_selected(&mut self, ctx: &egui::Context) {
        // Saved namespaces apply to this import only.
        let reimport = std::mem::take(&mut self.importer.reimport);
        if self.importer.source == Source::D1RiseOfIron {
            self.import_roi_selected(ctx, &reimport);
            return;
        }
        if self.importer.source != Source::D2Modern {
            self.import_foreign(ctx, &reimport);
            return;
        }
        let Some(modern) = self.importer.settings.modern_packages.clone() else {
            return;
        };
        let Ok(root) = data_root() else { return };
        // A reimported weapon brings ornaments as its recipe did, and any other as the importer's
        // Ornaments choice says.
        let ornaments = self.importer.settings.modern_ornaments;
        let selected: Arc<Vec<Weapon>> = Arc::new(
            self.importer
                .selected_weapons()
                .map(|weapon| {
                    let saved = reimport.get(&weapon.hash);
                    Weapon {
                        namespace: saved
                            .map(|saved| saved.namespace.clone())
                            .or_else(|| self.saved_namespace(Source::D2Modern, weapon.hash)),
                        ornaments: saved.map_or(ornaments, |saved| saved.ornaments),
                        ..weapon.clone()
                    }
                })
                .collect(),
        );
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
        let ornaments = self
            .ornament_donors()
            .into_iter()
            .map(|(hash, category)| {
                serde_json::json!({"hash":hash,"plug_category":category,
                "name":self.catalog.as_ref().map(|catalog| catalog.plug_label(hash, false))})
            })
            .collect::<Vec<_>>();
        let donors = Arc::new(
            serde_json::json!({"weapons":self.donor_summaries.iter().filter(|donor| stock(donor.hash)).map(|donor| serde_json::json!({"hash":donor.hash,"name":donor.name,"weapon_type":donor.type_name,"present_in_native":true,"bucket_hash":donor.bucket_hash,"collection_backed":donor.collection_backed,"unrelated_collection_condition":risky_donors.contains(&donor.hash)})).collect::<Vec<_>>(),
                "shaders":self.gear_donors_for(ItemKind::Shader).iter().filter(|donor| stock(donor.hash)).map(|donor| serde_json::json!({"hash":donor.hash,"name":donor.name})).collect::<Vec<_>>(),
                "emblems":self.gear_donors_for(ItemKind::Emblem).iter().filter(|donor| stock(donor.hash)).map(|donor| serde_json::json!({"hash":donor.hash,"name":donor.name})).collect::<Vec<_>>(),
                "gear":gear,"ornaments":ornaments,"weapon_ornaments":self.weapon_ornament_donors()}),
        );
        let names = selected.iter().map(|weapon| weapon.name.clone()).collect();
        self.run_import(ctx, names, move |index, native, progress| {
            let weapon = &selected[index];
            let folder = service::item_directory(
                &root.join(service::IMPORTS),
                Source::D2Modern.folder(),
                weapon.hash,
            )?;
            service::prepare_with_progress(weapon, &modern, native, &donors, &folder, progress)
        });
    }

    /// Converts `names.len()` items on bounded workers and saves each prepared recipe into the
    /// library, merging with an earlier import of the same item.
    fn run_import(
        &mut self,
        ctx: &egui::Context,
        names: Vec<String>,
        prepare: impl Fn(usize, &Path, &mut dyn FnMut(String)) -> anyhow::Result<PathBuf>
        + Send
        + Sync
        + 'static,
    ) {
        let existing: Vec<_> = self
            .recipe_entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.identity_hash))
            .collect();
        let Some(library) = self.sweeping_library() else {
            return;
        };
        let workers = import_workers(names.len());
        let imports = data_root().ok().map(|root| root.join(service::IMPORTS));
        if let Some(imports) = &imports {
            parhelion_import::store::open(imports);
        }
        // An open recipe keeps the conversion it names until it is saved or closed.
        let open: Vec<_> = serde_json::to_value(&self.recipe).into_iter().collect();
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.outcome = None;
        self.importer.import_started = Some(Instant::now());
        self.importer.importing = Some(Progress {
            total: names.len(),
            done: 0,
            slots: vec![None; workers],
            finishing: false,
        });
        self.importer.notice.clear();
        self.importer.cancel.store(false, Ordering::Relaxed);
        let cancel = self.importer.cancel.clone();
        let native = PathBuf::from(&self.packages);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let source = match crate::workflow::PackageSource::stock(&native) {
                Ok(source) => source,
                Err(error) => {
                    let _ = sender.send(Event::Imported {
                        paths: Vec::new(),
                        errors: vec![format!("Prepare native import source: {error}")],
                        cancelled: cancel.load(Ordering::Relaxed),
                        removed: None,
                    });
                    ctx.request_repaint();
                    return;
                }
            };
            let mut paths = Vec::new();
            let mut errors = Vec::new();
            let mut items = BTreeSet::new();
            let mut shared = parhelion_import::store::Shared::default();
            let mut done = 0;
            let report = batch::run(
                names.len(),
                workers,
                &cancel,
                |slot, index| {
                    let _ = sender.send(Event::Slot(slot, Some(names[index].clone())));
                    ctx.request_repaint();
                    let started = std::time::SystemTime::now();
                    let result = parhelion_import::cancellation::run(cancel.clone(), || {
                        prepare(index, source.path(), &mut |message| {
                            let _ = sender.send(Event::Progress(slot, message));
                            ctx.request_repaint();
                        })
                    });
                    let result = match result {
                        Ok(path) => {
                            // A conversion shares its payloads as soon as it is made, on its own
                            // worker, so the batch never ends on one long pass over every item.
                            let _ = sender
                                .send(Event::Progress(slot, "Sharing identical files…".into()));
                            let shared = imports
                                .as_deref()
                                .map(|imports| share_conversions(imports, &path, started))
                                .unwrap_or_default();
                            Ok(Some((path, shared)))
                        }
                        Err(error) if parhelion_import::cancellation::is_cancelled(&error) => {
                            Ok(None)
                        }
                        Err(error) => Err(format!("{error:#}")),
                    };
                    let _ = sender.send(Event::Slot(slot, None));
                    result
                },
                |index, result| {
                    let saved = result.and_then(|converted| {
                        let Some((recipe, conversion)) = converted else {
                            return Ok(None);
                        };
                        shared.add(conversion);
                        if cancel.load(Ordering::Relaxed) {
                            return Ok(None);
                        }
                        let saved = save_imported_recipe(&library, &existing, &recipe)?;
                        items.extend(
                            imports
                                .as_deref()
                                .and_then(|imports| item_folder(imports, &recipe)),
                        );
                        Ok(Some(saved))
                    });
                    match saved {
                        Ok(Some(path)) => {
                            paths.push(path);
                        }
                        Ok(None) => {}
                        Err(error) => errors.push(format!("{}: {error}", names[index])),
                    }
                    done += 1;
                    let _ = sender.send(Event::Done(done));
                    ctx.request_repaint();
                },
            );
            debug_assert_eq!(done, report.completed);
            if let Err(error) = source.finish(Ok(()), |warning| errors.push(warning)) {
                errors.push(error);
            }
            let _ = sender.send(Event::Finishing);
            ctx.request_repaint();
            let removed = remove_earlier_conversions(&library, &items, &open, shared);
            let _ = sender.send(Event::Imported {
                paths,
                errors,
                cancelled: report.cancelled,
                removed,
            });
            ctx.request_repaint();
        });
    }
}

/// The item folder, `imports/<importer>/<HASH>`, that holds a converted recipe.
fn item_folder(imports: &Path, recipe: &Path) -> Option<PathBuf> {
    conversion_folder(imports, recipe)?
        .parent()
        .map(Path::to_path_buf)
}

/// The conversion folder, `imports/<importer>/<HASH>/<kind>-<n>`, that holds a converted recipe.
fn conversion_folder(imports: &Path, recipe: &Path) -> Option<PathBuf> {
    let imports = imports.canonicalize().ok()?;
    let recipe = recipe.canonicalize().ok()?;
    let mut parts = recipe.strip_prefix(&imports).ok()?.components();
    let (importer, hash, conversion) = (parts.next()?, parts.next()?, parts.next()?);
    // The recipe lies inside the conversion folder.
    parts.next()?;
    Some(imports.join(importer).join(hash).join(conversion))
}

/// Shares the payloads of the conversions one item's import made since `started`: the conversion
/// that holds `recipe`, and a finished source export it made to start from. An export is
/// finished once its `export.json` is written.
fn share_conversions(
    imports: &Path,
    recipe: &Path,
    started: std::time::SystemTime,
) -> parhelion_import::store::Shared {
    let mut shared = parhelion_import::store::Shared::default();
    let Some(conversion) = conversion_folder(imports, recipe) else {
        return shared;
    };
    let mut folders = vec![conversion.clone()];
    if let Some(item) = conversion.parent()
        && let Ok(entries) = std::fs::read_dir(item)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            let export = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("source-"))
                .is_some_and(|index| {
                    !index.is_empty() && index.bytes().all(|c| c.is_ascii_digit())
                })
                && path.join("export.json").is_file();
            let made = entry
                .metadata()
                .and_then(|metadata| metadata.created())
                .is_ok_and(|made| made >= started);
            if export && made && !parhelion_import::store::is_shared(&path) {
                folders.push(path);
            }
        }
    }
    for folder in folders {
        shared.add(parhelion_import::store::share(imports, &folder, |_| true));
    }
    shared
}

/// Removes what an import leaves behind in the imported items' folders and describes it for the
/// log, with what the import's own conversions `shared`.
fn remove_earlier_conversions(
    library: &RecipeLibrary,
    items: &BTreeSet<PathBuf>,
    open: &[serde_json::Value],
    shared: parhelion_import::store::Shared,
) -> Option<String> {
    let mut removed = if items.is_empty() {
        Removed::default()
    } else {
        library
            .set_aside_unused_conversions(Some(items), open)
            .map(SetAside::remove)
            .unwrap_or_else(|error| {
                eprintln!("Earlier conversions kept: {error}");
                Removed::default()
            })
    };
    removed.shared += shared.files;
    removed.shared_bytes += shared.bytes;
    removed.errors.extend(shared.errors);
    describe_removal(&removed)
}

/// The log line for a removal of earlier conversions and the sharing of what stays. Both only
/// save space, so their failures are not import errors.
fn describe_removal(removed: &Removed) -> Option<String> {
    for error in &removed.errors {
        eprintln!("{error}");
    }
    let mut lines = Vec::new();
    match removed.folders {
        0 if removed.bytes > 0 => lines.push(format!(
            "Removed unused import assets, {}.",
            size(removed.bytes)
        )),
        0 => {}
        1 => lines.push(format!(
            "Removed 1 earlier conversion, {}.",
            size(removed.bytes)
        )),
        count => lines.push(format!(
            "Removed {count} earlier conversions, {}.",
            size(removed.bytes)
        )),
    }
    if removed.shared > 0 {
        lines.push(format!(
            "Merged {} duplicate import assets, {} saved.",
            removed.shared,
            size(removed.shared_bytes)
        ));
    }
    match removed.linked {
        0 => {}
        1 => lines.push("Moved the embedded model out of 1 recipe.".to_owned()),
        count => lines.push(format!("Moved the embedded models out of {count} recipes.")),
    }
    match removed.arranged {
        0 => {}
        1 => lines.push("Arranged the conversion folders of 1 recipe.".to_owned()),
        count => lines.push(format!(
            "Arranged the conversion folders of {count} recipes."
        )),
    }
    match removed.backups {
        0 => {}
        1 => lines.push("Removed 1 old recipe backup.".to_owned()),
        count => lines.push(format!("Removed {count} old recipe backups.")),
    }
    (!lines.is_empty()).then(|| lines.join(" "))
}

/// `bytes` in gigabytes, or megabytes below one.
fn size(bytes: u64) -> String {
    let gigabytes = bytes as f64 / f64::from(1 << 30);
    if gigabytes >= 1.0 {
        format!("{gigabytes:.1} GB")
    } else {
        format!("{:.0} MB", gigabytes * 1024.0)
    }
}

/// What the sweep is doing and how far it has got, counting the item at hand as Importing does.
fn cleanup_step(progress: &recipe_library::Progress) -> String {
    let step = match progress.step {
        Step::Linking => "Moving Embedded Models",
        Step::Reading => "Reading Recipes",
        Step::Arranging => "Arranging Conversions",
        Step::Checking => "Checking Imports",
        Step::Removing => "Removing Conversions",
        Step::Sharing => "Merging Duplicate Assets",
        Step::Collecting => "Checking for Unused Assets",
    };
    if progress.total == 0 {
        return step.to_owned();
    }
    let current = (progress.done + 1).min(progress.total);
    format!("{step} {current} of {}", progress.total)
}

/// Conversions are package-read and shader-compile heavy; a few at once is the sweet spot.
fn import_workers(weapons: usize) -> usize {
    thread::available_parallelism()
        .map_or(1, |cores| (cores.get() / 4).clamp(1, 3))
        .min(weapons.max(1))
}

/// Merges a converted recipe into an existing library entry for the same item, keeping the
/// authored gameplay and narrative edits, or saves it as a new entry. The perk sockets always
/// come from the conversion, with the base they were built on, so the entry they replace is
/// first kept in the library's backups whenever the merge changes them.
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
        let mut updated = baseline.clone();
        // The conversion's socket layout replaces the saved one, so a converter fix reaches
        // every reimported item. It names the sockets and plugs of the base it was built on,
        // so a saved recipe on another base takes that base with it.
        updated.overrides.socket_columns = recipe.overrides.socket_columns.clone();
        updated.overrides.socket_plug_variants = recipe.overrides.socket_plug_variants.clone();
        updated.overrides.base_sandbox_perks = recipe.overrides.base_sandbox_perks.clone();
        if updated.donor.item_hash != recipe.donor.item_hash {
            updated.donor = recipe.donor.clone();
        }
        // Only replace the exact generated source prefix. Other names are authored text.
        if recipe.namespace.starts_with("parhelion.reach.")
            && baseline.namespace == recipe.namespace
            && baseline.name == format!("Reach {}", recipe.name)
        {
            updated.name = recipe.name.clone();
        }
        stats::refresh(&baseline, &recipe, &mut updated);
        updated.overrides.imported_graph = recipe.overrides.imported_graph.clone();
        let generated_icon = match (
            &baseline.overrides.imported_graph,
            &baseline.overrides.icon_edit.imported_image,
        ) {
            (Some(graph), Some(image)) => parhelion_import::artwork::generated_image(
                graph,
                &serde_json::to_value(image).map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("Read generated inventory artwork: {error:#}"))?,
            _ => false,
        };
        if updated.icon_donor.is_none()
            && (updated.overrides.icon_edit.imported_image.is_none() || generated_icon)
        {
            updated.overrides.icon_edit.imported_image =
                recipe.overrides.icon_edit.imported_image.clone();
        }
        // The graph above came with the conversion's ornaments, or none when they were not
        // imported, bound to the socket layout the conversion brings. Saved ornaments would have
        // no socket left to offer them.
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
            if updated.overrides.fired_graph.is_none() && updated.overrides.projectile.is_none() {
                updated.overrides.fired_graph = recipe.overrides.fired_graph.clone();
            }
            // An exotic base lends its runtime but not its behavior or tier. The ordinary weapon
            // whose behavior it takes belongs to the base the conversion chose, as its sockets do.
            updated.overrides.behavior_donor = recipe.overrides.behavior_donor.clone();
            // A rarity the saved entry leaves to its base follows the conversion, which carries
            // the source tier, or an ordinary tier for an exotic base lent without its behavior.
            if updated.overrides.rarity.is_none() {
                updated.overrides.rarity = recipe.overrides.rarity;
            }
            melee::merge(&recipe, &mut updated)
                .map_err(|error| format!("Import melee activation: {error:#}"))?;
        }
        keep_bundled_selection(&baseline, &mut updated);
        validate_projectile_selection(&updated)?;
        // Custom perks and socket edits live in the same fields as the generated layout, so
        // taking the conversion's replaces them. The earlier entry stays recoverable.
        let replaced = baseline.overrides.socket_columns != updated.overrides.socket_columns
            || baseline.overrides.socket_plug_variants != updated.overrides.socket_plug_variants
            || baseline.overrides.base_sandbox_perks != updated.overrides.base_sandbox_perks
            || baseline.overrides.behavior_donor != updated.overrides.behavior_donor
            || baseline.donor != updated.donor;
        if replaced {
            library.save_existing_with_backup(path, &baseline, &updated)?;
        } else {
            library.save_existing_if_unchanged(path, &baseline, &updated)?;
        }
        return Ok(path.clone());
    }
    validate_projectile_selection(&recipe)?;
    library.save_new(&recipe)
}

/// A saved selection of a projectile that the previous model carried as a byte for byte copy of a
/// bundled preset fires the preset once the replacement model doesn't carry it. Projectile value
/// edits and added behaviors keep the imported selection, since a preset takes neither.
fn keep_bundled_selection(baseline: &WeaponRecipe, updated: &mut WeaponRecipe) {
    let Some(root) = updated
        .overrides
        .fired_graph
        .as_ref()
        .and_then(|fired| fired.imported.clone())
    else {
        return;
    };
    let Some(previous) = &baseline.overrides.imported_graph else {
        return;
    };
    if validate_projectile_selection(updated).is_ok()
        || updated
            .overrides
            .projectile
            .as_ref()
            .is_some_and(|edits| !edits.values.is_empty())
        || !updated.overrides.additional_behaviors.is_empty()
    {
        return;
    }
    if let Some(preset) = crate::weapon::projectile::bundled_copy(&previous.directory, &root) {
        updated.overrides.fired_graph = Some(crate::weapon::behavior::FiredGraph {
            bundled: Some(preset),
            source_graph: 0,
            imported: None,
            patches: Vec::new(),
            appends: Vec::new(),
            particles: Vec::new(),
        });
    }
}

/// A model replacement cannot leave the saved firing selection without its private assets.
fn validate_projectile_selection(recipe: &WeaponRecipe) -> Result<(), String> {
    let Some(symbol) = recipe
        .overrides
        .fired_graph
        .as_ref()
        .and_then(|fired| fired.imported.as_deref())
    else {
        return Ok(());
    };
    let graph = recipe
        .overrides
        .imported_graph
        .as_ref()
        .ok_or("The selected imported projectile requires a prepared weapon graph")?;
    graph
        .validate(
            recipe
                .identity
                .item_hash
                .parse_u32()
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("Prepared projectile assets changed: {error:#}"))?;
    crate::item::validate_imported_projectile(graph, symbol)
        .map_err(|error| format!("This import cannot supply the selected projectile {symbol}: {error}. The recipe was not saved."))
}
