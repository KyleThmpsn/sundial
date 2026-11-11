//! MCC water's body coordinates are float pairs per triangle corner, separate from wave UVs, and
//! the depth of the ground below each water vertex.
use super::*;
use std::collections::HashMap;

const WATER: u16 = 1;
/// The side, in meters, of the horizontal grid cells the ground's triangles are binned into.
const CELL: f32 = 8.;
/// The depth, in meters, a water vertex with no ground below takes. Reach's water is opaque well
/// before it at any murkiness the inspected levels use.
const DEEP: f32 = 100.;

/// The depth, in meters, of the level's ground below each water vertex (one with body
/// coordinates), by primitive and vertex index, where `ground` selects the primitives that count.
/// The depth runs to the highest ground surface directly below the vertex. A vertex with none below
/// but some above lies under the shore and takes zero, and one with none at all takes [`DEEP`].
/// Water whose refraction cannot read the scene depth fades by this as Reach fades shallow water
/// ([`crate::halo_reach::native::material`]'s water lighting), so its shore no longer ends at a
/// hard line where the ground crosses it. The depth interpolates between vertices, so the fade
/// follows the water mesh's triangles, 15 to 43 meters across on Paradiso's lake.
pub(super) fn depths(
    primitives: &[Primitive],
    ground: impl Fn(&Primitive) -> bool,
) -> HashMap<(u32, u32), f32> {
    let mut water = Vec::new();
    for (p, primitive) in primitives.iter().enumerate() {
        for (i, vertex) in primitive.vertices.iter().enumerate() {
            if vertex.water_uv.is_some() {
                water.push(((p as u32, i as u32), vertex.position));
            }
        }
    }
    if water.is_empty() {
        return HashMap::new();
    }
    let mut low = [f32::MAX; 2];
    let mut high = [f32::MIN; 2];
    for (_, position) in &water {
        for k in 0..2 {
            low[k] = low[k].min(position[k]);
            high[k] = high[k].max(position[k]);
        }
    }
    let last = [0, 1].map(|k| ((high[k] - low[k]) / CELL).floor() as i32);
    let cell = |x: f32, y: f32| {
        (
            (((x - low[0]) / CELL).floor() as i32).clamp(0, last[0]),
            (((y - low[1]) / CELL).floor() as i32).clamp(0, last[1]),
        )
    };
    let mut triangles = Vec::new();
    let mut grid = HashMap::<(i32, i32), Vec<u32>>::new();
    for primitive in primitives
        .iter()
        .filter(|primitive| ground(primitive))
        .filter(|primitive| {
            primitive
                .vertices
                .iter()
                .all(|vertex| vertex.water_uv.is_none())
        })
    {
        for face in primitive.indices.chunks_exact(3) {
            let Some(corners) = face
                .iter()
                .map(|&index| primitive.vertices.get(index as usize).map(|v| v.position))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let corners = [corners[0], corners[1], corners[2]];
            let minimum = [0, 1].map(|k| corners.iter().map(|c| c[k]).fold(f32::MAX, f32::min));
            let maximum = [0, 1].map(|k| corners.iter().map(|c| c[k]).fold(f32::MIN, f32::max));
            if maximum[0] < low[0]
                || minimum[0] > high[0]
                || maximum[1] < low[1]
                || minimum[1] > high[1]
            {
                continue;
            }
            let index = triangles.len() as u32;
            triangles.push(corners);
            let (from, to) = (cell(minimum[0], minimum[1]), cell(maximum[0], maximum[1]));
            for x in from.0..=to.0 {
                for y in from.1..=to.1 {
                    grid.entry((x, y)).or_default().push(index);
                }
            }
        }
    }
    water
        .into_iter()
        .map(|(key, [x, y, z])| {
            let mut below = None::<f32>;
            let mut above = false;
            for &index in grid.get(&cell(x, y)).into_iter().flatten() {
                let Some(height) = height_at(&triangles[index as usize], x, y) else {
                    continue;
                };
                if height > z {
                    above = true;
                } else {
                    below = Some(below.map_or(height, |b| b.max(height)));
                }
            }
            let depth = match below {
                Some(height) => (z - height).min(DEEP),
                None if above => 0.,
                None => DEEP,
            };
            (key, depth)
        })
        .collect()
}

