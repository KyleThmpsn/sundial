//! Import sources. Each keeps its own folder, listing, selection and view. Halo: Reach and
//! Marathon rows reuse the modern row, keyed by the authored item they import into.
use super::*;
use parhelion_import::{halo_reach::browse as reach, marathon::browse as marathon};

const SPARROW_BUCKET: u32 = 2025709351;

/// Retain the saved runtime and presentation selections before converting a new graph.
pub(super) fn reach_request(
    entry: &reach::Entry,
    donor: u32,
    donor_name: &str,
    previous: Option<&Path>,
) -> anyhow::Result<parhelion_import::halo_reach::ImportRequest> {
    use anyhow::{Context, ensure};
    let mut request = entry.request(donor, donor_name)?;
    if let Some(path) = previous {
        let saved = WeaponRecipe::load_json(path)
            .with_context(|| format!("Read saved item {}", path.display()))?;
        let kind = if entry.vehicle() {
            ItemKind::Sparrow
        } else {
            ItemKind::Weapon
        };
        ensure!(
            saved.kind == kind && saved.identity.item_hash.parse_u32()? == entry.destination()?,
            "The saved item changed before reimport. Refresh the library and try again"
        );
        if !entry.vehicle() {
            let generated_name = request.recipe["name"].clone();
            request.recipe = serde_json::to_value(&saved)?;
            request.recipe["overrides"]
                .as_object_mut()
                .context("Saved weapon overrides")?
                .remove("imported_graph");
            if generated_name
                .as_str()
                .is_some_and(|name| saved.name == format!("Reach {name}"))
            {
                request.recipe["name"] = generated_name;
            }
            if let Some(graph) = &saved.overrides.imported_graph {
                if parhelion_import::artwork::generated_image(
                    graph,
                    &request.recipe["overrides"]["icon_edit"]["imported_image"],
                )? {
                    request.recipe["overrides"]["icon_edit"]
                        .as_object_mut()
                        .context("Saved icon edit")?
                        .remove("imported_image");
                }
                request.retain_weapon_presentation(graph)?;
            }
            return Ok(request);
        }
        let summon = &saved
            .overrides
            .sparrow
            .as_ref()
            .context("The saved vehicle has no summon selection")?
            .summon;
        let entity = summon
            .entity()
            .map_err(anyhow::Error::msg)?
            .context("Select a native vehicle for this imported appearance before reimporting")?;
        request.runtime_entity = Some(entity);
        request.recipe["overrides"]["sparrow"]["summon"] = serde_json::to_value(summon)?;
        if let Some(graph) = &saved.overrides.imported_graph {
            request.retain_vehicle_presentation(graph)?;
        }
    }
    Ok(request)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::app) enum Source {
    #[default]
    D2Modern,
    HaloReach,
    Marathon,
    D1RiseOfIron,
}

