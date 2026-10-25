//! The map's cubemap volumes, from the level's own baked cubemaps. Destiny gives players, weapons and
//! other objects it lights their environment reflections from the cubemap volume they stand in, and
//! the donor's volumes hold the donor's surroundings, so the map places a volume for each of the
//! level's cubes instead.
//!
//! A scenario's structure BSP row names that BSP's cubemap bitmap at `+0x58`, one cube per image.
//! Each of the BSP's clusters (`0x8C` byte rows at `+0x138`) holds its bounds, three ranges in
//! world units, and at `+0x80` the cubes that serve it: `0x10` byte rows of a scenario cubemap
//! index (the scenario's `0x20` byte rows at `+0x7B8`) and, at `+0x04`, the cube's capture points.
//! A BSP's bitmap holds an image for each scenario cubemap its clusters name, in increasing order
//! of index. Each of `m30`'s eight cubemap bitmaps holds exactly as many images as distinct
//! cubemaps its clusters name, 4 to 24, and its later BSPs name indices up to 89. The increasing
//! order is inferred from those counts, not traced in Reach's code. A cube's volume spans every
//! cluster it alone serves. A cluster served by several cubes, as a level's large outer cluster is,
//! gives each of them the part of its bounds within half the distance to the cluster's nearest
//! other cube. Volumes of clusters served by one cube form interior volumes. The level's whole
//! bounds take the cube nearest the middle of its most shared cluster as an exterior fallback.
//! Shadowkeep sorts the submitted volumes by their box volume. Its resource +0x40 scales radiance
//! and does not encode these spatial categories.
//!
//! Reach decodes a dynamic cubemap as `64 * rgb^2 * a` (`calc_environment_map_dynamic_ps` in
//! `templated\environment_mapping.fx`), normalized to an average of one, and scales it by the baked
//! light where it is sampled. A cube's texels therefore take the light the level's lightmap sends to
//! its capture point ([`super::occlusion::lights`]), at the level's camera-derived radiance. Reach
//! samples its cubes with Y negated, so the faces swap +Y with -Y and flip vertically, which assumes
//! Destiny samples a cube along the world direction as D3D does. That was not traced.
use super::occlusion;
use crate::{
    halo_reach::{
        cache::Cache, level::Level, material, native, resource::Pages, rig::METRES_PER_UNIT,
    },
    presentation::Graph,
    tiger::reader::Reader,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The scenario's structure BSP rows and the cubemap bitmap each names.
const SCENARIO_BSPS: (usize, usize) = (0x50, 0xAC);
const BSP_CUBEMAP: usize = 0x58;
/// A structure BSP's clusters, each cluster's cubes, and each cube's capture points.
const CLUSTERS: (usize, usize) = (0x138, 0x8C);
const CLUSTER_CUBES: (usize, usize) = (0x80, 0x10);
const CUBE_POINTS: (usize, usize) = (0x04, 0x10);
/// The least half size of a shared cube's part of its cluster, in meters.
const LEAST_HALF: f32 = 2.;
/// Reach's dynamic cubemap scale.
const DYNAMIC_SCALE: f32 = 64.;
const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// One cubemap volume in the map's coordinates: its texture's symbol, its box's middle and half
/// sizes and where its cube was captured. Cube texels already contain their radiance scale.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CubemapVolume {
    pub texture: String,
    pub center: [f32; 3],
    pub extents: [f32; 3],
    pub capture: [f32; 3],
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Exterior,
    Shared,
    Interior,
}

/// A cube's share of the level: its BSP and image, its capture point, its box and category, in
/// meters in the level's coordinates.
struct Cube {
    bsp: usize,
    image: usize,
    capture: [f32; 3],
    low: [f32; 3],
    high: [f32; 3],
    kind: Kind,
}

fn mean(points: &[[f32; 3]]) -> [f32; 3] {
    let count = points.len().max(1) as f32;
    std::array::from_fn(|k| points.iter().map(|p| p[k]).sum::<f32>() / count)
}

/// The cluster served by the most cubes, its BSP, its middle and the image and capture point of
/// each cube serving it. Image indices count within one BSP's cubemap bitmap, so its cubes are
/// named by its own BSP.
struct Outer {
    bsp: usize,
    middle: [f32; 3],
    served: Vec<(usize, [f32; 3])>,
}

