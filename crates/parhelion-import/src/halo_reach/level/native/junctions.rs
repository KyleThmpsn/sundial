//! Splits the level's triangles at vertices that lie on their edges. Reach's structure compiler
//! cuts surfaces along cluster and portal planes, so one surface's edge often runs past a corner of
//! its neighbor, a T-junction. Drawn from floats, the corner stays on that edge. The converted
//! pieces store positions as 16-bit steps ([`super::POSITION_STEP`]), which move the corner off the
//! edge by up to half a step, and a hairline of the background showed along such edges: across
//! Reflection's ramps and walls and where its floors met them. A triangle with a vertex on one of
//! its edges is cut into a fan around its center through every such vertex, so it and its neighbor
//! share the corner and round it to one point.
use super::{dot, sub};
use crate::halo_reach::model::{Primitive, Vertex};
use std::collections::HashMap;

/// The farthest a vertex lies from an edge, in meters, and still counts as on it.
const ON_EDGE: f32 = 0.002;
/// The least distance, in meters, a vertex on an edge keeps from either end of it.
const FROM_END: f32 = 0.004;
/// The cell size, in meters, of the grid distinct positions are binned in.
const CELL: f32 = 0.5;
/// Positions within this many meters on every axis are one.
const SAME: f32 = 1e-4;

fn cell(point: [f32; 3]) -> [i32; 3] {
    point.map(|value| (value / CELL).floor() as i32)
}

/// The level's distinct vertex positions, each binned in every cell within [`ON_EDGE`] of it.
struct Points {
    points: Vec<[f32; 3]>,
    cells: HashMap<[i32; 3], Vec<u32>>,
}

impl Points {
    fn new(primitives: &[Primitive]) -> Self {
        let mut seen = HashMap::<[i64; 3], ()>::new();
        let mut points = Vec::new();
        let mut cells = HashMap::<[i32; 3], Vec<u32>>::new();
        for vertex in primitives.iter().flat_map(|primitive| &primitive.vertices) {
            let key = vertex.position.map(|value| (value / SAME).round() as i64);
            if seen.insert(key, ()).is_some() {
                continue;
            }
            let index = u32::try_from(points.len()).unwrap_or(u32::MAX);
            points.push(vertex.position);
            let low = cell(vertex.position.map(|value| value - ON_EDGE));
            let high = cell(vertex.position.map(|value| value + ON_EDGE));
            for x in low[0]..=high[0] {
                for y in low[1]..=high[1] {
                    for z in low[2]..=high[2] {
                        cells.entry([x, y, z]).or_default().push(index);
                    }
                }
            }
        }
        Self { points, cells }
    }

    /// The positions that lie on the edge from `a` to `b`, short of its ends, with their
    /// parameters along it, in order.
    fn on_edge(&self, a: [f32; 3], b: [f32; 3]) -> Vec<(f32, [f32; 3])> {
        let direction = sub(b, a);
        let squared = dot(direction, direction);
        let length = squared.sqrt();
        if length <= 2. * FROM_END {
            return Vec::new();
        }
        let mut found = Vec::new();
        // Walk the cells the edge passes through, by parameter along it.
        let mut at = cell(a);
        let end = cell(b);
        let step = direction.map(|d| if d > 0. { 1 } else { -1 });
        let mut next: [f32; 3] = std::array::from_fn(|k| {
            if direction[k] == 0. {
                f32::INFINITY
            } else {
                let boundary = (at[k] + i32::from(direction[k] > 0.)) as f32 * CELL;
                (boundary - a[k]) / direction[k]
            }
        });
        let delta = direction.map(|d| {
            if d == 0. {
                f32::INFINITY
            } else {
                CELL / d.abs()
            }
        });
        loop {
            for &index in self.cells.get(&at).into_iter().flatten() {
                let point = self.points[index as usize];
                let t = dot(sub(point, a), direction) / squared;
                if t * length <= FROM_END || (1. - t) * length <= FROM_END {
                    continue;
                }
                let closest: [f32; 3] = std::array::from_fn(|k| a[k] + direction[k] * t);
                let offset = sub(point, closest);
                if dot(offset, offset) <= ON_EDGE * ON_EDGE {
                    found.push((t, point));
                }
            }
            if at == end {
                break;
            }
            let axis = if next[0] <= next[1] && next[0] <= next[2] {
                0
            } else if next[1] <= next[2] {
                1
            } else {
                2
            };
            if next[axis] > 1. {
                break;
            }
            at[axis] += step[axis];
            next[axis] += delta[axis];
        }
        found.sort_by(|x, y| x.0.total_cmp(&y.0));
        found.dedup_by(|x, y| (x.0 - y.0) * length <= SAME);
        found
    }
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = dot(v, v).sqrt();
    if length > 0. {
        v.map(|value| value / length)
    } else {
        v
    }
}

