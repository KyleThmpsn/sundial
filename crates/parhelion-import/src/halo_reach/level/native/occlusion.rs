//! The map's ambient occlusion probes, which shade players, weapons and other objects Destiny
//! lights. The donor's occlusion record holds probes of a position and a sky visibility, 0.505 on
//! open ground, and the probes the donor placed describe the donor's own geometry. The map's copy
//! takes probes placed and valued from the converted level instead.
//!
//! Probes stand [`STANDING`] meters above the level's floors, near its spawn points and inside its
//! structure clusters: up to a [`SPAWN_SHARE`] of them at spawn points, then the floor points
//! farthest from every probe chosen so far, near the spawn points and the squads' spawn points
//! until [`NEAR_SPACING`] covers them and across the clusters after. Nightfall's clusters span
//! about 15 by 20 km of backdrop, and probes spread across them left its canyon 76 m from the
//! nearest. A campaign mission keeps its player starts at its
//! beginning, so probes near the spawns alone left every later stretch without one, and the
//! objects there took the donor's open-ground probe, bright even indoors. A point
//! whose rays meet the back of a face more often than [`INSIDE`] lies inside geometry and is
//! skipped. Each probe's value is the mean light arriving from [`RAYS`] directions over the whole
//! sphere: an escaping ray brings the sky, the upper quarter of the lightmap's ambient light, and a
//! ray that meets a lit face brings that face's baked light times [`ALBEDO`]. Destiny's own probes
//! hold geometric sky visibility. Destiny's combined ambient program lights a surface by the
//! square of twice a material value times its ambient visibility, so a dynamic object's shade
//! takes the square of the visibility it reads. Reach lit its objects from the baked light around
//! them, lamps included, so the value is the square root of that mean light as a share of the open
//! probes' typical light, times 0.505, and its square follows Reach's light. Taken without the
//! root, Breakneck's lamp-lit interiors held about 0.2 and the held weapon drew black there. The
//! open probes are the [`OPEN_SHARE`] of probes whose rays escape most often: few probes on
//! Breakneck or Sword Base see open sky in two of every five directions.
use super::{bounds, cross, dot, sub, unit};
use crate::halo_reach::level::{Level, lightmap::Atlas};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// How high above a floor a probe stands, in meters, about a player's chest.
const STANDING: f32 = 1.2;
/// The clear space a probe needs above its floor, in meters.
const HEADROOM: f32 = 2.0;
/// The least upward component of a face's normal that counts as floor.
const FLOOR_FACING: f32 = 0.7;
/// How far beyond the spawn points' bounds probes may stand, horizontally and vertically, in
/// meters.
const REGION_MARGIN: [f32; 3] = [32., 32., 16.];
/// One in this many probes at most stands at a spawn point. Taking every probe there left
/// Paradiso's 202 all on its 202 spawn points, and its caves drew objects by open-ground probes.
const SPAWN_SHARE: usize = 4;
/// The spacing the floors near the spawn points fill to before probes spread across the
/// structure clusters, in meters. A Forge World variant's clusters span the whole 27 km world,
/// where probes spread by distance alone would leave the variant's arena almost none.
const NEAR_SPACING: f32 = 6.;
/// The grid cell size squad spawn points mark nearby floor in, in meters.
const NEAR_CELL: f32 = 8.;
/// How far a ray looks for a face, in meters.
const REACH: f32 = 120.;
/// The cell size of the face grid, in meters.
const CELL: f32 = 4.;
/// The most grid cells a face's box may cover before rays test it directly instead. A mission's
/// distant backdrop faces span kilometres, and binning one in every cell it covers asked for
/// tens of gigabytes.
const LARGE_FACE_CELLS: i64 = 4096;
/// The spacing candidate floor points are thinned to, in meters.
const CANDIDATE_SPACING: f32 = 2.;
/// Rays per probe, and per inside test.
const RAYS: usize = 256;
const TEST_RAYS: usize = 48;
/// The share of rays meeting the back of a face beyond which a point lies inside geometry.
const INSIDE: f32 = 0.2;
/// The albedo a lit face reflects its baked light by.
const ALBEDO: f32 = 0.35;
/// The share of a lobe's light a face takes on average across its normals' angles to the lobe.
const LOBE_RESPONSE: f32 = 0.75;
/// The share of probes, those whose rays escape most often, that count as open.
const OPEN_SHARE: f32 = 0.1;
/// Destiny's sky visibility on open ground, and the range the map's values keep to.
pub(super) const OPEN_VISIBILITY: f32 = 0.505;
const VISIBILITY_RANGE: [f32; 2] = [0.04, 0.8];
const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];