impl Source {
    pub const ALL: [Source; 4] = [
        Source::D2Modern,
        Source::D1RiseOfIron,
        Source::HaloReach,
        Source::Marathon,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Source::D2Modern => "D2 Modern",
            Source::HaloReach => "Halo: Reach",
            Source::Marathon => "Marathon",
            Source::D1RiseOfIron => "D1 Rise of Iron",
        }
    }

    /// Where this source's browser view is kept, `importer/views/<importer>.json`. Earlier
    /// versions kept it at the data folder's top as `importer-view*.json`, which moves there.
    /// Until a move succeeds the earlier file stays in use.
    pub fn view_path(self, root: &Path) -> PathBuf {
        let former = root.join(match self {
            Source::D2Modern => "importer-view.json",
            Source::HaloReach => "importer-view-reach.json",
            Source::Marathon => "importer-view-marathon.json",
            Source::D1RiseOfIron => "importer-view-d1-roi.json",
        });
        let path = root
            .join("importer")
            .join("views")
            .join(format!("{}.json", self.folder()));
        parhelion_import::cache::take_over(&former, &path);
        if former.is_file() && !path.exists() {
            former
        } else {
            path
        }
    }

    /// These sources expose checked inventory artwork readers.
    pub fn icons(self) -> bool {
        matches!(self, Source::D2Modern | Source::D1RiseOfIron)
    }

    pub fn welcome(self) -> &'static str {
        match self {
            Source::D2Modern => "Choose your modern Destiny 2 installation or its packages folder.",
            Source::HaloReach => "Choose your MCC installation or its Halo: Reach maps folder.",
            Source::Marathon => "Choose your Marathon installation or its packages folder.",
            Source::D1RiseOfIron => "Choose your Rise of Iron Xbox One packages folder.",
        }
    }

    fn dialog_title(self) -> &'static str {
        match self {
            Source::D2Modern => "Modern Destiny 2 Build",
            Source::HaloReach => "Halo: Reach Maps",
            Source::Marathon => "Marathon Packages",
            Source::D1RiseOfIron => "Rise of Iron Xbox One Packages",
        }
    }

    /// The folder of the models folder that holds this importer's items, named for its module.
    pub fn folder(self) -> &'static str {
        match self {
            Source::D2Modern => "d2_mot",
            Source::HaloReach => "halo_reach",
            Source::Marathon => "marathon",
            Source::D1RiseOfIron => "d1_roi",
        }
    }

    /// The importer that wrote a recipe, from its namespace. The earlier bulk and import schemes
    /// end with their source item's hash, and earlier Marathon names hold its hash or a short
    /// word. Other names record no source item.
    pub fn of_namespace(namespace: &str) -> Option<Source> {
        [
            ("parhelion.reach.", Source::HaloReach),
            ("parhelion.d1_roi.", Source::D1RiseOfIron),
            ("parhelion.import.d1_roi.", Source::D1RiseOfIron),
            ("parhelion.d2_mot.", Source::D2Modern),
            ("parhelion.bulk.", Source::D2Modern),
            ("parhelion.marathon.", Source::Marathon),
        ]
        .into_iter()
        .find_map(|(prefix, source)| namespace.starts_with(prefix).then_some(source))
    }

    /// The source item hash that ends a namespace, as every Rise of Iron and modern Destiny 2
    /// namespace does and some earlier Marathon ones do.
    pub fn named_item(namespace: &str) -> Option<u32> {
        let hash = namespace.rsplit(['-', '.']).next()?;
        u32::from_str_radix(hash, 16).ok()
    }

    fn reading(self) -> &'static str {
        match self {
            Source::HaloReach => "Reading Maps",
            Source::D1RiseOfIron => "Reading Packages",
            _ => "Reading Items",
        }
    }
}

#[derive(Clone)]
pub(super) enum Origin {
    HaloReach(reach::Entry),
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
            Origin::HaloReach(entry) if entry.vehicle() => Some(Base::Vehicle),
            Origin::HaloReach(entry) => Some(Base::Weapon(entry.weapon_type())),
            Origin::Marathon(entry) => entry.weapon_type().map(Base::Weapon),
        }
    }

    /// The row's type: the native weapon type, Vehicle, or an unsupported item's own type.
    fn kind(&self) -> String {
        match self {
            Origin::HaloReach(entry) if entry.vehicle() => "Vehicle".to_owned(),
            Origin::HaloReach(entry) => entry.weapon_type().to_owned(),
            Origin::Marathon(entry) => entry
                .weapon_type()
                .map_or_else(|| entry.kind.clone(), str::to_owned),
        }
    }

    pub fn hover(&self) -> String {
        match self {
            Origin::HaloReach(entry) => {
                format!("{}\nMaps: {}", entry.path, entry.caches.join(", "))
            }
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
            Origin::HaloReach(entry) => (entry.destination(), entry.name(), entry.vehicle()),
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
    scan_warning: Option<String>,
    donor_stamp: (usize, usize),
    origins: BTreeMap<u32, Origin>,
}

impl Shelf {
    fn new(source: Source) -> Self {
        let mut browser = browser::Browser::default();
        let view_path = data_root().ok().map(|root| source.view_path(&root));
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
            scan_warning: None,
            donor_stamp: (usize::MAX, usize::MAX),
            origins: BTreeMap::new(),
        }
    }
}

