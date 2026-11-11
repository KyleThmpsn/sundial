//! Materialize embedded weapon perks once, without linking subsequent library edits.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Default)]
pub struct ImportReport {
    pub added: usize,
    pub errors: Vec<String>,
}

const HISTORY: &str = "imported-weapon-perks.json";

impl Library {
    /// Stable identities assigned when saved item perks enter this library. An edited
    /// imported entry retains its identity, while an authored duplicate gets a new one.
    pub(crate) fn imported_ids(&self) -> Result<BTreeSet<String>, String> {
        match fs::read(self.root.join(HISTORY)) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("Could not read embedded perk import history: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Receives fully resolved perks, including inherited stock effects and stats.
    /// Content identities ignore document IDs and socket positions. A perk the library already
    /// holds in another layout is not added again. A durable history keeps an imported perk from
    /// returning after the reader edits or deletes it, in any layout.
    pub fn import_embedded(
        &self,
        recipes: impl IntoIterator<Item = PerkRecipe>,
    ) -> Result<ImportReport, String> {
        let recipes = recipes.into_iter().collect::<Vec<_>>();
        self.import_incoming(&Incoming::new(&recipes))
    }

    /// As [`Self::import_embedded`], with the identities `incoming` already holds.
    pub(crate) fn import_incoming(&self, incoming: &Incoming<'_>) -> Result<ImportReport, String> {
        let _lock = self.lock()?;
        let mut history = self.imported_ids()?;
        let before = history.clone();
        let scan = self.readable_scan("Embedded perk import")?;
        let mut existing = BTreeSet::new();
        let mut perks = BTreeSet::new();
        for entry in &scan.entries {
            // A perk a saved item carries comes back in the form attaching gave it.
            let known = known(entry)?;
            existing.insert(known.content);
            existing.insert(known.attached);
            perks.insert(known.layout_free);
            perks.insert(known.attached_layout_free);
        }
        let mut report = ImportReport::default();
        for (recipe, keys) in &incoming.perks {
            let keys = recipe.validate().and_then(|()| keys.clone());
            let result = keys.and_then(|Keys { key, perk, bare }| {
                if history.contains(&key) || history.contains(&perk) {
                    history.insert(perk);
                    return Ok(());
                }
                // An untouched import of the same perk from before its source showed an icon
                // gives way to it, rather than staying beside it.
                let superseded = superseded(&scan.entries, &history, bare.as_deref())?;
                if !existing.contains(&key) && !perks.contains(&perk) {
                    let mut written = (*recipe).clone();
                    written.id = key.clone();
                    let path = self.root.join(format!("{key}.perk.json"));
                    let mut bytes =
                        serde_json::to_vec_pretty(&written).map_err(|error| error.to_string())?;
                    bytes.push(b'\n');
                    // Never overwrite a file, even if its name matches the content key.
                    sundial::storage::create_file(&path, &bytes)
                        .map_err(|error| format!("{}: {error}", path.display()))?;
                    existing.insert(key.clone());
                    perks.insert(perk.clone());
                    report.added += 1;
                    if let Some(entry) = superseded
                        && fs::read(&entry.path).ok().as_deref() == Some(entry.baseline.as_slice())
                    {
                        fs::remove_file(&entry.path)
                            .map_err(|error| format!("{}: {error}", entry.path.display()))?;
                        existing.remove(&entry.recipe.id);
                        report.added -= 1;
                    }
                }
                history.insert(key);
                history.insert(perk);
                Ok::<_, String>(())
            });
            if let Err(error) = result {
                report
                    .errors
                    .push(format!("Could not import {}: {error}", recipe.name));
            }
        }
        if history != before {
            let bytes = serde_json::to_vec_pretty(&history).map_err(|error| error.to_string())?;
            sundial::package_authoring::replace_authoring_file(&self.root.join(HISTORY), &bytes)
                .map_err(|error| format!("Could not save embedded perk import history: {error}"))?;
        }
        Ok(report)
    }

    /// Removes untouched imports that are another untouched import in a different layout, and
    /// returns how many went. The lowest content id of each perk stays. Edited entries and the
    /// reader's own copies always stay, and so does a file changed since the scan. Every removed
    /// import stays in the history, so it never returns. Weapons are unchanged, since they hold
    /// their own variants and no library entry is linked to them.
    #[cfg(feature = "d2-model-importer")]
    pub(crate) fn merge_duplicate_imports(&self) -> Result<usize, String> {
        let _lock = self.lock()?;
        let history = self.imported_ids()?;
        let scan = self.readable_scan("Merging duplicate perks")?;
        let mut kept = BTreeSet::new();
        let mut merged = 0;
        for (entry, perk, untouched) in merge_order(&scan.entries, &history)? {
            if !untouched || kept.insert(perk) {
                continue;
            }
            if fs::read(&entry.path).ok().as_deref() == Some(entry.baseline.as_slice()) {
                fs::remove_file(&entry.path)
                    .map_err(|error| format!("{}: {error}", entry.path.display()))?;
                merged += 1;
            }
        }
        Ok(merged)
    }

    /// The untouched imports the library does not need:
    ///
    /// - one no saved recipe carries any more, when a saved recipe carries a perk that shows the
    ///   same: its name, its description and its icon, which an import from before icons showed
    ///   lacks. Converting an item again can change what its perks hold, and each conversion
    ///   otherwise left its predecessor's perks beside the new ones. A perk that shows something
    ///   else stays, whatever its name.
    /// - one that is the reader's own perk, or an edited import, in another layout, which a saved
    ///   item carrying that perk gave back.
    ///
    /// `current` must hold every perk of every saved recipe, as [`Self::import_incoming`]
    /// received them, so a caller that could not read them all does not call this. Finding them
    /// reads the library without locking it, so saves go on meanwhile, and
    /// [`Self::remove_unchanged`] removes only those whose files still hold what was read. Edited
    /// entries and the reader's own copies are never among them.
    pub(crate) fn redundant_imports(&self, current: &Incoming<'_>) -> Result<Vec<Entry>, String> {
        let mut carried = BTreeSet::new();
        // The icons the carried perks show, by name and description.
        let mut shown = BTreeMap::<_, Vec<_>>::new();
        for (recipe, keys) in &current.perks {
            let keys = keys.as_ref().map_err(Clone::clone)?;
            carried.insert(keys.key.clone());
            carried.insert(keys.perk.clone());
            shown
                .entry((recipe.name.as_str(), recipe.description.as_str()))
                .or_default()
                .push(recipe.icon.as_ref());
        }
        let history = self.imported_ids()?;
        let scan = self.readable_scan("Removing replaced perks")?;
        let mut untouched = Vec::new();
        let mut kept = BTreeSet::new();
        for entry in &scan.entries {
            let known = known(entry)?;
            if history.contains(&entry.recipe.id) && known.content == entry.recipe.id {
                // An untouched import holds what a saved recipe gave, as `carried` does.
                untouched.push((entry, known.layout_free));
            } else {
                kept.insert(known.layout_free);
                kept.insert(known.attached_layout_free);
            }
        }
        let mut redundant = Vec::new();
        for (entry, perk) in untouched {
            let recipe = &entry.recipe;
            let shows = (recipe.name.as_str(), recipe.description.as_str());
            let icon = recipe.icon.as_ref();
            let replaced = !carried.contains(&recipe.id)
                && !carried.contains(&perk)
                && shown
                    .get(&shows)
                    .is_some_and(|icons| icon.is_none() || icons.contains(&icon));
            if replaced || kept.contains(&perk) {
                redundant.push(entry.clone());
            }
        }
        Ok(redundant)
    }

    /// Removes the `entries` whose files still hold the bytes they were read with, except the
    /// perks in `open`, and returns the files that went. Removed imports stay in the history, so
    /// they never return. A library another window holds is cleaned up the next time instead.
    pub(crate) fn remove_unchanged(
        &self,
        entries: &[Entry],
        open: &BTreeSet<String>,
    ) -> Result<Vec<PathBuf>, String> {
        let Ok(_lock) = self.lock() else {
            return Ok(Vec::new());
        };
        let mut removed = Vec::new();
        for entry in entries
            .iter()
            .filter(|entry| !open.contains(&entry.recipe.id))
        {
            if fs::read(&entry.path).ok().as_deref() == Some(entry.baseline.as_slice()) {
                fs::remove_file(&entry.path)
                    .map_err(|error| format!("{}: {error}", entry.path.display()))?;
                removed.push(entry.path.clone());
            }
        }
        Ok(removed)
    }

    /// The library's entries, or an error naming `work` when any could not be read, since
    /// working from a partial scan could duplicate or remove the wrong entry.
    fn readable_scan(&self, work: &str) -> Result<Scan, String> {
        let scan = self.scan()?;
        if !scan.errors.is_empty() {
            return Err(format!(
                "{work} paused until the library can be read:\n{}",
                scan.errors.join("\n")
            ));
        }
        Ok(scan)
    }
}

/// Perks that saved recipes carry, each with its identities, which importing them and finding
/// the imports they replaced both compare. Hashing the perks takes most of the time of either and
/// reads nothing the library holds, so it happens once, before the library is locked against
/// saves.
pub(crate) struct Incoming<'a> {
    perks: Vec<(&'a PerkRecipe, Result<Keys, String>)>,
}

impl<'a> Incoming<'a> {
    pub(crate) fn new(perks: impl IntoIterator<Item = &'a PerkRecipe>) -> Self {
        Self {
            perks: perks
                .into_iter()
                .map(|recipe| (recipe, Keys::of(recipe)))
                .collect(),
        }
    }
}

/// An incoming perk's content and layout-free identities, and the content identity it would have
/// without its icon when it has one.
#[derive(Clone)]
struct Keys {
    key: String,
    perk: String,
    bare: Option<String>,
}

impl Keys {
    fn of(recipe: &PerkRecipe) -> Result<Self, String> {
        let bare = recipe
            .icon
            .is_some()
            .then(|| {
                let mut bare = recipe.clone();
                bare.icon = None;
                bare.content_identity()
            })
            .transpose()?;
        Ok(Self {
            key: recipe.content_identity()?,
            perk: recipe.layout_free_identity()?,
            bare,
        })
    }
}

/// A library entry's identities as it is and in the form a saved item carries it
/// (`PerkRecipe::attached`).
#[derive(Clone)]
struct Known {
    content: String,
    layout_free: String,
    attached: String,
    attached_layout_free: String,
}

/// Each perk file's [`Known`] identities, by the SHA-256 of its bytes. Hashing a library of
/// hundreds of perks takes most of a second, and imports compare against it while it is locked
/// against saves.
static KNOWN: Mutex<BTreeMap<[u8; 32], Known>> = Mutex::new(BTreeMap::new());

fn known(entry: &Entry) -> Result<Known, String> {
    let digest: [u8; 32] = Sha256::digest(&entry.baseline).into();
    if let Some(known) = KNOWN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&digest)
    {
        return Ok(known.clone());
    }
    let carried = entry.recipe.attached();
    let known = Known {
        content: entry.recipe.content_identity()?,
        layout_free: entry.recipe.layout_free_identity()?,
        attached: carried.content_identity()?,
        attached_layout_free: carried.layout_free_identity()?,
    };
    KNOWN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(digest, known.clone());
    Ok(known)
}

/// The library's untouched import of a perk whose content identity without its icon is `bare`,
/// which the perk replaces. An import keeps its content identity as its id until the reader
/// edits it.
fn superseded<'a>(
    entries: &'a [Entry],
    history: &BTreeSet<String>,
    bare: Option<&str>,
) -> Result<Option<&'a Entry>, String> {
    let Some(bare) = bare.filter(|bare| history.contains(*bare)) else {
        return Ok(None);
    };
    for entry in entries.iter().filter(|entry| entry.recipe.id == bare) {
        if known(entry)?.content == bare {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

/// The library's entries with their layout-free identities and whether each is an untouched
/// import, which keeps its content identity as its id. Untouched imports are ordered by id, so a
/// merge keeps the same one on every pass.
#[cfg(feature = "d2-model-importer")]
fn merge_order<'a>(
    entries: &'a [Entry],
    history: &BTreeSet<String>,
) -> Result<Vec<(&'a Entry, String, bool)>, String> {
    let mut ordered = entries
        .iter()
        .map(|entry| {
            let untouched = history.contains(&entry.recipe.id)
                && entry.recipe.content_identity()? == entry.recipe.id;
            Ok((entry, entry.recipe.layout_free_identity()?, untouched))
        })
        .collect::<Result<Vec<_>, String>>()?;
    ordered.sort_by_key(|(entry, _, untouched)| {
        (*untouched, untouched.then(|| entry.recipe.id.clone()))
    });
    Ok(ordered)
}

#[cfg(test)]
mod tests;
