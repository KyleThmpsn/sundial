//! A level's player spawn points: the scenario's scenery placements of the multiplayer respawn
//! and initial spawn point objects, or a map variant's in place of those its palette offers.
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

/// One spawn point in meters, facing `yaw` radians about the vertical axis.
#[derive(Clone, Copy, Debug)]
pub struct Spawn {
    pub position: [f32; 3],
    pub yaw: f32,
}

/// Reads the scenario's spawn points, and with a map `variant` the variant's, facing along their
/// forward axes, in place of the scenario's own placements of spawn objects its palette offers.
pub fn read(cache: &Cache, variant: Option<&Variant>) -> Result<Vec<Spawn>> {
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
    ensure!(
        !spawns.is_empty(),
        "The scenario places no player spawn points"
    );
    Ok(spawns)
}