struct Face {
    origin: [f32; 3],
    edges: [[f32; 3]; 2],
    normal: [f32; 3],
    lightmap: Option<[[f32; 2]; 3]>,
}

struct Hit {
    distance: f32,
    face: u32,
    back: bool,
    weights: [f32; 2],
}

/// The level's faces near the probes, binned by cell, and those too large to bin with their
/// bounds.
struct Scene {
    faces: Vec<Face>,
    cells: HashMap<[i32; 3], Vec<u32>>,
    large: Vec<(u32, [f32; 3], [f32; 3])>,
}

fn cell(point: [f32; 3]) -> [i32; 3] {
    point.map(|v| (v / CELL).floor() as i32)
}

impl Scene {
    fn new(level: &Level, low: [f32; 3], high: [f32; 3]) -> Self {
        let mut faces = Vec::new();
        let mut cells = HashMap::<[i32; 3], Vec<u32>>::new();
        let mut large = Vec::new();
        for primitive in &level.primitives {
            let lit = primitive.vertices.iter().all(|v| v.lightmap.is_some());
            for corners in primitive.indices.chunks_exact(3) {
                let vertices = [0, 1, 2].map(|k| &primitive.vertices[corners[k] as usize]);
                let points = vertices.map(|v| v.position);
                let (a, b) = bounds(points.into_iter());
                if (0..3).any(|k| b[k] < low[k] - REACH || a[k] > high[k] + REACH) {
                    continue;
                }
                let edges = [sub(points[1], points[0]), sub(points[2], points[0])];
                let Some(normal) = unit(cross(edges[0], edges[1])) else {
                    continue;
                };
                let index = u32::try_from(faces.len()).unwrap_or(u32::MAX);
                faces.push(Face {
                    origin: points[0],
                    edges,
                    normal,
                    lightmap: lit.then(|| vertices.map(|v| v.lightmap.unwrap_or_default())),
                });
                let (from, to) = (cell(a), cell(b));
                let covered = (0..3)
                    .map(|k| i64::from(to[k]) - i64::from(from[k]) + 1)
                    .product::<i64>();
                if covered > LARGE_FACE_CELLS {
                    large.push((index, a, b));
                    continue;
                }
                for x in from[0]..=to[0] {
                    for y in from[1]..=to[1] {
                        for z in from[2]..=to[2] {
                            cells.entry([x, y, z]).or_default().push(index);
                        }
                    }
                }
            }
        }
        Self {
            faces,
            cells,
            large,
        }
    }

    /// The large faces, in order, whose bounds come within `reach` of `point`. No other large
    /// face holds a point within `reach` of it, so rays from `point` that look no farther test
    /// these alone. Nightfall's backdrop left about 9,500 large faces, and every ray tested all
    /// of them.
    fn near(&self, point: [f32; 3], reach: f32) -> Vec<u32> {
        // A margin keeps rounding from dropping a face that touches the reach.
        let reach = reach + 1.;
        self.large
            .iter()
            .filter(|(_, low, high)| {
                (0..3).all(|k| low[k] <= point[k] + reach && high[k] >= point[k] - reach)
            })
            .map(|&(index, ..)| index)
            .collect()
    }

