//! What a converted map's activity takes from the level besides its geometry and spawn points:
//! its kill volumes and its labelled objectives, read from one open cache.
use super::{
    super::cache::Cache,
    objectives::{self, Objective},
    spawn::Spawn,
    variant,
    volumes::{self, Volume},
};
use anyhow::Result;
use std::path::Path;

pub struct Activity {
    pub volumes: Vec<Volume>,
    pub objectives: Vec<Objective>,
}

/// Reads the kill volumes and labelled objectives of cache `map` in `maps`, with its `variant`
/// file, moved by `offset` meters. `spawns` are the level's spawn points already in the map's
/// coordinates.
pub fn load(
    maps: &Path,
    map: &str,
    variant: Option<&Path>,
    offset: [f32; 3],
    spawns: &[Spawn],
) -> Result<Activity> {
    let cache = Cache::open(&maps.join(map))?;
    let variant = variant
        .map(|path| variant::read(&cache, path))
        .transpose()?;
    Ok(Activity {
        volumes: volumes::read(&cache, variant.as_ref(), offset, spawns)?,
        objectives: objectives::read(&cache, variant.as_ref(), offset)?,
    })
}