/// The vertex `weights` of the way across `corners`, at `position`: every attribute blended, the
/// normal and tangent renormalized, and the bone binding and source index of the first corner.
fn blend(corners: &[&Vertex], weights: &[f32], position: [f32; 3]) -> Vertex {
    let sum3 = |get: &dyn Fn(&Vertex) -> [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|k| {
            corners
                .iter()
                .zip(weights)
                .map(|(v, w)| get(v)[k] * w)
                .sum()
        })
    };
    let tangent = normalized(sum3(&|v| [v.tangent[0], v.tangent[1], v.tangent[2]]));
    let lightmap = corners
        .iter()
        .map(|v| v.lightmap)
        .collect::<Option<Vec<_>>>()
        .map(|uvs| std::array::from_fn(|k| uvs.iter().zip(weights).map(|(uv, w)| uv[k] * w).sum()));
    Vertex {
        position,
        normal: normalized(sum3(&|v| v.normal)),
        tangent: [tangent[0], tangent[1], tangent[2], corners[0].tangent[3]],
        uv: std::array::from_fn(|k| corners.iter().zip(weights).map(|(v, w)| v.uv[k] * w).sum()),
        water_uv: corners
            .iter()
            .map(|vertex| vertex.water_uv)
            .collect::<Option<Vec<_>>>()
            .map(|values| {
                std::array::from_fn(|lane| {
                    values
                        .iter()
                        .zip(weights)
                        .map(|(uv, weight)| uv[lane] * weight)
                        .sum()
                })
            }),
        joints: corners[0].joints,
        weights: corners[0].weights,
        source: corners[0].source,
        lightmap: lightmap.or(corners[0].lightmap),
        lighting: corners
            .iter()
            .map(|v| v.lighting)
            .collect::<Option<Vec<_>>>()
            .map(|values| {
                std::array::from_fn(|row| {
                    std::array::from_fn(|lane| {
                        values
                            .iter()
                            .zip(weights)
                            .map(|(value, weight)| value[row][lane] * weight)
                            .sum()
                    })
                })
            }),
    }
}

/// Cuts every triangle of `primitives` with a vertex of the level on one of its edges into a fan
/// around its center through those vertices. Returns how many triangles were cut.
pub(super) fn split(primitives: &mut [Primitive]) -> usize {
    let points = Points::new(primitives);
    let mut cut = 0;
    for primitive in primitives.iter_mut() {
        let mut indices = Vec::with_capacity(primitive.indices.len());
        let faces = std::mem::take(&mut primitive.indices);
        for face in faces.chunks_exact(3) {
            let mut ring = Vec::new();
            for e in 0..3 {
                let (a, b) = (face[e], face[(e + 1) % 3]);
                ring.push(a);
                let (pa, pb) = (
                    primitive.vertices[a as usize].position,
                    primitive.vertices[b as usize].position,
                );
                for (t, position) in points.on_edge(pa, pb) {
                    let ends = [
                        &primitive.vertices[a as usize],
                        &primitive.vertices[b as usize],
                    ];
                    let vertex = blend(&ends, &[1. - t, t], position);
                    ring.push(u32::try_from(primitive.vertices.len()).unwrap_or(u32::MAX));
                    primitive.vertices.push(vertex);
                }
            }
            if ring.len() == 3 {
                indices.extend_from_slice(face);
                continue;
            }
            cut += 1;
            let corners = face
                .iter()
                .map(|&i| &primitive.vertices[i as usize])
                .collect::<Vec<_>>();
            let middle =
                std::array::from_fn(|k| corners.iter().map(|v| v.position[k]).sum::<f32>() / 3.);
            let center = blend(&corners, &[1. / 3.; 3], middle);
            let center_index = u32::try_from(primitive.vertices.len()).unwrap_or(u32::MAX);
            primitive.vertices.push(center);
            for k in 0..ring.len() {
                indices.extend_from_slice(&[center_index, ring[k], ring[(k + 1) % ring.len()]]);
            }
        }
        primitive.indices = indices;
    }
    cut
}
