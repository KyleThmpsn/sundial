//! Conversions no saved recipe names.
//!
//! Every conversion reserves a fresh `<kind>-<n>` folder in its item's import folder,
//! `imports/<importer>/<HASH>`, so a reimport leaves the conversion it replaced behind. Earlier
//! imports used `models/<name>/<HASH>`, which compares the same way. A recipe names the graph
//! inside one by path. A recipe an earlier version saved with its payloads embedded names none
//! until it is saved again (see `embedded`).
//! Saved recipes are the library's, its activities, and the snapshots that recovery, builds and
//! reinstalls read again: recipe backups, and the recipes of staged runs, kept installs and
//! package backups. A sweep of every item first saves the library's embedded recipes again and
//! removes the recipe backups past their retention (see `backups`), so the restored assets are
//! named and the old backups no longer keep the conversions they name.
//!
//! A sweep reads the snapshots without the library's write lease, then takes it and reads again
//! only the files that changed, so a long read never holds up a save. What each file names is kept
//! between sessions, so a sweep reads only the files changed since the last. Unused folders are
//! renamed aside under the lease and deleted after it, which can take minutes for a large library.
//!
//! Older imports kept their graph in the item folder itself. Those files go the same way when
//! nothing names the item folder. Perk and ability imports once reserved their conversions in
//! `FORMER_ROOTS` beside the models folder, and a sweep of every item treats each as one more
//! item folder.
//!
//! The conversions that stay then share their large payloads through the models folder's store
//! (`parhelion_import::store`), and a sweep of every item deletes stored files that no
//! conversion links any more.
use super::*;
use parhelion_import::{
    d2_mot::service::{IMPORTERS, IMPORTS},
    store,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};

mod arrange;
mod backups;
mod embedded;

/// The folder kinds the importers reserve in an item's model folder. Others, such as a folder
/// named by hand, are never removed.
const KINDS: [&str; 15] = [
    "ability",
    "appearance",
    "armor",
    "candidate",
    "d1-roi",
    "emblem",
    "gear",
    "marathon",
    "modern",
    "perk",
    "reach",
    "shader",
    "source",
    "vehicle",
    "weapon",
];
/// Data folders that held perk and ability conversions before those moved into item folders.
const FORMER_ROOTS: [&str; 2] = ["d1-abilities", "modern-perks"];
/// The folders an older import wrote beside a graph kept in the item folder itself. Other
/// folders there, such as one made by hand, are never removed.
const LEGACY_FOLDERS: [&str; 8] = [
    "animation",
    "audio",
    "character",
    "character-readback",
    "crosshair",
    "native-source",
    "particles",
    "projectile",
];
/// Files the modern importer keeps in the item folder, which outlive any one graph.
const ITEM_FILES: [&str; 2] = ["result.json", "timings.json"];
/// Data folders whose `recipes` folders hold snapshots.
const SNAPSHOT_ROOTS: [&str; 3] = ["backups", "installed", "staging"];
/// The deepest snapshot is a package backup's, `backups/packages/<backup>/recipes/<id>/`.
const SNAPSHOT_DEPTH: usize = 5;
/// Conversions set aside for removal. One a removal could not finish is removed by the next.
const REMOVING: &str = ".removing-";
/// Written in the data folder's `cache` when a removal may have left stored files that no
/// conversion links, so the next sweep of every item checks the store.
const UNLINKED: &str = "assets-unlinked";
/// Written in the data folder's `cache` when a sweep of every item has checked every stored
/// file. Checking reads each file's link count, seconds for a large store, so a sweep skips it
/// until a removal or `CHECK_AGAIN` has passed, since a payload replaced in place or a folder
/// deleted by hand also unlinks stored files.
const CHECKED: &str = "assets-checked";
const CHECK_AGAIN: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// What a sweep removed and shared. A folder it could not remove stays, and the next sweep tries
/// again.
#[derive(Debug, Default)]
pub(crate) struct Removed {
    pub folders: usize,
    /// Library recipes saved again with linked assets instead of embedded ones.
    pub linked: usize,
    /// Library recipes saved naming their graphs moved into their places.
    pub arranged: usize,
    /// Recipe backups past their retention, removed before the snapshots were read.
    pub backups: usize,
    /// The bytes freed, by removed conversions and by stored files no conversion links.
    pub bytes: u64,
    /// Payloads now linked to a stored copy, and the bytes they no longer hold.
    pub shared: usize,
    pub shared_bytes: u64,
    pub errors: Vec<String>,
}