/// A BSP's images, by the scenario cubemap each holds, from the cubemaps its clusters name.
fn images(cache: &Cache, clusters: &[usize]) -> Result<BTreeMap<usize, usize>> {
    let mut named = BTreeSet::new();
    for &cluster in clusters {
        for entry in cache.block(cluster + CLUSTER_CUBES.0, CLUSTER_CUBES.1)? {
            if let Ok(index) = usize::try_from(cache.i16(entry)?) {
                named.insert(index);
            }
        }
    }
    Ok(named
        .into_iter()
        .enumerate()
        .map(|(image, index)| (index, image))
        .collect())
}

/// The exterior cube spanning the level's `whole` bounds: the cube nearest the middle of the
/// `outer` cluster.
fn exterior(whole: Option<([f32; 3], [f32; 3])>, outer: Option<&Outer>) -> Option<Cube> {
    let ((low, high), outer) = (whole?, outer?);
    let &(image, capture) = outer.served.iter().min_by(|a, b| {
        let d = |p: [f32; 3]| {
            (0..3)
                .map(|k| (p[k] - outer.middle[k]).powi(2))
                .sum::<f32>()
        };
        d(a.1).total_cmp(&d(b.1))
    })?;
    Some(Cube {
        bsp: outer.bsp,
        image,
        capture,
        low,
        high,
        kind: Kind::Exterior,
    })
}

