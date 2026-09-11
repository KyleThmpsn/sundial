//! Persistent, shareable Parhelion recipe library.

use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::WeaponRecipe;

mod restore;
mod transfer;
pub(crate) use restore::RestoreDefaults;
pub(crate) use restore::RestoreRecipe;
pub(crate) use transfer::ImportReport;

const EVERY_END_FILE_NAME: &str = "every-end.parhelion.json";
const EVERY_END_TEMPLATE: &str = include_str!("../recipes/every-end.parhelion.json");
const SECOND_SUN_FILE_NAME: &str = "second-sun.parhelion.json";
const SECOND_SUN_TEMPLATE: &str = include_str!("../recipes/second-sun.parhelion.json");
pub(crate) const BUNDLED_RECIPES: [(&str, &str); 15] = [
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
];
const LIBRARY_STATE_SCHEMA: u32 = 1;
const LIBRARY_STATE_FILE_NAME: &str = "library-state.json";

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RecipeLibraryState {
    schema: u32,
    known_bundled_recipes: BTreeSet<String>,
    enabled_recipes: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeLibraryEntry {
    pub collection_destination: Option<crate::collection::Destination>,
    pub badge: Option<crate::presentation::Badge>,
    pub corner_icon: Option<crate::presentation::Artwork>,
    pub path: PathBuf,
    pub name: String,
    pub namespace: String,
    pub bundled: bool,
    pub donor_hash: u32,
    pub type_name: Option<String>,
    pub ammo_type: Option<crate::RecipeAmmoType>,
    pub damage_type: Option<crate::recipe::RecipeDamageType>,
    pub rarity: Option<crate::RecipeRarity>,
    pub icon_hash: u32,
    pub icon_edit: crate::WeaponIconEdit,
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
        let library = Self {
            root,
            canonical_root,
        };
        library.materialize_bundled_recipes()?;
        Ok(library)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
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
                    collection_destination: recipe.overrides.collection_destination,
                    badge: recipe.overrides.badge.clone(),
                    corner_icon: recipe.overrides.corner_icon.clone(),
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
                    type_name: recipe.type_name,
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
        let mut state = self.load_state()?;
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
        if changed || !self.state_path()?.is_file() {
            self.write_state(&state)?;
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
        let mut state = self.load_state()?;
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
        self.write_state(&state)
    }

    pub fn save_new(&self, recipe: &WeaponRecipe) -> Result<PathBuf, String> {
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

    /// Best-effort optimistic conflict check for an open document, not an OS-wide lock.
    /// Formatting-only external changes are harmless; semantic edits must be reconciled.
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
        let target = self.confined_existing_path(path)?;
        self.ensure_unique_identity(recipe, Some(&target))?;
        let encoded = encoded_recipe(recipe)?;
        if let Some(baseline) = baseline {
            let current = WeaponRecipe::load_json(&target).map_err(|error| {
                format!("Could not check the saved recipe before replacing it: {error}")
            })?;
            if &current != baseline {
                return Err("This recipe changed on disk after you opened it. Your draft is still here. Export it to a separate file, then reopen the library recipe to reconcile the changes.".into());
            }
        }
        atomic_write_replace(&target, encoded.as_bytes())
    }

    pub fn import(&self, source: &Path) -> Result<(PathBuf, WeaponRecipe), String> {
        let recipe = WeaponRecipe::load_json(source)
            .map_err(|error| format!("Could not import recipe {}: {error}", source.display()))?;
        let destination = self.save_new(&recipe)?;
        Ok((destination, recipe))
    }

    fn materialize_bundled_recipes(&self) -> Result<(), String> {
        for (file_name, encoded) in BUNDLED_RECIPES {
            WeaponRecipe::from_json_str(encoded)
                .map_err(|error| format!("Bundled recipe {file_name} is invalid: {error}"))?;
            let path = self.root.join(file_name);
            match atomic_write_create_new(&path, encoded.as_bytes()) {
                Ok(()) | Err(WriteNewError::AlreadyExists) => {}
                Err(WriteNewError::Other(error)) => return Err(error),
            }
        }
        Ok(())
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

    fn load_state(&self) -> Result<RecipeLibraryState, String> {
        let path = self.state_path()?;
        let encoded = match fs::read_to_string(&path) {
            Ok(encoded) => encoded,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RecipeLibraryState {
                    schema: LIBRARY_STATE_SCHEMA,
                    ..RecipeLibraryState::default()
                });
            }
            Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
        };
        let state = sundial::package_authoring::parse_json::<RecipeLibraryState>(&encoded)
            .map_err(|error| format!("Could not decode {}: {error}", path.display()))?;
        if state.schema != LIBRARY_STATE_SCHEMA {
            return Err(format!(
                "Unsupported recipe library state schema {}; expected {LIBRARY_STATE_SCHEMA}",
                state.schema
            ));
        }
        Ok(state)
    }

    fn write_state(&self, state: &RecipeLibraryState) -> Result<(), String> {
        let path = self.state_path()?;
        let mut encoded = serde_json::to_string_pretty(state)
            .map_err(|error| format!("Could not encode recipe library state: {error}"))?;
        encoded.push('\n');
        atomic_write_replace(&path, encoded.as_bytes())
    }
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

fn write_temp_file(parent: &Path, bytes: &[u8]) -> Result<NamedTempFile, String> {
    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|error| format!("Could not create a temporary recipe file: {error}"))?;
    temporary
        .write_all(bytes)
        .map_err(|error| format!("Could not write a temporary recipe file: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("Could not flush a temporary recipe file: {error}"))?;
    Ok(temporary)
}

fn atomic_write_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    sundial::package_authoring::replace_authoring_file(path, bytes)
        .map_err(|error| format!("Could not atomically replace {}: {error}", path.display(),))
}

fn atomic_write_create_new(path: &Path, bytes: &[u8]) -> Result<(), WriteNewError> {
    let parent = path.parent().ok_or_else(|| {
        WriteNewError::Other(format!("Path has no parent directory: {}", path.display()))
    })?;
    let temporary = write_temp_file(parent, bytes).map_err(WriteNewError::Other)?;
    temporary
        .persist_noclobber(path)
        .map(|_| ())
        .map_err(|error| {
            if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                WriteNewError::AlreadyExists
            } else {
                WriteNewError::Other(format!(
                    "Could not atomically create {}: {}",
                    path.display(),
                    error.error
                ))
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_override_does_not_invent_a_damage_override() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let mut recipe = WeaponRecipe::new_weapon("parhelion.inherited-damage").unwrap();
        recipe.overrides.inventory_slot = Some(crate::recipe::RecipeInventorySlot::Kinetic);
        recipe.overrides.modern_damage_type = None;
        let path = library.save_new(&recipe).unwrap();
        let scan = library.scan().unwrap();
        let entry = scan
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .unwrap();
        assert_eq!(entry.damage_type, None);
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
        assert!(library.load_state().is_err());
        assert!(
            library
                .enabled_paths(&library.scan().unwrap().entries)
                .is_err()
        );
        assert!(state_path.is_dir());
    }

    #[test]
    fn recipe_discovery_ignores_non_json_files_and_json_named_directories() {
        let directory = tempfile::tempdir().unwrap();
        let recipe = directory.path().join("weapon.JSON");
        let non_recipe = directory.path().join("notes.txt");
        let subdirectory = directory.path().join("folder.json");
        fs::write(&recipe, b"{}").unwrap();
        fs::write(&non_recipe, b"notes").unwrap();
        fs::create_dir(&subdirectory).unwrap();
        assert_eq!(
            collect_recipe_paths([Ok(non_recipe), Ok(subdirectory), Ok(recipe.clone())]).unwrap(),
            vec![recipe]
        );
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
    fn bundled_identities_are_unique_and_new_recipes_use_distinct_donors() {
        let mut namespaces = BTreeSet::new();
        let mut hashes = BTreeSet::new();
        for (file_name, json) in BUNDLED_RECIPES {
            let recipe = WeaponRecipe::from_json_str(json).unwrap();
            recipe.to_spec().unwrap();
            assert!(namespaces.insert(recipe.namespace.clone()));
            for hash in recipe.identity.parsed_hashes(&recipe.namespace).unwrap() {
                assert!(
                    hashes.insert(hash),
                    "duplicate identity in {file_name}: {hash:08X}"
                );
            }
            assert_new_bundle_donors(file_name, &recipe);
        }
    }

    fn assert_new_bundle_donors(file_name: &str, recipe: &WeaponRecipe) {
        if matches!(
            file_name,
            EVERY_END_FILE_NAME | SECOND_SUN_FILE_NAME | "redacted.parhelion.json"
        ) {
            return;
        }
        if matches!(
            file_name,
            "still-here.parhelion.json"
                | "dead-air.parhelion.json"
                | "night-shift.parhelion.json"
                | "good-company.parhelion.json"
        ) {
            // These recipes intentionally inherit their gameplay donor's appearance.
            assert!(recipe.presentation_donor.is_none());
        } else {
            assert_ne!(
                recipe.donor.item_hash,
                recipe.presentation_donor.as_ref().unwrap().item_hash
            );
        }
        assert_eq!(
            recipe.overrides.rarity,
            Some(
                if matches!(
                    recipe.namespace.as_str(),
                    "parhelion.dead-air" | "parhelion.reclamation-order"
                ) {
                    crate::RecipeRarity::Exotic
                } else {
                    crate::RecipeRarity::Legendary
                }
            )
        );
        assert!(recipe.inventory_hint.is_none());
        assert!(recipe.runtime_component_donors.is_empty());
        assert!(recipe.overrides.weapon_pattern_index.is_none());
        assert!(recipe.overrides.raw_payload_patches.is_empty());
        assert!(!recipe.flavor.contains("authored with Parhelion"));
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
        };
        library.write_state(&state).unwrap();
        let scan = library.scan().unwrap();
        let enabled = library.enabled_paths(&scan.entries).unwrap();
        assert_eq!(enabled.len(), BUNDLED_RECIPES.len() - 1);
        assert!(!enabled.contains(&library.root().join(SECOND_SUN_FILE_NAME)));
        assert_eq!(library.enabled_paths(&scan.entries).unwrap(), enabled);
    }

    #[test]
    fn bundled_weapons_offer_distinct_trait_choices_and_keep_custom_defaults() {
        for (filename, json) in BUNDLED_RECIPES {
            let recipe = WeaponRecipe::from_json_str(json).unwrap();
            let sockets: &[usize] = match filename {
                "good-company.parhelion.json" | "reclamation-order.parhelion.json" => &[3, 4, 8],
                _ => &[3, 4],
            };
            for &socket in sockets {
                let column = recipe.overrides.socket_columns[socket].as_ref().unwrap();
                let expected_count = if matches!(
                    filename,
                    EVERY_END_FILE_NAME
                        | "stay.parhelion.json"
                        | "good-company.parhelion.json"
                        | "reclamation-order.parhelion.json"
                ) {
                    4
                } else {
                    3
                };
                assert_eq!(
                    column.choices.len(),
                    expected_count,
                    "{filename} socket {socket}"
                );
                let hashes = column
                    .choices
                    .iter()
                    .map(|hash| hash.parse_u32().unwrap())
                    .collect::<BTreeSet<_>>();
                assert_eq!(
                    hashes.len(),
                    expected_count,
                    "{filename} has duplicate choices"
                );
            }
            for variant in &recipe.overrides.socket_plug_variants {
                assert_eq!(
                    variant.choice_index, 0,
                    "{filename} custom perk must stay the default"
                );
                let column = recipe.overrides.socket_columns[usize::from(variant.socket_index)]
                    .as_ref()
                    .unwrap();
                assert_eq!(
                    column.choices[0].parse_u32().unwrap(),
                    variant.source_plug_hash.parse_u32().unwrap(),
                    "{filename}"
                );
            }
        }
    }

    #[test]
    fn all_bundled_defaults_are_seeded() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let every_end_path = library.root().join(EVERY_END_FILE_NAME);
        let second_sun_path = library.root().join(SECOND_SUN_FILE_NAME);

        let every_end = WeaponRecipe::load_json(every_end_path).unwrap();
        let second_sun = WeaponRecipe::load_json(second_sun_path).unwrap();
        let scan = library.scan().unwrap();
        let enabled = library.enabled_paths(&scan.entries).unwrap();

        assert_eq!(every_end, WeaponRecipe::every_end());
        assert_eq!(second_sun, WeaponRecipe::second_sun().unwrap());
        assert_eq!(scan.entries.len(), BUNDLED_RECIPES.len());
        assert!(scan.entries.iter().all(|entry| entry.bundled));
        assert_eq!(enabled.len(), BUNDLED_RECIPES.len());
        assert!(directory.path().join(LIBRARY_STATE_FILE_NAME).is_file());
    }

    #[test]
    fn new_weapon_identities_match_native_derivation_and_defaults_have_shaders() {
        for (name, json) in BUNDLED_RECIPES {
            let recipe = WeaponRecipe::from_json_str(json).unwrap();
            if matches!(
                name,
                "stay.parhelion.json"
                    | "still-here.parhelion.json"
                    | "vaultbreaker.parhelion.json"
                    | "good-company.parhelion.json"
                    | "reclamation-order.parhelion.json"
            ) {
                assert_eq!(
                    recipe.to_spec().unwrap().identity,
                    crate::WeaponCloneIdentity::from_namespace(&recipe.namespace).unwrap()
                );
            }
            let shader = recipe.overrides.socket_columns[5].as_ref().unwrap();
            assert_ne!(shader.choices[0].parse_u32().unwrap(), 0xFD368D30, "{name}");
        }
    }

    #[test]
    fn dead_air_replaces_ornament_and_catalyst_with_effigy_traits() {
        let recipe =
            WeaponRecipe::from_json_str(include_str!("../recipes/dead-air.parhelion.json"))
                .unwrap();
        assert_eq!(
            recipe.overrides.modern_damage_type,
            Some(crate::recipe::RecipeDamageType::Solar)
        );
        for (index, hash) in [(6, 0x62C9F17F), (7, 0x2F98742C)] {
            let column = recipe.overrides.socket_columns[index].as_ref().unwrap();
            assert_eq!(column.socket_type, Some(92));
            assert_eq!(column.choices.len(), 2);
            assert_eq!(column.choices[0].parse_u32().unwrap(), hash);
        }
    }

    #[test]
    fn saved_dead_air_and_night_shift_edits_are_bundled() {
        for (filename, damage, power_cap_group, hue_shift_degrees) in [
            (
                "dead-air.parhelion.json",
                crate::recipe::RecipeDamageType::Solar,
                15,
                140,
            ),
            (
                "night-shift.parhelion.json",
                crate::recipe::RecipeDamageType::Void,
                14,
                -170,
            ),
        ] {
            let (_, json) = BUNDLED_RECIPES
                .iter()
                .find(|(name, _)| *name == filename)
                .unwrap();
            let recipe = WeaponRecipe::from_json_str(json).unwrap();
            assert!(recipe.presentation_donor.is_none(), "{filename}");
            assert_eq!(
                recipe.overrides.inventory_slot,
                Some(crate::recipe::RecipeInventorySlot::Kinetic)
            );
            assert_eq!(recipe.overrides.modern_damage_type, Some(damage));
            assert_eq!(recipe.overrides.power_cap_group, Some(power_cap_group));
            assert!(recipe.overrides.icon_edit.flip_horizontal);
            assert_eq!(
                recipe.overrides.icon_edit.hue_shift_degrees,
                hue_shift_degrees
            );
            assert!(recipe.overrides.raw_payload_patches.is_empty());
        }
    }

    #[test]
    fn every_end_has_four_traits_and_keeps_requested_defaults() {
        let recipe = WeaponRecipe::from_json_str(EVERY_END_TEMPLATE).unwrap();
        for (socket, expected) in [
            (3, [0x5223_56C5, 0xA5A4_B58A, 0x0EC3_FDC8, 0x7481_2567]),
            (4, [0xB517_FC25, 0xD201_3CA1, 0xF351_D2CC, 0xA9CA_6B03]),
        ] {
            let choices = &recipe.overrides.socket_columns[socket]
                .as_ref()
                .unwrap()
                .choices;
            assert_eq!(
                choices
                    .iter()
                    .map(|hash| hash.parse_u32().unwrap())
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn june_ninth_is_a_void_smg_in_the_kinetic_slot() {
        let recipe =
            WeaponRecipe::from_json_str(include_str!("../recipes/june-ninth.parhelion.json"))
                .unwrap();
        assert_eq!(recipe.donor.item_hash.parse_u32().unwrap(), 0xC7ED_ADF6);
        assert_eq!(
            recipe.overrides.inventory_slot,
            Some(crate::recipe::RecipeInventorySlot::Kinetic)
        );
        assert_eq!(
            recipe.overrides.modern_damage_type,
            Some(crate::recipe::RecipeDamageType::Void)
        );
        assert_eq!(
            recipe.overrides.ammo_type,
            Some(crate::recipe::RecipeAmmoType::Primary)
        );
        assert_eq!(
            recipe
                .presentation_donor
                .as_ref()
                .unwrap()
                .item_hash
                .parse_u32()
                .unwrap(),
            0x9BAD_D9A6
        );
        assert_eq!(recipe.overrides.socket_columns.len(), 12);
    }

    #[test]
    fn opening_library_never_overwrites_seeded_user_edits() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("recipes");
        let library = RecipeLibrary::open(root.clone()).unwrap();
        let path = library.root().join(EVERY_END_FILE_NAME);
        fs::write(&path, "user-owned edit").unwrap();

        RecipeLibrary::open(root).unwrap();

        assert_eq!(fs::read_to_string(path).unwrap(), "user-owned edit");
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
    fn discovery_sorts_valid_recipes_and_reports_invalid_json() {
        let directory = tempfile::tempdir().unwrap();
        let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
        let second = WeaponRecipe::new_weapon("parhelion.alpha").unwrap();
        library.save_new(&second).unwrap();
        fs::write(library.root().join("broken.json"), "not json").unwrap();

        let scan = library.scan().unwrap();
        let enabled = library.enabled_paths(&scan.entries).unwrap();

        assert_eq!(scan.entries.len(), BUNDLED_RECIPES.len() + 1);
        assert!(
            scan.entries.windows(2).all(|pair| {
                pair[0].name.to_ascii_lowercase() < pair[1].name.to_ascii_lowercase()
            })
        );
        let custom = scan
            .entries
            .iter()
            .find(|entry| entry.name == "New Weapon")
            .unwrap();
        assert!(!custom.bundled);
        assert!(!enabled.contains(&custom.path));
        assert_eq!(scan.errors.len(), 1);
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
        assert_eq!(
            reserved_path.file_name().unwrap(),
            "weapon-con.parhelion.json"
        );

        let mut long = WeaponRecipe::new_weapon("parhelion.long-filename").unwrap();
        long.name = "a".repeat(500);
        let long_path = library.save_new(&long).unwrap();
        let file_name = long_path.file_name().unwrap().to_string_lossy();
        assert!(file_name.len() <= 100);
        assert!(file_name.ends_with(".parhelion.json"));

        let mut same_long_name = WeaponRecipe::new_weapon("parhelion.long-filename-two").unwrap();
        same_long_name.name = "a".repeat(500);
        let suffixed_path = library.save_new(&same_long_name).unwrap();
        let suffixed_name = suffixed_path.file_name().unwrap().to_string_lossy();
        assert!(suffixed_name.len() <= 101);
        assert!(suffixed_name.ends_with("-1.parhelion.json"));
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