/// How far a sweep has got.
#[derive(Clone, Debug, Default)]
pub(crate) struct Progress {
    pub step: Step,
    /// What the step has finished, of `total`: recipe files, item folders, sets of conversions,
    /// conversions or stored files.
    pub done: usize,
    pub total: usize,
    /// The recipe file or item being worked on.
    pub current: String,
    /// The bytes freed by removed conversions and stored files.
    pub freed: u64,
    /// The bytes payloads no longer hold now that they share a stored copy.
    pub saved: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Step {
    /// Saving library recipes that embed their imported assets with linked ones.
    Linking,
    /// Reading the saved recipes that changed since the last sweep.
    #[default]
    Reading,
    /// Moving library recipes' graphs into their places.
    Arranging,
    /// Setting aside each item folder's unused conversions.
    Checking,
    /// Deleting what was set aside.
    Removing,
    /// Sharing the payloads of the conversions that stay.
    Sharing,
    /// Deleting stored files no conversion links.
    Collecting,
}

/// A sweep's progress, shared with whoever shows it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Status(Arc<Mutex<Progress>>);

impl Status {
    pub(crate) fn get(&self) -> Progress {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn update(&self, change: impl FnOnce(&mut Progress)) {
        change(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner));
    }

    fn start(&self, step: Step, total: usize) {
        self.update(|progress| {
            progress.step = step;
            progress.done = 0;
            progress.total = total;
            progress.current.clear();
        });
    }
}

/// Conversions a sweep moved out of the way and has not yet deleted.
#[derive(Debug, Default)]
pub(crate) struct SetAside {
    sets: Vec<Aside>,
    linked: usize,
    arranged: usize,
    backups: usize,
    errors: Vec<String>,
    sharing: Option<Sharing>,
    status: Status,
}

/// The item folders a sweep looked in, whose remaining conversions share their payloads.
#[derive(Debug)]
struct Sharing {
    /// The `imports` folder, which holds the store.
    store: PathBuf,
    /// The data folder's `cache`, which holds `UNLINKED` and `CHECKED`.
    cache: PathBuf,
    items: Vec<PathBuf>,
    /// A conversion made after the sweep began may still be in progress.
    began: SystemTime,
    /// Whether the sweep looked in every item folder, so that every link to the store is known.
    every: bool,
}

impl SetAside {
    /// Deletes the conversions set aside, then shares the payloads of the conversions that stay.
    /// After a sweep of every item, stored files no conversion links are deleted. It holds no
    /// lease, so the library stays usable.
    pub(crate) fn remove(self) -> Removed {
        let SetAside {
            sets,
            linked,
            arranged,
            backups,
            errors,
            sharing,
            status,
        } = self;
        let mut removed = Removed {
            linked,
            arranged,
            backups,
            errors,
            ..Removed::default()
        };
        status.start(Step::Removing, sets.len());
        let mut unlinked = false;
        for aside in sets {
            status.update(|progress| progress.current = item_name(&aside.path));
            let bytes = size(&aside.path);
            match fs::remove_dir_all(&aside.path) {
                Ok(()) => {
                    removed.folders += aside.folders;
                    removed.bytes += bytes;
                    unlinked = true;
                    status.update(|progress| progress.freed += bytes);
                    if let Some(item) = aside.path.parent() {
                        remove_if_empty(item);
                    }
                }
                Err(error) => removed.errors.push(format!(
                    "Could not remove {}: {error}",
                    aside.path.display()
                )),
            }
            status.update(|progress| progress.done += 1);
        }
        if let Some(sharing) = sharing {
            if unlinked {
                mark(&sharing.cache.join(UNLINKED), &mut removed.errors);
                let _ = fs::remove_file(sharing.cache.join(CHECKED));
            }
            let mut shared = store::Shared::default();
            let folders = shareable(&sharing.items, sharing.began, &mut shared.errors);
            status.start(Step::Sharing, folders.len());
            for (folder, graph) in &folders {
                status.update(|progress| progress.current = item_name(folder));
                // The pass can take minutes, and an import may meanwhile free a folder and reserve
                // its name again for a conversion still being written.
                if !*graph && !finished_before(folder, sharing.began) {
                    status.update(|progress| progress.done += 1);
                    continue;
                }
                let result = if *graph {
                    // Older imports kept their graph in the item folder itself, beside later
                    // conversions.
                    store::share(&sharing.store, folder, |inner| {
                        inner
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| {
                                conversion(name).is_none() && !name.starts_with(REMOVING)
                            })
                    })
                } else {
                    store::share(&sharing.store, folder, |_| true)
                };
                status.update(|progress| {
                    progress.done += 1;
                    progress.saved += result.bytes;
                });
                shared.add(result);
            }
            removed.shared = shared.files;
            removed.shared_bytes = shared.bytes;
            removed.errors.extend(shared.errors);
            if sharing.every && store_unchecked(&sharing.cache) {
                // Both go before the check, so one it never finishes is made again, and a removal
                // meanwhile leaves `UNLINKED` for the next sweep.
                let _ = fs::remove_file(sharing.cache.join(CHECKED));
                let _ = fs::remove_file(sharing.cache.join(UNLINKED));
                status.start(Step::Collecting, 0);
                let collected = store::collect(&sharing.store, &mut |checked, total| {
                    status.update(|progress| {
                        progress.done = checked;
                        progress.total = total;
                    });
                });
                removed.bytes += collected.bytes;
                status.update(|progress| progress.freed += collected.bytes);
                if collected.errors.is_empty() {
                    mark(&sharing.cache.join(CHECKED), &mut removed.errors);
                }
                removed.errors.extend(collected.errors);
            }
        }
        removed
    }
}

