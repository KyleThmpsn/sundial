//! The map's cubemap volumes, from the level's own baked cubemaps. Destiny gives players, weapons and
//! other objects it lights their environment reflections from the cubemap volume they stand in, and
//! the donor's volumes hold the donor's surroundings, so the map places a volume for each of the
//! level's cubes instead.
//!
//! A scenario's structure BSP row names that BSP's cubemap bitmap at `+0x58`, one cube per image.
//! Each of the BSP's clusters (`0x8C` byte rows at `+0x138`) holds its bounds, three ranges in
//! world units, its mesh at `+0x40` and at `+0x80` the cubes that serve it, Reach's
//! `structure_cluster_cubemap` rows of `0x10` bytes: a scenario cubemap index (the scenario's
//! `0x20` byte rows at `+0x7B8`), at `+0x02` the cubemap bitmap index, the image of the BSP's
//! bitmap, and at `+0x04` the cube's reference points. A cube's volume spans every cluster it alone
//! serves. A cluster served by several cubes, as a level's large outer cluster is, gives each of
//! them the part of its bounds within half the distance to the cluster's nearest other cube.
//! Volumes of clusters served by one cube form interior volumes. The level's whole bounds take the
//! cube nearest the middle of its most shared cluster as an exterior fallback. Shadowkeep sorts the
//! submitted volumes by their box volume. Its resource +0x40 scales radiance and does not encode
//! these spatial categories.
//!
//! Reach decodes a dynamic cubemap as `64 * rgb^2 * a` (`calc_environment_map_dynamic_ps` in
//! `templated\environment_mapping.fx`) and scales it by local baked light. The translated cube
//! normalizes each channel by its diffuse mip average and a bounded grid baseline. The native
//! spatial grid supplies broad baked chromaticity, while native visibility carries light magnitude.
//! This is a native approximation of source illumination, not full directional RGB lightmap storage.
//! The common light reference divided by open visibility squared applies brightness once. Reach
//! samples its cubes with Y negated, so the faces swap +Y with -Y and flip vertically, which assumes
//! Destiny samples a cube along the world direction as D3D does. That was not traced.
use super::occlusion;
mod grid;
use crate::{
    halo_reach::{
        cache::{Cache, Tag},
        level::{Level, lighting::Analytical},
        material, native,
        resource::Pages,
        rig::METRES_PER_UNIT,
    },
    presentation::Graph,
    tiger::reader::Reader,
};
use anyhow::{Context, Result, ensure};
pub use grid::Grid;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The scenario's structure BSP rows and the cubemap bitmap each names.
const SCENARIO_BSPS: (usize, usize) = (0x50, 0xAC);
const BSP_CUBEMAP: usize = 0x58;
/// A structure BSP's clusters, each cluster's mesh and cubes, and each cube's image and capture
/// points.
const CLUSTERS: (usize, usize) = (0x138, 0x8C);
const CLUSTER_MESH: usize = 0x40;
const CLUSTER_CUBES: (usize, usize) = (0x80, 0x10);
const CUBE_IMAGE: usize = 0x02;
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
    #[serde(default)]
    pub grid: Option<Grid>,
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

