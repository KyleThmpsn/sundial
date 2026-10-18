//! A level's kill volumes: the trigger volumes its scenario's kill list names, as the boxes and
//! sectors Reach places them, the safe zone whose outside may be left only for a while, and a
//! map variant's soft kill boundaries. Each becomes a flat footprint with a bottom and a top, the
//! form a native kill volume record keeps, and the convex prisms a Havok packfile holds for it.
//!
//! A trigger volume is a `0x7C`-byte row at scenario `+0x280`: a name string id, a type at
//! `+0x0C` (0 box, 1 sector), forward and up axes at `+0x10` and `+0x1C`, a position at `+0x28`
//! and extents at `+0x34` in world units, a sink at `+0x40` and a sector's points at `+0x44` in
//! `0x14`-byte rows, each a position. A box's position is its corner and its extents its lengths
//! along forward, left and up: Hang 'Em High's safe zone encloses every scenery row that way and
//! would reach far outside the level as a centre. The scenario lists the volumes that kill at
//! `+0x4C8` and its safe zones at `+0x4D4` as `i16` indices in 4-byte rows. A name with `soft`
//! in it starts a countdown instead of killing at once. A sector's points lie where its boundary
//! meets the ground, so its volume spans their heights with a margin below and its extents' height
//! above.
use super::{
    super::cache::Cache,
    METRES_PER_UNIT,
    spawn::Spawn,
    variant::{Placement, Shape, Variant},
};
use crate::tiger::physics::{self, Hull};
use anyhow::{Context, Result, bail, ensure};

const TRIGGER_VOLUMES: usize = 0x280;
const TRIGGER_VOLUME_SIZE: usize = 0x7C;
const VOLUME_NAME: usize = 0x00;
const VOLUME_TYPE: usize = 0x0C;
const VOLUME_FORWARD: usize = 0x10;
const VOLUME_UP: usize = 0x1C;
const VOLUME_POSITION: usize = 0x28;
const VOLUME_EXTENTS: usize = 0x34;
const VOLUME_SINK: usize = 0x40;
const VOLUME_POINTS: usize = 0x44;
const POINT_SIZE: usize = 0x14;
const BOX: i16 = 0;
const SECTOR: i16 = 1;
const KILL_TRIGGERS: usize = 0x4C8;
const SAFE_TRIGGERS: usize = 0x4D4;
const TRIGGER_SIZE: usize = 4;
/// The multiplayer object type of a variant's soft kill boundary.
const SOFT_KILL_BOUNDARY: u8 = 26;
/// How far below a sector's lowest point its volume reaches, and how far above its highest point
/// when its extents give it less, in meters.
const SECTOR_BELOW: f32 = 0.5;
const SECTOR_ABOVE: f32 = 1.5;
/// How far the catch-all volume below the level and the countdown volumes around its safe zone
/// reach outward and downward, and how far the countdown volumes rise above the safe zone, in
/// meters.
const REACH: f32 = 500.;
const SOFT_RISE: f32 = 300.;
/// How far below the lowest spawn point the catch-all begins when the scenario has no safe zone.
const CATCH_ALL_DROP: f32 = 30.;
/// The corners of a round boundary's footprint.
const ROUND_CORNERS: usize = 16;
/// The most corners a footprint keeps, as its triangles index corners by byte.
const MAX_CORNERS: usize = 200;
/// Corners closer than this, in meters, are one corner.
const SAME_CORNER: f32 = 1e-3;

/// What a volume does to a player inside it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// Kills at once.
    Kill,
    /// Starts a countdown, then kills.
    SoftKill,
}