    /// The nearest face along the ray from `origin` toward unit `direction` within `reach`,
    /// testing of the large faces only `large`, which [`Scene::near`] gives for `origin`.
    fn cast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        reach: f32,
        large: &[u32],
    ) -> Option<Hit> {
        let mut at = cell(origin);
        let step = direction.map(|d| if d > 0. { 1 } else { -1 });
        let mut next: [f32; 3] = std::array::from_fn(|k| {
            if direction[k] == 0. {
                f32::INFINITY
            } else {
                let boundary = (at[k] + i32::from(direction[k] > 0.)) as f32 * CELL;
                (boundary - origin[k]) / direction[k]
            }
        });
        let delta = direction.map(|d| {
            if d == 0. {
                f32::INFINITY
            } else {
                CELL / d.abs()
            }
        });
        let mut best: Option<Hit> = None;
        let nearest = |index: u32, best: &mut Option<Hit>| {
            let face = &self.faces[index as usize];
            if let Some((distance, weights)) = intersect(face, origin, direction)
                && distance > 1e-3
                && distance <= reach
                && best.as_ref().is_none_or(|hit| distance < hit.distance)
            {
                *best = Some(Hit {
                    distance,
                    face: index,
                    back: dot(face.normal, direction) > 0.,
                    weights,
                });
            }
        };
        for &index in large {
            nearest(index, &mut best);
        }
        loop {
            let exit = next[0].min(next[1]).min(next[2]);
            for &index in self.cells.get(&at).into_iter().flatten() {
                nearest(index, &mut best);
            }
            if best.as_ref().is_some_and(|hit| hit.distance <= exit) {
                return best;
            }
            if exit > reach {
                return best;
            }
            let axis = if next[0] <= next[1] && next[0] <= next[2] {
                0
            } else if next[1] <= next[2] {
                1
            } else {
                2
            };
            at[axis] += step[axis];
            next[axis] += delta[axis];
        }
    }
}

/// The distance along the ray to `face` and the hit's weights of its second and third corners,
/// from either side.
fn intersect(face: &Face, origin: [f32; 3], direction: [f32; 3]) -> Option<(f32, [f32; 2])> {
    let [e1, e2] = face.edges;
    let p = cross(direction, e2);
    let determinant = dot(e1, p);
    if determinant.abs() < 1e-9 {
        return None;
    }
    let inverse = 1. / determinant;
    let s = sub(origin, face.origin);
    let u = dot(s, p) * inverse;
    if !(0. ..=1.).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(direction, q) * inverse;
    if v < 0. || u + v > 1. {
        return None;
    }
    Some((dot(e2, q) * inverse, [u, v]))
}

/// `count` directions spread evenly over the sphere.
fn sphere(count: usize) -> Vec<[f32; 3]> {
    let golden = std::f32::consts::PI * (3. - 5f32.sqrt());
    (0..count)
        .map(|i| {
            let z = 1. - 2. * (i as f32 + 0.5) / count as f32;
            let radius = (1. - z * z).max(0.).sqrt();
            let angle = golden * i as f32;
            [radius * angle.cos(), radius * angle.sin(), z]
        })
        .collect()
}

/// A texel's baked light as irradiance over pi, in the atlas's units, where the average lit
/// texel's is one: the lobe's at [`LOBE_RESPONSE`] plus the ambient light, or the ambient alone.
fn texel_light(atlas: &Atlas, uv: [f32; 2], ambient_only: bool) -> f32 {
    let x = ((uv[0] * atlas.width as f32) as usize).min(atlas.width - 1);
    let y = ((uv[1] * atlas.height as f32) as usize).min(atlas.height - 1);
    let at = (y * atlas.width + x) * 4;
    let channel = |bytes: &[u8], k: usize| (f32::from(bytes[at + k]) / 255.).powi(2);
    (0..3)
        .map(|k| {
            let lobe = if ambient_only {
                0.
            } else {
                channel(&atlas.lobe, k) * LOBE_RESPONSE
            };
            (lobe + channel(&atlas.ambient, k)) * LUMINANCE[k]
        })
        .sum::<f32>()
        * atlas.scale
}

/// The sky's light: the upper quarter of the atlas's ambient light, which open ground receives.
fn sky_light(atlas: &Atlas) -> f32 {
    let mut values = (0..atlas.width * atlas.height)
        .map(|i| {
            let uv = [
                ((i % atlas.width) as f32 + 0.5) / atlas.width as f32,
                ((i / atlas.width) as f32 + 0.5) / atlas.height as f32,
            ];
            texel_light(atlas, uv, true)
        })
        .filter(|&value| value > 0.)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return 1.;
    }
    let quarter = values.len() * 3 / 4;
    *values
        .select_nth_unstable_by(quarter, |a, b| a.total_cmp(b))
        .1
}

