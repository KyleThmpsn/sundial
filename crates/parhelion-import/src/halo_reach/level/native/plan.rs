//! The level seen from above: the highest surface in each cell of a grid over the play area, and
//! whether that surface is water. Parhelion's activity editor draws it as the map's overview.
use super::Spawn;
use crate::halo_reach::{material::Material, model::Primitive};
use anyhow::{Context, Result};
use std::path::Path;

/// The plan's image, beside the conversion.
pub const IMAGE: &str = "plan.png";
/// Cells along the plan's longer side.
const CELLS: f32 = 1024.;
/// How far the plan reaches past the spawn points' bounds, in meters.
const MARGIN: f32 = 250.;
/// How far above the highest spawn point a surface still shows, in meters. Roofs and ceilings
/// higher up leave what they cover in view.
const HEADROOM: f32 = 30.;

/// The plan image's frame. Its rows run from the largest y down. Red and green hold a cell's
/// height between `floor` (0) and `ceiling` (65,535), high byte first. Blue is 255 where the
/// surface is water. Alpha is 0 where no surface lies in the cell.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Plan {
    pub image: String,
    pub width: u32,
    pub height: u32,
    /// The image's lower left and upper right corners in the map's coordinates, in meters.
    pub minimum: [f32; 2],
    pub maximum: [f32; 2],
    pub floor: f32,
    pub ceiling: f32,
}

/// Twice the signed area of `a`, `b` and `p`, positive when they turn counterclockwise.
fn edge(a: [f32; 3], b: [f32; 3], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Draws `primitives`, moved by `offset`, over the play area of `spawns` (map coordinates) and
/// writes the image to `output`. Decals draw nothing. `None` when no surface lies in the frame.
pub fn write(
    primitives: &[Primitive],
    materials: &[Material],
    offset: [f32; 3],
    spawns: &[Spawn],
    output: &Path,
) -> Result<Option<Plan>> {
    let surfaces = primitives
        .iter()
        .filter_map(|primitive| {
            let material = primitive.material.and_then(|index| materials.get(index));
            (!material.is_some_and(|material| material.decal()))
                .then(|| (primitive, material.is_some_and(|material| material.water())))
        })
        .collect::<Vec<_>>();
    let place =
        |position: [f32; 3]| -> [f32; 3] { std::array::from_fn(|k| position[k] + offset[k]) };
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for vertex in surfaces
        .iter()
        .flat_map(|(primitive, _)| &primitive.vertices)
    {
        let point = place(vertex.position);
        for k in 0..2 {
            low[k] = low[k].min(point[k]);
            high[k] = high[k].max(point[k]);
        }
    }
    let mut cut = f32::INFINITY;
    if !spawns.is_empty() {
        cut = spawns
            .iter()
            .map(|spawn| spawn.position[2])
            .fold(f32::NEG_INFINITY, f32::max)
            + HEADROOM;
        for k in 0..2 {
            let (least, most) = spawns
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), spawn| {
                    (a.min(spawn.position[k]), b.max(spawn.position[k]))
                });
            low[k] = low[k].max(least - MARGIN);
            high[k] = high[k].min(most + MARGIN);
        }
    }
    if !(0..2).all(|k| low[k].is_finite() && high[k].is_finite() && low[k] < high[k]) {
        return Ok(None);
    }
    let cell = ((high[0] - low[0]).max(high[1] - low[1]) / CELLS).max(0.05);
    let width = ((high[0] - low[0]) / cell).ceil().max(1.) as usize;
    let height = ((high[1] - low[1]) / cell).ceil().max(1.) as usize;
    let right = low[0] + width as f32 * cell;
    let top = low[1] + height as f32 * cell;
    // Each surface's corners in cells, x right and y down from the top edge, with their heights.
    let points = surfaces
        .iter()
        .map(|(primitive, _)| {
            primitive
                .vertices
                .iter()
                .map(|vertex| {
                    let point = place(vertex.position);
                    [
                        (point[0] - low[0]) / cell,
                        (top - point[1]) / cell,
                        point[2],
                    ]
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let mut water = vec![false; width * height];
    // Workers take bands of rows, each drawing every surface's part inside its band.
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, 8)
        .min(height);
    let rows = height.div_ceil(workers);
    std::thread::scope(|scope| {
        for (band, (depth, water)) in depth
            .chunks_mut(rows * width)
            .zip(water.chunks_mut(rows * width))
            .enumerate()
        {
            let (surfaces, points) = (&surfaces, &points);
            scope.spawn(move || {
                let first = band * rows;
                let last = first + depth.len() / width - 1;
                for ((primitive, wet), points) in surfaces.iter().zip(points) {
                    for face in primitive.indices.chunks_exact(3) {
                        let corner = |k: usize| points.get(face[k] as usize).copied();
                        let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else {
                            continue;
                        };
                        // Walls and other upright faces cover no cell.
                        let area = edge(a, b, [c[0], c[1]]);
                        if !area.is_finite() || area.abs() <= 1e-9 {
                            continue;
                        }
                        let span = |k: usize, limit: usize| {
                            let least = (a[k].min(b[k]).min(c[k]) - 0.5).ceil().max(0.);
                            let most = (a[k].max(b[k]).max(c[k]) - 0.5).floor().min(limit as f32);
                            (least <= most).then_some((least as usize, most as usize))
                        };
                        let (Some((x0, x1)), Some((y0, y1))) = (span(0, width - 1), span(1, last))
                        else {
                            continue;
                        };
                        for y in y0.max(first)..=y1 {
                            for x in x0..=x1 {
                                let p = [x as f32 + 0.5, y as f32 + 0.5];
                                let u = edge(b, c, p) / area;
                                let v = edge(c, a, p) / area;
                                let w = 1. - u - v;
                                if u < -1e-5 || v < -1e-5 || w < -1e-5 {
                                    continue;
                                }
                                let z = u * a[2] + v * b[2] + w * c[2];
                                let index = (y - first) * width + x;
                                if z <= cut && z > depth[index] {
                                    depth[index] = z;
                                    water[index] = *wet;
                                }
                            }
                        }
                    }
                }
            });
        }
    });
    let (floor, ceiling) = depth
        .iter()
        .filter(|z| z.is_finite())
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &z| {
            (a.min(z), b.max(z))
        });
    if !floor.is_finite() {
        return Ok(None);
    }
    let ceiling = if ceiling - floor > 0.01 {
        ceiling
    } else {
        floor + 1.
    };
    let mut pixels = vec![0; width * height * 4];
    for ((pixel, &z), &wet) in pixels.chunks_exact_mut(4).zip(&depth).zip(&water) {
        if z.is_finite() {
            let level = ((z - floor) / (ceiling - floor) * 65_535.).round() as u16;
            pixel.copy_from_slice(&[
                (level >> 8) as u8,
                level as u8,
                if wet { 255 } else { 0 },
                255,
            ]);
        }
    }
    image::RgbaImage::from_raw(u32::try_from(width)?, u32::try_from(height)?, pixels)
        .context("Plan image size")?
        .save_with_format(output.join(IMAGE), image::ImageFormat::Png)
        .context("Could not write the map plan")?;
    Ok(Some(Plan {
        image: IMAGE.to_owned(),
        width: u32::try_from(width)?,
        height: u32::try_from(height)?,
        minimum: low,
        maximum: [right, top],
        floor,
        ceiling,
    }))
}