/// The image of its BSP's cubemap bitmap that the cluster cube row `entry` names, if any.
fn image(cache: &Cache, entry: usize) -> Result<Option<usize>> {
    Ok(usize::try_from(cache.i16(entry + CUBE_IMAGE)?).ok())
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
    loaded: u32,
    offset: [f32; 3],
    radiance: f32,
    reference: f32,
    samples: &[([f32; 3], [f32; 3])],
) -> Result<(Vec<CubemapVolume>, Value)> {
    ensure!(
        radiance.is_finite() && radiance > 0.,
        "Invalid cubemap radiance"
    );
    ensure!(
        reference.is_finite() && reference > 0.,
        "Invalid cubemap light reference"
    );
    let colors = grid::Colors::new(samples);
    let calibrated = occlusion::diffuse_reference(reference, radiance);
    ensure!(
        calibrated.is_finite() && calibrated > 0.,
        "Cubemap light reference is out of range"
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
                let Some(image) = image(cache, entry)? else {
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
    let mut volumes = Vec::new();
    let mut kinds = [0usize; 3];
    let mut skipped = Vec::new();
    // Each texture's symbol and its top level's mean luminance.
    let mut textures = BTreeMap::<(usize, usize), (String, f32)>::new();
    for cube in &placed {
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
                let last = source
                    .mips
                    .last()
                    .context("Source cube has no mip levels")?;
                let count = last.iter().map(|face| face.len() / 4).sum::<usize>();
                ensure!(count > 0, "Source cube has no diffuse texels");
                let mut diffuse = [0.; 3];
                for texel in last.iter().flat_map(|face| face.chunks_exact(4)) {
                    for k in 0..3 {
                        diffuse[k] += DYNAMIC_SCALE
                            * (f32::from(texel[k]) / 255.).powi(2)
                            * f32::from(texel[3])
                            / 255.
                            / count as f32;
                    }
                }
                let floor = (diffuse.iter().sum::<f32>() / 3. * 1e-4).max(1e-6);
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
                                            let source = DYNAMIC_SCALE
                                                * (f32::from(texel[k]) / 255.).powi(2)
                                                * alpha;
                                            (source + floor) / (diffuse[k] + floor)
                                                * calibrated
                                                * colors.baseline[k]
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
                textures.insert(key, (symbol.clone(), luminance));
                symbol
            }
        };
        let center = std::array::from_fn(|k| (cube.low[k] + cube.high[k]) / 2. + offset[k]);
        let extents = std::array::from_fn(|k| ((cube.high[k] - cube.low[k]) / 2.).max(LEAST_HALF));
        let grid = colors.write(
            native,
            graph,
            &format!("cubemap-grid-{}", volumes.len()),
            center,
            extents,
        )?;
        volumes.push(CubemapVolume {
            texture,
            grid: Some(grid),
            center,
            extents,
            capture: std::array::from_fn(|k| cube.capture[k] + offset[k]),
        });
        kinds[cube.kind as usize] += 1;
    }
    let report = json!({
        "volumes": volumes.len(),
        "textures": textures.len(),
        "native_radiance": radiance,
        "calibration_light": reference,
        "diffuse_reference": calibrated,
        "grid_samples": samples.len(),
        "grid_baseline": colors.baseline,
        "grid_role": "Unit-luminance baked colour. Native visibility supplies local brightness.",
        "interior": kinds[Kind::Interior as usize],
        "shared": kinds[Kind::Shared as usize],
        "exterior": kinds[Kind::Exterior as usize],
        "mean_luminance": textures.values().map(|(.., mean)| *mean).collect::<Vec<_>>(),
        "skipped": skipped,
    });
    Ok((volumes, report))
}

/// One cluster of a loaded structure BSP: its BSP, its box in meters in the level's coordinates,
/// and the image and reference points of each cube serving it.
struct Cluster {
    bsp: usize,
    low: [f32; 3],
    high: [f32; 3],
    cubes: Vec<(usize, Vec<[f32; 3]>)>,
}

/// The clusters of a level's loaded structure BSPs and each BSP's cubemap bitmap, for finding the
/// cube Reach gives a surface's `dynamic` environment map.
///
/// Reach's lightmapped entry points (`static_per_pixel`, `static_per_vertex` and their single
/// pass forms) sample only `dynamic_environment_map_0`. The structure renderer binds there the
/// image the first cube row of a cluster mesh's cluster names
/// (`c_dynamic_cubemap_sample::get_cluster_cubemap_index`), and for a placed instance the bitmap
/// index of its own `cubemap sample 0`, with a blend factor of one. Only probe-lit entry points
/// blend toward `dynamic_environment_map_1` by `dynamic_environment_blend.w`, which objects and
/// the camera's sample advance from their previous cube to their current one as they move. A
/// placed object's current cube is the one with the reference point nearest it among the cubes of
/// the cluster holding it (`search_for_cubemap_sample_in_cluster`), which a resting object fully
/// blends to.
pub(super) struct Clusters {
    bitmaps: BTreeMap<usize, Tag>,
    clusters: Vec<Cluster>,
    /// The image of each BSP's mesh drawn by a cluster, from the first cube row of the first
    /// cluster drawing it, by scenario BSP index and mesh.
    meshes: BTreeMap<(usize, usize), usize>,
    /// Whether each image of each BSP's cubemap bitmap is a cube, by scenario BSP index.
    cubes: BTreeMap<usize, Vec<bool>>,
}

