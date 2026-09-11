use super::*;
use std::time::SystemTime;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SortOrder {
    #[default]
    Name,
    WeaponType,
    RecentlyModified,
}

impl SortOrder {
    pub(super) const ALL: [Self; 3] = [Self::Name, Self::WeaponType, Self::RecentlyModified];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::WeaponType => "Weapon Type",
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
    pub(super) highlighted: BTreeSet<PathBuf>,
    pub(super) reveal: Option<PathBuf>,
    pub(super) export_selection: Option<BTreeSet<PathBuf>>,
    pub(super) restore: Option<crate::recipe_library::RestoreRecipe>,
    pub(super) notice: Option<String>,
    pub(super) errors: Vec<String>,
    pub(super) job: Option<thread::JoinHandle<TransferResult>>,
    modified: BTreeMap<PathBuf, SystemTime>,
    donor_indices: BTreeMap<u32, usize>,
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

    pub(super) fn matching_entries<'a>(
        &self,
        entries: &'a [RecipeLibraryEntry],
        donors: &[WeaponDonorSummary],
        query: &str,
    ) -> Vec<(&'a RecipeLibraryEntry, String)> {
        entries
            .iter()
            .filter_map(|entry| {
                let details = library_entry_details(entry, self.donor(donors, entry.donor_hash));
                library_entry_matches(entry, &details, query).then_some((entry, details))
            })
            .collect()
    }

    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub(crate) fn refresh_metadata(&mut self, entries: &[RecipeLibraryEntry]) {
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

    pub(super) fn sort_entries(
        &self,
        entries: &mut [(&RecipeLibraryEntry, String)],
        donors: &[WeaponDonorSummary],
    ) {
        entries.sort_by_cached_key(|(entry, _)| {
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
        self.library_query.clear();
        self.library_open = self.build_selection_draft.is_none();
    }

    pub(super) fn poll_library_transfer(&mut self) {
        if !self
            .library_state
            .job
            .as_ref()
            .is_some_and(|job| job.is_finished())
        {
            return;
        }
        let job = self.library_state.job.take().unwrap();
        let result = job.join();
        self.library_state.errors.clear();
        let message = match result {
            Ok(TransferResult::Imported(report)) => {
                let message = if report.errors.is_empty() {
                    format!("Imported {} recipes.", report.paths.len())
                } else {
                    format!(
                        "Imported {} recipes. {} could not be imported.",
                        report.paths.len(),
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
                format!("Exported {count} recipes.")
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
