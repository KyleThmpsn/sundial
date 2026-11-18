//! Named build lists: selections of recipes and maps set aside to return to.
//!
//! A list holds file names, as the active selection does, so it follows a recipe through its
//! edits and keeps naming one that has left the library until it returns. Loading a list gives
//! the paths the library holds now. It changes neither the list nor the active selection.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use super::{MapLibraryEntry, RecipeLibrary, RecipeLibraryEntry, entry_file_name, map_file_name};

/// The longest name a build list takes, in characters.
pub(crate) const NAME_LIMIT: usize = 60;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct BuildList {
    recipes: BTreeSet<String>,
    maps: BTreeSet<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

/// A build list as the library holds it now.
pub(crate) struct Loaded {
    pub(crate) paths: BTreeSet<PathBuf>,
    /// How many of the list's recipes and maps the library no longer holds.
    pub(crate) missing: usize,
}

impl RecipeLibrary {
    /// The names of the saved build lists, in name order.
    pub(crate) fn build_lists(&self) -> Result<Vec<String>, String> {
        let _lock = self.lock()?;
        let (state, _) = self.read_state()?;
        Ok(state.build_lists.into_keys().collect())
    }

    /// Saves the recipes and maps among `paths` as the build list `name`, replacing a list of
    /// that name whatever its letter case, and returns the name as stored.
    pub(crate) fn save_build_list(
        &self,
        name: &str,
        paths: &BTreeSet<PathBuf>,
        entries: &[RecipeLibraryEntry],
        maps: &[MapLibraryEntry],
    ) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Enter a name for the build list".into());
        }
        if name.chars().count() > NAME_LIMIT {
            return Err(format!(
                "A build list name holds at most {NAME_LIMIT} characters"
            ));
        }
        let list = BuildList {
            recipes: entries
                .iter()
                .filter(|entry| paths.contains(&entry.path))
                .map(entry_file_name)
                .collect::<Result<_, _>>()?,
            maps: maps
                .iter()
                .filter(|map| paths.contains(&map.path))
                .map(map_file_name)
                .collect::<Result<_, _>>()?,
            extra: BTreeMap::new(),
        };
        let _lock = self.lock()?;
        let (mut state, original) = self.read_state()?;
        state
            .build_lists
            .retain(|existing, _| existing.to_lowercase() != name.to_lowercase());
        state.build_lists.insert(name.to_owned(), list);
        self.write_state_checked(&state, original.as_deref())?;
        Ok(name.to_owned())
    }

    /// The build list `name` as paths of the `entries` and `maps` it still names.
    pub(crate) fn build_list(
        &self,
        name: &str,
        entries: &[RecipeLibraryEntry],
        maps: &[MapLibraryEntry],
    ) -> Result<Loaded, String> {
        let _lock = self.lock()?;
        let (state, _) = self.read_state()?;
        let list = state
            .build_lists
            .get(name)
            .ok_or_else(|| format!("The build list {name} is no longer saved"))?;
        let mut paths = BTreeSet::new();
        for entry in entries {
            if list.recipes.contains(&entry_file_name(entry)?) {
                paths.insert(entry.path.clone());
            }
        }
        for map in maps {
            if list.maps.contains(&map_file_name(map)?) {
                paths.insert(map.path.clone());
            }
        }
        Ok(Loaded {
            missing: (list.recipes.len() + list.maps.len()).saturating_sub(paths.len()),
            paths,
        })
    }

    pub(crate) fn delete_build_list(&self, name: &str) -> Result<(), String> {
        let _lock = self.lock()?;
        let (mut state, original) = self.read_state()?;
        if state.build_lists.remove(name).is_none() {
            return Ok(());
        }
        self.write_state_checked(&state, original.as_deref())
    }
}
