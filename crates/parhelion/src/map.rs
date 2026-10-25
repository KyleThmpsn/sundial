//! Maps authored as new activities. A map is listed by activity and display rows cloned from a
//! donor activity's pair, a Crucible map or a mission, is offered in private matches by an entry
//! cloned from a Crucible donor's, and
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
mod destination;
pub mod diagnostics;
#[cfg(feature = "d2-model-importer")]
mod encounters;
#[cfg(feature = "d2-model-importer")]
mod geometry;
pub(crate) mod mission;
#[cfg(feature = "d2-model-importer")]
mod picture;
#[cfg(feature = "d2-model-importer")]
mod placed;
#[cfg(feature = "d2-model-importer")]
mod plan;
#[cfg(feature = "d2-model-importer")]
mod private_match;
#[cfg(feature = "d2-model-importer")]
mod registry;
#[cfg(feature = "d2-model-importer")]
mod streaming;
#[cfg(feature = "d2-model-importer")]
mod triggers;
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
/// The longest destination label a map's localization entry takes, in bytes.
const MAX_DESTINATION_LENGTH: usize = 96;
/// The fixed destination rows of the definition and display tables.
#[cfg(feature = "d2-model-importer")]
pub(crate) const DESTINATION_ROWS: usize = 48;

fn destination_label_valid(label: &str) -> bool {
    !label.trim().is_empty()
        && label.len() <= MAX_DESTINATION_LENGTH
        && !label.chars().any(char::is_control)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MapRecipe {
    pub schema: u32,
    /// The new activity's internal name, which every named tag of its world starts from.
    pub name: String,
    pub title: String,
    pub description: String,
    /// A label shared by authored activities in an otherwise unused native destination row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    /// The internal name of the stock activity the new map is cloned from: a Crucible map for a
    /// multiplayer level, a mission or strike for a stretch of a campaign mission.
    pub donor: String,
    /// Another game's level the map draws and collides with, placed in the donor's world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<MapSource>,
    #[serde(default, skip_serializing_if = "MapOptions::is_default")]
    pub options: MapOptions,
    /// Embedded artwork for the activity's private match portrait and landscape images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<crate::presentation::Artwork>,
    /// Stock private-match activity whose portrait and landscape artwork should be used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_donor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MapOptions {
    /// None allows mission_ activities and otherwise preserves the donor's policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vehicle_summoning: Option<bool>,
    pub ambient_audio: bool,
    pub death_boundaries: bool,
}

impl Default for MapOptions {
    fn default() -> Self {
        Self {
            vehicle_summoning: None,
            ambient_audio: true,
            death_boundaries: true,
        }
    }
}

impl MapOptions {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }

    #[cfg(feature = "d2-model-importer")]
    pub(crate) fn vehicle_summoning(&self, name: &str) -> Option<bool> {
        self.vehicle_summoning
            .or_else(|| name.starts_with("mission_").then_some(true))
    }
}

/// A Halo: Reach multiplayer level or stretch of a campaign mission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MapSource {
    /// The folder of Reach `.map` caches.
    pub maps: PathBuf,
    /// The cache whose level becomes the map.
    pub map: String,
    /// The selected zone sets in route order. Automatic source crossings partition a campaign
    /// into connected native bubbles. Script-only variants stay with their preceding area.
    /// A multiplayer level normally has one zone set. Players start in the first selected area.
    #[serde(default = "first_zone_set", skip_serializing_if = "is_first_zone_set")]
    pub zone_sets: Vec<usize>,
    /// A map variant (`.mvar`) of that cache, whose Forge objects and spawn points stand in for the
    /// scenario's own placements of everything its palette offers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<PathBuf>,
    /// The vgmstream command line decoder that reads the level's sounds from Reach's
    /// `fmod/pc/sfx.fsb` beside the maps folder. Without it the build uses the vgmstream the
    /// importer finds, and with none the map plays no ambience.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_decoder: Option<PathBuf>,
    /// Mission recording banks, with at most one dialogue language. An empty list uses MCC's
    /// `sfx.fsb` and `english.fsb` beside the maps folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_banks: Vec<PathBuf>,
    /// Selected HaloScript roots to compile into a Lua sidecar beside the converted map.
    /// An empty list exports source analysis alone. Installing the controller is separate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mission_scripts: Vec<String>,
    /// JSON native mission bindings with explicit source names, target activity and SDK identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_bindings: Option<PathBuf>,
    /// Encounter prefixes for native squad bring-up. They stop at their first source wait.
    /// These openings use Normal, solo difficulty and do not execute full mission progression.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encounter_starts: Vec<String>,
    /// Ordered encounter openings activated by a named source player volume. Each opening still
    /// ends at its first wait. The recipe records the chosen source gate explicitly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encounter_gates: Vec<EncounterGate>,
    /// Whole meters the level moves by.
    #[serde(default)]
    pub offset: [i32; 3],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterGate {
    /// Any of these source volumes activates the next opening while the player is inside it.
    pub volumes: Vec<String>,
    pub scripts: Vec<String>,
}

fn first_zone_set() -> Vec<usize> {
    vec![0]
}

