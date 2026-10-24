//! A level's atmospheric fog, the atmosphere fog tag (`fogg`) of its scenario's fog palette that the
//! clusters of its loaded structure BSPs select. Forge World's palette names one,
//! `levels\multi\forge_halo\post_fx\forge_halo`.
use std::collections::BTreeMap;

use crate::halo_reach::{cache::Cache, cache::Tag, native};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// The scenario's fog palette, `0x18` byte rows each naming a fog tag at `+0x08`.
const PALETTE: (usize, usize) = (0x534, 0x18);
const PALETTE_FOG: usize = 0x08;
/// The scenario's per-BSP cluster rows, `0x54` bytes, with each cluster's fog palette index, an
/// `i16` in 4-byte rows, at `+0x30`. Compiled U13 caches strip the BSP reference at `+0x00`.
/// Their rows follow the scenario BSP list, with a fog choice for every cluster or no choices.
/// Every cluster of
/// The Package (`ff45_corvette`) selects its palette's sixth entry,
/// `levels\solo\m45\post_fx\m45_sky_corvette`, not its first, `m45_sky_mission`.
const CLUSTERS: (usize, usize) = (0x558, 0x54);
const CLUSTER_FOGS: usize = 0x30;
/// A fog tag's flags (sky band, ground band, fog light and patchy fog from bit 0), its distance
/// bias, then its sky and ground bands, each a floor, a height above it, a thickness, a falloff
/// end, a color and that color's intensity, and its fog light's color and intensity.
const FLAGS: usize = 0x00;
const BIAS: usize = 0x04;
const SKY: usize = 0x0C;
const GROUND: usize = 0x2C;
const LIGHT_COLOR: usize = 0x58;
const LIGHT_INTENSITY: usize = 0x64;
const SKY_ENABLED: u16 = 1;
const GROUND_ENABLED: u16 = 2;
const LIGHT_ENABLED: u16 = 4;

/// The level's fog and a report of its tag's values, or none when the palette names no fog tag.
/// The fog is the palette entry the most clusters of `bsps` select, the lowest on a tie, or the
/// palette's first fog tag when none selects one. A level draws one fog, so where its clusters
/// select several the report lists them. A disabled band does not fog. The fog light, a glow
/// toward the sun, is not drawn.
pub(super) fn read(
    cache: &Cache,
    bsps: &[Tag],
    lift: f32,
) -> Result<(Option<native::material::Fog>, Value)> {
    let scenario = cache.only("scnr")?.address()?;
    let rows = cache.block(scenario + PALETTE.0, PALETTE.1)?;
    let scenario_bsps = cache.block(scenario + 0x50, 0xAC)?;
    let cluster_rows = cache.block(scenario + CLUSTERS.0, CLUSTERS.1)?;
    let mut selected = BTreeMap::<usize, usize>::new();
    for (index, &row) in cluster_rows.iter().enumerate() {
        let bsp = match cache.reference(row, Some("sbsp"))? {
            Some(bsp) => bsp,
            None => {
                ensure!(
                    cluster_rows.len() == scenario_bsps.len(),
                    "Fog cluster rows with stripped BSP references do not match the scenario BSP list"
                );
                cache
                    .reference(scenario_bsps[index], Some("sbsp"))?
                    .context("Fog cluster row has no corresponding scenario BSP")?
            }
        };
        if !bsps.iter().any(|loaded| loaded.path == bsp.path) {
            continue;
        }
        let fogs = cache.block(row + CLUSTER_FOGS, 4)?;
        let clusters = cache.block(bsp.address()? + 0x138, 0x8C)?.len();
        ensure!(
            fogs.is_empty() || fogs.len() == clusters,
            "Fog choices for {} do not match its {clusters} clusters",
            bsp.path
        );
        for cluster in fogs {
            if let Ok(index) = usize::try_from(cache.i16(cluster)?) {
                ensure!(
                    index < rows.len(),
                    "Fog palette index {index} for {} is outside {} entries",
                    bsp.path,
                    rows.len()
                );
                *selected.entry(index).or_default() += 1;
            }
        }
    }
    let chosen = selected
        .iter()
        .filter(|&(&index, _)| index < rows.len())
        .max_by_key(|&(&index, &count)| (count, std::cmp::Reverse(index)))
        .map(|(&index, _)| index);
    let tag = match chosen {
        Some(index) => cache.reference(rows[index] + PALETTE_FOG, Some("fogg"))?,
        None => {
            let mut first = None;
            for &row in &rows {
                if let Some(found) = cache.reference(row + PALETTE_FOG, Some("fogg"))? {
                    first = Some(found);
                    break;
                }
            }
            first
        }
    };
    let selections = selected
        .iter()
        .map(|(index, count)| json!({"index": index, "clusters": count}))
        .collect::<Vec<_>>();
    let Some(tag) = tag else {
        return Ok((
            None,
            json!({
                "palette": rows.len(),
                "selected": selections,
                "reason": "The selected fog palette entry names no fog tag",
            }),
        ));
    };
    let at = tag.address()?;
    let flags = cache.u16(at + FLAGS)?;
    let band = |offset: usize, enabled: bool| -> Result<(native::material::Band, Value)> {
        let [base, height, thickness, reach] = cache.floats::<4>(at + offset)?;
        let color = cache.floats::<3>(at + offset + 0x10)?;
        let intensity = cache.f32(at + offset + 0x1C)?;
        let value = json!({
            "enabled": enabled,
            "base": base,
            "height": height,
            "thickness": thickness,
            "falloff_end": reach,
            "color": color,
            "intensity": intensity,
        });
        let band = if enabled {
            native::material::Band::from_thickness(
                base,
                height,
                thickness,
                reach,
                color.map(|c| c * intensity),
            )
        } else {
            native::material::Band::clear()
        };
        Ok((band, value))
    };
    let (sky, sky_report) = band(SKY, flags & SKY_ENABLED != 0)?;
    let (ground, ground_report) = band(GROUND, flags & GROUND_ENABLED != 0)?;
    let bias = cache.f32(at + BIAS)?;
    let report = json!({
        "palette": rows.len(),
        "selected": selections,
        "tag": tag.path,
        "flags": format!("{flags:04X}"),
        "distance_bias": bias,
        "sky": sky_report,
        "ground": ground_report,
        "sky_rate": sky.rate,
        "ground_rate": ground.rate,
        "fog_light": {
            "enabled": flags & LIGHT_ENABLED != 0,
            "color": cache.floats::<3>(at + LIGHT_COLOR)?,
            "intensity": cache.f32(at + LIGHT_INTENSITY)?,
            "drawn": false,
        },
        "lift": lift,
    });
    Ok((
        Some(native::material::Fog {
            sky,
            ground,
            bias,
            lift,
        }),
        report,
    ))
}