/// One volume in the map's coordinates: a footprint in meters and the bottom and top it spans.
#[derive(Clone, Debug)]
pub struct Volume {
    pub name: String,
    pub kind: Kind,
    /// Three or more corners around the footprint, in order.
    pub corners: Vec<[f32; 2]>,
    /// The bottom and the top.
    pub z: [f32; 2],
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Twice the signed area of `corners`, positive counterclockwise.
fn doubled_area(corners: &[[f32; 2]]) -> f32 {
    let n = corners.len();
    (0..n)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// The turn from `o` through `a` to `b`, positive to the left.
fn turn(o: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Whether `p` lies strictly inside the triangle `a`, `b`, `c`, wound `ccw` or not.
fn inside(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2], ccw: bool) -> bool {
    [turn(a, b, p), turn(b, c, p), turn(c, a, p)]
        .iter()
        .all(|&t| if ccw { t > 0. } else { t < 0. })
}

/// `corners` without repeats of their neighbours.
fn distinct(corners: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(corners.len());
    for corner in corners {
        if out.last().is_none_or(|last| {
            (last[0] - corner[0]).abs() > SAME_CORNER || (last[1] - corner[1]).abs() > SAME_CORNER
        }) {
            out.push(corner);
        }
    }
    while out.len() > 1
        && out.first().zip(out.last()).is_some_and(|(first, last)| {
            (last[0] - first[0]).abs() <= SAME_CORNER && (last[1] - first[1]).abs() <= SAME_CORNER
        })
    {
        out.pop();
    }
    out
}

/// The four corners of the rectangle over `x` and `y`, counterclockwise.
fn rectangle(x: [f32; 2], y: [f32; 2]) -> Vec<[f32; 2]> {
    vec![[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]]
}

/// A round footprint of `ROUND_CORNERS` corners around `centre`.
fn round(centre: [f32; 2], radius: f32) -> Vec<[f32; 2]> {
    (0..ROUND_CORNERS)
        .map(|k| {
            let angle = k as f32 / ROUND_CORNERS as f32 * std::f32::consts::TAU;
            [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
            ]
        })
        .collect()
}

impl Volume {
    fn moved(mut self, offset: [f32; 3]) -> Self {
        for corner in &mut self.corners {
            corner[0] += offset[0];
            corner[1] += offset[1];
        }
        self.z = [self.z[0] + offset[2], self.z[1] + offset[2]];
        self
    }

    /// The lowest and highest corner of the volume's box.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut low = [f32::INFINITY, f32::INFINITY, self.z[0]];
        let mut high = [f32::NEG_INFINITY, f32::NEG_INFINITY, self.z[1]];
        for corner in &self.corners {
            for k in 0..2 {
                low[k] = low[k].min(corner[k]);
                high[k] = high[k].max(corner[k]);
            }
        }
        (low, high)
    }

    /// The footprint's centre of area, or its corners' mean when it has no area.
    pub fn centroid(&self) -> [f32; 2] {
        let n = self.corners.len();
        let doubled = doubled_area(&self.corners);
        if doubled.abs() > 1e-6 {
            let mut centre = [0f32; 2];
            for i in 0..n {
                let (a, b) = (self.corners[i], self.corners[(i + 1) % n]);
                let weight = a[0] * b[1] - b[0] * a[1];
                centre[0] += (a[0] + b[0]) * weight;
                centre[1] += (a[1] + b[1]) * weight;
            }
            return centre.map(|value| value / (3. * doubled));
        }
        let mut mean = [0f32; 2];
        for corner in &self.corners {
            mean[0] += corner[0] / n as f32;
            mean[1] += corner[1] / n as f32;
        }
        mean
    }

    /// The footprint cut into triangles of corner indices, by clipping ears, or fanned from its
    /// first corner when no ear is left.
    pub fn triangles(&self) -> Result<Vec<[u8; 3]>> {
        let corners = &self.corners;
        ensure!(
            (3..=MAX_CORNERS).contains(&corners.len()),
            "Volume {} has {} corners",
            self.name,
            corners.len()
        );
        let index = |k: usize| u8::try_from(k).context("Volume corner index");
        let ccw = doubled_area(corners) > 0.;
        let mut ring: Vec<usize> = (0..corners.len()).collect();
        let mut out = Vec::with_capacity(corners.len() - 2);
        while ring.len() > 3 {
            let n = ring.len();
            let ear = (0..n).find(|&k| {
                let (a, b, c) = (ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]);
                let bend = turn(corners[a], corners[b], corners[c]);
                let convex = if ccw { bend > 0. } else { bend < 0. };
                convex
                    && ring.iter().all(|&other| {
                        other == a
                            || other == b
                            || other == c
                            || !inside(corners[other], corners[a], corners[b], corners[c], ccw)
                    })
            });
            match ear {
                Some(k) => {
                    out.push([
                        index(ring[(k + n - 1) % n])?,
                        index(ring[k])?,
                        index(ring[(k + 1) % n])?,
                    ]);
                    ring.remove(k);
                }
                None => {
                    for k in 1..ring.len() - 1 {
                        out.push([index(ring[0])?, index(ring[k])?, index(ring[k + 1])?]);
                    }
                    return Ok(out);
                }
            }
        }
        out.push([index(ring[0])?, index(ring[1])?, index(ring[2])?]);
        Ok(out)
    }

