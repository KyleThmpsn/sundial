//! What a converted map's activity takes from the level besides its geometry and spawn points:
//! its kill volumes, its labelled objectives and where its AI squads stand, read from one open
//! cache.
use super::{
    super::cache::Cache,
    objectives::{self, Objective},
    spawn::Spawn,
    squads, variant,
    volumes::{self, Volume},
};
use anyhow::Result;
use std::path::Path;

pub struct Activity {
    pub volumes: Vec<Volume>,
    pub objectives: Vec<Objective>,
    /// The squads in the structure BSPs the zone sets load, with their spawn points there.
    pub squads: Vec<squads::Squad>,
}

/// Reads the kill volumes, labelled objectives and squad spawn points of cache `map` in `maps`,
/// with its `variant` file, moved by `offset` meters. `spawns` are the level's spawn points
/// already in the map's coordinates, and `zone_sets` the scenario zone sets the map loads.
pub fn load(
    maps: &Path,
    map: &str,
    variant: Option<&Path>,
    offset: [f32; 3],
    spawns: &[Spawn],
    zone_sets: &[usize],
) -> Result<Activity> {
    let cache = Cache::open(&maps.join(map))?;
    let variant = variant
        .map(|path| variant::read(&cache, path))
        .transpose()?;
    let loaded = super::bsps(&cache, zone_sets)?
        .into_iter()
        .map(|bsp| bsp.tag)
        .collect::<Vec<_>>();
    Ok(Activity {
        volumes: volumes::read(&cache, variant.as_ref(), offset, spawns)?,
        objectives: objectives::read(&cache, variant.as_ref(), offset)?,
        squads: squads::read_squads(&cache, offset, super::bsp_mask(&cache, &loaded)?)?,
    })
}