impl Importer {
    pub fn folder(&self) -> Option<&PathBuf> {
        match self.source {
            Source::D2Modern => self.settings.modern_packages.as_ref(),
            Source::HaloReach => self.settings.reach_maps.as_ref(),
            Source::Marathon => self.settings.marathon_packages.as_ref(),
            Source::D1RiseOfIron => self.settings.d1_roi.as_ref(),
        }
    }

    fn folder_mut(&mut self) -> &mut Option<PathBuf> {
        match self.source {
            Source::D2Modern => &mut self.settings.modern_packages,
            Source::HaloReach => &mut self.settings.reach_maps,
            Source::Marathon => &mut self.settings.marathon_packages,
            Source::D1RiseOfIron => &mut self.settings.d1_roi,
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
            scan_warning: self.scan_warning.take(),
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
        self.scan_warning = next.scan_warning;
        self.donor_stamp = next.donor_stamp;
        self.origins = next.origins;
        self.source = source;
        self.icons = icons::Icons::default();
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
            Source::D2Modern => service::packages_path(&path).map_err(|error| error.to_string()),
            Source::HaloReach => reach::maps_path(&path).map_err(|error| format!("{error:#}")),
            Source::Marathon => {
                marathon::packages_path(&path).map_err(|error| format!("{error:#}"))
            }
            Source::D1RiseOfIron => {
                parhelion_import::d1_roi::packages_path(&path).map_err(|error| format!("{error:#}"))
            }
        };
        match resolved {
            Ok(path) => {
                *self.importer.folder_mut() = Some(path);
                if source == Source::D1RiseOfIron {
                    self.importer.roi = None;
                }
                self.importer.weapons.clear();
                self.importer.origins.clear();
                self.importer.browser.dirty = true;
                self.importer.browser.reset_scroll = true;
                self.importer.browser.anchor = None;
                self.importer.selected.clear();
                self.importer.scan_error = None;
                self.importer.scan_warning = None;
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
                Source::HaloReach => {
                    reach::browse_cached(&folder, &cache, refresh, progress).map(|entries| {
                        let (weapons, origins) = rows(entries.into_iter().map(Origin::HaloReach));
                        (weapons, origins, None)
                    })
                }
                Source::Marathon => marathon::browse_cached(&folder, &cache, refresh, progress)
                    .map(|listing| {
                        let (weapons, origins) =
                            rows(listing.entries.into_iter().map(Origin::Marathon));
                        (weapons, origins, listing.warning)
                    }),
                Source::D2Modern | Source::D1RiseOfIron => {
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

    /// The first installed native item an origin can build on: a weapon of its type, or a Sparrow
    /// for a vehicle. Stock items that unlock through their own collectible come first. A Marathon
    /// import can build on a later candidate, as [`Self::foreign_donors`] describes.
    pub(super) fn foreign_donor(
        &self,
        origin: &Origin,
        risky: &BTreeSet<u32>,
    ) -> Option<(u32, String)> {
        self.foreign_donors(origin, risky).into_iter().next()
    }

    /// Every installed native item an origin can build on, in [`Self::foreign_donor`]'s order.
    /// A Halo: Reach import takes the first. A Marathon import gets the whole list and takes the
    /// earliest whose own first-person animation best fits the source weapon's hold, reloads and
    /// reload motion, or with Marathon animations on, the first whose states request the source's
    /// reloads.
    pub(super) fn foreign_donors(
        &self,
        origin: &Origin,
        risky: &BTreeSet<u32>,
    ) -> Vec<(u32, String)> {
        let stock = |hash: u32| {
            self.catalog
                .as_ref()
                .and_then(|catalog| catalog.item_definition_tag(hash))
                .is_none_or(crate::package_profile::is_stock_item_definition)
        };
        let order = |donors: &mut dyn Iterator<Item = &WeaponDonorSummary>| {
            let mut donors = donors.filter(|donor| stock(donor.hash)).collect::<Vec<_>>();
            donors.sort_by_key(|donor| {
                (
                    !donor.collection_backed,
                    donor.name.to_lowercase(),
                    donor.hash,
                )
            });
            donors
                .into_iter()
                .map(|donor| (donor.hash, donor.name.clone()))
                .collect()
        };
        match origin.base() {
            None => Vec::new(),
            Some(Base::Vehicle) => order(&mut self.gear_donors_for(ItemKind::Sparrow).iter()),
            Some(Base::Weapon(kind)) => order(
                &mut self
                    .donor_summaries
                    .iter()
                    .filter(|donor| donor.type_name == kind && !risky.contains(&donor.hash)),
            ),
        }
    }

    /// Converts the selected Halo: Reach or Marathon items, each onto its chosen native base.
    /// `reimport` names the library recipe each item a library reimport updates. A Marathon item
    /// brings ornaments as its recipe did, and any other as the importer's Ornaments choice says.
    pub(super) fn import_foreign(&mut self, ctx: &egui::Context, reimport: &BTreeMap<u32, Saved>) {
        let source = self.importer.source;
        let Some(folder) = self.importer.folder().cloned() else {
            return;
        };
        let Ok(root) = data_root() else { return };
        let risky = self.risky_donors();
        let source_animation = self.importer.settings.marathon_animations;
        let ornaments = self.importer.settings.marathon_ornaments;
        let existing: BTreeMap<_, _> = self
            .recipe_entries
            .iter()
            .map(|entry| (entry.identity_hash, entry.path.clone()))
            .collect();
        let jobs: Vec<_> = self
            .importer
            .selected_weapons()
            .filter_map(|weapon| {
                let mut origin = self.importer.origins.get(&weapon.hash)?.clone();
                let donor = self.foreign_donors(&origin, &risky);
                if donor.is_empty() {
                    return None;
                }
                // Earlier Marathon imports were named by source hash or a short word. Halo:
                // Reach recipes were only ever named by their entry slug.
                let saved = reimport.get(&weapon.hash);
                if let Origin::Marathon(entry) = &mut origin {
                    entry.saved_namespace = saved.map(|saved| saved.namespace.clone());
                }
                let options = marathon::Options {
                    source_animation,
                    ornaments: saved.map_or(ornaments, |saved| saved.ornaments),
                };
                Some((weapon.clone(), origin, donor, options))
            })
            .collect();
        let names = jobs
            .iter()
            .map(|(weapon, ..)| weapon.name.clone())
            .collect();
        let cache = root.join("importer/catalog");
        let relisted = self.importer.relisted.clone();
        self.run_import(ctx, names, move |index, native, progress| {
            let (weapon, origin, donors, options) = &jobs[index];
            let (donor, donor_name) = &donors[0];
            let kind = match origin {
                Origin::HaloReach(entry) if entry.vehicle() => "vehicle",
                Origin::HaloReach(_) | Origin::Marathon(_) => "weapon",
            };
            let item = service::item_directory(
                &root.join(service::IMPORTS),
                source.folder(),
                weapon.hash,
            )?;
            // The importer works in `work`, and its graphs then move into their places in the
            // conversion folder.
            let conversion = service::reserve_assets(&item, kind)?;
            let output = conversion.join("work");
            match origin {
                Origin::HaloReach(entry) => {
                    progress("Translating source model…".into());
                    let request = reach_request(
                        entry,
                        *donor,
                        donor_name,
                        existing.get(&weapon.hash).map(PathBuf::as_path),
                    )?;
                    let recipe = reach::import(&request, &folder, native, &output)?;
                    parhelion_import::settle_graphs(&recipe, &conversion)?;
                    Ok(recipe)
                }
                Origin::Marathon(entry) => {
                    // A game update renumbers the tags a listing names, so an entry listed
                    // before one is found again in the current packages.
                    let current = marathon::current(&folder, &cache, entry)?;
                    if current.listed != entry.listed {
                        relisted.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    let recipe = marathon::import(
                        &current, donors, *options, &folder, native, &output, progress,
                    )?;
                    marathon::settle(&recipe, &conversion)?;
                    Ok(recipe)
                }
            }
        });
    }
}
