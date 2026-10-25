//! The map's lights, from the lamps the level's lightmapper baked. Reach lit its surfaces with these
//! lamps only through its lightmap, so a converted floor draws lit while a player standing at the
//! lamp draws unlit. Each becomes a Destiny point light that lights players, weapons and every
//! other surface within its range.
//!
//! A scenario structure BSP row names that BSP's structure lighting info (`stli`) at `+0x20`. Its
//! generic light definitions (`0x50` byte rows at `+0x04`) hold a type at `+0x00` (0 omni, 1 spot,
//! 2 directional), a color at `+0x08`, an intensity at `+0x14` and far attenuation bounds at
//! `+0x2C`, in world units. Its generic light instances (`0x84` byte rows at `+0x10`) hold a
//! definition index at `+0x00` and an origin at `+0x08`. These names come from the field
//! definitions built into the Reach tag build's executable, read with MCC U13 data. Directional
//! instances are the sun, which the map's sun already takes.
//!
//! Destiny's point light shading (pixel program `80FEB406`) fades its color by
//! `saturate(1 - distance / range)^2`, with no inverse-square term, so its color is the light at
//! its middle. Reach's lights mostly fade over their far attenuation bounds without inverse-square
//! falloff (Sword Base's definitions all hold flags `0xA`). A light's Destiny color is its source
//! color, clamped and raised to 2.2 as Reach's analytical lights decode it, times its intensity and
//! the level's radiance over pi times the lightmap's average lit level, as a surface emits its
//! Reach radiance over that level times the radiance, and its range is its far attenuation end.
//! That this matches the brightness Reach baked is an inference that needs an in-game check. Spot
//! lights become point lights of the same range, lighting all around them.
use crate::halo_reach::{cache::Cache, rig::METRES_PER_UNIT};
use anyhow::{Context, Result};
use serde_json::{Value, json};

const SCENARIO_BSPS: (usize, usize) = (0x50, 0xAC);
const BSP_LIGHTING: usize = 0x20;
const DEFINITIONS: (usize, usize) = (0x04, 0x50);
const INSTANCES: (usize, usize) = (0x10, 0x84);
const DEFINITION_TYPE: usize = 0x00;
const DEFINITION_FLAGS: usize = 0x02;
const DEFINITION_COLOR: usize = 0x08;
const DEFINITION_INTENSITY: usize = 0x14;
const DEFINITION_FAR: usize = 0x2C;
const INSTANCE_DEFINITION: usize = 0x00;
const INSTANCE_ORIGIN: usize = 0x08;
const INSTANCE_LIGHT_TYPE: usize = 0x2C;
const DIRECTIONAL: i16 = 2;
const SPOT: i16 = 1;

/// One of the level's lights: its position in the map's coordinates, its range in meters and its
/// color times its intensity in Destiny's units.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MapLight {
    pub position: [f32; 3],
    pub range: f32,
    pub color: [f32; 3],
}

/// The lights of the structure BSPs in `loaded`, a mask by scenario index, moved by `offset`
/// meters, at `radiance` over a lightmap whose average lit level is `level`, and a report.
pub(super) fn read(
    cache: &Cache,
    loaded: u32,
    offset: [f32; 3],
    radiance: f32,
    level: f32,
) -> Result<(Vec<MapLight>, Value)> {
    let scenario = cache.only("scnr")?.address()?;
    let scale = radiance / (std::f32::consts::PI * level);
    let mut lights = Vec::new();
    let mut kinds = Vec::new();
    let (mut directional, mut spots, mut unlit) = (0usize, 0usize, 0usize);
    for (bsp, row) in cache
        .block(scenario + SCENARIO_BSPS.0, SCENARIO_BSPS.1)?
        .into_iter()
        .enumerate()
    {
        if bsp >= 32 || (loaded >> bsp) & 1 == 0 {
            continue;
        }
        let Some(tag) = cache.reference(row + BSP_LIGHTING, Some("stli"))? else {
            continue;
        };
        let at = tag.address()?;
        let definitions = cache.block(at + DEFINITIONS.0, DEFINITIONS.1)?;
        for instance in cache.block(at + INSTANCES.0, INSTANCES.1)? {
            let index = usize::try_from(cache.i32(instance + INSTANCE_DEFINITION)?)?;
            let definition = *definitions
                .get(index)
                .context("Light instance definition outside its stli")?;
            match cache.i16(definition + DEFINITION_TYPE)? {
                DIRECTIONAL => {
                    directional += 1;
                    continue;
                }
                SPOT => spots += 1,
                _ => {}
            }
            let intensity = cache.f32(definition + DEFINITION_INTENSITY)?;
            let color = cache
                .floats::<3>(definition + DEFINITION_COLOR)?
                .map(|v| v.clamp(0., 1.).powf(2.2) * intensity * scale);
            let range = cache.floats::<2>(definition + DEFINITION_FAR)?[1] * METRES_PER_UNIT;
            let origin = cache.floats::<3>(instance + INSTANCE_ORIGIN)?;
            let position = std::array::from_fn(|k| origin[k] * METRES_PER_UNIT + offset[k]);
            let finite = color.iter().chain(&position).all(|v| v.is_finite()) && range.is_finite();
            if !finite || range <= 0. || color.iter().all(|&c| c <= 0.) {
                unlit += 1;
                continue;
            }
            lights.push(MapLight {
                position,
                range,
                color,
            });
            kinds.push(json!({
                "flags": cache.u16(definition + DEFINITION_FLAGS)?,
                "light_type": cache.i16(instance + INSTANCE_LIGHT_TYPE)?,
            }));
        }
    }
    let report = json!({
        "lights": lights.len(),
        "spots_as_points": spots,
        "directional_skipped": directional,
        "unlit_skipped": unlit,
        "scale": scale,
        "ranges": lights.iter().map(|light| light.range).collect::<Vec<_>>(),
        "colors": lights.iter().map(|light| light.color).collect::<Vec<_>>(),
        "kinds": kinds,
    });
    Ok((lights, report))
}
