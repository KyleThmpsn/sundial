use super::*;
use std::time::SystemTime;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SortOrder {
    Name,
    WeaponType,
    /// The default: what you edited last is what you are most likely to want next.
    #[default]
    RecentlyModified,
}

impl SortOrder {
    pub(super) const ALL: [Self; 3] = [Self::Name, Self::WeaponType, Self::RecentlyModified];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::WeaponType => "Item Type",
            Self::RecentlyModified => "Recently Modified",
        }
    }
}

pub(super) enum TransferResult {
    Imported(crate::recipe_library::ImportReport),
    Exported(Result<(PathBuf, usize), String>),
}

#[derive(Default)]
pub(crate) struct LibraryState {
    pub(super) sort: SortOrder,
    /// `None` shows every kind.
    pub(super) kind: Option<ItemKind>,
    pub(super) activities_only: bool,
    /// The item pickers' Type, Damage, Ammo and Rarity filters. Like the kind and the order, the
    /// library and the build selection share them.
    pub(super) filter: ItemChoiceFilter,
    pub(super) highlighted: BTreeSet<PathBuf>,
    pub(super) reveal: Option<PathBuf>,
    pub(super) export_selection: Option<BTreeSet<PathBuf>>,
    pub(super) restore: Option<crate::recipe_library::RestoreRecipe>,
    pub(super) delete: Option<crate::recipe_library::DeleteRecipe>,
    /// Whether the Studio project import window is open.
    pub(super) studio_import: bool,
    pub(in crate::app) notice: Option<String>,
    pub(super) errors: Vec<String>,
    pub(super) job: Option<thread::JoinHandle<TransferResult>>,
    modified: BTreeMap<PathBuf, SystemTime>,
    donor_indices: BTreeMap<u32, usize>,
    listing: Option<Listing>,
}

/// The recipes a query and the filters show, in order, each as its index in the library with its
/// details line.
pub(super) struct Listing {
    query: String,
    sort: SortOrder,
    kind: Option<ItemKind>,
    activities_only: bool,
    filter: ItemChoiceFilter,
    pub(super) shown: Vec<(usize, String)>,
}

impl Drop for LibraryState {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.join();
        }
    }
}

impl LibraryState {
    pub(crate) fn refresh_donors(&mut self, donors: &[WeaponDonorSummary]) {
        self.listing = None;
        self.donor_indices.clear();
        for (index, donor) in donors.iter().enumerate() {
            self.donor_indices.entry(donor.hash).or_insert(index);
        }
    }

