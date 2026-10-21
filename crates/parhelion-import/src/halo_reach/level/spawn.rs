//! A level's player spawn points: the scenario's scenery placements of the multiplayer respawn
//! and initial spawn point objects, or a map variant's in place of those its palette offers. A
//! campaign scenario places none, and its player starting locations in the loaded zone set stand
//! in for them.
use super::{super::cache::Cache, METRES_PER_UNIT, variant::Variant};
use anyhow::{Context, Result, ensure};

/// The scenario's scenery block and each row's palette index, placement flags, position in
/// world units and rotation in radians, yaw first.
const SCENERY: usize = 0xFC;
const SCENERY_SIZE: usize = 0xDC;
const SCENERY_FLAGS: usize = 0x04;
const SCENERY_POSITION: usize = 0x08;
const SCENERY_ROTATION: usize = 0x14;
/// A placement the scenario never creates.
const NEVER_PLACED: u32 = 1 << 6;
/// The scenery palette: one tag reference per row.
const PALETTE: usize = 0x108;
const PALETTE_SIZE: usize = 0x10;
const SPAWN_OBJECTS: [&str; 2] = [
    r"objects\multi\spawning\respawn_point",
    r"objects\multi\spawning\initial_spawn_point",
];
/// The scenario's player starting locations: each row's position in world units, facing in
/// radians, yaw first, and the insertion point it belongs to.
const STARTS: usize = 0x274;
const START_SIZE: usize = 0x1C;
const START_FACING: usize = 0x0C;
const START_INSERTION: usize = 0x14;
/// How far outside a cluster's bounds, in meters, a starting location still counts as inside it.
const START_MARGIN: f32 = 2.;

/// One spawn point in meters, facing `yaw` radians about the vertical axis.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Spawn {
    pub position: [f32; 3],
    pub yaw: f32,
}

/// One player starting location: where it stands, in meters, and its insertion point, the place a
/// mission starts from.
pub struct Start {
    pub spawn: Spawn,
    pub insertion: i16,
}

/// The scenario's player starting locations.
pub fn starts(cache: &Cache) -> Result<Vec<Start>> {
    let scenario = cache.only("scnr")?.address()?;
    cache
        .block(scenario + STARTS, START_SIZE)?
        .into_iter()
        .map(|row| {
            Ok(Start {
                spawn: Spawn {
                    position: cache.floats::<3>(row)?.map(|value| value * METRES_PER_UNIT),
                    yaw: cache.f32(row + START_FACING)?,
                },
                insertion: cache.i16(row + START_INSERTION)?,
            })
        })
        .collect()
}

/// The starting locations of the first insertion point with any inside the clusters of the
/// unshared BSPs among `bsps`, else of the first with any inside the clusters of any of them. A
/// campaign stretch loads the mission's shared terrain BSP, whose clusters can hold every insertion
/// point's starting locations: Tip of the Spear's span 1.1 km over three insertion points.
pub fn starts_in(cache: &Cache, bsps: &[super::Bsp]) -> Result<Vec<Spawn>> {
    let starts = starts(cache)?;
    for boxes in [
        super::cluster_boxes(cache, bsps.iter().filter(|bsp| !bsp.shared))?,
        super::cluster_boxes(cache, bsps)?,
    ] {
        let Some(insertion) = starts
            .iter()
            .find(|start| inside(&boxes, start.spawn.position))
            .map(|start| start.insertion)
        else {
            continue;
        };
        return Ok(starts
            .iter()
            .filter(|start| start.insertion == insertion && inside(&boxes, start.spawn.position))
            .map(|start| start.spawn)
            .collect());
    }
    Ok(Vec::new())
}

/// Whether `point` lies within `START_MARGIN` of any of `boxes`.
pub fn inside(boxes: &[([f32; 3], [f32; 3])], point: [f32; 3]) -> bool {
    boxes.iter().any(|(low, high)| {
        (0..3).all(|k| point[k] >= low[k] - START_MARGIN && point[k] <= high[k] + START_MARGIN)
    })
}

/// Reads the scenario's spawn points, and with a map `variant` the variant's, facing along their
/// forward axes, in place of the scenario's own placements of spawn objects its palette offers.
/// Without either, the scenario's player starting locations in the structure BSPs its zone set
/// `zone_set` loads stand in ([`starts_in`]).
pub fn read(cache: &Cache, variant: Option<&Variant>, zone_set: usize) -> Result<Vec<Spawn>> {
    let scenario = cache.only("scnr")?.address()?;
    let mut spawn_rows = Vec::new();
    for (index, row) in cache
        .block(scenario + PALETTE, PALETTE_SIZE)?
        .into_iter()
        .enumerate()
    {
        let Some(tag) = cache.reference(row, Some("scen"))? else {
            continue;
        };
        if SPAWN_OBJECTS.contains(&tag.path.as_str())
            && !variant.is_some_and(|variant| variant.replaces(&tag))
        {
            spawn_rows.push(i16::try_from(index).context("Scenery palette is too long")?);
        }
    }
    let mut spawns = Vec::new();
    for row in cache.block(scenario + SCENERY, SCENERY_SIZE)? {
        if !spawn_rows.contains(&cache.i16(row)?)
            || cache.u32(row + SCENERY_FLAGS)? & NEVER_PLACED != 0
        {
            continue;
        }
        let position = cache
            .floats::<3>(row + SCENERY_POSITION)?
            .map(|value| value * METRES_PER_UNIT);
        spawns.push(Spawn {
            position,
            yaw: cache.f32(row + SCENERY_ROTATION)?,
        });
    }
    for placement in variant.map_or(&[][..], |variant| variant.placements.as_slice()) {
        if SPAWN_OBJECTS.contains(&placement.tag.path.as_str()) {
            let [forward, ..] = placement.axes;
            spawns.push(Spawn {
                position: placement.position,
                yaw: forward[1].atan2(forward[0]),
            });
        }
    }
    if spawns.is_empty() {
        spawns = starts_in(cache, &super::bsps(cache, &[zone_set])?)?;
    }
    ensure!(
        !spawns.is_empty(),
        "The scenario places no player spawn points or starting locations in zone set {zone_set}"
    );
    Ok(spawns)
}