    /// The volume's convex prisms, one per triangle of its footprint: six vertices and the
    /// planes of its bottom, top and three sides, each facing out.
    pub(crate) fn hulls(&self) -> Result<Vec<Hull>> {
        let [z0, z1] = self.z;
        ensure!(z1 > z0, "Volume {} has no height", self.name);
        let ccw = doubled_area(&self.corners) > 0.;
        self.triangles()?
            .into_iter()
            .map(|triangle| {
                let corners = triangle.map(|k| self.corners[usize::from(k)]);
                let mut vertices = Vec::with_capacity(6);
                for [x, y] in corners {
                    vertices.push([x, y, z1]);
                    vertices.push([x, y, z0]);
                }
                let mut planes = vec![[0., 0., -1., z0], [0., 0., 1., -z1]];
                for k in 0..3 {
                    let (p, q) = (corners[k], corners[(k + 1) % 3]);
                    let edge = [q[0] - p[0], q[1] - p[1]];
                    let length = (edge[0] * edge[0] + edge[1] * edge[1]).sqrt();
                    ensure!(
                        length > SAME_CORNER,
                        "Volume {} has a flat triangle",
                        self.name
                    );
                    let normal = if ccw {
                        [edge[1] / length, -edge[0] / length]
                    } else {
                        [-edge[1] / length, edge[0] / length]
                    };
                    planes.push([
                        normal[0],
                        normal[1],
                        0.,
                        -(normal[0] * p[0] + normal[1] * p[1]),
                    ]);
                }
                Ok(Hull {
                    vertices,
                    planes,
                    radius: 0.,
                })
            })
            .collect()
    }
}

