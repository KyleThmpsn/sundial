//! Atomic saves with optimistic concurrency checks for independent perk documents.
use super::PerkRecipe;
use crate::bundled_defaults::{self, AppliedVersions, DefaultsRefresh};
mod embedded;
mod restore;
pub use embedded::ImportReport;
use fs2::FileExt;
pub(crate) use restore::RestoreDefaults;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const BUNDLED_VERSIONS_FILE_NAME: &str = "bundled-versions.json";

#[derive(Clone, Debug)]
pub struct Library {
    root: PathBuf,
    refresh: Option<DefaultsRefresh>,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub recipe: PerkRecipe,
    pub path: PathBuf,
    pub baseline: Vec<u8>,
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Default)]
pub struct Scan {
    pub entries: Vec<Entry>,
    pub errors: Vec<String>,
}

impl Library {
    pub fn open_default() -> Result<Self, String> {
        let root = sundial::package_authoring::parhelion_data_directory()
            .ok_or("Could not locate the custom perk library")?
            .join("perks");
        let mut library = Self::open(root)?;
        library.refresh = library.materialize_bundled()?;
        Ok(library)
    }

    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        Ok(Self {
            root: root.canonicalize().map_err(|error| error.to_string())?,
            refresh: None,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Bundled perks replaced while opening, because a release changed them.
    #[must_use]
    pub(crate) fn defaults_refresh(&self) -> Option<&DefaultsRefresh> {
        self.refresh.as_ref()
    }

    pub fn scan(&self) -> Result<Scan, String> {
        let mut scan = Scan::default();
        for entry in fs::read_dir(&self.root).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if !entry.file_name().to_string_lossy().ends_with(".perk.json") {
                continue;
            }
            match Self::read(&entry.path()) {
                Ok(entry) => scan.entries.push(entry),
                Err(error) => scan
                    .errors
                    .push(format!("{}: {error}", entry.path().display())),
            }
        }
        scan.entries
            .sort_by_cached_key(|entry| entry.recipe.name.to_lowercase());
        Ok(scan)
    }

    /// Adds the bundled examples the library does not hold yet, except ones the reader
    /// deleted on purpose. A copy left over from an earlier release is backed up and
    /// replaced; a copy edited under the current release is kept.
    pub(crate) fn materialize_bundled(&self) -> Result<Option<DefaultsRefresh>, String> {
        let removed = self.removed_bundled()?;
        let mut versions = AppliedVersions::load(self.root.join(BUNDLED_VERSIONS_FILE_NAME))?;
        let mut stale = Vec::new();
        for (encoded, recipe) in bundled_recipes()? {
            let key = recipe.id.to_string();
            if removed.contains(&key) {
                continue;
            }
            let digest = bundled_defaults::digest(encoded.as_bytes());
            let path = self.root.join(format!("{}.perk.json", recipe.id));
            match sundial::storage::create_file(&path, encoded.as_bytes()) {
                Ok(_) => {
                    versions.record(&key, digest);
                    continue;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!(
                        "Could not add {} to My Perks: {}",
                        recipe.name, error
                    ));
                }
            }
            if versions.is_current(&key, &digest) {
                continue;
            }
            let Some(current) = self.read_restore_target(&path)? else {
                continue;
            };
            if current == encoded.as_bytes() {
                versions.record(&key, digest);
                continue;
            }
            // Only edits worth keeping become a duplicate; a re-saved or unreadable copy
            // is preserved by the backup alone.
            let edited = serde_json::from_slice::<PerkRecipe>(&current)
                .ok()
                .filter(|edited| edited.validate().is_ok() && *edited != recipe);
            stale.push((key, encoded, digest, current, path, recipe, edited));
        }
        let refresh = if stale.is_empty() {
            None
        } else {
            let _lock = self.lock()?;
            let backup = self.create_restore_backup()?;
            let mut names = Vec::new();
            let mut copies = Vec::new();
            for (key, encoded, digest, current, path, recipe, edited) in stale {
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("A bundled custom perk has no file name")?;
                bundled_defaults::back_up(&backup, file_name, &current)?;
                if let Some(edited) = edited {
                    copies.push(self.save_copy(edited)?);
                }
                self.replace_restore_target(
                    &path,
                    &Some(current),
                    &Some(encoded.as_bytes().to_vec()),
                )?;
                versions.record(&key, digest);
                names.push(recipe.name);
            }
            Some(DefaultsRefresh {
                names,
                copies,
                backup,
            })
        };
        versions.save()?;
        Ok(refresh)
    }