/// The level's cubemap volumes, moved by `offset` meters, with their cube textures added to
/// `graph`, and a report. Only the structure BSPs in `loaded`, a mask by scenario index, place
/// volumes, as only they place the map's geometry.
#[expect(clippy::too_many_arguments)]
pub(super) fn volumes(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    level: &Level,
    loaded: u32,
    offset: [f32; 3],
    radiance: f32,
) -> Result<(Vec<CubemapVolume>, Value)> {
    ensure!(
        radiance.is_finite() && radiance > 0.,
        "Invalid cubemap radiance"
    );
    let scenario = cache.only("scnr")?.address()?;
    let metres = |v: f32| v * METRES_PER_UNIT;
    let mut cubes = BTreeMap::<(usize, usize), Cube>::new();
    let mut bitmaps = BTreeMap::new();
    let mut whole: Option<([f32; 3], [f32; 3])> = None;
    let mut outer: Option<Outer> = None;
    for (bsp, row) in cache
        .block(scenario + SCENARIO_BSPS.0, SCENARIO_BSPS.1)?
        .into_iter()
        .enumerate()
    {
        if bsp >= 32 || (loaded >> bsp) & 1 == 0 {
            continue;
        }
        let (Some(structure), Ok(Some(bitmap))) = (
            cache.reference(row, Some("sbsp"))?,
            cache.reference(row + BSP_CUBEMAP, Some("bitm")),
        ) else {
            continue;
        };
        bitmaps.insert(bsp, bitmap);
        let clusters = cache.block(structure.address()? + CLUSTERS.0, CLUSTERS.1)?;
        let images = images(cache, &clusters)?;
        for cluster in clusters {
            let bounds = cache.floats::<6>(cluster)?;
            let low = [bounds[0], bounds[2], bounds[4]].map(metres);
            let high = [bounds[1], bounds[3], bounds[5]].map(metres);
            whole = Some(match whole {
                None => (low, high),
                Some((l, h)) => (
                    std::array::from_fn(|k| l[k].min(low[k])),
                    std::array::from_fn(|k| h[k].max(high[k])),
                ),
            });
            let mut served = Vec::new();
            for entry in cache.block(cluster + CLUSTER_CUBES.0, CLUSTER_CUBES.1)? {
                let Some(&image) = usize::try_from(cache.i16(entry)?)
                    .ok()
                    .and_then(|index| images.get(&index))
                else {
                    continue;
                };
                let points = cache
                    .block(entry + CUBE_POINTS.0, CUBE_POINTS.1)?
                    .into_iter()
                    .map(|point| Ok(cache.floats::<3>(point)?.map(metres)))
                    .collect::<Result<Vec<_>>>()?;
                if !points.is_empty() {
                    served.push((image, mean(&points)));
                }
            }
            let middle: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
            if served.len() > outer.as_ref().map_or(1, |outer| outer.served.len()) {
                outer = Some(Outer {
                    bsp,
                    middle,
                    served: served.clone(),
                });
            }
            for &(image, point) in &served {
                let (part_low, part_high, kind) = if served.len() == 1 {
                    (low, high, Kind::Interior)
                } else {
                    let nearest = served
                        .iter()
                        .filter(|(other, _)| *other != image)
                        .map(|(_, p)| {
                            (0..3)
                                .map(|k| (p[k] - point[k]).powi(2))
                                .sum::<f32>()
                                .sqrt()
                        })
                        .fold(f32::INFINITY, f32::min);
                    let half = (nearest / 2.).max(LEAST_HALF);
                    (
                        std::array::from_fn(|k| (point[k] - half).max(low[k])),
                        std::array::from_fn(|k| (point[k] + half).min(high[k])),
                        Kind::Shared,
                    )
                };
                let cube = cubes.entry((bsp, image)).or_insert(Cube {
                    bsp,
                    image,
                    capture: point,
                    low: part_low,
                    high: part_high,
                    kind,
                });
                cube.low = std::array::from_fn(|k| cube.low[k].min(part_low[k]));
                cube.high = std::array::from_fn(|k| cube.high[k].max(part_high[k]));
                cube.kind = cube.kind.max(kind);
            }
        }
    }
    let mut placed = cubes.into_values().collect::<Vec<_>>();
    // The whole level falls back to the outer cluster's middle cube.
    placed.extend(exterior(whole, outer.as_ref()));
    let lights = occlusion::lights(
        level,
        &placed.iter().map(|cube| cube.capture).collect::<Vec<_>>(),
    );
    let mut volumes = Vec::new();
    let mut kinds = [0usize; 3];
    let mut skipped = Vec::new();
    // Each texture's symbol, its capture point's light and its top level's mean luminance.
    let mut textures = BTreeMap::<(usize, usize), (String, f32, f32)>::new();
    for (cube, light) in placed.iter().zip(lights) {
        let key = (cube.bsp, cube.image);
        let texture = match textures.get(&key) {
            Some((symbol, ..)) => symbol.clone(),
            None => {
                let bitmap = &bitmaps[&cube.bsp];
                let source = match material::cube(cache, pages, bitmap, cube.image) {
                    Ok(cube) => cube,
                    Err(error) => {
                        skipped.push(json!({"bsp": cube.bsp, "image": cube.image, "reason": format!("{error:#}")}));
                        continue;
                    }
                };
                let scale = DYNAMIC_SCALE * light * radiance;
                let mips = source
                    .mips
                    .iter()
                    .enumerate()
                    .map(|(level, faces)| {
                        let size = (source.size >> level).max(1);
                        let radiance = faces.each_ref().map(|face| {
                            face.chunks_exact(4)
                                .map(|texel| {
                                    let alpha = f32::from(texel[3]) / 255.;
                                    std::array::from_fn(|k| {
                                        if k == 3 {
                                            1.
                                        } else {
                                            (f32::from(texel[k]) / 255.).powi(2) * alpha * scale
                                        }
                                    })
                                })
                                .collect::<Vec<[f32; 4]>>()
                        });
                        // Preserve each source mip while adapting the sampled direction's Y sign.
                        [0, 1, 3, 2, 4, 5].map(|face| {
                            (0..size * size)
                                .map(|i| radiance[face][(size - 1 - i / size) * size + i % size])
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect::<Vec<_>>();
                let top = mips.first().map_or(&[][..], |faces| &faces[..]);
                let texels = top.iter().map(Vec::len).sum::<usize>().max(1);
                let luminance = top
                    .iter()
                    .flatten()
                    .map(|t| (0..3).map(|k| t[k] * LUMINANCE[k]).sum::<f32>())
                    .sum::<f32>()
                    / texels as f32;
                let symbol = format!("cubemap-{}-{}", cube.bsp, cube.image);
                native::material::cube_mips(native, graph, &symbol, source.size, &mips)?;
                textures.insert(key, (symbol.clone(), light, luminance));
                symbol
            }
        };
        volumes.push(CubemapVolume {
            texture,
            center: std::array::from_fn(|k| (cube.low[k] + cube.high[k]) / 2. + offset[k]),
            extents: std::array::from_fn(|k| ((cube.high[k] - cube.low[k]) / 2.).max(LEAST_HALF)),
            capture: std::array::from_fn(|k| cube.capture[k] + offset[k]),
        });
        kinds[cube.kind as usize] += 1;
    }
    let report = json!({
        "volumes": volumes.len(),
        "textures": textures.len(),
        "native_radiance": radiance,
        "interior": kinds[Kind::Interior as usize],
        "shared": kinds[Kind::Shared as usize],
        "exterior": kinds[Kind::Exterior as usize],
        "lights": textures.values().map(|(_, light, _)| *light).collect::<Vec<_>>(),
        "mean_luminance": textures.values().map(|(.., mean)| *mean).collect::<Vec<_>>(),
        "skipped": skipped,
    });
    Ok((volumes, report))
}