/// A folder of set-aside conversions, and how many it holds.
#[derive(Debug)]
struct Aside {
    path: PathBuf,
    folders: usize,
}

/// One snapshot file's names, with the length and times it had when it was read.
#[derive(Clone, Deserialize, Serialize)]
struct Snapshot {
    stamp: (u64, Option<SystemTime>, Option<SystemTime>),
    names: BTreeSet<String>,
}

/// The snapshots the last sweep read, kept in the data folder's `cache` so the next sweep reads
/// only the files that changed. Backups never change, and they are most of what a sweep reads.
const SAVED: &str = "conversion-names.json";
/// Bump when what a snapshot's names hold changes.
const SAVED_FORMAT: u32 = 5;

#[derive(Deserialize, Serialize)]
struct Saved {
    format: u32,
    /// The folders the names were read against, as `Roots::key` spells them.
    roots: String,
    snapshots: BTreeMap<PathBuf, Snapshot>,
}

/// Where conversions live: the data folder's `imports` folder, which also holds the store, and
/// the `models` folder that earlier versions used, when it is there.
struct Folders {
    data: PathBuf,
    imports: PathBuf,
    models: Option<PathBuf>,
}

/// Where a saved path can name a conversion, as compared keys: the `imports` and `models`
/// folders, and each of `FORMER_ROOTS` in the data folder.
struct Roots {
    imports: String,
    models: Option<String>,
    data: String,
    /// Each former root's key, and how a path saved under another data folder spells it.
    former: Vec<(String, String)>,
}

impl Roots {
    fn new(folders: &Folders) -> Self {
        let data = key(&folders.data.to_string_lossy());
        let former = FORMER_ROOTS
            .iter()
            .map(|root| {
                (
                    format!("{data}{SEPARATOR}{root}"),
                    format!("{SEPARATOR}parhelion{SEPARATOR}{root}{SEPARATOR}"),
                )
            })
            .collect();
        Self {
            imports: key(&folders.imports.to_string_lossy()),
            models: folders
                .models
                .as_ref()
                .map(|models| key(&models.to_string_lossy())),
            data,
            former,
        }
    }

    /// How lowered JSON text spells each root, so a recipe that names one by its real path, such
    /// as the target of a linked `models` folder, is read too.
    fn spellings(&self) -> Vec<String> {
        std::iter::once(&self.imports)
            .chain(&self.models)
            .chain(self.former.iter().map(|(root, _)| root))
            .map(|root| root.replace('\\', "\\\\").to_ascii_lowercase())
            .collect()
    }

    /// The folders these keys came from, as one string to compare.
    fn key(&self) -> String {
        format!(
            "{}\n{}\n{}",
            self.imports,
            self.models.as_deref().unwrap_or_default(),
            self.data
        )
    }

    /// Adds the conversion folder `text`, a compared key, names.
    fn name(&self, text: &str, named: &mut BTreeSet<String>) {
        if let Some(rest) = below(text, &self.imports, IMPORTS_SPELLED) {
            // `<importer>/<HASH>/<conversion>`, whatever lies below it.
            let parts: Vec<_> = rest.splitn(4, SEPARATOR).take(3).collect();
            if parts.len() == 3 && parts.iter().all(|part| !part.is_empty()) {
                let imports = &self.imports;
                named.insert(format!("{imports}{SEPARATOR}{}", parts.join(SEPARATOR)));
            }
            return;
        }
        if let Some(rest) = self
            .models
            .as_deref()
            .and_then(|models| below(text, models, MODELS))
        {
            self.name_in_models(rest, named);
            return;
        }
        for (root, spelled) in &self.former {
            // `<root>/<conversion>`, whatever lies below it.
            let folder = below(text, root, spelled).and_then(|rest| rest.split(SEPARATOR).next());
            if let Some(folder) = folder.filter(|folder| conversion(folder).is_some()) {
                named.insert(format!("{root}{SEPARATOR}{folder}"));
            }
        }
    }

