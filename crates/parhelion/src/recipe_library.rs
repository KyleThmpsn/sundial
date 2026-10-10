//! Persistent, shareable Parhelion recipe library.

use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    WeaponRecipe,
    bundled_defaults::{self, AppliedVersions, DefaultsRefresh},
};

mod delete;
mod mutation;
mod restore;
mod shaders;
#[cfg(test)]
mod transactions_tests;
mod transfer;
pub(crate) use delete::DeleteRecipe;
pub(crate) use restore::RestoreDefaults;
pub(crate) use restore::RestoreRecipe;
pub(crate) use transfer::ImportReport;

const EVERY_END_FILE_NAME: &str = "every-end.parhelion.json";
const EVERY_END_TEMPLATE: &str = include_str!("../recipes/every-end.parhelion.json");
const SECOND_SUN_FILE_NAME: &str = "second-sun.parhelion.json";
const SECOND_SUN_TEMPLATE: &str = include_str!("../recipes/second-sun.parhelion.json");
pub(crate) const BUNDLED_RECIPES: [(&str, &str); 18] = [
    (EVERY_END_FILE_NAME, EVERY_END_TEMPLATE),
    (SECOND_SUN_FILE_NAME, SECOND_SUN_TEMPLATE),
    (
        "redacted.parhelion.json",
        include_str!("../recipes/redacted.parhelion.json"),
    ),
    (
        "june-ninth.parhelion.json",
        include_str!("../recipes/june-ninth.parhelion.json"),
    ),
    (
        "stay.parhelion.json",
        include_str!("../recipes/stay.parhelion.json"),
    ),
    (
        "still-here.parhelion.json",
        include_str!("../recipes/still-here.parhelion.json"),
    ),
    (
        "unsent.parhelion.json",
        include_str!("../recipes/unsent.parhelion.json"),
    ),
    (
        "last-watch.parhelion.json",
        include_str!("../recipes/last-watch.parhelion.json"),
    ),
    (
        "periapsis.parhelion.json",
        include_str!("../recipes/periapsis.parhelion.json"),
    ),
    (
        "holdover.parhelion.json",
        include_str!("../recipes/holdover.parhelion.json"),
    ),
    (
        "dead-air.parhelion.json",
        include_str!("../recipes/dead-air.parhelion.json"),
    ),
    (
        "night-shift.parhelion.json",
        include_str!("../recipes/night-shift.parhelion.json"),
    ),
    (
        "vaultbreaker.parhelion.json",
        include_str!("../recipes/vaultbreaker.parhelion.json"),
    ),
    (
        "good-company.parhelion.json",
        include_str!("../recipes/good-company.parhelion.json"),
    ),
    (
        "reclamation-order.parhelion.json",
        include_str!("../recipes/reclamation-order.parhelion.json"),
    ),
    (
        "hammer-time.parhelion.json",
        include_str!("../recipes/hammer-time.parhelion.json"),
    ),
    (
        "ravenous-horizon.parhelion.json",
        include_str!("../recipes/ravenous-horizon.parhelion.json"),
    ),
    (
        "suros-renaissance.parhelion.json",
        include_str!("../recipes/suros-renaissance.parhelion.json"),
    ),
];
const LIBRARY_STATE_SCHEMA: u32 = 1;
const LIBRARY_STATE_FILE_NAME: &str = "library-state.json";
const BUNDLED_VERSIONS_FILE_NAME: &str = "bundled-recipe-versions.json";
const REMOVED_BUNDLED_FILE_NAME: &str = "removed-bundled-recipes.txt";

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct RecipeLibraryState {
    schema: u32,
    known_bundled_recipes: BTreeSet<String>,
    enabled_recipes: BTreeSet<String>,
    #[serde(flatten)]
    extra: std::collections::BTreeMap<String, serde_json::Value>,
}

/// A badge a library recipe carries. Its artwork stays in the recipe file, named here by its
/// fingerprint, and [`load_badge`] reads it when someone picks the badge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryBadge {
    pub name: String,
    pub description: String,
    pub icon: Option<u64>,
}

