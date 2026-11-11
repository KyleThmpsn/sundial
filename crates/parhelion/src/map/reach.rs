//! Recipes for the Halo: Reach levels of an MCC maps folder that no library recipe draws yet. A
//! level becomes a private match map when it is a multiplayer or Firefight level, cloned from
//! `pvp_sabre`, and a mission when it is a campaign level, cloned from `mission_proton` and
//! loading its zone sets in scenario order but those named as cinematics, those that load the
//! same structure BSPs as one already chosen and those that load a structure BSP whose render
//! geometry holds no data in the cache. Every zone set of the inspected campaign caches loads
//! a playable BSP, cinematics included, so that flag selects nothing. Its title and description are the menu
//! text MCC shows. Its offset centres a multiplayer level's spawn points on the origin with the
//! lowest at height zero, and puts a mission's first player start at the origin, as the library's
//! earlier Reach recipes were placed.
use std::path::Path;

use parhelion_import::halo_reach::level;

use super::MapRecipe;

/// The donors a new Reach map clones.
const MULTIPLAYER_DONOR: &str = "pvp_sabre";
const MISSION_DONOR: &str = "mission_proton";
/// Caches in the maps folder that hold shared content, not a level.
const NOT_LEVELS: [&str; 2] = ["campaign.map", "shared.map"];

/// One level the library lacks: its cache and the recipe made for it, or why none was made.
pub struct Proposal {
    pub map: String,
    pub recipe: Result<MapRecipe, String>,
}

/// A proposal for every cache in `maps` that no recipe in `existing` names, in file order.
pub fn missing(maps: &Path, existing: &[MapRecipe]) -> Result<Vec<Proposal>, String> {
    let mut caches = std::fs::read_dir(maps)
        .map_err(|error| format!("Could not read {}: {error}", maps.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".map") && !NOT_LEVELS.contains(&name.as_str()))
        .collect::<Vec<_>>();
    caches.sort();
    let mut names = existing
        .iter()
        .map(|recipe| recipe.name.clone())
        .collect::<Vec<_>>();
    let mut proposals = Vec::new();
    for map in caches {
        if existing.iter().any(|recipe| {
            recipe
                .source
                .as_ref()
                .is_some_and(|source| source.map == map)
        }) {
            continue;
        }
        let recipe = level::entry(maps, &map)
            .map_err(|error| format!("{error:#}"))
            .and_then(|entry| recipe(maps, &entry, &names));
        if let Ok(recipe) = &recipe {
            names.push(recipe.name.clone());
        }
        proposals.push(Proposal { map, recipe });
    }
    Ok(proposals)
}

/// Whether zone set `name` is a cinematic's, named with `cinematic` as Winter Contingency's
/// `zoneset_intro_cinematic` is or with a `cin` word as Long Night of Solace's
/// `set_cin_bombdelivery` is.
fn cinematic(name: &str) -> bool {
    name.contains("cinematic") || name.split('_').any(|word| word == "cin")
}

/// Whether cache `map` is a campaign level, named `m` and two digits as `m10` and `m70_bonus` are.
fn campaign(map: &str) -> bool {
    let bytes = map.as_bytes();
    bytes.len() > 3 && bytes[0] == b'm' && bytes[1].is_ascii_digit() && bytes[2].is_ascii_digit()
}

/// The recipe for `entry`, whose internal name avoids every name in `taken`.
fn recipe(maps: &Path, entry: &level::Entry, taken: &[String]) -> Result<MapRecipe, String> {
    let text = entry
        .text
        .as_ref()
        .ok_or("The level has no menu text in maps/info")?;
    let mission = campaign(&entry.map);
    let (prefix, donor) = if mission {
        ("mission_reach_", MISSION_DONOR)
    } else {
        ("pvp_reach_", MULTIPLAYER_DONOR)
    };
    let (zone_sets, offset) = if mission {
        let mut zone_sets = Vec::<usize>::new();
        for (index, set) in entry.zone_sets.iter().enumerate() {
            let repeats = zone_sets
                .iter()
                .any(|&chosen| entry.zone_sets[chosen].bsps == set.bsps);
            if !cinematic(&set.name) && !repeats && !entry.incomplete.contains(&index) {
                zone_sets.push(index);
            }
        }
        let start = zone_sets
            .first()
            .and_then(|&index| entry.zone_sets[index].starts.first())
            .ok_or("The first zone set past the cinematics has no player start")?;
        (zone_sets, start.map(|value| -value.round() as i32))
    } else {
        if entry.spawns.is_empty() {
            return Err("The level places no spawn points".into());
        }
        if entry.incomplete.contains(&0) {
            return Err("A structure BSP's render geometry holds no data in the cache".into());
        }
        let low = [0, 1, 2].map(|k| entry.spawns.iter().map(|s| s[k]).fold(f32::MAX, f32::min));
        let high = [0, 1, 2].map(|k| entry.spawns.iter().map(|s| s[k]).fold(f32::MIN, f32::max));
        let offset = [
            -((low[0] + high[0]) / 2.).round() as i32,
            -((low[1] + high[1]) / 2.).round() as i32,
            -low[2].round() as i32,
        ];
        (vec![0], offset)
    };
    let name = unique(&format!("{prefix}{}", slug(&text.title)), taken)?;
    let recipe: MapRecipe = serde_json::from_value(serde_json::json!({
        "schema": super::SCHEMA,
        "name": name,
        "title": text.title,
        "description": text.description,
        "donor": donor,
        "source": {
            "maps": maps.to_string_lossy().replace('\\', "/"),
            "map": entry.map,
            "zone_sets": zone_sets,
            "offset": offset,
        },
    }))
    .map_err(|error| error.to_string())?;
    recipe.validate().map_err(|error| error.to_string())?;
    Ok(recipe)
}

/// Lowercase ASCII letters and digits of `title`, other runs as one underscore.
fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_owned()
}

/// `name`, cut to Sunrise's 39 bytes, with a number appended while `taken` holds it.
fn unique(name: &str, taken: &[String]) -> Result<String, String> {
    let cut = |limit: usize| {
        name[..name.len().min(limit)]
            .trim_end_matches('_')
            .to_owned()
    };
    let base = cut(super::MAX_NAME_LENGTH);
    if !taken.contains(&base) {
        return Ok(base);
    }
    (2..100)
        .map(|n| {
            let suffix = format!("_{n}");
            format!("{}{suffix}", cut(super::MAX_NAME_LENGTH - suffix.len()))
        })
        .find(|candidate| !taken.contains(candidate))
        .ok_or_else(|| format!("No free internal name for {name}"))
}