    /// Adds the conversion folder `rest`, a path below the earlier `models` folder, names.
    fn name_in_models(&self, rest: &str, named: &mut BTreeSet<String>) {
        let Some(models) = &self.models else {
            return;
        };
        // `<name>/<hash>/<conversion>`, whatever lies below it.
        let parts: Vec<_> = rest.splitn(4, SEPARATOR).take(3).collect();
        if parts.len() == 3 && parts.iter().all(|part| !part.is_empty()) {
            named.insert(format!("{models}{SEPARATOR}{}", parts.join(SEPARATOR)));
        }
        // The item folder itself, or a path in it outside any conversion, names the graph an
        // older import kept there.
        if parts.len() >= 2
            && parts[..2].iter().all(|part| !part.is_empty())
            && parts.get(2).is_none_or(|part| conversion(part).is_none())
        {
            named.insert(format!("{models}{SEPARATOR}{}", parts[..2].join(SEPARATOR)));
        }
    }
}

impl RecipeLibrary {
    /// Sets aside the conversions that no saved recipe and none of `open`, recipe documents
    /// still being edited, names. It looks in `items`, item model folders, or in every item
    /// folder when that is `None`. The newest finished modern source export stays, since the
    /// next import of the item starts from it. Nothing is set aside when a saved recipe cannot
    /// be read.
    pub(crate) fn set_aside_unused_conversions(
        &self,
        items: Option<&BTreeSet<PathBuf>>,
        open: &[serde_json::Value],
    ) -> Result<SetAside, String> {
        self.set_aside_with(items, open, Status::default(), || {})
    }

    /// Sets aside as `set_aside_unused_conversions` does, reporting to `status` here and in
    /// `SetAside::remove`.
    pub(crate) fn set_aside_reporting(
        &self,
        items: Option<&BTreeSet<PathBuf>>,
        open: &[serde_json::Value],
        status: Status,
    ) -> Result<SetAside, String> {
        self.set_aside_with(items, open, status, || {})
    }

    /// `between` runs after the first read and before the write lease is taken.
    fn set_aside_with(
        &self,
        items: Option<&BTreeSet<PathBuf>>,
        open: &[serde_json::Value],
        status: Status,
        between: impl FnOnce(),
    ) -> Result<SetAside, String> {
        let Some(folders) = self.folders()? else {
            return Ok(SetAside::default());
        };
        let roots = Roots::new(&folders);
        let began = SystemTime::now();
        // Recipes saved with embedded assets name their restored assets once saved again, and
        // backups past their retention no longer keep their conversions.
        let mut errors = Vec::new();
        let (linked, backups) = if items.is_none() {
            (
                self.link_embedded_recipes(&status, &mut errors),
                self.prune_recipe_backups(began, &mut errors),
            )
        } else {
            (0, 0)
        };
        let read = self.read_snapshots(&roots, self.saved_snapshots(&roots), Some(&status))?;
        let arranged = if items.is_none() {
            self.arrange_conversions(&roots, &read, open, &status, &mut errors)
        } else {
            0
        };
        between();
        let _lock = self.lock()?;
        // Under the lease only the files saved meanwhile are read, so it goes unreported.
        let snapshots = self.read_snapshots(&roots, read, None)?;
        self.save_snapshots(&roots, &snapshots);
        let mut named: BTreeSet<String> = snapshots
            .into_values()
            .flat_map(|snapshot| snapshot.names)
            .collect();
        for document in open {
            names(document, &roots, &mut named);
        }
        let mut aside = SetAside {
            status: status.clone(),
            linked,
            arranged,
            backups,
            errors,
            ..SetAside::default()
        };
        // Only a sweep of every item finishes earlier removals, so two sweeps never delete the
        // same folder at once.
        let (items, leftovers) = match items {
            Some(items) => (items.iter().cloned().collect(), false),
            None => {
                let mut items = Vec::new();
                for root in std::iter::once(&folders.imports).chain(&folders.models) {
                    if root.is_dir() {
                        items.extend(item_folders(root, &mut aside.errors)?);
                    }
                }
                items.extend(former_roots(&folders.data));
                (items, true)
            }
        };
        status.start(Step::Checking, items.len());
        for item in &items {
            status.update(|progress| progress.current = item_name(item));
            match set_aside(item, &named, leftovers, &mut aside.errors) {
                Ok(sets) => aside.sets.extend(sets),
                Err(error) => aside.errors.push(error),
            }
            status.update(|progress| progress.done += 1);
        }
        aside.sharing = Some(Sharing {
            store: folders.imports,
            cache: folders.data.join("cache"),
            items,
            began,
            every: leftovers,
        });
        Ok(aside)
    }