/// The height of `triangle` at horizontal position `x`, `y`, when it covers that position.
fn height_at(triangle: &[[f32; 3]; 3], x: f32, y: f32) -> Option<f32> {
    let [a, b, c] = triangle;
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    if area.abs() < 1e-6 {
        return None;
    }
    let u = ((b[0] - x) * (c[1] - y) - (c[0] - x) * (b[1] - y)) / area;
    let v = ((c[0] - x) * (a[1] - y) - (a[0] - x) * (c[1] - y)) / area;
    let w = 1. - u - v;
    (u >= -1e-4 && v >= -1e-4 && w >= -1e-4).then(|| u * a[2] + v * b[2] + w * c[2])
}
const COORDINATE_SLOT: usize = 7;
const DECLARATION: u32 = 0x0018_001b;

pub(super) struct Coordinates {
    starts: BTreeMap<usize, usize>,
    bytes: Vec<u8>,
}

impl Coordinates {
    pub(super) fn read(
        cache: &Cache,
        pages: &mut Pages,
        geometry: &Geometry,
        mesh: usize,
        parts: &[usize],
    ) -> Result<Option<Self>> {
        let parts = parts
            .iter()
            .copied()
            .map(|part| Ok((part, cache.u16(part + 18)? & WATER != 0)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(part, water)| water.then_some(part))
            .collect::<Vec<_>>();
        if parts.is_empty() {
            return Ok(None);
        }
        let row = *geometry
            .sections
            .get(mesh)
            .context("Water mesh outside geometry")?;
        let starts = cache.block(row + 64, 2)?;
        ensure!(
            starts.len() == parts.len(),
            "Water part coordinate ranges differ"
        );
        let buffer = geometry
            .section_buffer(cache, mesh, COORDINATE_SLOT)?
            .context("Water mesh has no body-coordinate stream")?;
        let (_, declaration, bytes) = geometry.vertex_buffer(cache, pages, buffer)?;
        ensure!(
            declaration == DECLARATION,
            "Unsupported water-coordinate declaration {declaration:#x}"
        );
        // The declaration's 24-byte stride describes a triangle's three pairs. The source
        // allocation can include unused capacity, so part ranges bound the consumed prefix.
        let starts = parts
            .into_iter()
            .zip(starts)
            .map(|(part, start)| Ok((part, usize::from(cache.u16(start)?))))
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Some(Self { starts, bytes }))
    }

    pub(super) fn apply(
        &self,
        cache: &Cache,
        part: usize,
        range: usize,
        topology: u8,
        indices: &[u32],
        primitive: &mut Primitive,
    ) -> Result<()> {
        let Some(&start) = self.starts.get(&part) else {
            return Ok(());
        };
        ensure!(topology == 3, "Water coordinates require a triangle list");
        let part_first = usize::try_from(cache.i32(part + 4)?)?;
        let first = usize::try_from(cache.i32(range)?)?;
        let count = usize::try_from(cache.i32(range + 4)?)?;
        let start = start
            .checked_add(
                first
                    .checked_sub(part_first)
                    .context("Water subpart precedes part")?,
            )
            .context("Water coordinate start overflow")?;
        ensure!(
            count.is_multiple_of(3) && start.is_multiple_of(3),
            "Incomplete water triangle range"
        );
        let source = indices
            .get(
                first
                    ..first
                        .checked_add(count)
                        .context("Water index range overflow")?,
            )
            .context("Water draw outside index buffer")?;
        let end = start
            .checked_add(count)
            .and_then(|end| end.checked_mul(8))
            .context("Water coordinate range overflow")?;
        let bytes = self
            .bytes
            .get(start * 8..end)
            .context("Water coordinates outside stream")?;
        let original = primitive
            .vertices
            .iter()
            .map(|vertex| (vertex.source, vertex))
            .collect::<BTreeMap<_, _>>();
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        let mut shared = BTreeMap::new();
        for (face, values) in source.chunks_exact(3).zip(bytes.chunks_exact(24)) {
            if face[0] == face[1] || face[0] == face[2] || face[1] == face[2] {
                continue;
            }
            for (&index, pair) in face.iter().zip(values.chunks_exact(8)) {
                let uv = std::array::from_fn(|lane| {
                    f32::from_le_bytes(pair[lane * 4..lane * 4 + 4].try_into().unwrap())
                });
                ensure!(
                    uv.iter().all(|value| value.is_finite()),
                    "Nonfinite water coordinates"
                );
                let key = (index, uv.map(f32::to_bits));
                let target = match shared.get(&key) {
                    Some(&target) => target,
                    None => {
                        let mut vertex = (*original
                            .get(&index)
                            .context("Water corner outside decoded part")?)
                        .clone();
                        vertex.water_uv = Some(uv);
                        let target = u32::try_from(vertices.len())?;
                        vertices.push(vertex);
                        shared.insert(key, target);
                        target
                    }
                };
                triangles.push(target);
            }
        }
        primitive.vertices = vertices;
        primitive.indices = triangles;
        Ok(())
    }
}