/// What a probe sees: the mean light arriving, the share of rays that escape and the share that
/// meet the back of a face.
struct View {
    light: f32,
    open: f32,
    back: f32,
}

fn view(
    scene: &Scene,
    atlas: Option<&Atlas>,
    sky: f32,
    point: [f32; 3],
    directions: &[[f32; 3]],
) -> View {
    let (mut light, mut open, mut back) = (0f32, 0usize, 0usize);
    let large = scene.near(point, REACH);
    for &direction in directions {
        match scene.cast(point, direction, REACH, &large) {
            None => {
                light += sky;
                open += 1;
            }
            Some(hit) if hit.back => back += 1,
            Some(hit) => {
                let face = &scene.faces[hit.face as usize];
                let baked = match (atlas, face.lightmap) {
                    (Some(atlas), Some(uv)) => {
                        let [u, v] = hit.weights;
                        let w = 1. - u - v;
                        texel_light(
                            atlas,
                            [0, 1].map(|k| w * uv[0][k] + u * uv[1][k] + v * uv[2][k]),
                            false,
                        )
                    }
                    _ => 1.,
                };
                light += ALBEDO * baked;
            }
        }
    }
    let count = directions.len().max(1) as f32;
    View {
        light: light / count,
        open: open as f32 / count,
        back: back as f32 / count,
    }
}

/// The mean light arriving at each of `points`, in the level's coordinates, as a probe's value
/// measures it: in the atlas's units, where the average lit texel's irradiance over pi is one.
pub(super) fn lights(level: &Level, points: &[[f32; 3]]) -> anyhow::Result<Vec<f32>> {
    if points.is_empty() {
        return Ok(Vec::new());
    }
    let (low, high) = bounds(points.iter().copied());
    let scene = Scene::new(level, low, high);
    let atlas = level.lightmap.as_ref();
    let sky = atlas.map_or(1., sky_light);
    let directions = sphere(RAYS);
    crate::cancellation::parallel_map(points, |&point| {
        view(&scene, atlas, sky, point, &directions).light
    })
}