    /// Also reads the snapshots below `roots`, folders chosen in preferences such as the staging
    /// folder and the package backup folder, which may lie outside the data folder.
    pub(crate) fn with_snapshot_roots(mut self, roots: impl IntoIterator<Item = PathBuf>) -> Self {
        self.snapshot_roots = roots.into_iter().collect();
        self
    }

    /// The data folder that holds the library, resolved from where the library was opened, so a
    /// `recipes` folder linked elsewhere still finds the data folder beside it.
    fn data_folder(&self) -> Result<PathBuf, String> {
        let parent = self
            .root
            .parent()
            .ok_or("Recipe library has no data folder")?;
        fs::canonicalize(parent)
            .map_err(|error| format!("Could not resolve the data folder: {error}"))
    }

    fn saved_path(&self) -> Option<PathBuf> {
        Some(self.data_folder().ok()?.join("cache").join(SAVED))
    }

    /// The snapshots the last sweep read against the same folders. A missing or unreadable file
    /// only means every snapshot is read again.
    fn saved_snapshots(&self, roots: &Roots) -> BTreeMap<PathBuf, Snapshot> {
        self.saved_path()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            .filter(|saved| saved.format == SAVED_FORMAT && saved.roots == roots.key())
            .map(|saved| saved.snapshots)
            .unwrap_or_default()
    }

    /// Keeps what this sweep read for the next one, replacing the file whole. A failed write only
    /// costs the next sweep its reads.
    fn save_snapshots(&self, roots: &Roots, snapshots: &BTreeMap<PathBuf, Snapshot>) {
        let Some(path) = self.saved_path() else {
            return;
        };
        let saved = Saved {
            format: SAVED_FORMAT,
            roots: roots.key(),
            snapshots: snapshots.clone(),
        };
        let write = || -> std::io::Result<()> {
            let folder = path.parent().ok_or(std::io::ErrorKind::NotFound)?;
            fs::create_dir_all(folder)?;
            let mut temporary = tempfile::NamedTempFile::new_in(folder)?;
            std::io::Write::write_all(&mut temporary, &serde_json::to_vec(&saved)?)?;
            temporary.persist(&path)?;
            Ok(())
        };
        if let Err(error) = write() {
            eprintln!("Conversion names not kept: {error}");
        }
    }

    /// Where the data folder keeps conversions, `None` before any import. The `imports` folder
    /// may not exist yet when only earlier conversions do.
    fn folders(&self) -> Result<Option<Folders>, String> {
        let data = self.data_folder()?;
        let resolve = |name: &str| match fs::canonicalize(data.join(name)) {
            Ok(path) => Ok(Some(path)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("Could not resolve the {name} folder: {error}")),
        };
        let (imports, models) = (resolve(IMPORTS)?, resolve("models")?);
        if imports.is_none() && models.is_none() {
            return Ok(None);
        }
        Ok(Some(Folders {
            imports: imports.unwrap_or_else(|| data.join(IMPORTS)),
            data,
            models,
        }))
    }