fn is_first_zone_set(zone_sets: &[usize]) -> bool {
    zone_sets == [0]
}

impl MapRecipe {
    pub(crate) fn validate(&self) -> AuthoringResult<()> {
        if self.image.is_some() && self.image_donor.is_some() {
            return Err(invalid(
                "Choose either uploaded artwork or an activity image donor",
            ));
        }
        if (self.image.is_some() || self.image_donor.is_some()) && !self.donor.starts_with("pvp_") {
            return Err(invalid(
                "Activity artwork currently requires a private-match donor",
            ));
        }
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
        if self
            .destination
            .as_deref()
            .is_some_and(|label| !destination_label_valid(label))
        {
            return Err(invalid(format!(
                "Destination must be 1 to {MAX_DESTINATION_LENGTH} bytes without control characters"
            )));
        }
        if self.title.trim().is_empty() || self.description.trim().is_empty() {
            return Err(invalid(format!(
                "Map {} needs a title and a description",
                self.name
            )));
        }
        if self.source.is_none() && (!self.options.ambient_audio || !self.options.death_boundaries)
        {
            return Err(invalid(
                "Ambient audio and death boundaries require a source level",
            ));
        }
        if self
            .source
            .as_ref()
            .is_some_and(|source| source.zone_sets.is_empty())
        {
            return Err(invalid(format!("Map {} loads no zone set", self.name)));
        }
        if let Some(source) = &self.source {
            if !source.encounter_gates.is_empty() && source.encounter_starts.is_empty() {
                return Err(invalid(
                    "Encounter gates require an initial encounter opening",
                ));
            }
            for gate in &source.encounter_gates {
                if gate.volumes.is_empty()
                    || gate.scripts.is_empty()
                    || gate.volumes.iter().collect::<BTreeSet<_>>().len() != gate.volumes.len()
                {
                    return Err(invalid(
                        "Encounter gates need distinct source volumes and selected scripts",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The folder of `*.json` map recipes. The library lists them, and a build includes those its
/// selection names.
pub fn library_directory() -> Option<PathBuf> {
    sundial::package_authoring::parhelion_data_directory().map(|root| root.join("maps"))
}

/// Private-match activities with artwork, read from the same source tables the build uses.
#[cfg(feature = "d2-model-importer")]
pub(crate) struct ImageChoice {
    pub name: String,
    pub preview: Result<image::RgbaImage, String>,
}

#[cfg(feature = "d2-model-importer")]
pub(crate) fn image_choices(directory: &Path) -> Result<Vec<ImageChoice>, String> {
    use crate::tag_payload::{read_tag, read_u32};
    use sundial::package_authoring::{open_shadowkeep_package_manager, resolve_live_named_tag};
    use tiger_pkg::TagHash;
    let manager = open_shadowkeep_package_manager(directory)?;
    let load = || -> AuthoringResult<Vec<ImageChoice>> {
        let tag = resolve_live_named_tag(&manager, "investment_globals", None).map_err(invalid)?;
        let globals = read_tag(&manager, tag, "investment globals")?;
        let root = read_tag(
            &manager,
            TagHash(read_u32(&globals, 0x10)?),
            "investment root",
        )?;
        let activities = read_tag(
            &manager,
            TagHash(read_u32(&root, 8 + 4 * 16)?),
            "activity table",
        )?;
        let table = read_tag(
            &manager,
            TagHash(read_u32(&root, 8 + 64 * 16)?),
            "private match table",
        )?;
        let displays = read_tag(
            &manager,
            TagHash(read_u32(&globals, 0x10 + 43 * 16)?),
            "private match displays",
        )?;
        let icons = read_tag(
            &manager,
            TagHash(read_u32(&globals, 0x10 + 75 * 16)?),
            "item icon table",
        )?;
        Ok(private_match::image_rows(&activities, &table, &displays)?
            .into_iter()
            .map(|(name, row)| ImageChoice {
                name,
                preview: picture::preview(&manager, &icons, row).map_err(|error| error.to_string()),
            })
            .collect())
    };
    load().map_err(|error| error.to_string())
}

/// How many destination rows the packages leave for authored labels, read from the same source
/// tables the build uses.
#[cfg(feature = "d2-model-importer")]
pub(crate) fn blank_destination_rows(directory: &Path) -> Result<usize, String> {
    use crate::tag_payload::{read_tag, read_u32};
    use sundial::package_authoring::{open_shadowkeep_package_manager, resolve_live_named_tag};
    use tiger_pkg::TagHash;
    let manager = open_shadowkeep_package_manager(directory)?;
    let load = || -> AuthoringResult<usize> {
        let tag = resolve_live_named_tag(&manager, "investment_globals", None).map_err(invalid)?;
        let globals = read_tag(&manager, tag, "investment globals")?;
        let root = read_tag(
            &manager,
            TagHash(read_u32(&globals, 0x10)?),
            "investment root",
        )?;
        let activities = read_tag(
            &manager,
            TagHash(read_u32(&root, 8 + 4 * 16)?),
            "activity table",
        )?;
        Ok(destination::rows(&manager, &root, &globals, &activities)?
            .available
            .len())
    };
    load().map_err(|error| error.to_string())
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