    fn donor<'a>(
        &self,
        donors: &'a [WeaponDonorSummary],
        hash: u32,
    ) -> Option<&'a WeaponDonorSummary> {
        self.donor_indices
            .get(&hash)
            .and_then(|index| donors.get(*index))
            .filter(|donor| donor.hash == hash)
    }

    /// The recipes `query` and the filters show. A library of imported weapons holds thousands,
    /// and each needs its details read to match and sort, so the listing is kept until the query,
    /// a filter, the order or the library changes. [`Self::keep_listing`] gives it back.
    pub(super) fn take_listing(
        &mut self,
        entries: &[RecipeLibraryEntry],
        donors: &[WeaponDonorSummary],
        query: &str,
    ) -> Listing {
        if let Some(listing) = self.listing.take()
            && listing.query == query
            && listing.sort == self.sort
            && listing.kind == self.kind
            && listing.activities_only == self.activities_only
            && listing.filter == self.filter
        {
            return listing;
        }
        let mut shown = self.matching_entries(entries, donors, query);
        self.sort_entries(entries, &mut shown, donors);
        Listing {
            query: query.to_owned(),
            sort: self.sort,
            kind: self.kind,
            activities_only: self.activities_only,
            filter: self.filter.clone(),
            shown,
        }
    }

    pub(super) fn keep_listing(&mut self, listing: Listing) {
        self.listing = Some(listing);
    }

    /// Draws the item filters, offering each weapon type a recipe in `entries` has. Weapon
    /// choices show while the list can hold weapons. A changed choice changes the listing.
    pub(super) fn draw_filters(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        entries: &[RecipeLibraryEntry],
        donors: &[WeaponDonorSummary],
    ) {
        let mut filter = std::mem::take(&mut self.filter);
        let weapons = self.kind.is_none_or(|kind| kind == ItemKind::Weapon);
        let state = &*self;
        filter.draw(ui, (id, "filters"), weapons, move || {
            entries.iter().filter_map(move |entry| {
                library_entry_fields(entry, state.donor(donors, entry.donor_hash)).weapon_type
            })
        });
        self.filter = filter;
    }

    fn matching_entries(
        &self,
        entries: &[RecipeLibraryEntry],
        donors: &[WeaponDonorSummary],
        query: &str,
    ) -> Vec<(usize, String)> {
        entries
            .iter()
            .enumerate()
            .filter(|_| !self.activities_only)
            .filter(|(_, entry)| self.kind.is_none_or(|kind| entry.kind == kind))
            .filter_map(|(index, entry)| {
                let donor = self.donor(donors, entry.donor_hash);
                if !self.filter.passes(library_entry_fields(entry, donor)) {
                    return None;
                }
                let details = library_entry_details(entry, donor);
                library_entry_matches(entry, &details, query).then_some((index, details))
            })
            .collect()
    }

    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub(crate) fn refresh_metadata(&mut self, entries: &[RecipeLibraryEntry]) {
        self.listing = None;
        self.modified = entries
            .iter()
            .filter_map(|entry| {
                let modified = std::fs::metadata(&entry.path).ok()?.modified().ok()?;
                Some((entry.path.clone(), modified))
            })
            .collect();
        let paths: BTreeSet<_> = entries.iter().map(|entry| &entry.path).collect();
        self.highlighted.retain(|path| paths.contains(path));
        if let Some(selected) = &mut self.export_selection {
            selected.retain(|path| paths.contains(path));
        }
    }

    fn sort_entries(
        &self,
        entries: &[RecipeLibraryEntry],
        shown: &mut [(usize, String)],
        donors: &[WeaponDonorSummary],
    ) {
        shown.sort_by_cached_key(|(index, _)| {
            let entry = &entries[*index];
            let group = if self.sort == SortOrder::WeaponType {
                library_entry_type(entry, self.donor(donors, entry.donor_hash)).to_lowercase()
            } else {
                String::new()
            };
            let modified = if self.sort == SortOrder::RecentlyModified {
                self.modified.get(&entry.path).copied()
            } else {
                None
            };
            (
                group,
                std::cmp::Reverse(modified),
                entry.name.to_lowercase(),
                entry.path.clone(),
            )
        });
    }
}

impl PackageAuthoringApp {
    pub(in crate::app) fn reveal_library_entries(&mut self, paths: Vec<PathBuf>) {
        self.library_state.reveal = paths.first().cloned();
        self.library_state.highlighted = paths.into_iter().collect();
        // A search or filter could hide the recipes this shows.
        self.library_query.clear();
        self.library_state.filter = ItemChoiceFilter::default();
        self.library_open = self.build_selection_draft.is_none();
    }

    pub(super) fn poll_library_transfer(&mut self) {
        if !self
            .library_state
            .job
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            return;
        }
        let job = self.library_state.job.take().unwrap();
        let result = job.join();
        self.library_state.errors.clear();
        let message = match result {
            Ok(TransferResult::Imported(report)) => {
                let imported = recipe_count(report.paths.len());
                let message = if report.errors.is_empty() {
                    format!("Imported {imported}.")
                } else {
                    format!(
                        "Imported {imported}. {} could not be imported.",
                        report.errors.len()
                    )
                };
                if !report.paths.is_empty() {
                    self.refresh_recipe_library();
                    self.reveal_library_entries(report.paths);
                }
                self.library_state.errors = report.errors;
                message
            }
            Ok(TransferResult::Exported(Ok((path, count)))) => {
                self.library_state.export_selection = None;
                self.log.push(LogEntry::info(format!(
                    "Exported recipe bundle {}",
                    path.display()
                )));
                format!("Exported {}.", recipe_count(count))
            }
            Ok(TransferResult::Exported(Err(error))) => {
                self.library_state.errors.push(error);
                "Recipe export failed.".into()
            }
            Err(_) => {
                self.library_state.errors.push("The recipe transfer stopped unexpectedly. Refresh the library before trying again.".into());
                "Recipe transfer stopped.".into()
            }
        };
        self.log.push(LogEntry::info(&message));
        for error in &self.library_state.errors {
            self.log.push(LogEntry::error(error));
        }
        self.library_state.notice = Some(message);
    }
}