    /// The conversions each saved recipe names, reusing what `previous` read from a file that
    /// has not changed since.
    fn read_snapshots(
        &self,
        roots: &Roots,
        mut previous: BTreeMap<PathBuf, Snapshot>,
        status: Option<&Status>,
    ) -> Result<BTreeMap<PathBuf, Snapshot>, String> {
        let data = self.data_folder()?;
        let mut paths = self.recipe_paths()?;
        if let Some(maps) = &self.maps {
            paths.extend(json_files(maps)?);
        }
        for root in SNAPSHOT_ROOTS {
            collect_snapshots(&data.join(root), SNAPSHOT_DEPTH, false, &mut paths)?;
        }
        for root in &self.snapshot_roots {
            collect_snapshots(root, SNAPSHOT_DEPTH, false, &mut paths)?;
        }
        paths.sort();
        paths.dedup();
        let spellings = roots.spellings();
        let mut read = BTreeMap::new();
        let read_error = |path: &Path, error: std::io::Error| {
            format!("Could not read {}: {error}", path.display())
        };
        if let Some(status) = status {
            status.start(Step::Reading, paths.len());
        }
        for (index, path) in paths.into_iter().enumerate() {
            if let Some(status) = status {
                status.update(|progress| progress.done = index);
            }
            // A snapshot removed meanwhile, such as a pruned staged run, names nothing.
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(read_error(&path, error)),
            };
            let stamp = (
                metadata.len(),
                metadata.modified().ok(),
                metadata.created().ok(),
            );
            if let Some(snapshot) = previous
                .remove(&path)
                .filter(|snapshot| snapshot.stamp == stamp)
            {
                read.insert(path, snapshot);
                continue;
            }
            if let Some(status) = status {
                status.update(|progress| {
                    progress.current = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                });
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(read_error(&path, error)),
            };
            let mut found = BTreeSet::new();
            // Only a path under `imports`, `models` or a former root keeps a conversion, so the
            // large snapshots that embed their assets, which name none, are not parsed. Windows
            // paths may be spelled in any case.
            let lowered = bytes.to_ascii_lowercase();
            let text = String::from_utf8_lossy(&lowered);
            if ["imports\\\\", "imports/", "models\\\\", "models/"]
                .iter()
                .chain(&FORMER_ROOTS)
                .any(|marker| text.contains(marker))
                || spellings.iter().any(|root| text.contains(root.as_str()))
            {
                let document: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|error| read_error(&path, error.into()))?;
                names(&document, roots, &mut found);
            }
            read.insert(
                path,
                Snapshot {
                    stamp,
                    names: found,
                },
            );
        }
        Ok(read)
    }
}

/// Every item model folder, `models/<importer>/<HASH>` as `item_directory` names them, and the
/// `models/<name>/<HASH>` folders earlier imports made. A folder above them that cannot be listed
/// is reported and skipped.
fn item_folders(models: &Path, errors: &mut Vec<String>) -> Result<Vec<PathBuf>, String> {
    let mut items = Vec::new();
    for name in subfolders(models)? {
        let folders = match subfolders(&name) {
            Ok(folders) => folders,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        for item in folders {
            let hash = item
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if hash.len() == 8
                && hash
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
            {
                items.push(item);
            }
        }
    }
    Ok(items)
}

/// The folders directly inside `directory`, leaving out links.
fn subfolders(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let list_error =
        |error: std::io::Error| format!("Could not list {}: {error}", directory.display());
    let mut folders = Vec::new();
    for entry in fs::read_dir(directory).map_err(list_error)? {
        let path = entry.map_err(list_error)?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.is_dir() && !redirected(&metadata) {
            folders.push(path);
        }
    }
    Ok(folders)
}

/// Moves the conversions of one item folder that `named` leaves out into a folder of their own.
/// With `leftovers`, sets an earlier removal could not finish are removed with it.
fn set_aside(
    item: &Path,
    named: &BTreeSet<String>,
    leftovers: bool,
    errors: &mut Vec<String>,
) -> Result<Vec<Aside>, String> {
    let item = match fs::canonicalize(item) {
        Ok(item) => item,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Could not resolve {}: {error}", item.display())),
    };
    // A sweep of every item also clears an item folder an earlier removal left empty.
    if leftovers {
        remove_if_empty(&item);
        if !item.exists() {
            return Ok(Vec::new());
        }
    }
    let mut sets = Vec::new();
    let mut conversions = Vec::new();
    for path in subfolders(&item)? {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(REMOVING) {
            if !leftovers {
                continue;
            }
            sets.push(Aside {
                folders: fs::read_dir(&path).map_or(0, Iterator::count),
                path,
            });
        } else if let Some((kind, index)) = conversion(name) {
            // A finished export, with when it finished. Freed indices are reserved again, so the
            // highest index is not always the newest export.
            let finished = (kind == "source")
                .then(|| fs::metadata(path.join("export.json")).and_then(|m| m.modified()))
                .and_then(Result::ok);
            conversions.push((path, index, finished));
        }
    }
    let newest_export = conversions
        .iter()
        .filter_map(|(path, index, finished)| Some((finished.as_ref()?, *index, path)))
        .max()
        .map(|(_, _, path)| path.clone());
    let unused: Vec<_> = conversions
        .into_iter()
        .map(|(path, _, _)| path)
        .filter(|path| {
            newest_export.as_ref() != Some(path) && !named.contains(&key(&path.to_string_lossy()))
        })
        .collect();
    let legacy = if item.join("asset-graph.json").is_file()
        && !named.contains(&key(&item.to_string_lossy()))
    {
        legacy_entries(&item)?
    } else {
        Vec::new()
    };
    if unused.is_empty() && legacy.is_empty() {
        return Ok(sets);
    }
    let mut aside = Aside {
        path: tempfile::Builder::new()
            .prefix(REMOVING)
            .tempdir_in(&item)
            .map(tempfile::TempDir::keep)
            .map_err(|error| format!("Could not prepare {}: {error}", item.display()))?,
        folders: 0,
    };
    for path in unused {
        let destination = aside.path.join(path.file_name().unwrap_or_default());
        match fs::rename(&path, destination) {
            Ok(()) => aside.folders += 1,
            Err(error) => errors.push(format!("Could not remove {}: {error}", path.display())),
        }
    }
    // An older graph counts as one conversion.
    let mut moved = false;
    for path in legacy {
        let destination = aside.path.join(path.file_name().unwrap_or_default());
        match fs::rename(&path, destination) {
            Ok(()) => moved = true,
            Err(error) => errors.push(format!("Could not remove {}: {error}", path.display())),
        }
    }
    aside.folders += usize::from(moved);
    sets.push(aside);
    Ok(sets)
}

/// What an older import wrote in the item folder itself beside its graph: its files, other than
/// the modern importer's `ITEM_FILES`, and its `LEGACY_FOLDERS`. `asset-graph.json` comes last,
/// so a removal that stops partway leaves the graph for the next sweep to find.
fn legacy_entries(item: &Path) -> Result<Vec<PathBuf>, String> {
    let list_error = |error: std::io::Error| format!("Could not list {}: {error}", item.display());
    let mut entries = Vec::new();
    for entry in fs::read_dir(item).map_err(list_error)? {
        let path = entry.map_err(list_error)?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let legacy = if redirected(&metadata) {
            false
        } else if metadata.is_dir() {
            LEGACY_FOLDERS.contains(&name)
        } else {
            !ITEM_FILES.contains(&name)
        };
        if legacy {
            entries.push(path);
        }
    }
    entries.sort_by_key(|path| {
        path.file_name()
            .is_some_and(|name| name == "asset-graph.json")
    });
    Ok(entries)
}

/// Whether a removal since the last check of every stored file may have unlinked some, or that
/// check is missing or older than `CHECK_AGAIN`.
fn store_unchecked(cache: &Path) -> bool {
    cache.join(UNLINKED).exists()
        || fs::metadata(cache.join(CHECKED))
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|checked| checked.elapsed().ok())
            .is_none_or(|age| age > CHECK_AGAIN)
}