/// One recipe of the library, as its lists and pickers read it. Artwork stays in the file, so a
/// library of illustrated recipes costs no more to hold than a plain one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeLibraryEntry {
    pub kind: crate::ItemKind,
    pub collection_destination: Option<crate::collection::Destination>,
    pub armor_class: Option<crate::ArmorClass>,
    pub badge: Option<LibraryBadge>,
    /// The release watermark's fingerprint. [`load_corner_icon`] reads the artwork.
    pub corner_icon: Option<u64>,
    pub path: PathBuf,
    pub name: String,
    pub namespace: String,
    pub bundled: bool,
    pub donor_hash: u32,
    /// The weapon whose type this recipe shows and files under in Collections.
    pub type_donor_hash: u32,
    /// The weapon this recipe builds, so another recipe can be recognized as building on it.
    pub identity_hash: u32,
    pub type_name: Option<String>,
    /// The type label a subclass's classes give it when it sets none, such as Guardian Subclass.
    pub class_type_name: Option<&'static str>,
    pub ammo_type: Option<crate::RecipeAmmoType>,
    pub damage_type: Option<crate::recipe::RecipeDamageType>,
    pub rarity: Option<crate::RecipeRarity>,
    pub icon_hash: u32,
    pub icon_edit: crate::WeaponIconEdit,
    /// The art the icon shows in place of its image: a vehicle's silhouette or a generated icon.
    pub art: Option<crate::icon_art::Art>,
}

