//! Crucible maps authored as new activities. A map is listed by activity and display rows cloned
//! from a donor map's pair, is offered in private matches by an entry cloned from the donor's, and
//! owns its packages as a stock map does: `w64_<name>_activities` holds its activity root and
//! scenario, where named tags select them, and with geometry its slice set and object registry.
//! `w64_<name>` holds its world, the map root chain and converted pieces. No stock map changes.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{AuthoringResult, error::invalid};

#[cfg(feature = "d2-model-importer")]
mod activity;
#[cfg(feature = "d2-model-importer")]
mod chain;
#[cfg(feature = "d2-model-importer")]
mod geometry;
#[cfg(feature = "d2-model-importer")]
mod picture;
#[cfg(feature = "d2-model-importer")]
mod plan;
#[cfg(feature = "d2-model-importer")]
mod private_match;
#[cfg(feature = "d2-model-importer")]
mod registry;
#[cfg(not(feature = "d2-model-importer"))]
mod unsupported;
#[cfg(feature = "d2-model-importer")]
mod volumes;
#[cfg(feature = "d2-model-importer")]
mod world;

#[cfg(feature = "d2-model-importer")]
pub(crate) use plan::Plan;
#[cfg(feature = "d2-model-importer")]
use plan::{Resolved, allocate, donor_tag};
#[cfg(not(feature = "d2-model-importer"))]
pub(crate) use unsupported::Plan;

/// A map's own picture in the private match list: a copy of its donor's picture chain holding the
/// source level's picture, and the donor's row of the item icon table, which the copy's row
/// copies. The map's private match display then names the copy's row.
pub(crate) struct Picture {
    /// The map's index in the plan.
    pub map: usize,
    /// The hash the picture's icon row takes.
    pub hash: u32,
    pub donor_row: u16,
    pub donor: crate::icon_chain::Donor,
    pub pixels: [Vec<u8>; 2],
}

/// The current map recipe schema.
const SCHEMA: u32 = 1;
/// Sunrise keeps an internal name in 40 bytes with its terminator.
const MAX_NAME_LENGTH: usize = 39;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MapRecipe {
    pub schema: u32,
    /// The new activity's internal name, which every named tag of its world starts from.
    pub name: String,
    pub title: String,
    pub description: String,
    /// The internal name of the stock Crucible map the new map is cloned from.
    pub donor: String,
    /// Another game's level the map draws and collides with, placed in the donor's world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<MapSource>,
}

/// A Halo: Reach multiplayer level.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MapSource {
    /// The folder of Reach `.map` caches.
    pub maps: PathBuf,
    /// The cache whose first structure BSP becomes the map.
    pub map: String,
    /// A map variant (`.mvar`) of that cache, whose Forge objects and spawn points stand in for the
    /// scenario's own placements of everything its palette offers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<PathBuf>,
    /// The vgmstream command line decoder that reads the level's sounds from Reach's
    /// `fmod/pc/sfx.fsb` beside the maps folder. Without it the map plays no ambience.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_decoder: Option<PathBuf>,
    /// Whole meters the level moves by.
    #[serde(default)]
    pub offset: [i32; 3],
}

impl MapRecipe {
    fn validate(&self) -> AuthoringResult<()> {
        if self.schema != SCHEMA {
            return Err(invalid(format!(
                "Map {} uses schema {}, expected {SCHEMA}",
                self.name, self.schema
            )));
        }
        for (label, name) in [("name", &self.name), ("donor", &self.donor)] {
            if name.is_empty()
                || name.len() > MAX_NAME_LENGTH
                || !name
                    .bytes()
                    .all(|c| c == b'_' || c.is_ascii_lowercase() || c.is_ascii_digit())
            {
                return Err(invalid(format!(
                    "Map {label} {name:?} must be 1 to {MAX_NAME_LENGTH} lowercase letters, digits or underscores"
                )));
            }
        }
        if self.title.trim().is_empty() || self.description.trim().is_empty() {
            return Err(invalid(format!(
                "Map {} needs a title and a description",
                self.name
            )));
        }
        Ok(())
    }
}

/// The folder of `*.json` map recipes. The library lists them, and a build includes those its
/// selection names.
pub fn library_directory() -> Option<PathBuf> {
    sundial::package_authoring::parhelion_data_directory().map(|root| root.join("maps"))
}

/// The map recipe at `path`, checked.
pub fn load_recipe(path: &Path) -> AuthoringResult<MapRecipe> {
    let bytes = std::fs::read(path)
        .map_err(|error| invalid(format!("Could not read {}: {error}", path.display())))?;
    let map: MapRecipe = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("Map recipe {}: {error}", path.display())))?;
    map.validate()?;
    Ok(map)
}

/// Every map recipe file in `directory` with its recipe, or the error the file gives, by path. A
/// missing folder holds none.
pub fn library_files(
    directory: &Path,
) -> AuthoringResult<Vec<(PathBuf, AuthoringResult<MapRecipe>)>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(invalid(format!(
                "Could not list maps in {}: {error}",
                directory.display()
            )));
        }
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths
        .into_iter()
        .map(|path| {
            let recipe = load_recipe(&path);
            (path, recipe)
        })
        .collect())
}

/// The map recipes at `paths`, which must not share a name.
pub fn load_selected<'a>(
    paths: impl IntoIterator<Item = &'a Path>,
) -> AuthoringResult<Vec<MapRecipe>> {
    let mut maps = Vec::new();
    let mut names = BTreeSet::new();
    for path in paths {
        let map = load_recipe(path)?;
        if !names.insert(map.name.clone()) {
            return Err(invalid(format!("Two map recipes are named {}", map.name)));
        }
        maps.push(map);
    }
    Ok(maps)
}

/// Every map recipe in `directory`. A missing folder holds none.
pub fn load_library(directory: &Path) -> AuthoringResult<Vec<MapRecipe>> {
    let files = library_files(directory)?;
    for (_, recipe) in &files {
        if let Err(error) = recipe {
            return Err(invalid(error.to_string()));
        }
    }
    load_selected(files.iter().map(|(path, _)| path.as_path()))
}

/// The saved library's maps, or none without a data folder.
pub fn library() -> Result<Vec<MapRecipe>, String> {
    library_directory()
        .map(|directory| load_library(&directory))
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(|error| error.to_string())
}