/// The trigger volume at `row`, in meters, or none when it has no footprint or height.
fn trigger(cache: &Cache, row: usize) -> Result<Option<Volume>> {
    let name = cache.string_id(cache.u32(row + VOLUME_NAME)?)?;
    let kind = if name.contains("soft") {
        Kind::SoftKill
    } else {
        Kind::Kill
    };
    let scale = |v: [f32; 3]| v.map(|value| value * METRES_PER_UNIT);
    let forward = cache.floats::<3>(row + VOLUME_FORWARD)?;
    let up = cache.floats::<3>(row + VOLUME_UP)?;
    let position = scale(cache.floats::<3>(row + VOLUME_POSITION)?);
    let extents = scale(cache.floats::<3>(row + VOLUME_EXTENTS)?);
    let (corners, z) = match cache.i16(row + VOLUME_TYPE)? {
        BOX => {
            let left = cross(up, forward);
            let corner = |a: f32, b: f32, c: f32| -> [f32; 3] {
                std::array::from_fn(|k| {
                    position[k]
                        + forward[k] * extents[0] * a
                        + left[k] * extents[1] * b
                        + up[k] * extents[2] * c
                })
            };
            let bottom = [
                corner(0., 0., 0.),
                corner(1., 0., 0.),
                corner(1., 1., 0.),
                corner(0., 1., 0.),
            ];
            let top = [
                corner(0., 0., 1.),
                corner(1., 0., 1.),
                corner(1., 1., 1.),
                corner(0., 1., 1.),
            ];
            let mut low = [f32::INFINITY; 3];
            let mut high = [f32::NEG_INFINITY; 3];
            for point in bottom.iter().chain(top.iter()) {
                for k in 0..3 {
                    low[k] = low[k].min(point[k]);
                    high[k] = high[k].max(point[k]);
                }
            }
            let level = up[2].abs() > 0.999 && forward[2].abs() < 1e-3;
            let corners = if level {
                bottom.iter().map(|[x, y, _]| [*x, *y]).collect()
            } else {
                rectangle([low[0], high[0]], [low[1], high[1]])
            };
            (corners, [low[2], high[2]])
        }
        SECTOR => {
            let mut corners = Vec::new();
            let (mut z0, mut z1) = (f32::INFINITY, f32::NEG_INFINITY);
            for point in cache.block(row + VOLUME_POINTS, POINT_SIZE)? {
                let [x, y, z] = scale(cache.floats::<3>(point)?);
                corners.push([x, y]);
                z0 = z0.min(z);
                z1 = z1.max(z);
            }
            let sink = cache.f32(row + VOLUME_SINK)?.max(0.) * METRES_PER_UNIT;
            (
                corners,
                [z0 - SECTOR_BELOW - sink, z1 + extents[2].max(SECTOR_ABOVE)],
            )
        }
        other => bail!("Trigger volume {name} has type {other}"),
    };
    let corners = distinct(corners);
    if corners.len() < 3 || z[1] <= z[0] {
        return Ok(None);
    }
    ensure!(
        corners.len() <= MAX_CORNERS,
        "Trigger volume {name} has {} corners",
        corners.len()
    );
    Ok(Some(Volume {
        name,
        kind,
        corners,
        z,
    }))
}

/// A variant's soft kill boundary as a volume in meters, or none for an object without a shape.
fn boundary(placement: &Placement) -> Option<Volume> {
    let p = placement.position;
    let [forward, left, _] = placement.axes;
    let name = placement
        .label
        .clone()
        .unwrap_or_else(|| "soft_kill_boundary".to_owned());
    let (corners, z) = match placement.shape {
        Shape::None => return None,
        Shape::Sphere { radius } => (round([p[0], p[1]], radius), [p[2] - radius, p[2] + radius]),
        Shape::Cylinder {
            radius,
            top,
            bottom,
        } => (round([p[0], p[1]], radius), [p[2] - bottom, p[2] + top]),
        Shape::Box {
            width,
            length,
            top,
            bottom,
        } => {
            let corner = |a: f32, b: f32| {
                [
                    p[0] + forward[0] * length * a + left[0] * width * b,
                    p[1] + forward[1] * length * a + left[1] * width * b,
                ]
            };
            (
                vec![
                    corner(-0.5, -0.5),
                    corner(0.5, -0.5),
                    corner(0.5, 0.5),
                    corner(-0.5, 0.5),
                ],
                [p[2] - bottom, p[2] + top],
            )
        }
    };
    let corners = distinct(corners);
    (corners.len() >= 3 && z[1] > z[0]).then_some(Volume {
        name,
        kind: Kind::SoftKill,
        corners,
        z,
    })
}

/// The bounds of `spawns`.
fn spawn_bounds(spawns: &[Spawn]) -> Result<([f32; 3], [f32; 3])> {
    ensure!(!spawns.is_empty(), "The level has no spawn points");
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for spawn in spawns {
        for k in 0..3 {
            low[k] = low[k].min(spawn.position[k]);
            high[k] = high[k].max(spawn.position[k]);
        }
    }
    Ok((low, high))
}

