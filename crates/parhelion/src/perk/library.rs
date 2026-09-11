//! Atomic saves with optimistic concurrency checks for independent perk documents.
use super::PerkRecipe;
use fs2::FileExt;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Library {
    root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub recipe: PerkRecipe,
    pub path: PathBuf,
    pub baseline: Vec<u8>,
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
        let library = Self::open(root)?;
        library.materialize_bundled()?;
        Ok(library)
    }

    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        Ok(Self {
            root: root.canonicalize().map_err(|error| error.to_string())?,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
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

    fn materialize_bundled(&self) -> Result<(), String> {
        for encoded in super::bundled::RECIPES {
            let recipe: PerkRecipe = serde_json::from_str(encoded)
                .map_err(|error| format!("Could not read a bundled custom perk: {error}"))?;
            recipe.validate()?;
            let path = self.root.join(format!("{}.perk.json", recipe.id));
            if path.try_exists().map_err(|error| error.to_string())? {
                continue;
            }
            let mut temporary =
                tempfile::NamedTempFile::new_in(&self.root).map_err(|error| error.to_string())?;
            temporary
                .write_all(encoded.as_bytes())
                .map_err(|error| error.to_string())?;
            temporary
                .as_file()
                .sync_all()
                .map_err(|error| error.to_string())?;
            match temporary.persist_noclobber(&path) {
                Ok(_) => {}
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!(
                        "Could not add {} to My Perks: {}",
                        recipe.name, error.error
                    ));
                }
            }
        }
        Ok(())
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
            path,
            baseline: bytes,
        })
    }

    pub fn save_drafts(&self, bytes: &[u8], expected: Option<&[u8]>) -> Result<(), String> {
        self.save_checked(&self.root.join("workbench-drafts.json"), bytes, expected)
    }

    pub fn export(&self, recipe: &PerkRecipe, path: &Path) -> Result<(), String> {
        use sundial::package_authoring::{path_is_within, resolve_path_for_comparison};

        let target = resolve_path_for_comparison(path).map_err(|error| error.to_string())?;
        let root = resolve_path_for_comparison(self.root()).map_err(|error| error.to_string())?;
        if path_is_within(&target, &root) {
            return Err("Choose an export location outside My Perks. Use Save Perk or Save Copy to update the library.".into());
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
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("library.lock"))
            .map_err(|error| error.to_string())?;
        lock.try_lock_exclusive()
            .map_err(|_| "Another custom perk save is in progress")?;
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
