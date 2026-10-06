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
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn exporting_through_a_link_and_parent_cannot_overwrite_a_library_recipe() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("library")).unwrap();
        let original = library.scan().unwrap().entries[0].path.clone();
        let bytes = fs::read(&original).unwrap();
        let child = library.root().join("child");
        fs::create_dir(&child).unwrap();
        let alias = directory.path().join("alias");
        std::os::unix::fs::symlink(child, &alias).unwrap();
        let target = alias.join("..").join(original.file_name().unwrap());
        let mut edited = WeaponRecipe::load_json(&original).unwrap();
        edited.flavor = "Must not replace the saved recipe".into();
        assert!(
            library
                .export(&edited, &target)
                .unwrap_err()
                .contains("outside")
        );
        assert_eq!(fs::read(&original).unwrap(), bytes);
        let outside = directory.path().join("export.parhelion.json");
        library.export(&edited, &outside).unwrap();
        assert_eq!(WeaponRecipe::load_json(&outside).unwrap(), edited);
    }

    #[test]
    fn selection_edits_preserve_membership_of_unreadable_recipes() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let scan = library.scan().unwrap();
        let enabled = library.enabled_paths(&scan.entries).unwrap();
        let broken = enabled.iter().next().unwrap();
        let original = fs::read(broken).unwrap();
        fs::write(broken, b"{").unwrap();
        // Deselect everything visible, without touching the hidden broken recipe.
        let visible = library.scan().unwrap().entries;
        library
            .save_enabled_paths(&BTreeSet::new(), &visible)
            .unwrap();
        fs::write(broken, original).unwrap();
        let scan = library.scan().unwrap();
        assert_eq!(
            library.enabled_paths(&scan.entries).unwrap(),
            BTreeSet::from([broken.clone()])
        );
    }

    #[test]
    fn discovery_errors_are_not_reported_as_an_empty_library() {
        let error = collect_recipe_paths([Err(std::io::Error::other("directory read failed"))])
            .unwrap_err();
        assert!(error.contains("directory read failed"));
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing.parhelion.json");
        let error = collect_recipe_paths([Ok(missing.clone())]).unwrap_err();
        assert!(error.contains(&missing.display().to_string()));
    }

    #[test]
    fn unreadable_selection_state_is_not_replaced_with_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let state_path = library.state_path().unwrap();
        fs::create_dir(&state_path).unwrap();
        assert!(library.read_state().is_err());
        assert!(
            library
                .enabled_paths(&library.scan().unwrap().entries)
                .is_err()
        );
        assert!(state_path.is_dir());
    }

    #[test]
    fn library_preview_tracks_authored_icon_and_its_inheritance() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let path = library.root().join(EVERY_END_FILE_NAME);
        let mut recipe = WeaponRecipe::load_json(&path).unwrap();
        for source in [0, 1, 2] {
            recipe.presentation_donor = (source > 0).then(|| crate::WeaponDonorReference {
                item_hash: 0x1234ABCD.into(),
                expected_name: None,
            });
            recipe.icon_donor = (source > 1).then(|| crate::WeaponDonorReference {
                item_hash: 0x5678ABCD.into(),
                expected_name: None,
            });
            recipe.overrides.icon_edit.hue_shift_degrees = 45;
            library.save_existing(&path, &recipe).unwrap();
            let scan = library.scan().unwrap();
            let entry = scan
                .entries
                .iter()
                .find(|entry| entry.path == path)
                .unwrap();
            assert_eq!(
                entry.icon_hash,
                match source {
                    0 => recipe.donor.item_hash.parse_u32().unwrap(),
                    1 => 0x1234ABCD,
                    _ => 0x5678ABCD,
                }
            );
            assert_eq!(entry.icon_edit, recipe.overrides.icon_edit);
        }
    }

    #[test]
    fn bundled_recipes_validate_with_unique_identities() {
        let mut namespaces = BTreeSet::new();
        let mut hashes = BTreeSet::new();
        for (file_name, json) in BUNDLED_RECIPES {
            let recipe = WeaponRecipe::from_json_str(json).unwrap();
            let spec = recipe.to_spec().unwrap();
            match file_name {
                EVERY_END_FILE_NAME => {
                    assert_eq!(recipe.namespace, "parhelion.every-end");
                    assert!(!recipe.identity_is_name_derived());
                    assert_eq!(spec.identity.item_hash, 0x5355_4E44);
                }
                SECOND_SUN_FILE_NAME => {
                    assert!(recipe.identity_is_name_derived());
                    assert_eq!(
                        (
                            spec.identity.item_hash,
                            spec.identity.collectible_hash,
                            spec.identity.unlock_hash
                        ),
                        (0x757D_33F5, 0xD347_B59A, 0xF137_B0F4),
                    );
                }
                _ => {}
            }
            assert!(namespaces.insert(recipe.namespace.clone()));
            for hash in recipe.identity.parsed_hashes(&recipe.namespace).unwrap() {
                assert!(
                    hashes.insert(hash),
                    "duplicate identity in {file_name}: {hash:08X}"
                );
            }
        }
    }

    #[test]
    fn new_bundles_are_enabled_without_reenabling_disabled_old_bundles() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let state = RecipeLibraryState {
            schema: LIBRARY_STATE_SCHEMA,
            known_bundled_recipes: BTreeSet::from([
                EVERY_END_FILE_NAME.into(),
                SECOND_SUN_FILE_NAME.into(),
            ]),
            enabled_recipes: BTreeSet::from([EVERY_END_FILE_NAME.into()]),
            ..RecipeLibraryState::default()
        };
        library.write_state(&state).unwrap();
        let scan = library.scan().unwrap();
        let enabled = library.enabled_paths(&scan.entries).unwrap();
        assert_eq!(enabled.len(), BUNDLED_RECIPES.len() - 1);
        assert!(!enabled.contains(&library.root().join(SECOND_SUN_FILE_NAME)));
        assert_eq!(library.enabled_paths(&scan.entries).unwrap(), enabled);
    }

    #[test]
    fn reopening_keeps_user_edits_while_the_bundled_recipe_is_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("recipes");
        let library = RecipeLibrary::open(root.clone()).unwrap();
        assert!(library.defaults_refresh().is_none());
        let path = library.root().join(EVERY_END_FILE_NAME);
        fs::write(&path, "user-owned edit").unwrap();

        let library = RecipeLibrary::open(root).unwrap();

        assert!(library.defaults_refresh().is_none());
        assert_eq!(fs::read_to_string(path).unwrap(), "user-owned edit");
    }

    /// Simulates a copy left by an earlier release by dropping its applied version.
    fn forget_applied_version(library: &RecipeLibrary, file_name: &str) {
        let path = library.versions_path().unwrap();
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["applied"].as_object_mut().unwrap().remove(file_name);
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    }

    #[test]
    fn a_changed_bundled_recipe_replaces_the_stale_copy_and_keeps_edits_as_a_duplicate() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("recipes");
        let library = RecipeLibrary::open(root.clone()).unwrap();
        let path = library.root().join(EVERY_END_FILE_NAME);
        let mut edited = WeaponRecipe::load_json(&path).unwrap();
        edited.flavor = "My own story.".into();
        library.save_existing(&path, &edited).unwrap();
        let edited_bytes = fs::read(&path).unwrap();
        let custom = WeaponRecipe::new_weapon("parhelion.untouched").unwrap();
        let custom_path = library.save_new(&custom).unwrap();
        forget_applied_version(&library, EVERY_END_FILE_NAME);

        let library = RecipeLibrary::open(root.clone()).unwrap();

        let refresh = library.defaults_refresh().unwrap();
        assert_eq!(refresh.names, vec![edited.name.clone()]);
        assert_eq!(refresh.copies, vec![format!("{} Copy", edited.name)]);
        assert_eq!(fs::read(&path).unwrap(), EVERY_END_TEMPLATE.as_bytes());
        assert_eq!(
            fs::read(refresh.backup.join(EVERY_END_FILE_NAME)).unwrap(),
            edited_bytes
        );
        assert_eq!(WeaponRecipe::load_json(custom_path).unwrap(), custom);
        let scan = library.scan().unwrap();
        let copy = scan
            .entries
            .iter()
            .find(|entry| entry.name == format!("{} Copy", edited.name))
            .expect("the edited copy is kept in the library");
        assert!(!copy.bundled);
        let copy = WeaponRecipe::load_json(&copy.path).unwrap();
        assert_eq!(copy.flavor, edited.flavor);
        assert_ne!(copy.namespace, edited.namespace);

        assert!(
            RecipeLibrary::open(root)
                .unwrap()
                .defaults_refresh()
                .is_none()
        );
    }

    #[test]
    fn an_unavailable_backup_folder_does_not_block_the_recipe_library() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("recipes");
        let library = RecipeLibrary::open(root.clone()).unwrap();
        let path = library.root().join(EVERY_END_FILE_NAME);
        let mut edited = WeaponRecipe::load_json(&path).unwrap();
        edited.flavor = "Keep this edit.".into();
        library.save_existing(&path, &edited).unwrap();
        let original = fs::read(&path).unwrap();
        forget_applied_version(&library, EVERY_END_FILE_NAME);
        fs::write(directory.path().join("backups"), b"not a directory").unwrap();

        let reopened = RecipeLibrary::open(root).unwrap();

        assert!(reopened.defaults_refresh().is_none());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!reopened.scan().unwrap().entries.is_empty());
    }

    #[test]
    fn a_stale_copy_that_is_not_a_recipe_is_only_backed_up() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("recipes");
        let library = RecipeLibrary::open(root.clone()).unwrap();
        let path = library.root().join(EVERY_END_FILE_NAME);
        fs::write(&path, "not a recipe").unwrap();
        forget_applied_version(&library, EVERY_END_FILE_NAME);

        let library = RecipeLibrary::open(root).unwrap();

        let refresh = library.defaults_refresh().unwrap();
        assert!(refresh.copies.is_empty());
        assert_eq!(fs::read(&path).unwrap(), EVERY_END_TEMPLATE.as_bytes());
        assert_eq!(
            fs::read_to_string(refresh.backup.join(EVERY_END_FILE_NAME)).unwrap(),
            "not a recipe"
        );
        assert_eq!(library.scan().unwrap().entries.len(), BUNDLED_RECIPES.len());
    }

    #[test]
    fn a_library_without_applied_versions_refreshes_only_differing_copies() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("recipes");
        let library = RecipeLibrary::open(root.clone()).unwrap();
        let path = library.root().join(SECOND_SUN_FILE_NAME);
        fs::write(&path, "left by an older release").unwrap();
        fs::remove_file(library.versions_path().unwrap()).unwrap();

        let library = RecipeLibrary::open(root).unwrap();

        let refresh = library.defaults_refresh().unwrap();
        assert_eq!(refresh.names.len(), 1);
        assert_eq!(fs::read(&path).unwrap(), SECOND_SUN_TEMPLATE.as_bytes());
        assert!(library.versions_path().unwrap().is_file());
    }

    #[test]
    fn failed_recipe_discovery_preserves_selection_until_the_recipe_is_repaired() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let scan = library.scan().unwrap();
        let enabled = library.enabled_paths(&scan.entries).unwrap();
        let path = library.root().join(EVERY_END_FILE_NAME);
        let original = fs::read(&path).unwrap();
        let state_before = fs::read(library.state_path().unwrap()).unwrap();

        fs::write(&path, "incomplete edit").unwrap();
        let scan = library.scan().unwrap();
        assert_eq!(scan.errors.len(), 1);
        let available = library.enabled_paths(&scan.entries).unwrap();
        assert!(!available.contains(&path));
        assert_eq!(
            fs::read(library.state_path().unwrap()).unwrap(),
            state_before
        );

        fs::write(&path, original).unwrap();
        let scan = library.scan().unwrap();
        assert!(scan.errors.is_empty());
        assert_eq!(library.enabled_paths(&scan.entries).unwrap(), enabled);
    }

    #[test]
    fn save_new_and_import_never_overwrite_or_duplicate_an_identity() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let recipe = WeaponRecipe::new_weapon("parhelion.first").unwrap();
        let imported_recipe = WeaponRecipe::new_weapon("parhelion.imported").unwrap();
        let external = directory.path().join("shared.json");
        imported_recipe.save_json(&external).unwrap();

        let first = library.save_new(&recipe).unwrap();
        let (second, imported) = library.import(&external).unwrap();

        assert_ne!(first, second);
        assert_eq!(imported, imported_recipe);
        assert!(first.is_file());
        assert!(second.is_file());
        assert!(library.import(&external).is_err());
        assert_eq!(WeaponRecipe::load_json(&second).unwrap(), imported_recipe);
    }

    #[test]
    fn duplicate_allocates_a_fresh_copy_and_preserves_recipe_content() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
            "Library Copy Fixture",
            0x1234_5678,
            "Fixture Donor",
        )
        .unwrap();
        recipe.flavor = "A copied story.".into();
        recipe.overrides.ammo_type = Some(crate::RecipeAmmoType::Heavy);
        let original_path = library.save_new(&recipe).unwrap();

        let collision = WeaponRecipe::new_named_weapon_for_donor(
            "Library Copy Fixture Copy",
            0x1234_5678,
            "Fixture Donor",
        )
        .unwrap();
        library.save_new(&collision).unwrap();

        let copy_path = library.duplicate(&recipe).unwrap();
        let copy = WeaponRecipe::load_json(&copy_path).unwrap();
        assert_eq!(copy.name, "Library Copy Fixture Copy 2");
        assert_ne!(copy.namespace, recipe.namespace);
        assert_eq!(copy.donor, recipe.donor);
        assert_eq!(copy.overrides, recipe.overrides);
        assert_eq!(copy.flavor, recipe.flavor);
        assert_eq!(WeaponRecipe::load_json(original_path).unwrap(), recipe);
    }

    #[test]
    fn save_existing_is_confined_to_library_root() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let recipe = WeaponRecipe::new_weapon("parhelion.confined").unwrap();

        assert!(
            library
                .save_existing(&directory.path().join("outside.json"), &recipe)
                .is_err()
        );

        let every_end = library.root().join(EVERY_END_FILE_NAME);
        let traversal = library
            .root()
            .join("..")
            .join("recipes")
            .join(EVERY_END_FILE_NAME);
        assert!(library.save_existing(&traversal, &recipe).is_err());
        assert_eq!(
            WeaponRecipe::load_json(every_end).unwrap(),
            WeaponRecipe::every_end()
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_existing_rejects_a_symlinked_recipe() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let outside = directory.path().join("outside.json");
        WeaponRecipe::new_weapon("parhelion.outside")
            .unwrap()
            .save_json(&outside)
            .unwrap();
        let link = library.root().join("linked.parhelion.json");
        symlink(&outside, &link).unwrap();

        assert!(
            library
                .save_existing(
                    &link,
                    &WeaponRecipe::new_weapon("parhelion.replacement").unwrap()
                )
                .is_err()
        );
        assert_eq!(
            WeaponRecipe::load_json(outside).unwrap().namespace,
            "parhelion.outside"
        );
    }

    #[test]
    fn save_rejects_duplicate_namespace_or_any_identity_hash() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let first = WeaponRecipe::new_weapon("parhelion.unique").unwrap();
        let first_path = library.save_new(&first).unwrap();

        let duplicate_namespace = WeaponRecipe::new_weapon("parhelion.unique").unwrap();
        assert!(library.save_new(&duplicate_namespace).is_err());

        let mut duplicate_hash = WeaponRecipe::new_weapon("parhelion.other").unwrap();
        duplicate_hash.identity.flavor_hash = first.identity.flavor_hash.clone();
        duplicate_hash.identity.name_hash = first.identity.name_hash.clone();
        duplicate_hash.identity.source_hash = first.identity.source_hash.clone();
        let error = library.save_new(&duplicate_hash).unwrap_err();
        assert!(error.contains("identity hash"), "{error}");

        assert_eq!(WeaponRecipe::load_json(first_path).unwrap(), first);
    }

    #[test]
    fn save_existing_excludes_only_itself_from_collision_checks() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let mut first = WeaponRecipe::new_weapon("parhelion.first-existing").unwrap();
        let first_path = library.save_new(&first).unwrap();
        let second = WeaponRecipe::new_weapon("parhelion.second-existing").unwrap();
        let second_path = library.save_new(&second).unwrap();

        first.flavor = "A safe in-place edit.".to_owned();
        library.save_existing(&first_path, &first).unwrap();
        assert_eq!(WeaponRecipe::load_json(&first_path).unwrap(), first);

        let before = fs::read(&first_path).unwrap();
        first.namespace = second.namespace.clone();
        assert!(library.save_existing(&first_path, &first).is_err());
        assert_eq!(fs::read(first_path).unwrap(), before);
        assert_eq!(WeaponRecipe::load_json(second_path).unwrap(), second);
    }

    #[test]
    fn generated_recipe_filenames_are_bounded_and_windows_safe() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let mut reserved = WeaponRecipe::new_weapon("parhelion.reserved-filename").unwrap();
        reserved.name = "CON".to_owned();
        let reserved_path = library.save_new(&reserved).unwrap();
        assert!(
            !reserved_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .split('.')
                .next()
                .unwrap()
                .eq_ignore_ascii_case("CON")
        );
        assert_eq!(WeaponRecipe::load_json(&reserved_path).unwrap(), reserved);

        let mut long = WeaponRecipe::new_weapon("parhelion.long-filename").unwrap();
        long.name = "a".repeat(500);
        let long_path = library.save_new(&long).unwrap();
        let file_name = long_path.file_name().unwrap().to_string_lossy();
        assert!(file_name.len() <= 255);
        assert!(file_name.ends_with(".parhelion.json"));

        let mut same_long_name = WeaponRecipe::new_weapon("parhelion.long-filename-two").unwrap();
        same_long_name.name = "a".repeat(500);
        let suffixed_path = library.save_new(&same_long_name).unwrap();
        let suffixed_name = suffixed_path.file_name().unwrap().to_string_lossy();
        assert!(suffixed_name.len() <= 255);
        assert!(suffixed_name.ends_with(".parhelion.json"));
        assert_ne!(suffixed_path, long_path);
        assert_eq!(WeaponRecipe::load_json(&long_path).unwrap(), long);
        assert_eq!(
            WeaponRecipe::load_json(&suffixed_path).unwrap(),
            same_long_name
        );
    }

    #[test]
    fn disabling_bundled_recipe_survives_library_restart() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("recipes");
        let library = RecipeLibrary::open(root.clone()).unwrap();
        let scan = library.scan().unwrap();
        let mut enabled = library.enabled_paths(&scan.entries).unwrap();
        let every_end = scan
            .entries
            .iter()
            .find(|entry| entry.namespace == "parhelion.every-end")
            .unwrap();
        enabled.remove(&every_end.path);
        library.save_enabled_paths(&enabled, &scan.entries).unwrap();

        let reopened = RecipeLibrary::open(root).unwrap();
        let reopened_scan = reopened.scan().unwrap();
        let reopened_enabled = reopened.enabled_paths(&reopened_scan.entries).unwrap();

        assert_eq!(reopened_enabled, enabled);
        assert!(!reopened_enabled.contains(&every_end.path));
    }
}
