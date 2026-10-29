//! Library entries and identity claims read from recipe files.
//!
//! A recipe reads through the strict JSON checks, which build the whole document before
//! decoding it, so an imported weapon's recipe of about 150 KB takes milliseconds. A library of
//! 2,000 of them held the window for seconds when it opened, and twice more on every save: once
//! for the identity check and once for the rescan. Files are read on several threads, and what a
//! file gave is kept while its length and modification time stay the same. A recipe that embeds a
//! model reads in later sessions from its text without the model's files, which
//! [`WeaponRecipe::load_listing`] keeps.
use super::*;
use std::{
    collections::BTreeMap,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::SystemTime,
};

/// The most threads a scan reads recipes on.
const MAX_WORKERS: usize = 8;

/// The namespace and identity hashes a recipe claims ([`WeaponRecipe::load_claim`]).
type Claim = (String, [u32; 12]);

#[derive(Clone, PartialEq, Eq)]
struct Version {
    len: u64,
    modified: SystemTime,
}

fn version(path: &Path) -> Option<Version> {
    let metadata = fs::metadata(path).ok()?;
    Some(Version {
        len: metadata.len(),
        modified: metadata.modified().ok()?,
    })
}

/// What was read from each file, with the version it was read from.
struct Kept<T>(Mutex<BTreeMap<PathBuf, (Version, T)>>);

impl<T: Clone> Kept<T> {
    const fn new() -> Self {
        Self(Mutex::new(BTreeMap::new()))
    }

    /// What `read` gives for `path`, read again only when the file changed. A file replaced while
    /// it was read keeps nothing, so the next read sees the new one. Failures are never kept.
    fn get<E>(&self, path: &Path, read: impl FnOnce(&Path) -> Result<T, E>) -> Result<T, E> {
        let before = version(path);
        if let Some(before) = &before
            && let Some((kept, value)) = self.lock().get(path)
            && kept == before
        {
            return Ok(value.clone());
        }
        let value = read(path)?;
        if let Some(before) = before.filter(|before| version(path).as_ref() == Some(before)) {
            self.lock().insert(path.to_owned(), (before, value.clone()));
        }
        Ok(value)
    }

    /// Forgets the files of folder `root` other than `paths`, which were removed or renamed.
    fn keep_only(&self, root: &Path, paths: &[PathBuf]) {
        let paths: BTreeSet<_> = paths.iter().collect();
        self.lock()
            .retain(|path, _| path.parent() != Some(root) || paths.contains(path));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<PathBuf, (Version, T)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

static ENTRIES: Kept<RecipeLibraryEntry> = Kept::new();
static CLAIMS: Kept<Claim> = Kept::new();

/// The library entry of each recipe file of folder `root` at `paths`, in the same order.
pub(super) fn entries(root: &Path, paths: &[PathBuf]) -> Vec<Result<RecipeLibraryEntry, String>> {
    ENTRIES.keep_only(root, paths);
    WeaponRecipe::keep_listings(root, paths);
    in_parallel(paths, |path| {
        ENTRIES
            .get(path, entry)
            .map_err(|error| format!("{}: {error}", path.display()))
    })
}

/// The namespace and identity hashes each recipe file of folder `root` at `paths` claims, in the
/// same order.
pub(super) fn claims(
    root: &Path,
    paths: &[PathBuf],
) -> Vec<Result<Claim, crate::recipe::RecipeError>> {
    CLAIMS.keep_only(root, paths);
    in_parallel(paths, |path| {
        CLAIMS.get(path, |path| WeaponRecipe::load_claim(path))
    })
}

/// `read` applied to each of `paths` on up to [`MAX_WORKERS`] threads, each taking the next path
/// no thread has taken. The results keep the order of `paths`.
pub(super) fn in_parallel<T: Send>(paths: &[PathBuf], read: impl Fn(&Path) -> T + Sync) -> Vec<T> {
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(MAX_WORKERS)
        .min(paths.len());
    if workers <= 1 {
        return paths.iter().map(|path| read(path)).collect();
    }
    let next = AtomicUsize::new(0);
    let mut results = thread::scope(|scope| {
        let handles = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = paths.get(index) else {
                            break done;
                        };
                        done.push((index, read(path)));
                    }
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect::<Vec<_>>()
    });
    results.sort_unstable_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, value)| value).collect()
}

fn entry(path: &Path) -> Result<RecipeLibraryEntry, crate::recipe::RecipeError> {
    let (recipe, armor_origin, imported) = WeaponRecipe::load_listing_details(path)?;
    let import_revision = imported.map(|imported| imported.revision);
    let import_ornaments = imported.is_some_and(|imported| imported.ornaments);
    // A mod is its one perk, so it shows that perk's own image when it has one.
    let perk_icon = (recipe.kind == crate::ItemKind::Mod)
        .then(
            || match &recipe.overrides.socket_plug_variants.first()?.icon {
                Some(crate::perk::Icon::Image { image, .. }) => Some(image.fingerprint()),
                _ => None,
            },
        )
        .flatten();
    Ok(RecipeLibraryEntry {
        perk_icon,
        // Read first, while the fields below have not yet moved out of the recipe.
        art: crate::icon_art::Art::of(&recipe),
        armor_origin,
        import_revision,
        import_ornaments,
        kind: recipe.kind,
        collection_destination: recipe.overrides.collection_destination,
        collection_position: recipe.overrides.collection_position,
        armor_class: recipe.overrides.armor_class,
        badge: recipe.overrides.badge.as_ref().map(|badge| LibraryBadge {
            name: badge.name.clone(),
            description: badge.description.clone(),
            icon: badge
                .icon
                .as_ref()
                .map(crate::presentation::Artwork::fingerprint),
        }),
        corner_icon: recipe
            .overrides
            .corner_icon
            .as_ref()
            .map(crate::presentation::Artwork::fingerprint),
        bundled: path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                BUNDLED_RECIPES
                    .iter()
                    .any(|(file_name, _)| name == *file_name)
            }),
        path: path.to_owned(),
        donor_hash: recipe.donor.item_hash.parse_u32().unwrap_or_default(),
        type_donor_hash: recipe.type_donor_hash(),
        identity_hash: recipe.identity.item_hash.parse_u32().unwrap_or_default(),
        type_name: recipe.type_name,
        class_type_name: (recipe.kind == crate::ItemKind::Subclass)
            .then(|| {
                crate::subclass::class_type_name(
                    recipe.overrides.subclass_every_class,
                    recipe.overrides.subclass_class,
                )
            })
            .flatten(),
        ammo_type: recipe.overrides.ammo_type,
        damage_type: recipe.overrides.modern_damage_type,
        rarity: recipe.overrides.rarity,
        icon_hash: recipe
            .icon_donor
            .as_ref()
            .or(recipe.presentation_donor.as_ref())
            .unwrap_or(&recipe.donor)
            .item_hash
            .parse_u32()
            .unwrap_or_default(),
        icon_edit: recipe.overrides.icon_edit,
        name: recipe.name,
        namespace: recipe.namespace,
    })
}
