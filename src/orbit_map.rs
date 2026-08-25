use std::{
    collections::{BTreeSet, HashSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tiger_pkg::PackageManager;

use crate::{
    catalog::package::{array_at, u32_at},
    hash::fnv1_name_hash,
};

const FILE_NAME: &str = "orbit_map.txt";
const FILE_SIZE_LIMIT: usize = 64 * 1024;
const ORBIT_SCENARIO_NAME: &str = "orbit_d2:scenario_client";
const SCENARIO_CLASS: u32 = 0x8080_9994;
const SCENARIO_BUBBLE_DESCRIPTOR: usize = 80;
const BUBBLE_CLASS: u32 = 0x8080_924D;
const BUBBLE_STRIDE: usize = 24;
const BUBBLE_STATE_DESCRIPTOR: usize = 8;
const SLICE_STATE_CLASS: u32 = 0x8080_924F;

const KNOWN_BUBBLE_NAMES: &[&str] = &[
    "orbit_dreaming_city_d2",
    "orbit_dreaming_city_gambit_d2",
    "orbit_earth_d2",
    "orbit_earth_gambit_d2",
    "orbit_eden_d2",
    "orbit_fleet_d2",
    "orbit_fleet_gambit_d2",
    "orbit_hiveship_d2",
    "orbit_hyperion_d2",
    "orbit_mars_d2",
    "orbit_mars_gambit_d2",
    "orbit_mercury_d2",
    "orbit_moon_d2",
    "orbit_pandora_d2",
    "orbit_planet_x_d2",
    "orbit_planet_x_gambit_d2",
    "orbit_reef_d2",
    "orbit_reef_gambit_d2",
    "orbit_trials_d2",
    "orbit_venus_d2",
];

// Package metadata supplies the canonical destination stems and the orbit scenario supplies the
// available bubble hashes. The packages do not name the relationship between those identifiers,
// so keep the known internal pairings exact and only emit rows whose two package-backed sides exist.
const KNOWN_DESTINATIONS: &[(&str, &str)] = &[
    ("advent_summer_event", "orbit_earth_d2"),
    ("arcade_ember", "orbit_earth_d2"),
    ("arcade_homecoming", "orbit_earth_d2"),
    ("arcade_reunion", "orbit_earth_d2"),
    ("arcade_spark", "orbit_earth_d2"),
    ("arcade_thunder", "orbit_earth_d2"),
    ("black_garden", "orbit_moon_d2"),
    ("cabal_ship", "orbit_moon_d2"),
    ("cayde_6_feet_under", "orbit_earth_d2"),
    ("city_tower_d16_t0", "orbit_earth_d2"),
    ("city_tower_d2", "orbit_earth_d2"),
    ("commando", "orbit_earth_d2"),
    ("cosmo_killers_01", "orbit_earth_d2"),
    ("cosmo_launchpad", "orbit_earth_d2"),
    ("d2_campaign_social_space", "orbit_earth_d2"),
    ("dreaming_city", "orbit_dreaming_city_d2"),
    ("dungeon_prophecy", "orbit_pandora_d2"),
    ("eden", "orbit_eden_d2"),
    ("edz", "orbit_earth_d2"),
    ("fleet", "orbit_fleet_d2"),
    ("gambit_badlands", "orbit_earth_d2"),
    ("gambit_dreamycliffs", "orbit_dreaming_city_d2"),
    ("gambit_hold", "orbit_mars_d2"),
    ("gambit_ledge", "orbit_planet_x_d2"),
    ("gambit_scrap", "orbit_reef_d2"),
    ("gambit_trinity", "orbit_fleet_d2"),
    ("infinite_forest_live", "orbit_mercury_d2"),
    ("infinite_forest_spring", "orbit_mercury_d2"),
    ("last_city_crater", "orbit_earth_d2"),
    ("last_city_liberation", "orbit_earth_d2"),
    ("leviathan", "orbit_planet_x_d2"),
    ("leviathan_v310", "orbit_planet_x_d2"),
    ("luna", "orbit_moon_d2"),
    ("mercury_destination", "orbit_mercury_d2"),
    ("mercury_lost_woods", "orbit_mercury_d2"),
    ("orphaned", "orbit_earth_d2"),
    ("pandora", "orbit_pandora_d2"),
    ("penumbra", "orbit_planet_x_d2"),
    ("planet_x", "orbit_planet_x_d2"),
    ("polaris", "orbit_mars_d2"),
    ("prison_of_elders", "orbit_reef_d2"),
    ("pvp_anomaly_2", "orbit_moon_d2"),
    ("pvp_arena_hive_2", "orbit_moon_d2"),
    ("pvp_bacon", "orbit_mercury_d2"),
    ("pvp_bannerfall_2", "orbit_earth_d2"),
    ("pvp_city_defense_2", "orbit_earth_d2"),
    ("pvp_cliffside", "orbit_earth_d2"),
    ("pvp_colony_ship_2", "orbit_earth_d2"),
    ("pvp_echo", "orbit_planet_x_d2"),
    ("pvp_elevator", "orbit_mars_d2"),
    ("pvp_estoc", "orbit_mars_d2"),
    ("pvp_factory_2", "orbit_earth_d2"),
    ("pvp_fort", "orbit_mars_d2"),
    ("pvp_glaive", "orbit_trials_d2"),
    ("pvp_greenhouse_2", "orbit_planet_x_d2"),
    ("pvp_grove", "orbit_planet_x_d2"),
    ("pvp_hull", "orbit_earth_d2"),
    ("pvp_katana", "orbit_planet_x_d2"),
    ("pvp_longshot_2", "orbit_mercury_d2"),
    ("pvp_manhattan", "orbit_mercury_d2"),
    ("pvp_mojo", "orbit_fleet_d2"),
    ("pvp_ness", "orbit_mercury_d2"),
    ("pvp_observatory", "orbit_earth_d2"),
    ("pvp_peak", "orbit_dreaming_city_d2"),
    ("pvp_pickles", "orbit_reef_d2"),
    ("pvp_sabre", "orbit_earth_d2"),
    ("pvp_shaft", "orbit_eden_d2"),
    ("pvp_slag", "orbit_earth_d2"),
    ("pvp_street", "orbit_trials_d2"),
    ("pvp_utopia", "orbit_fleet_d2"),
    ("pvp_vex_tube", "orbit_mercury_d2"),
    ("pvp_wilderness_town_2", "orbit_earth_d2"),
    ("sabotage", "orbit_planet_x_d2"),
    ("sky_island", "orbit_earth_d2"),
    ("sundial", "orbit_mercury_d2"),
    ("tangled_shore", "orbit_reef_d2"),
    ("the_journey", "orbit_earth_d2"),
    ("trials_social_space", "orbit_trials_d2"),
    ("trophy_hall", "orbit_planet_x_d2"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub destination: String,
    pub orbit: String,
}

pub(crate) struct Scan {
    pub backdrops: Vec<String>,
    pub entries: Vec<Entry>,
}

pub(crate) fn scan(manager: &PackageManager) -> Result<Scan, String> {
    let mut backdrops = scan_backdrops(manager)?;
    backdrops.sort();
    backdrops.dedup();

    let installed_stems = manager
        .lookup
        .named_tags
        .iter()
        .filter(|entry| {
            entry.class_hash == SCENARIO_CLASS && entry.name.ends_with(":scenario_client")
        })
        .filter_map(|entry| manager.package_paths.get(&entry.hash.pkg_id()))
        .filter_map(|package| sunrise_package_stem(&package.name))
        .collect::<BTreeSet<_>>();
    let installed_backdrops = backdrops.iter().map(String::as_str).collect::<HashSet<_>>();
    let entries = KNOWN_DESTINATIONS
        .iter()
        .filter(|(destination, orbit)| {
            installed_stems.contains(*destination) && installed_backdrops.contains(*orbit)
        })
        .map(|(destination, orbit)| Entry {
            destination: (*destination).to_owned(),
            orbit: (*orbit).to_owned(),
        })
        .collect();

    Ok(Scan { backdrops, entries })
}

pub(crate) fn document(entries: &[Entry]) -> String {
    let mut entries = entries.to_vec();
    entries.sort_by(|first, second| first.destination.cmp(&second.destination));
    let mut document = String::from(
        "# Generated by Sundial from the installed Destiny 2 packages.\r\n\
# Rebuilt when Sunrise settings are saved in Sundial.\r\n",
    );
    for entry in entries {
        document.push_str(&entry.destination);
        document.push_str(" = ");
        document.push_str(&entry.orbit);
        document.push_str("\r\n");
    }
    document
}

pub(crate) fn path(settings_path: &Path) -> Result<PathBuf, String> {
    crate::generated_file::path(settings_path, FILE_NAME)
}

pub(crate) fn save(settings_path: &Path, document: &str) -> Result<PathBuf, String> {
    crate::generated_file::save(
        settings_path,
        FILE_NAME,
        document,
        FILE_SIZE_LIMIT,
        "Orbit map",
    )
}

fn scan_backdrops(manager: &PackageManager) -> Result<Vec<String>, String> {
    let scenario_tag = manager
        .get_named_tag(ORBIT_SCENARIO_NAME, SCENARIO_CLASS)
        .ok_or("The install has no orbit_d2 scenario tag")?;
    let scenario = manager
        .read_tag(scenario_tag)
        .map_err(|error| format!("Could not read the Orbit scenario: {error}"))?;
    let (bubble_count, bubble_rows, bubble_class) =
        array_at(&scenario, SCENARIO_BUBBLE_DESCRIPTOR)?;
    if bubble_class != BUBBLE_CLASS {
        return Err(format!(
            "Orbit bubbles use unexpected class 0x{bubble_class:08X}"
        ));
    }

    let known_names = KNOWN_BUBBLE_NAMES
        .iter()
        .map(|name| (fnv1_name_hash(name), *name))
        .collect::<std::collections::HashMap<_, _>>();
    let character_creator_hash = fnv1_name_hash("character_creator");
    let mut backdrops = Vec::new();
    for index in 0..bubble_count {
        let bubble = bubble_rows + index * BUBBLE_STRIDE;
        let hash = u32_at(&scenario, bubble)?;
        if hash == character_creator_hash {
            continue;
        }
        let Ok((state_count, _, state_class)) =
            array_at(&scenario, bubble + BUBBLE_STATE_DESCRIPTOR)
        else {
            continue;
        };
        if state_count == 0 || state_class != SLICE_STATE_CLASS {
            continue;
        }
        if let Some(name) = known_names.get(&hash) {
            backdrops.push((*name).to_owned());
        }
    }
    if backdrops.is_empty() {
        return Err("The installed orbit_d2 scenario contains no usable backdrops".into());
    }
    Ok(backdrops)
}

fn sunrise_package_stem(package_name: &str) -> Option<String> {
    if !package_name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    let mut stem = package_name.to_ascii_lowercase();
    if let Some(value) = stem.strip_suffix("_activities") {
        stem = value.to_owned();
    } else if let Some(suffix) = stem.rfind("_unp") {
        let digits = &stem[suffix + 4..];
        if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
            stem.truncate(suffix);
        }
    }
    (!stem.is_empty() && stem.len() <= 40).then_some(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_stems_match_sunrise_normalization() {
        assert_eq!(
            sunrise_package_stem("edz_activities").as_deref(),
            Some("edz")
        );
        assert_eq!(sunrise_package_stem("edz_unp12").as_deref(), Some("edz"));
        assert_eq!(sunrise_package_stem("POLARIS").as_deref(), Some("polaris"));
        assert_eq!(sunrise_package_stem("bad-name"), None);
    }

    #[test]
    fn document_uses_internal_names_and_crlf() {
        let document = document(&[Entry {
            destination: "edz".into(),
            orbit: "orbit_earth_d2".into(),
        }]);
        assert!(document.ends_with("edz = orbit_earth_d2\r\n"));
        assert!(!document.contains("Earth"));
    }
}
