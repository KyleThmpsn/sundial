//! Import sources. Each keeps its own folder, listing, selection and view. Halo: Reach and
//! Marathon rows reuse the modern row, keyed by the authored item they import into.
use super::*;
use parhelion_import::{halo_reach::browse as reach, marathon::browse as marathon};

const SPARROW_BUCKET: u32 = 2025709351;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::app) enum Source {
    #[default]
    Modern,
    Reach,
    Marathon,
    Roi,
}

impl Source {
    pub const ALL: [Source; 4] = [Source::Modern, Source::Roi, Source::Reach, Source::Marathon];

    pub fn label(self) -> &'static str {
        match self {
            Source::Modern => "D2 Modern",
            Source::Reach => "Halo: Reach",
            Source::Marathon => "Marathon",
            Source::Roi => "D1 Rise of Iron",
        }
    }

    pub fn view_file(self) -> &'static str {
        match self {
            Source::Modern => "importer-view.json",
            Source::Reach => "importer-view-reach.json",
            Source::Marathon => "importer-view-marathon.json",
            Source::Roi => "importer-view-d1-roi.json",
        }
    }

    /// Only modern items carry inventory artwork to list.
    pub fn icons(self) -> bool {
        self == Source::Modern
    }

    pub fn welcome(self) -> &'static str {
        match self {
            Source::Modern => "Choose your modern Destiny 2 installation or its packages folder.",
            Source::Reach => "Choose your MCC installation or its Halo: Reach maps folder.",
            Source::Marathon => "Choose your Marathon installation or its packages folder.",
            Source::Roi => "Choose your Rise of Iron installation or its packages folder.",
        }
    }

    fn dialog_title(self) -> &'static str {
        match self {
            Source::Modern => "Modern Destiny 2 Build",
            Source::Reach => "Halo: Reach Maps",
            Source::Marathon => "Marathon Packages",
            Source::Roi => "Rise of Iron Packages",
        }
    }

    /// Asset folder kind under an item's model folder.
    fn assets(self) -> &'static str {
        match self {
            Source::Modern => "modern",
            Source::Reach => "reach",
            Source::Marathon => "marathon",
            Source::Roi => "d1-roi",
        }
    }

    fn reading(self) -> &'static str {
        match self {
            Source::Reach => "Reading Maps",
            Source::Roi => "Reading Packages",
            _ => "Reading Items",
        }
    }
}

#[derive(Clone)]
pub(super) enum Origin {
    Reach(reach::Entry),
    Marathon(marathon::Entry),
}

/// The native base an origin builds on.
pub(super) enum Base {
    Weapon(&'static str),
    Vehicle,
}

impl Origin {
    pub fn base(&self) -> Option<Base> {
        match self {
            Origin::Reach(entry) if entry.vehicle() => Some(Base::Vehicle),
            Origin::Reach(entry) => Some(Base::Weapon(entry.weapon_type())),
            Origin::Marathon(entry) => entry.weapon_type().map(Base::Weapon),
        }
    }

    /// The row's type: the native weapon type, Vehicle, or an unsupported item's own type.
    fn kind(&self) -> String {
        match self {
            Origin::Reach(entry) if entry.vehicle() => "Vehicle".to_owned(),
            Origin::Reach(entry) => entry.weapon_type().to_owned(),
            Origin::Marathon(entry) => entry
                .weapon_type()
                .map_or_else(|| entry.kind.clone(), str::to_owned),
        }
    }