/// Writes the empty file `path`, creating its folder.
fn mark(path: &Path, errors: &mut Vec<String>) {
    let written = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(path, b""));
    if let Err(error) = written {
        errors.push(format!("Could not write {}: {error}", path.display()));
    }
}

/// The folders of `items` whose payloads the store has not yet finished with, each with whether
/// it is an item folder that keeps an older graph. A conversion made after `began` is left for
/// the next sweep, since it may still be in progress.
fn shareable(
    items: &[PathBuf],
    began: SystemTime,
    errors: &mut Vec<String>,
) -> Vec<(PathBuf, bool)> {
    let mut shareable = Vec::new();
    for item in items {
        let folders = match subfolders(item) {
            Ok(folders) => folders,
            Err(error) => {
                // A removed item has nothing to share.
                if item.exists() {
                    errors.push(error);
                }
                continue;
            }
        };
        for path in folders {
            let is_conversion = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| conversion(name).is_some());
            if is_conversion && finished_before(&path, began) {
                shareable.push((path, false));
            }
        }
        if item.join("asset-graph.json").is_file() && !store::is_shared(item) {
            shareable.push((item.clone(), true));
        }
    }
    shareable
}

/// Whether the conversion folder `path` is unshared and was both made and last changed before
/// `began`. A conversion still being written changes as its files arrive, even when a folder
/// reserved again under a freed name keeps the earlier creation time.
fn finished_before(path: &Path, began: SystemTime) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let made = metadata.created().or_else(|_| metadata.modified());
    made.is_ok_and(|made| made < began)
        && metadata.modified().is_ok_and(|changed| changed < began)
        && !store::is_shared(path)
}

/// How progress names the item model folder that holds `folder`, a conversion or a set aside:
/// `<importer>/<HASH>`, or `<name>` for an earlier `models/<name>/<HASH>`. An item folder is its
/// own, and a former root names itself.
fn item_name(folder: &Path) -> String {
    let in_item = folder
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| conversion(name).is_some() || name.starts_with(REMOVING));
    let item = if in_item {
        folder.parent()
    } else {
        Some(folder)
    };
    let name = |path: Option<&Path>| {
        path.and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let (own, parent) = (name(item), name(item.and_then(Path::parent)));
    if FORMER_ROOTS.contains(&own.as_str()) {
        own
    } else if IMPORTERS.contains(&parent.as_str()) {
        format!("{parent}/{own}")
    } else {
        parent
    }
}