impl Clusters {
    /// The clusters of the structure BSPs in `loaded`, a mask by scenario index, that name a
    /// cubemap bitmap.
    pub(super) fn read(cache: &Cache, loaded: u32) -> Result<Self> {
        let scenario = cache.only("scnr")?.address()?;
        let metres = |v: f32| v * METRES_PER_UNIT;
        let mut result = Self {
            bitmaps: BTreeMap::new(),
            clusters: Vec::new(),
            meshes: BTreeMap::new(),
            cubes: BTreeMap::new(),
        };
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
            // The bitmap's image rows (`0x38` bytes at `+0x7C`), a cube where the type at `+0x06`
            // is 2.
            let cubes = cache
                .block(bitmap.address()? + 0x7C, 0x38)?
                .into_iter()
                .map(|row| Ok(cache.u8(row + 0x06)? == 2))
                .collect::<Result<Vec<_>>>()?;
            result.cubes.insert(bsp, cubes);
            result.bitmaps.insert(bsp, bitmap);
            let mut drawn = BTreeSet::new();
            for cluster in cache.block(structure.address()? + CLUSTERS.0, CLUSTERS.1)? {
                let bounds = cache.floats::<6>(cluster)?;
                let entries = cache.block(cluster + CLUSTER_CUBES.0, CLUSTER_CUBES.1)?;
                // A mesh draws in the first cluster that names it, as the level reads it.
                if let Ok(mesh) = usize::try_from(cache.i16(cluster + CLUSTER_MESH)?)
                    && drawn.insert(mesh)
                    && let Some(&entry) = entries.first()
                    && let Some(image) = image(cache, entry)?
                {
                    result.meshes.insert((bsp, mesh), image);
                }
                let mut cubes = Vec::new();
                for entry in entries {
                    let Some(image) = image(cache, entry)? else {
                        continue;
                    };
                    let points = cache
                        .block(entry + CUBE_POINTS.0, CUBE_POINTS.1)?
                        .into_iter()
                        .map(|point| Ok(cache.floats::<3>(point)?.map(metres)))
                        .collect::<Result<Vec<_>>>()?;
                    if !points.is_empty() {
                        cubes.push((image, points));
                    }
                }
                if !cubes.is_empty() {
                    result.clusters.push(Cluster {
                        bsp,
                        low: [bounds[0], bounds[2], bounds[4]].map(metres),
                        high: [bounds[1], bounds[3], bounds[5]].map(metres),
                        cubes,
                    });
                }
            }
        }
        Ok(result)
    }

    /// The cube at `point`, in meters in the level's coordinates, among the structure BSPs `bsps`
    /// (a primitive's mask, zero for any): of the smallest cluster whose box holds the point, or
    /// of the cluster whose box lies nearest, the cube with the reference point nearest it. Its
    /// BSP's cubemap bitmap and image.
    pub(super) fn cube(&self, point: [f32; 3], bsps: u32) -> Option<(Tag, usize)> {
        let distance =
            |a: [f32; 3], b: [f32; 3]| (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>();
        // Distance outside a box, zero within it, then the box's volume.
        let rank = |cluster: &Cluster| {
            let outside = (0..3)
                .map(|k| {
                    (cluster.low[k] - point[k])
                        .max(point[k] - cluster.high[k])
                        .max(0.)
                        .powi(2)
                })
                .sum::<f32>()
                .sqrt();
            let volume = (0..3)
                .map(|k| (cluster.high[k] - cluster.low[k]).max(0.))
                .product::<f32>();
            (outside, volume)
        };
        let cluster = self
            .clusters
            .iter()
            .filter(|cluster| bsps == 0 || (bsps >> cluster.bsp) & 1 != 0)
            .min_by(|a, b| {
                let (a, b) = (rank(a), rank(b));
                a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))
            })?;
        let nearest = |points: &[[f32; 3]]| {
            points
                .iter()
                .map(|&p| distance(p, point))
                .fold(f32::INFINITY, f32::min)
        };
        let (image, _) = cluster
            .cubes
            .iter()
            .min_by(|a, b| nearest(&a.1).total_cmp(&nearest(&b.1)))?;
        Some((self.bitmaps.get(&cluster.bsp)?.clone(), *image))
    }

    /// The cube Reach binds for the cluster mesh `mesh` of the structure BSP with scenario index
    /// `bsp`, when its cluster names one.
    fn mesh(&self, bsp: usize, mesh: usize) -> Option<(Tag, usize)> {
        self.image(bsp, *self.meshes.get(&(bsp, mesh))?)
    }

    /// The image `image` of the cubemap bitmap of the structure BSP with scenario index `bsp`,
    /// when the bitmap holds it as a cube. Reach's `render_method_submit_dynamic_cubemap` binds a
    /// default texture for an index outside the bitmap or an image of another type.
    fn image(&self, bsp: usize, image: usize) -> Option<(Tag, usize)> {
        if !self.cubes.get(&bsp)?.get(image).copied()? {
            return None;
        }
        Some((self.bitmaps.get(&bsp)?.clone(), image))
    }
}

/// `material` with the cube `cube` as its dynamic environment map
/// ([`native::material::DYNAMIC_ROLE`]).
pub(super) fn with_dynamic_cube(
    material: &material::Material,
    (bitmap, image): (Tag, usize),
) -> material::Material {
    let mut material = material.clone();
    if let Some(property) = material.properties.first_mut() {
        property.textures.push(material::Mapping {
            role: native::material::DYNAMIC_ROLE.into(),
            bitmap: Some(bitmap),
            transform: [1., 1., 0., 0.],
            frame: image,
            address: 0x11,
            filter: 0,
            functions: Vec::new(),
        });
    }
    material
}