/// Reads the kill volumes of `cache`'s scenario and of `variant`, moved by `offset` meters, after
/// a catch-all volume below the level that kills and, around the scenario's safe zone, four
/// volumes that start the countdown. Without a safe zone the catch-all begins below the lowest of
/// `spawns`, which are already in the map's coordinates.
pub fn read(
    cache: &Cache,
    variant: Option<&Variant>,
    offset: [f32; 3],
    spawns: &[Spawn],
) -> Result<Vec<Volume>> {
    let scenario = cache.only("scnr")?.address()?;
    let rows = cache.block(scenario + TRIGGER_VOLUMES, TRIGGER_VOLUME_SIZE)?;
    let indices = |field: usize| -> Result<Vec<usize>> {
        cache
            .block(scenario + field, TRIGGER_SIZE)?
            .into_iter()
            .map(|row| {
                let index = cache.i16(row)?;
                usize::try_from(index)
                    .ok()
                    .filter(|&index| index < rows.len())
                    .with_context(|| {
                        format!(
                            "Trigger volume {index} is outside the scenario's {} volumes",
                            rows.len()
                        )
                    })
            })
            .collect()
    };
    let mut volumes = Vec::new();
    for index in indices(KILL_TRIGGERS)? {
        if let Some(volume) = trigger(cache, rows[index])? {
            volumes.push(volume.moved(offset));
        }
    }
    let mut safe = None;
    for index in indices(SAFE_TRIGGERS)? {
        if let Some(zone) = trigger(cache, rows[index])? {
            safe = Some(zone.moved(offset).bounds());
            break;
        }
    }
    for placement in variant.map_or(&[][..], |variant| variant.placements.as_slice()) {
        if placement.kind != SOFT_KILL_BOUNDARY {
            continue;
        }
        if let Some(volume) = boundary(placement) {
            volumes.push(volume.moved(offset));
        }
    }
    let (low, high) = match safe {
        Some(bounds) => bounds,
        None => {
            let (low, high) = spawn_bounds(spawns)?;
            ([low[0], low[1], low[2] - CATCH_ALL_DROP], high)
        }
    };
    volumes.insert(
        0,
        Volume {
            name: "catch_all".to_owned(),
            kind: Kind::Kill,
            corners: rectangle(
                [low[0] - REACH, high[0] + REACH],
                [low[1] - REACH, high[1] + REACH],
            ),
            z: [low[2] - REACH, low[2]],
        },
    );
    if safe.is_some() {
        let z = [low[2], high[2] + SOFT_RISE];
        let sides = [
            (
                "safe_zone_west",
                [low[0] - REACH, low[0]],
                [low[1] - REACH, high[1] + REACH],
            ),
            (
                "safe_zone_east",
                [high[0], high[0] + REACH],
                [low[1] - REACH, high[1] + REACH],
            ),
            (
                "safe_zone_south",
                [low[0], high[0]],
                [low[1] - REACH, low[1]],
            ),
            (
                "safe_zone_north",
                [low[0], high[0]],
                [high[1], high[1] + REACH],
            ),
        ];
        for (name, x, y) in sides {
            volumes.push(Volume {
                name: name.to_owned(),
                kind: Kind::SoftKill,
                corners: rectangle(x, y),
                z,
            });
        }
    }
    Ok(volumes)
}

/// A Havok packfile holding each volume's prisms, in the layout of `template`, a stock volume
/// packfile: a shape array with a list shape of up to three prisms or a static compound of more
/// per volume.
pub fn packfile(template: &[u8], volumes: &[Volume]) -> Result<Vec<u8>> {
    let hulls = volumes
        .iter()
        .map(Volume::hulls)
        .collect::<Result<Vec<_>>>()?;
    physics::volume_shapes(template, &hulls)
}