/// The index of a folder an importer reserved, such as `d1-roi-3`.
fn conversion(name: &str) -> Option<(&str, u32)> {
    let (kind, index) = name.rsplit_once('-')?;
    if !KINDS.contains(&kind) || index.is_empty() || !index.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((kind, index.parse().ok()?))
}

/// Adds the conversion folder each string of `document` names.
fn names(document: &serde_json::Value, roots: &Roots, named: &mut BTreeSet<String>) {
    match document {
        serde_json::Value::String(text) => roots.name(&key(text), named),
        serde_json::Value::Array(values) => {
            for value in values {
                names(value, roots, named);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                names(value, roots, named);
            }
        }
        _ => {}
    }
}

/// Removes the item folder `item` once nothing is left in it, such as after its only conversion
/// went, and then the `<importer>` or earlier `<name>` folder above it the same way. A folder with anything in it stays, since removing a directory
/// fails unless it is empty.
fn remove_if_empty(item: &Path) {
    if fs::remove_dir(item).is_err() {
        return;
    }
    if let Some(parent) = item.parent()
        && parent
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|root| root == IMPORTS || root == "models")
    {
        let _ = fs::remove_dir(parent);
    }
}

/// The part of `text`, a compared key, below `root`. A path saved under another data folder,
/// before the folder moved or in a copy of the library, names the same place by where `spelled`,
/// the root's own folders below the data folder, appears in it.
fn below<'a>(text: &'a str, root: &str, spelled: &str) -> Option<&'a str> {
    text.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix(SEPARATOR))
        .or_else(|| text.find(spelled).map(|at| &text[at + spelled.len()..]))
}

/// The former roots that exist as plain folders. A linked one is left alone, since its
/// conversions would not compare by the path a recipe saved.
fn former_roots(data: &Path) -> Vec<PathBuf> {
    FORMER_ROOTS
        .iter()
        .map(|root| data.join(root))
        .filter(|root| {
            fs::symlink_metadata(root)
                .is_ok_and(|metadata| metadata.is_dir() && !redirected(&metadata))
        })
        .collect()
}

#[cfg(windows)]
const SEPARATOR: &str = "\\";
#[cfg(not(windows))]
const SEPARATOR: &str = "/";
/// The imports folder and the earlier models folder inside a data folder, as compared keys spell
/// them.
#[cfg(windows)]
const IMPORTS_SPELLED: &str = r"\parhelion\imports\";
#[cfg(not(windows))]
const IMPORTS_SPELLED: &str = "/parhelion/imports/";
#[cfg(windows)]
const MODELS: &str = r"\parhelion\models\";
#[cfg(not(windows))]
const MODELS: &str = "/parhelion/models/";

/// A path as text to compare. Windows paths compare without case, separator style or the
/// verbatim prefix `canonicalize` adds.
fn key(path: &str) -> String {
    if cfg!(windows) {
        let path = path.replace('/', "\\");
        let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
        path.trim_end_matches('\\').to_lowercase()
    } else {
        path.trim_end_matches('/').to_owned()
    }
}

/// The JSON files of a `recipes` folder at most `depth` folders below `directory`, and those
/// further below it. Links are not followed, since a snapshot folder holds none.
fn collect_snapshots(
    directory: &Path,
    depth: usize,
    inside: bool,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Could not list {}: {error}", directory.display())),
    };
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Could not list {}: {error}", directory.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if redirected(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            if depth > 0 {
                let inside = inside || entry.file_name() == "recipes";
                collect_snapshots(&path, depth - 1, inside, files)?;
            }
        } else if inside && is_json(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn json_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Could not list {}: {error}", directory.display())),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("Could not list {}: {error}", directory.display()))?
            .path();
        if is_json(&path) && path.is_file() {
            files.push(path);
        }
    }
    Ok(files)
}

fn is_json(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

/// A symbolic link, junction or other reparse point.
fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

/// The bytes deleting the files below `directory` frees, not following links. A file with other
/// names, such as a shared payload, keeps its content.
fn size(directory: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![directory.to_owned()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.is_dir() && !redirected(&metadata) {
                pending.push(entry.path());
            } else if metadata.is_file()
                && (metadata.len() < store::SMALLEST
                    || store::links(&entry.path()).is_ok_and(|links| links == 1))
            {
                total += metadata.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests;