    pub fn hover(&self) -> String {
        match self {
            Origin::Reach(entry) => format!("{}\nMaps: {}", entry.path, entry.caches.join(", ")),
            Origin::Marathon(entry) => format!("Source Item: {:08X}", entry.item),
        }
    }
}

/// Rows for a listing, keyed by the authored item each imports into.
fn rows(origins: impl Iterator<Item = Origin>) -> (Vec<Weapon>, BTreeMap<u32, Origin>) {
    let mut weapons = Vec::new();
    let mut keyed = BTreeMap::new();
    for origin in origins {
        let (hash, name, vehicle) = match &origin {
            Origin::Reach(entry) => (entry.destination(), entry.name(), entry.vehicle()),
            Origin::Marathon(entry) => (entry.destination(), entry.name.clone(), false),
        };
        let Ok(hash) = hash else { continue };
        if keyed.contains_key(&hash) {
            continue;
        }
        weapons.push(Weapon {
            hash,
            name,
            weapon_type: origin.kind(),
            bucket_hash: vehicle.then_some(SPARROW_BUCKET),
            ..Weapon::default()
        });
        keyed.insert(hash, origin);
    }
    (weapons, keyed)
}

/// One source's listing state while another source is shown.
pub(super) struct Shelf {
    weapons: Vec<Weapon>,
    selected: BTreeSet<u32>,
    browser: browser::Browser,
    view_path: Option<PathBuf>,
    read_requested: bool,
    catalog_target: PathBuf,
    scan_error: Option<String>,
    donor_stamp: (usize, usize),
    origins: BTreeMap<u32, Origin>,
}

impl Shelf {
    fn new(source: Source) -> Self {
        let mut browser = browser::Browser::default();
        let view_path = data_root().ok().map(|root| root.join(source.view_file()));
        if let Some(path) = &view_path {
            browser.apply_view(browser::View::load(path));
        }
        Self {
            weapons: Vec::new(),
            selected: BTreeSet::new(),
            browser,
            view_path,
            read_requested: false,
            catalog_target: PathBuf::new(),
            scan_error: None,
            donor_stamp: (usize::MAX, usize::MAX),
            origins: BTreeMap::new(),
        }
    }
}

impl Importer {
    pub fn folder(&self) -> Option<&PathBuf> {
        match self.source {
            Source::Modern => self.settings.modern_packages.as_ref(),
            Source::Reach => self.settings.reach_maps.as_ref(),
            Source::Marathon => self.settings.marathon_packages.as_ref(),
            Source::Roi => self.settings.d1_roi.as_ref(),
        }
    }

    fn folder_mut(&mut self) -> &mut Option<PathBuf> {
        match self.source {
            Source::Modern => &mut self.settings.modern_packages,
            Source::Reach => &mut self.settings.reach_maps,
            Source::Marathon => &mut self.settings.marathon_packages,
            Source::Roi => &mut self.settings.d1_roi,
        }
    }

    /// Shelves the shown listing and shows `source`'s. A running scan or import keeps its tab.
    pub fn select_source(&mut self, source: Source) {
        if source == self.source || self.busy() {
            return;
        }
        self.save_view();
        let shown = Shelf {
            weapons: std::mem::take(&mut self.weapons),
            selected: std::mem::take(&mut self.selected),
            browser: std::mem::take(&mut self.browser),
            view_path: self.view_path.take(),
            read_requested: self.read_requested,
            catalog_target: std::mem::take(&mut self.catalog_target),
            scan_error: self.scan_error.take(),
            donor_stamp: self.donor_stamp,
            origins: std::mem::take(&mut self.origins),
        };
        self.shelves.insert(self.source, shown);
        let next = self
            .shelves
            .remove(&source)
            .unwrap_or_else(|| Shelf::new(source));
        self.weapons = next.weapons;
        self.selected = next.selected;
        self.browser = next.browser;
        self.browser.dirty = true;
        self.view_path = next.view_path;
        self.read_requested = next.read_requested;
        self.catalog_target = next.catalog_target;
        self.scan_error = next.scan_error;
        self.donor_stamp = next.donor_stamp;
        self.origins = next.origins;
        self.source = source;
        self.outcome = None;
        self.notice.clear();
    }

    pub fn reading_label(&self) -> &'static str {
        self.source.reading()
    }
}

impl PackageAuthoringApp {
    pub(super) fn draw_source_tabs(&mut self, ui: &mut egui::Ui) {
        let busy = self.importer.busy();
        for source in Source::ALL {
            let shown = self.importer.source == source;
            let response = ui
                .add_enabled(
                    shown || !busy,
                    egui::Button::new(source.label()).selected(shown),
                )
                .on_disabled_hover_text("Busy.");
            if response.clicked() {
                self.importer.select_source(source);
            }
        }
    }

