//! Source zone residency and the scenario's automatic preload and commit volumes.
//! Script-only zone variants stay with the preceding walkable area. Sector and rotated-box
//! bounds conservatively cover their source volumes, so target streaming may start earlier.
use super::{super::cache::Cache, METRES_PER_UNIT, ZONE_SET_SIZE, ZONE_SETS};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Bounds {
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

impl Bounds {
    pub fn center(self) -> [f32; 3] {
        std::array::from_fn(|k| (self.minimum[k] + self.maximum[k]) * 0.5)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Bubble {
    pub name: String,
    pub zone_sets: Vec<usize>,
    pub preload: Vec<Bounds>,
    pub commit: Vec<Bounds>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Plan {
    pub bubbles: Vec<Bubble>,
}

fn bounds(cache: &Cache, row: usize, offset: [f32; 3]) -> Result<Bounds> {
    let vector =
        |at| -> Result<[f32; 3]> { Ok([cache.f32(at)?, cache.f32(at + 4)?, cache.f32(at + 8)?]) };
    let extents = vector(row + 0x34)?;
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    let points = match cache.i16(row + 0xC)? {
        0 => {
            let forward = vector(row + 0x10)?;
            let up = vector(row + 0x1C)?;
            let left = [
                up[1] * forward[2] - up[2] * forward[1],
                up[2] * forward[0] - up[0] * forward[2],
                up[0] * forward[1] - up[1] * forward[0],
            ];
            let origin = vector(row + 0x28)?;
            (0..8)
                .map(|corner| {
                    std::array::from_fn(|k| {
                        origin[k]
                            + [forward, left, up]
                                .iter()
                                .enumerate()
                                .filter(|(axis, _)| corner & (1 << axis) != 0)
                                .map(|(axis, direction)| direction[k] * extents[axis])
                                .sum::<f32>()
                    })
                })
                .collect::<Vec<_>>()
        }
        1 => cache
            .block(row + 0x44, 0x14)?
            .into_iter()
            .map(vector)
            .collect::<Result<Vec<_>>>()?,
        kind => anyhow::bail!("Unsupported zone switch volume type {kind}"),
    };
    ensure!(!points.is_empty(), "Zone switch volume has no points");
    for point in points {
        for k in 0..3 {
            ensure!(
                point[k].is_finite(),
                "Zone switch volume has a nonfinite point"
            );
            minimum[k] = minimum[k].min(point[k]);
            maximum[k] = maximum[k].max(point[k]);
        }
    }
    if cache.i16(row + 0xC)? == 1 {
        minimum[2] -= cache.f32(row + 0x40)?;
        maximum[2] += extents[2];
    }
    for k in 0..3 {
        minimum[k] = minimum[k] * METRES_PER_UNIT + offset[k];
        maximum[k] = maximum[k] * METRES_PER_UNIT + offset[k];
        ensure!(
            minimum[k].is_finite() && maximum[k].is_finite() && minimum[k] < maximum[k],
            "Zone switch volume has invalid bounds"
        );
    }
    Ok(Bounds { minimum, maximum })
}

/// Selected zones in route order. A single zone or a scenario with no automatic switches keeps
/// the ordinary one-bubble path. No BSP is omitted when a script-only variant joins an area.
pub fn read(path: &Path, selected: &[usize], offset: [f32; 3]) -> Result<Option<Plan>> {
    if selected.len() < 2 {
        return Ok(None);
    }
    ensure!(
        selected.iter().copied().collect::<BTreeSet<_>>().len() == selected.len(),
        "Repeated streaming zone"
    );
    let cache = Cache::open(path)?;
    let scenario = cache.only("scnr")?.address()?;
    let zones = cache.block(scenario + ZONE_SETS, ZONE_SET_SIZE)?;
    let volumes = cache.block(scenario + 0x280, 0x7C)?;
    let switches = cache.block(scenario + 0x2C8, 8)?;
    if switches.is_empty() {
        return Ok(None);
    }
    let mut preloads = vec![Vec::new(); zones.len()];
    let mut commits = vec![Vec::new(); zones.len()];
    for row in switches {
        let volume = usize::try_from(cache.i16(row + 4)?)
            .ok()
            .and_then(|index| volumes.get(index))
            .context("Zone switch volume index")?;
        for (field, output) in [(2, &mut preloads), (6, &mut commits)] {
            let zone = cache.i16(row + field)?;
            if zone == -1 {
                continue;
            }
            let target = usize::try_from(zone)
                .ok()
                .and_then(|index| output.get_mut(index))
                .context("Zone switch destination index")?;
            target.push(bounds(&cache, *volume, offset)?);
        }
    }
    let mut bubbles: Vec<Bubble> = Vec::new();
    for &index in selected {
        let row = *zones
            .get(index)
            .context("Selected streaming zone is absent")?;
        if !bubbles.is_empty() && commits[index].is_empty() {
            bubbles.last_mut().unwrap().zone_sets.push(index);
            continue;
        }
        bubbles.push(Bubble {
            name: cache.string_id(cache.u32(row)?)?,
            zone_sets: vec![index],
            preload: preloads[index].clone(),
            commit: commits[index].clone(),
        });
    }
    ensure!(
        bubbles.len() <= 21,
        "Streaming graph exceeds the native 64-node ownership mask"
    );
    Ok((bubbles.len() > 1).then_some(Plan { bubbles }))
}