/// Reads the badge of the recipe at `path`, artwork included.
pub(crate) fn load_badge(path: &Path) -> Result<Option<crate::presentation::Badge>, String> {
    WeaponRecipe::load_json(path)
        .map(|recipe| recipe.overrides.badge)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Reads the release watermark of the recipe at `path`.
pub(crate) fn load_corner_icon(
    path: &Path,
) -> Result<Option<crate::presentation::Artwork>, String> {
    WeaponRecipe::load_json(path)
        .map(|recipe| recipe.overrides.corner_icon)
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecipeLibraryScan {
    pub entries: Vec<RecipeLibraryEntry>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeLibrary {
    root: PathBuf,
    canonical_root: PathBuf,
    refresh: Option<DefaultsRefresh>,
}

impl RecipeLibrary {
    pub fn open_default() -> Result<Self, String> {
        let root = sundial::package_authoring::parhelion_recipe_library_directory()
            .ok_or_else(|| "Could not locate Sundial's per-user data directory".to_owned())?;
        Self::open(root)
    }

    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|error| {
            format!(
                "Could not create recipe library {}: {error}",
                root.display()
            )
        })?;
        let canonical_root = fs::canonicalize(&root).map_err(|error| {
            format!(
                "Could not resolve recipe library {}: {error}",
                root.display()
            )
        })?;
        let mut library = Self {
            root,
            canonical_root,
            refresh: None,
        };
        library.refresh = library.materialize_bundled_recipes()?;
        Ok(library)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Bundled recipes replaced while opening, because a release changed them.
    #[must_use]
    pub(crate) fn defaults_refresh(&self) -> Option<&DefaultsRefresh> {
        self.refresh.as_ref()
    }

    pub fn scan(&self) -> Result<RecipeLibraryScan, String> {
        let mut paths = self.recipe_paths()?;
        paths.sort_by_cached_key(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default()
        });

        let mut scan = RecipeLibraryScan::default();
        for path in paths {
            match WeaponRecipe::load_json(&path) {
                Ok(recipe) => scan.entries.push(RecipeLibraryEntry {
                    // Read first, while the fields below have not yet moved out of the recipe.
                    art: crate::icon_art::Art::of(&recipe),
                    kind: recipe.kind,
                    collection_destination: recipe.overrides.collection_destination,
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
                    path,
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
                }),
                Err(error) => scan.errors.push(format!("{}: {error}", path.display())),
            }
        }
        scan.entries.sort_by_cached_key(|entry| {
            (
                entry.name.to_ascii_lowercase(),
                entry.namespace.to_ascii_lowercase(),
                entry.path.clone(),
            )
        });
        Ok(scan)
    }

    pub fn enabled_paths(
        &self,
        entries: &[RecipeLibraryEntry],
    ) -> Result<BTreeSet<PathBuf>, String> {
        let _lock = self.lock()?;
        let (mut state, original) = self.read_state()?;
        let mut changed = false;
        for entry in entries.iter().filter(|entry| entry.bundled) {
            let file_name = entry_file_name(entry)?;
            if state.known_bundled_recipes.insert(file_name.clone()) {
                state.enabled_recipes.insert(file_name);
                changed = true;
            }
        }
        // Discovery is not a selection edit: an unreadable or temporarily missing
        // recipe must not lose its saved membership. Return only usable entries.
        if changed || original.is_none() {
            self.write_state_checked(&state, original.as_deref())?;
        }
        Ok(entries
            .iter()
            .filter_map(|entry| {
                entry_file_name(entry)
                    .ok()
                    .filter(|name| state.enabled_recipes.contains(name))
                    .map(|_| entry.path.clone())
            })
            .collect())
    }

    pub fn save_enabled_paths(
        &self,
        paths: &BTreeSet<PathBuf>,
        visible_entries: &[RecipeLibraryEntry],
    ) -> Result<(), String> {
        let _lock = self.lock()?;
        let (mut state, original) = self.read_state()?;
        let selected: BTreeSet<String> = paths
            .iter()
            .map(|path| {
                path.strip_prefix(&self.root)
                    .ok()
                    .filter(|relative| relative.components().count() == 1)
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        format!(
                            "Enabled recipe is not a direct library entry: {}",
                            path.display()
                        )
                    })
            })
            .collect::<Result<_, _>>()?;
        // Only replace membership for recipes the user could actually select.
        // Missing/malformed entries retain their saved membership until repaired.
        for entry in visible_entries {
            state.enabled_recipes.remove(&entry_file_name(entry)?);
        }
        state.enabled_recipes.extend(selected);
        self.write_state_checked(&state, original.as_deref())
    }

    pub fn save_new(&self, recipe: &WeaponRecipe) -> Result<PathBuf, String> {
        let _lock = self.lock()?;
        self.save_new_locked(recipe)
    }

    fn save_new_locked(&self, recipe: &WeaponRecipe) -> Result<PathBuf, String> {
        self.ensure_unique_identity(recipe, None)?;
        let base = recipe.slug();
        for suffix in 0..=u16::MAX {
            let file_name = if suffix == 0 {
                format!("{base}.parhelion.json")
            } else {
                format!("{base}-{suffix}.parhelion.json")
            };
            let path = self.root.join(file_name);
            match write_recipe_create_new(&path, recipe) {
                Ok(()) => return Ok(path),
                Err(WriteNewError::AlreadyExists) => {}
                Err(WriteNewError::Other(error)) => return Err(error),
            }
        }
        Err(format!(
            "Could not allocate a unique recipe filename for {:?}",
            recipe.name
        ))
    }

    #[cfg(test)]
    pub fn save_existing(&self, path: &Path, recipe: &WeaponRecipe) -> Result<(), String> {
        self.save_existing_checked(path, recipe, None)
    }

    /// Serializes library writers and checks external edits again before publication.
    /// Formatting-only changes since opening are harmless. Semantic edits need reconciliation.
    pub fn save_existing_if_unchanged(
        &self,
        path: &Path,
        baseline: &WeaponRecipe,
        recipe: &WeaponRecipe,
    ) -> Result<(), String> {
        self.save_existing_checked(path, recipe, Some(baseline))
    }

    fn save_existing_checked(
        &self,
        path: &Path,
        recipe: &WeaponRecipe,
        baseline: Option<&WeaponRecipe>,
    ) -> Result<(), String> {
        let _lock = self.lock()?;
        let target = self.confined_existing_path(path)?;
        self.ensure_unique_identity(recipe, Some(&target))?;
        let encoded = encoded_recipe(recipe)?;
        let original = fs::read(&target)
            .map_err(|error| format!("Could not read the recipe before saving: {error}"))?;
        if let Some(baseline) = baseline {
            let current = WeaponRecipe::from_json_str(
                std::str::from_utf8(&original)
                    .map_err(|error| format!("Could not read the saved recipe: {error}"))?,
            )
            .map_err(|error| {
                format!("Could not check the saved recipe before replacing it: {error}")
            })?;
            if !current.same_saved_content(baseline) {
                return Err("This recipe changed on disk after you opened it. Your draft is still here. Export it to a separate file, then reopen the library recipe to reconcile the changes.".into());
            }
        }
        mutation::publish(&target, encoded.as_bytes(), Some(&original))
    }

    pub fn import(&self, source: &Path) -> Result<(PathBuf, WeaponRecipe), String> {
        let recipe = WeaponRecipe::load_json(source)
            .map_err(|error| format!("Could not import recipe {}: {error}", source.display()))?;
        let destination = self.save_new(&recipe)?;
        Ok((destination, recipe))
    }

    /// Adds missing bundled recipes. A copy left over from an earlier release is backed up
    /// and replaced; a copy edited under the current release is kept.
    fn materialize_bundled_recipes(&self) -> Result<Option<DefaultsRefresh>, String> {
        let _lock = self.lock()?;
        let mut versions = AppliedVersions::load(self.versions_path()?)?;
        // An unreadable record only means a deleted bundled recipe comes back, which is
        // worth less than opening the library at all.
        let removed = self.removed_bundled().unwrap_or_default();
        let mut stale = Vec::new();
        for (file_name, encoded) in BUNDLED_RECIPES {
            let recipe = WeaponRecipe::from_json_str(encoded)
                .map_err(|error| format!("Bundled recipe {file_name} is invalid: {error}"))?;
            let digest = bundled_defaults::digest(encoded.as_bytes());
            let path = self.root.join(file_name);
            // A bundled recipe that was deleted stays deleted until defaults are restored.
            if removed.contains(file_name) && fs::symlink_metadata(&path).is_err() {
                continue;
            }
            match atomic_write_create_new(&path, encoded.as_bytes()) {
                Ok(()) => {
                    versions.record(file_name, digest);
                    continue;
                }
                Err(WriteNewError::AlreadyExists) => {}
                Err(WriteNewError::Other(error)) => return Err(error),
            }
            if versions.is_current(file_name, &digest) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                format!(
                    "Could not inspect bundled recipe {}: {error}",
                    path.display()
                )
            })?;
            if !metadata.is_file() {
                continue;
            }
            let current = fs::read(&path).map_err(|error| {
                format!("Could not read bundled recipe {}: {error}", path.display())
            })?;
            if current == encoded.as_bytes() {
                versions.record(file_name, digest);
                continue;
            }
            // Only edits worth keeping become a duplicate; a re-saved or unreadable copy
            // is preserved by the backup alone.
            let edited =
                WeaponRecipe::from_json_str(std::str::from_utf8(&current).unwrap_or_default())
                    .ok()
                    .filter(|edited| *edited != recipe);
            stale.push((file_name, encoded, digest, current, recipe, edited));
        }
        let refresh = if stale.is_empty() {
            None
        } else if let Ok(backup) = self.create_restore_backup() {
            let mut names = Vec::new();
            let mut copies = Vec::new();
            for (file_name, encoded, digest, current, recipe, edited) in stale {
                if bundled_defaults::back_up(&backup, file_name, &current).is_err() {
                    continue;
                }
                // Keeping the reader's edits comes first, and a default whose edits cannot be
                // kept is left exactly as it is. Refreshing it is worth nothing beside losing
                // their work, and it must never cost them the library: this runs while it opens,
                // so returning here would leave them with no recipes at all. The next launch
                // tries again, because nothing is recorded for a default that was not replaced.
                let edited_copy = if let Some(edited) = edited {
                    let Ok((copy, copied_recipe)) = self.duplicate_locked(&edited) else {
                        continue;
                    };
                    let copied_bytes = encoded_recipe(&copied_recipe)?;
                    Some((copy, copied_recipe.name, copied_bytes))
                } else {
                    None
                };
                if mutation::publish(
                    &self.root.join(file_name),
                    encoded.as_bytes(),
                    Some(&current),
                )
                .is_err()
                {
                    // The copy was made for a refresh that did not happen. Left behind, the next
                    // launch would set another beside it.
                    if let Some((copy, _, bytes)) = &edited_copy {
                        let _ = mutation::remove(copy, bytes.as_bytes());
                    }
                    continue;
                }
                if let Some((_, name, _)) = edited_copy {
                    copies.push(name);
                }
                versions.record(file_name, digest);
                names.push(recipe.name);
            }
            (!names.is_empty()).then_some(DefaultsRefresh {
                names,
                copies,
                backup,
            })
        } else {
            // Opening the library still works when its backup folder cannot be created.
            // Leave every stale default intact and retry the refresh on a later launch.
            None
        };
        // The record is an optimisation, not the result: failing to write it costs one repeated
        // comparison next launch, which then finds the defaults already current. Opening the
        // library is worth more than recording that this ran.
        let _ = versions.save();
        Ok(refresh)
    }

    fn versions_path(&self) -> Result<PathBuf, String> {
        self.state_path()
            .map(|state| state.with_file_name(BUNDLED_VERSIONS_FILE_NAME))
    }

    fn check_root(&self) -> Result<(), String> {
        let current = fs::canonicalize(&self.root)
            .map_err(|error| format!("Could not resolve the recipe library: {error}"))?;
        if current != self.canonical_root {
            return Err("The recipe library location changed. Reopen the library first.".into());
        }
        Ok(())
    }

    fn lock(&self) -> Result<fs::File, String> {
        self.check_root()?;
        let lock = sundial::storage::try_lock_file(&self.canonical_root.join(".library.lock"))
            .map_err(|error| format!("Could not acquire the recipe library write lock. Retry after other library operations finish: {error}"))?;
        self.check_root()?;
        Ok(lock)
    }

    fn confined_existing_path(&self, path: &Path) -> Result<PathBuf, String> {
        let relative = path.strip_prefix(&self.root).map_err(|_| {
            format!(
                "Refusing to save outside the recipe library: {}",
                path.display()
            )
        })?;
        let mut components = relative.components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(format!(
                "Refusing to save a non-direct recipe library path: {}",
                path.display()
            ));
        }
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            format!(
                "Could not inspect library recipe {}: {error}",
                path.display()
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "Refusing to replace a symlink or non-file recipe: {}",
                path.display()
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| format!("Recipe has no parent directory: {}", path.display()))?;
        let canonical_parent = fs::canonicalize(parent).map_err(|error| {
            format!(
                "Could not resolve recipe parent {}: {error}",
                parent.display()
            )
        })?;
        if canonical_parent != self.canonical_root {
            return Err(format!(
                "Refusing to save through a path outside the recipe library: {}",
                path.display()
            ));
        }
        fs::canonicalize(path).map_err(|error| {
            format!(
                "Could not resolve library recipe {}: {error}",
                path.display()
            )
        })
    }

    fn ensure_unique_identity(
        &self,
        candidate: &WeaponRecipe,
        excluded_path: Option<&Path>,
    ) -> Result<(), String> {
        candidate
            .validate()
            .map_err(|error| format!("Recipe is invalid: {error}"))?;
        let candidate_hashes = candidate
            .identity
            .parsed_hashes(&candidate.namespace)
            .map_err(|error| format!("Recipe identity is invalid: {error}"))?
            .into_iter()
            .collect::<BTreeSet<_>>();
        for path in self.recipe_paths()? {
            if excluded_path.is_some_and(|excluded| {
                fs::canonicalize(&path).is_ok_and(|existing| existing == excluded)
            }) {
                continue;
            }
            let existing = WeaponRecipe::load_json(&path).map_err(|error| {
                format!(
                    "Could not verify recipe identity uniqueness because {} is invalid: {error}",
                    path.display()
                )
            })?;
            if existing
                .namespace
                .eq_ignore_ascii_case(&candidate.namespace)
            {
                return Err(format!(
                    "Recipe namespace {:?} is already used by {}",
                    candidate.namespace,
                    path.display()
                ));
            }
            let existing_hashes = existing
                .identity
                .parsed_hashes(&existing.namespace)
                .map_err(|error| format!("Invalid identity in {}: {error}", path.display()))?
                .into_iter()
                .collect::<BTreeSet<_>>();
            if let Some(collision) = candidate_hashes.intersection(&existing_hashes).next() {
                return Err(format!(
                    "Recipe identity hash 0x{collision:08X} is already used by {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    /// Discovery and collision checks must see the same files and the same failures.
    fn recipe_paths(&self) -> Result<Vec<PathBuf>, String> {
        self.check_root()?;
        let entries = fs::read_dir(&self.root).map_err(|error| {
            format!(
                "Could not read recipe library {}: {error}",
                self.root.display()
            )
        })?;
        collect_recipe_paths(entries.map(|entry| entry.map(|entry| entry.path())))
    }

    fn state_path(&self) -> Result<PathBuf, String> {
        self.root
            .parent()
            .map(|parent| parent.join(LIBRARY_STATE_FILE_NAME))
            .ok_or_else(|| {
                format!(
                    "Recipe library {} has no state directory",
                    self.root.display()
                )
            })
    }

    fn read_state(&self) -> Result<(RecipeLibraryState, Option<Vec<u8>>), String> {
        let path = self.state_path()?;
        let original = mutation::read_optional(&path)?;
        let Some(encoded) = &original else {
            return Ok((
                RecipeLibraryState {
                    schema: LIBRARY_STATE_SCHEMA,
                    ..RecipeLibraryState::default()
                },
                None,
            ));
        };
        let state: RecipeLibraryState =
            sundial::package_authoring::read_json(encoded.as_slice())
                .map_err(|error| format!("Could not decode {}: {error}", path.display()))?;
        if state.schema != LIBRARY_STATE_SCHEMA {
            return Err(format!(
                "Unsupported recipe library state schema {}. Expected {LIBRARY_STATE_SCHEMA}",
                state.schema
            ));
        }
        Ok((state, original))
    }

    #[cfg(test)]
    fn write_state(&self, state: &RecipeLibraryState) -> Result<(), String> {
        let _lock = self.lock()?;
        let path = self.state_path()?;
        let original = mutation::read_optional(&path)?;
        self.write_state_checked(state, original.as_deref())
    }

    fn write_state_checked(
        &self,
        state: &RecipeLibraryState,
        original: Option<&[u8]>,
    ) -> Result<(), String> {
        mutation::publish(&self.state_path()?, &encode_state(state)?, original)
    }
}

fn encode_state(state: &RecipeLibraryState) -> Result<Vec<u8>, String> {
    let mut encoded = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("Could not encode recipe library state: {error}"))?;
    encoded.push(b'\n');
    Ok(encoded)
}

fn entry_file_name(entry: &RecipeLibraryEntry) -> Result<String, String> {
    entry
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("Recipe has no UTF-8 filename: {}", entry.path.display()))
}

enum WriteNewError {
    AlreadyExists,
    Other(String),
}

fn write_recipe_create_new(path: &Path, recipe: &WeaponRecipe) -> Result<(), WriteNewError> {
    let encoded = encoded_recipe(recipe).map_err(WriteNewError::Other)?;
    atomic_write_create_new(path, encoded.as_bytes())
}

fn encoded_recipe(recipe: &WeaponRecipe) -> Result<String, String> {
    let mut encoded = recipe.to_json_pretty().map_err(|error| error.to_string())?;
    encoded.push('\n');
    Ok(encoded)
}

/// A failed directory entry or metadata read is not evidence that a recipe is absent.
/// Keep this iterator boundary testable without relying on OS permission behavior.
fn collect_recipe_paths(
    entries: impl IntoIterator<Item = std::io::Result<PathBuf>>,
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for entry in entries {
        let path =
            entry.map_err(|error| format!("Could not read a recipe library entry: {error}"))?;
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            continue;
        }
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Could not inspect recipe {}: {error}", path.display()))?;
        if metadata.is_file() {
            paths.push(path);
        }
    }
    Ok(paths)
}

#[cfg(test)]
fn atomic_write_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    sundial::package_authoring::replace_authoring_file(path, bytes)
        .map_err(|error| format!("Could not atomically replace {}: {error}", path.display(),))
}

fn atomic_write_create_new(path: &Path, bytes: &[u8]) -> Result<(), WriteNewError> {
    sundial::storage::create_file(path, bytes).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            WriteNewError::AlreadyExists
        } else {
            WriteNewError::Other(format!(
                "Could not atomically create {}: {error}",
                path.display()
            ))
        }
    })
}

#[cfg(test)]
mod tests;