    pub(super) fn choose_source_folder(&mut self, ctx: &egui::Context) {
        let source = self.importer.source;
        let Some(path) = rfd::FileDialog::new()
            .set_title(source.dialog_title())
            .pick_folder()
        else {
            return;
        };
        let resolved = match source {
            Source::Modern => service::packages_path(&path).map_err(|error| error.to_string()),
            Source::Reach => reach::maps_path(&path).map_err(|error| format!("{error:#}")),
            Source::Marathon => {
                marathon::packages_path(&path).map_err(|error| format!("{error:#}"))
            }
            Source::Roi => {
                parhelion_import::d1_roi::packages_path(&path).map_err(|error| format!("{error:#}"))
            }
        };
        match resolved {
            Ok(path) => {
                *self.importer.folder_mut() = Some(path);
                if source == Source::Roi {
                    self.importer.roi = None;
                }
                self.importer.weapons.clear();
                self.importer.origins.clear();
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
            Err(error) => self.importer.notice = error,
        }
    }

    /// Lists Halo: Reach or Marathon items on a worker, through the stored listing.
    pub(super) fn scan_foreign(&mut self, ctx: &egui::Context, refresh: bool) {
        let source = self.importer.source;
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Ok(root) = data_root() else { return };
        let (sender, receiver) = mpsc::channel();
        self.importer.receiver = Some(receiver);
        self.importer.read_requested = true;
        self.importer.catalog_target = self.packages.clone();
        self.importer.scan_progress = Some(ScanProgress::CheckingCache);
        self.importer.scan_started = Some(Instant::now());
        self.importer.scan_error = None;
        self.importer.outcome = None;
        self.importer.notice.clear();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let cache = root.join("importer/catalog");
            let progress = |completed, total| {
                let _ = sender.send(Event::ScanProgress(ScanProgress::ReadingItems {
                    completed,
                    total,
                    weapons: 0,
                }));
                ctx.request_repaint();
            };
            let result = match source {
                Source::Reach => reach::browse_cached(&folder, &cache, refresh, progress)
                    .map(|entries| rows(entries.into_iter().map(Origin::Reach))),
                Source::Marathon => marathon::browse_cached(&folder, &cache, refresh, progress)
                    .map(|entries| rows(entries.into_iter().map(Origin::Marathon))),
                Source::Modern | Source::Roi => {
                    Err(anyhow::anyhow!("This source uses a separate reader"))
                }
            }
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::Scanned(result));
            ctx.request_repaint();
        });
    }

    /// Native weapons whose collectible depends on an unrelated condition, which bases skip.
    pub(super) fn risky_donors(&self) -> BTreeSet<u32> {
        self.catalog
            .as_ref()
            .map(|catalog| catalog.items_with_unrelated_collection_conditions())
            .unwrap_or_default()
    }

    /// The installed native item an origin builds on: a weapon of its type, or a Sparrow for a
    /// vehicle. Stock items that unlock through their own collectible come first.
    pub(super) fn foreign_donor(
        &self,
        origin: &Origin,
        risky: &BTreeSet<u32>,
    ) -> Option<(u32, String)> {
        let stock = |hash: u32| {
            self.catalog
                .as_ref()
                .and_then(|catalog| catalog.item_definition_tag(hash))
                .is_none_or(crate::package_profile::is_stock_item_definition)
        };
        let pick = |donors: &mut dyn Iterator<Item = &WeaponDonorSummary>| {
            donors
                .filter(|donor| stock(donor.hash))
                .min_by_key(|donor| {
                    (
                        !donor.collection_backed,
                        donor.name.to_lowercase(),
                        donor.hash,
                    )
                })
                .map(|donor| (donor.hash, donor.name.clone()))
        };
        match origin.base()? {
            Base::Vehicle => pick(&mut self.gear_donors_for(ItemKind::Sparrow).iter()),
            Base::Weapon(kind) => pick(
                &mut self
                    .donor_summaries
                    .iter()
                    .filter(|donor| donor.type_name == kind && !risky.contains(&donor.hash)),
            ),
        }
    }

    /// Converts the selected Halo: Reach or Marathon items, each onto its chosen native base.
    pub(super) fn import_foreign(&mut self, ctx: &egui::Context) {
        let source = self.importer.source;
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Ok(root) = data_root() else { return };
        let native = PathBuf::from(&self.packages);
        let risky = self.risky_donors();
        let jobs: Vec<(Weapon, Origin, (u32, String))> = self
            .importer
            .selected_weapons()
            .filter_map(|weapon| {
                let origin = self.importer.origins.get(&weapon.hash)?;
                let donor = self.foreign_donor(origin, &risky)?;
                Some((weapon.clone(), origin.clone(), donor))
            })
            .collect();
        let names = jobs
            .iter()
            .map(|(weapon, _, _)| weapon.name.clone())
            .collect();
        self.run_import(ctx, names, move |index, progress| {
            let (weapon, origin, (donor, donor_name)) = &jobs[index];
            let folder_name = match origin {
                Origin::Reach(_) => format!("Reach {}", weapon.name),
                Origin::Marathon(_) => weapon.name.clone(),
            };
            let item = service::model_directory(&root.join("models"), &folder_name, weapon.hash)?;
            let output = service::reserve_assets(&item, source.assets())?.join("import");
            match origin {
                Origin::Reach(entry) => {
                    progress("Translating source model…".into());
                    let request = entry.request(*donor, donor_name)?;
                    reach::import(&request, &folder, &native, &output)
                }
                Origin::Marathon(entry) => marathon::import(
                    entry, *donor, donor_name, &folder, &native, &output, progress,
                ),
            }
        });
    }
}