    /// Saves `recipe` under a fresh id and a "Copy" name, returning the copy's name.
    fn save_copy(&self, mut recipe: PerkRecipe) -> Result<String, String> {
        let name = if recipe.name.trim().is_empty() {
            "Untitled Perk"
        } else {
            recipe.name.as_str()
        };
        recipe.name = format!("{name} Copy");
        loop {
            recipe.id = PerkRecipe::new().id;
            let mut bytes =
                serde_json::to_vec_pretty(&recipe).map_err(|error| error.to_string())?;
            bytes.push(b'\n');
            let path = self.root.join(format!("{}.perk.json", recipe.id));
            match sundial::storage::create_file(&path, &bytes) {
                Ok(()) => return Ok(recipe.name),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!(
                        "Could not keep your edited {}: {error}",
                        recipe.name
                    ));
                }
            }
        }
    }

    pub fn read(path: &Path) -> Result<Entry, String> {
        let baseline = fs::read(path).map_err(|error| error.to_string())?;
        let recipe: PerkRecipe =
            serde_json::from_slice(&baseline).map_err(|error| error.to_string())?;
        recipe.validate()?;
        Ok(Entry {
            recipe,
            path: path.to_owned(),
            baseline,
            modified: fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok(),
        })
    }

    pub fn save(&self, recipe: &PerkRecipe, expected: Option<&[u8]>) -> Result<Entry, String> {
        recipe.validate()?;
        let path = self.root.join(format!("{}.perk.json", recipe.id));
        let mut bytes = serde_json::to_vec_pretty(recipe).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        self.save_checked(&path, &bytes, expected)?;
        Ok(Entry {
            recipe: recipe.clone(),
            modified: fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok(),
            path,
            baseline: bytes,
        })
    }

    pub fn save_drafts(&self, bytes: &[u8], expected: Option<&[u8]>) -> Result<(), String> {
        self.save_checked(&self.root.join("workbench-drafts.json"), bytes, expected)
    }

    /// Removes a saved perk. The file must still hold `expected`, so a copy edited outside
    /// the workbench is preserved rather than deleted. A file that is already gone counts as
    /// deleted. A bundled example stays deleted when the library is opened again.
    pub fn delete(&self, recipe: &PerkRecipe, expected: &[u8]) -> Result<(), String> {
        let path = self.root.join(format!("{}.perk.json", recipe.id));
        let _lock = self.lock()?;
        match fs::read(&path) {
            Ok(current) if current == expected => {
                fs::remove_file(&path).map_err(|error| error.to_string())?;
            }
            Ok(_) => {
                return Err(format!(
                    "{} changed outside the workbench. The existing file was preserved.",
                    path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        if bundled_recipes()?
            .iter()
            .any(|(_, bundled)| bundled.id == recipe.id)
        {
            let mut removed = self.removed_bundled()?;
            if removed.insert(recipe.id.to_string()) {
                let listing = removed.into_iter().collect::<Vec<_>>().join("\n");
                fs::write(self.removed_bundled_path(), listing)
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    fn removed_bundled_path(&self) -> PathBuf {
        self.root.join("removed-bundled.txt")
    }

    /// The ids of bundled examples the reader deleted, one per line.
    fn removed_bundled(&self) -> Result<BTreeSet<String>, String> {
        match fs::read_to_string(self.removed_bundled_path()) {
            Ok(listing) => Ok(listing
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn lock(&self) -> Result<fs::File, String> {
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("library.lock"))
            .map_err(|error| error.to_string())?;
        lock.try_lock_exclusive()
            .map_err(|_| "Another custom perk save is in progress")?;
        Ok(lock)
    }

    pub fn export(&self, recipe: &PerkRecipe, path: &Path) -> Result<(), String> {
        use sundial::package_authoring::{path_is_within, resolve_path_for_comparison};

        let target = resolve_path_for_comparison(path).map_err(|error| error.to_string())?;
        let root = resolve_path_for_comparison(self.root()).map_err(|error| error.to_string())?;
        if path_is_within(&target, &root) {
            return Err("Choose an export location outside My Perks. Use Save to Library or Save as New Perk to update the library.".into());
        }
        let bytes = serde_json::to_vec_pretty(recipe).map_err(|error| error.to_string())?;
        sundial::package_authoring::replace_authoring_file(path, &bytes)
            .map_err(|error| error.to_string())
    }

    fn save_checked(
        &self,
        path: &Path,
        bytes: &[u8],
        expected: Option<&[u8]>,
    ) -> Result<(), String> {
        let _lock = self.lock()?;
        let current = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        if current.as_deref() != expected {
            return Err(format!(
                "{} changed outside the workbench. The existing file was preserved. Save your perk as a new copy or export it.",
                path.display()
            ));
        }
        sundial::package_authoring::replace_authoring_file(path, bytes)
            .map_err(|error| error.to_string())
    }
}

/// The bundled examples as shipped, each with its validated recipe.
fn bundled_recipes() -> Result<Vec<(&'static str, PerkRecipe)>, String> {
    super::bundled::RECIPES
        .iter()
        .map(|encoded| {
            let recipe: PerkRecipe = serde_json::from_str(encoded)
                .map_err(|error| format!("Could not read a bundled custom perk: {error}"))?;
            recipe.validate()?;
            Ok((*encoded, recipe))
        })
        .collect()
}

#[cfg(test)]
mod tests;