/// `count` probes for `level`, each a position in the map's coordinates (the level's moved by
/// `offset`) and a sky visibility, the open probes' typical light that [`OPEN_VISIBILITY`] stands
/// for, and a report. `spawns` are the level's spawn points, `squads` its AI squads' spawn points
/// and `regions` the boxes of its structure clusters, in its own coordinates. None when the level
/// has no spawn points or no floor near them.
pub(super) fn probes(
    level: &Level,
    spawns: &[[f32; 3]],
    squads: &[[f32; 3]],
    regions: &[([f32; 3], [f32; 3])],
    count: usize,
    offset: [f32; 3],
) -> anyhow::Result<(Vec<[f32; 4]>, Option<f32>, Value)> {
    if spawns.is_empty() || count == 0 {
        return Ok((
            Vec::new(),
            None,
            json!({"reason": "The level has no spawn points or the donor no probes"}),
        ));
    }
    let started = std::time::Instant::now();
    let (spawn_low, spawn_high) = bounds(spawns.iter().copied());
    let spawn_region = (
        std::array::from_fn::<f32, 3, _>(|k| spawn_low[k] - REGION_MARGIN[k]),
        std::array::from_fn::<f32, 3, _>(|k| spawn_high[k] + REGION_MARGIN[k] + STANDING),
    );
    let regions = std::iter::once(spawn_region)
        .chain(
            regions
                .iter()
                .map(|&(low, high)| (low, [high[0], high[1], high[2] + STANDING])),
        )
        .collect::<Vec<_>>();
    let low: [f32; 3] =
        std::array::from_fn(|k| regions.iter().map(|r| r.0[k]).fold(f32::INFINITY, f32::min));
    let high: [f32; 3] = std::array::from_fn(|k| {
        regions
            .iter()
            .map(|r| r.1[k])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let scene = Scene::new(level, low, high);
    let scene_seconds = started.elapsed().as_secs_f64();
    let atlas = level.lightmap.as_ref();
    let sky = atlas.map_or(1., sky_light);
    // Floor points above upward faces in the region, one per spacing cell.
    let mut thinned = HashMap::<[i32; 3], [f32; 3]>::new();
    for face in &scene.faces {
        if face.normal[2] < FLOOR_FACING {
            continue;
        }
        let middle: [f32; 3] =
            std::array::from_fn(|k| face.origin[k] + (face.edges[0][k] + face.edges[1][k]) / 3.);
        let point = [middle[0], middle[1], middle[2] + STANDING];
        if !regions
            .iter()
            .any(|(low, high)| (0..3).all(|k| point[k] >= low[k] && point[k] <= high[k]))
        {
            continue;
        }
        thinned
            .entry(point.map(|v| (v / CANDIDATE_SPACING).floor() as i32))
            .or_insert(point);
    }
    // Sorted, so the choice does not follow the map's iteration order.
    let mut candidates = thinned.into_values().collect::<Vec<_>>();
    candidates.sort_by(|a, b| {
        a[0].total_cmp(&b[0])
            .then(a[1].total_cmp(&b[1]))
            .then(a[2].total_cmp(&b[2]))
    });
    let test = sphere(TEST_RAYS);
    let usable = |point: [f32; 3]| {
        let reach = HEADROOM - STANDING;
        scene
            .cast(point, [0., 0., 1.], reach, &scene.near(point, reach))
            .is_none()
            && view(&scene, None, sky, point, &test).back <= INSIDE
    };
    let placing = std::time::Instant::now();
    let mut chosen = Vec::with_capacity(count);
    let mut rejected = 0usize;
    let standing = spawns
        .iter()
        .map(|spawn| [spawn[0], spawn[1], spawn[2] + STANDING])
        .collect::<Vec<_>>();
    let seeds = count.div_ceil(SPAWN_SHARE).max(1);
    spread(&mut chosen, &standing, seeds, 0., &usable, &mut rejected);
    let seeded = chosen.len();
    // Near play: within the spawn points' bounds, or within the margin of a squad's spawn point,
    // to the nearest grid cell. A mission's player starts hold only its beginning, and its
    // squads stand along the rest of the way.
    let cell = |point: [f32; 3]| point.map(|v| (v / NEAR_CELL).floor() as i32);
    let mut squad_cells = HashSet::<[i32; 3]>::new();
    for point in squads {
        let low = cell(std::array::from_fn(|k| point[k] - REGION_MARGIN[k]));
        let high = cell(std::array::from_fn(|k| {
            point[k] + REGION_MARGIN[k] + STANDING
        }));
        for x in low[0]..=high[0] {
            for y in low[1]..=high[1] {
                for z in low[2]..=high[2] {
                    squad_cells.insert([x, y, z]);
                }
            }
        }
    }
    let (near_low, near_high) = spawn_region;
    let near = candidates
        .iter()
        .copied()
        .filter(|&point| {
            (0..3).all(|k| point[k] >= near_low[k] && point[k] <= near_high[k])
                || squad_cells.contains(&cell(point))
        })
        .collect::<Vec<_>>();
    spread(
        &mut chosen,
        &near,
        count,
        NEAR_SPACING,
        &usable,
        &mut rejected,
    );
    let near_spawns = chosen.len() - seeded;
    spread(&mut chosen, &candidates, count, 0., &usable, &mut rejected);
    if chosen.is_empty() {
        return Ok((
            Vec::new(),
            None,
            json!({"reason": "No usable floor point near the spawn points", "candidates": candidates.len(), "rejected": rejected}),
        ));
    }
    let placing_seconds = placing.elapsed().as_secs_f64();
    let placed = chosen.len();
    // Fewer usable points than the donor's probes repeat the first ones.
    for index in 0..count.saturating_sub(placed) {
        chosen.push(chosen[index % placed]);
    }
    let directions = sphere(RAYS);
    let viewing = std::time::Instant::now();
    let views = crate::cancellation::parallel_map(&chosen, |&point| {
        view(&scene, atlas, sky, point, &directions)
    })?;
    let views_seconds = viewing.elapsed().as_secs_f64();
    let median = |mut values: Vec<f32>| {
        if values.is_empty() {
            return None;
        }
        let middle = values.len() / 2;
        Some(
            *values
                .select_nth_unstable_by(middle, |a, b| a.total_cmp(b))
                .1,
        )
    };
    let mut by_openness = views.iter().collect::<Vec<_>>();
    by_openness.sort_by(|a, b| b.open.total_cmp(&a.open));
    let open_count = ((views.len() as f32 * OPEN_SHARE).ceil() as usize).clamp(1, views.len());
    let least_open = by_openness[open_count - 1].open;
    let reference = median(
        by_openness[..open_count]
            .iter()
            .map(|view| view.light)
            .collect(),
    )
    .unwrap_or(1.)
    .max(1e-4);
    let probes = chosen
        .iter()
        .zip(&views)
        .map(|(point, view)| {
            let visibility = (OPEN_VISIBILITY * (view.light / reference).sqrt())
                .clamp(VISIBILITY_RANGE[0], VISIBILITY_RANGE[1]);
            [
                point[0] + offset[0],
                point[1] + offset[1],
                point[2] + offset[2],
                visibility,
            ]
        })
        .collect::<Vec<_>>();
    let values = probes.iter().map(|p| p[3]).collect::<Vec<_>>();
    let report = json!({
        "probes": probes.len(),
        "placed": placed,
        "spawn_seeds": seeded,
        "near_spawns": near_spawns,
        "candidates": candidates.len(),
        "rejected": rejected,
        "faces": scene.faces.len(),
        "large_faces": scene.large.len(),
        "binned_faces": scene.cells.values().map(Vec::len).sum::<usize>(),
        "seconds": {"scene": scene_seconds, "placing": placing_seconds, "views": views_seconds},
        "sky_light": sky,
        "open_probes": open_count,
        "least_open_share": least_open,
        "reference_light": reference,
        "visibility_min": values.iter().copied().fold(f32::INFINITY, f32::min),
        "visibility_median": median(values.clone()),
        "visibility_max": values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        "region": [low, high],
        "rows": probes
            .iter()
            .zip(&views)
            .map(|(probe, view)| json!({
                "position": [probe[0], probe[1], probe[2]],
                "visibility": probe[3],
                "light": view.light,
                "open": view.open,
            }))
            .collect::<Vec<_>>(),
    });
    Ok((probes, Some(reference), report))
}

/// Adds points of `pool` to `chosen` by farthest point sampling, each the one farthest from every
/// point chosen, until `chosen` holds `count` or the farthest lies within `spacing` meters of one.
/// Points `usable` refuses count as `rejected`.
fn spread(
    chosen: &mut Vec<[f32; 3]>,
    pool: &[[f32; 3]],
    count: usize,
    spacing: f32,
    usable: &dyn Fn([f32; 3]) -> bool,
    rejected: &mut usize,
) {
    let distance = |a: [f32; 3], b: [f32; 3]| {
        let d = sub(a, b);
        dot(d, d)
    };
    let mut nearest = pool
        .iter()
        .map(|&c| {
            chosen
                .iter()
                .map(|&p| distance(c, p))
                .fold(f32::INFINITY, f32::min)
        })
        .collect::<Vec<_>>();
    while chosen.len() < count {
        let Some((index, &farthest)) = nearest
            .iter()
            .enumerate()
            .filter(|(_, d)| **d >= 0.)
            .max_by(|a, b| a.1.total_cmp(b.1))
        else {
            break;
        };
        if farthest < spacing * spacing {
            break;
        }
        let point = pool[index];
        nearest[index] = -1.;
        if !usable(point) {
            *rejected += 1;
            continue;
        }
        chosen.push(point);
        for (slot, &candidate) in nearest.iter_mut().zip(pool) {
            if *slot >= 0. {
                *slot = slot.min(distance(candidate, point));
            }
        }
    }
}