/// Whether `material`'s environment map is Reach's `dynamic` one, which reads the cube of the
/// cluster it is drawn in.
pub(super) fn dynamic_environment(material: &material::Material) -> bool {
    material
        .options
        .get("environment_mapping")
        .is_some_and(|option| option == "dynamic")
        && !material.properties.is_empty()
}

/// Splits each of the level's surfaces with a `dynamic` environment map by the cube Reach binds
/// for it ([`Clusters`]): a cluster mesh's by its cluster's first cube row, a placed instance's by
/// its own cube sample, and otherwise the cube [`Clusters::cube`] finds at the middle of its
/// bounds. Each variant carries its cube ([`with_dynamic_cube`]) and the analytical light of the
/// material it copies. Returns the variants added and how many surfaces took each source.
pub(super) fn dynamic(
    cache: &Cache,
    level: &mut Level,
    loaded: u32,
    analytical: &mut BTreeMap<u32, Analytical>,
) -> Result<Value> {
    if !level.materials.iter().any(dynamic_environment) {
        return Ok(json!({"variants": 0}));
    }
    let clusters = Clusters::read(cache, loaded)?;
    // Each level BSP's scenario index, and each placed instance's image by its BSP's scenario
    // index and its index among that BSP's instances, as its primitives name it.
    let scenario = level
        .bsps
        .iter()
        .map(|tag| {
            let mask = super::super::bsp_mask(cache, std::slice::from_ref(tag))?;
            Ok((mask.count_ones() == 1).then(|| mask.trailing_zeros() as usize))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut counts = BTreeMap::<usize, usize>::new();
    let mut instances = BTreeMap::new();
    // Placed instances whose stored image is a cube of their BSP's bitmap, names another image or
    // none, and the first few that name another, by scenario BSP index and image.
    let mut samples = [0usize; 3];
    let mut invalid = Vec::new();
    for instance in &level.instances {
        let count = counts.entry(instance.bsp).or_default();
        if instance.outside() {
            *count += 1;
            continue;
        }
        if let (Some(&Some(bsp)), Some(image)) = (scenario.get(instance.bsp), instance.cubemap) {
            if clusters.image(bsp, image).is_some() {
                samples[0] += 1;
            } else {
                samples[1] += 1;
                if invalid.len() < 16 {
                    invalid.push([bsp, image]);
                }
            }
            instances.insert((bsp, format!("instance{count}")), image);
        } else {
            samples[2] += 1;
        }
        *count += 1;
    }
    let mut sources = [0usize; 3];
    let mut variants = BTreeMap::<(usize, u32, usize), usize>::new();
    for primitive in &mut level.primitives {
        let Some(index) = primitive.material else {
            continue;
        };
        if !level.materials.get(index).is_some_and(dynamic_environment)
            || primitive.vertices.is_empty()
        {
            continue;
        }
        let bsp =
            (primitive.bsps.count_ones() == 1).then(|| primitive.bsps.trailing_zeros() as usize);
        let bound = bsp.and_then(|bsp| match &primitive.instance {
            Some(name) => instances
                .get(&(bsp, name.clone()))
                .and_then(|&image| clusters.image(bsp, image))
                .map(|cube| (1, cube)),
            None => clusters.mesh(bsp, primitive.section).map(|cube| (0, cube)),
        });
        let (source, cube) = match bound {
            Some(bound) => bound,
            None => {
                let (low, high) = super::bounds(primitive.vertices.iter().map(|v| v.position));
                let middle = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
                let Some(cube) = clusters.cube(middle, primitive.bsps) else {
                    continue;
                };
                (2, cube)
            }
        };
        sources[source] += 1;
        let key = (index, cube.0.datum, cube.1);
        let variant = match variants.get(&key) {
            Some(&variant) => variant,
            None => {
                let variant = level.materials.len();
                let copy = with_dynamic_cube(&level.materials[index], cube);
                level.materials.push(copy);
                if let Some(&light) = analytical.get(&u32::try_from(index)?) {
                    analytical.insert(u32::try_from(variant)?, light);
                }
                variants.insert(key, variant);
                variant
            }
        };
        primitive.material = Some(variant);
    }
    Ok(json!({
        "variants": variants.len(),
        "cluster_surfaces": sources[0],
        "instance_surfaces": sources[1],
        "nearest_surfaces": sources[2],
        "instance_cubes": samples[0],
        "instance_other_images": samples[1],
        "instance_without_image": samples[2],
        "instance_other_examples": invalid,
    }))
}
