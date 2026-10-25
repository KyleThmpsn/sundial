//! Optional source provenance for the Sunrise Map Diagnostics HUD. This document describes
//! selected source content, never the player's active source zone or native streaming state.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const FILE_NAME: &str = "map-diagnostics.json";
pub const BYTE_LIMIT: u64 = 256 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub maps: Vec<Map>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    pub activity: String,
    pub bubble_hash: u32,
    pub source_cache: String,
    pub source_build: String,
    pub zones: Vec<Zone>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    pub index: u32,
    pub name: String,
    pub bsps: Vec<String>,
}

impl Catalog {
    pub fn validate(&self) -> Result<(), String> {
        let text = |value: &str, limit: usize, empty: bool| {
            (empty || !value.is_empty())
                && value.len() <= limit
                && !value.contains(char::is_control)
        };
        if self.schema != 1 || self.maps.is_empty() || self.maps.len() > 128 {
            return Err("Invalid map diagnostics schema or map count".into());
        }
        let mut activities = BTreeSet::new();
        for map in &self.maps {
            if !text(&map.activity, 39, false)
                || !map
                    .activity
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
                || !activities.insert(&map.activity)
                || !text(&map.source_cache, 127, false)
                || map.source_cache.contains(['/', '\\'])
                || !text(&map.source_build, 63, false)
                || map.zones.is_empty()
                || map.zones.len() > 64
            {
                return Err("Invalid or repeated map diagnostics identity".into());
            }
            let mut zones = BTreeSet::new();
            for zone in &map.zones {
                if !zones.insert(zone.index)
                    || !text(&zone.name, 255, true)
                    || zone.bsps.is_empty()
                    || zone.bsps.len() > 64
                    || zone.bsps.iter().any(|bsp| !text(bsp, 255, false))
                    || zone.bsps.iter().collect::<BTreeSet<_>>().len() != zone.bsps.len()
                {
                    return Err("Invalid map diagnostics source zone".into());
                }
            }
        }
        Ok(())
    }
}
