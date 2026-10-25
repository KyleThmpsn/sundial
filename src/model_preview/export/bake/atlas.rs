//! Independent triangle charts for material maps that cannot share primary UV texels.
use super::*;

const EDGE: usize = 2048;

pub(in crate::model_preview::export) struct Budget {
    remaining: usize,
    pending: usize,
}

impl Budget {
    pub(in crate::model_preview::export) fn new(pending: usize) -> Self {
        Self {
            remaining: 16 * 1024 * 1024,
            pending,
        }
    }

    fn edge(&self) -> Result<usize, String> {
        // A partially filled page uses at most two cells per member. Reserve that
        // allowance for every pending triangle, including later material groups.
        let share = self.remaining / self.pending.max(1) / 2;
        let edge = (share as f64).sqrt().floor() as usize;
        if edge < 16 {
            return Err("Repacked native detail exceeds the export texture budget".into());
        }
        Ok((1usize << edge.ilog2()).min(EDGE))
    }
}

fn span(points: [[f32; 2]; 3], axis: usize) -> f32 {
    let values = points.map(|v| v[axis]);
    values.into_iter().fold(f32::NEG_INFINITY, f32::max)
        - values.into_iter().fold(f32::INFINITY, f32::min)
}

fn required(
    model: &Model,
    triangle: usize,
    bindings: &shader::Bindings<'_>,
) -> Result<f32, String> {
    let indices = model.triangles[triangle];
    let primary = indices.map(|i| model.uvs[i as usize]);
    let detail = indices.map(|i| model.detail_uvs[i as usize]);
    if !primary
        .iter()
        .chain(&detail)
        .flatten()
        .all(|v| v.is_finite())
    {
        return Err("The model has a non-finite texture coordinate".into());
    }
    let mut density = 1.0_f32;
    if let Some(map) = model.triangle_dye_maps.get(triangle).copied().flatten() {
        let texture = &model.textures[map.texture];
        for axis in 0..2 {
            density = density.max(
                span(primary, axis)
                    * f32::from_bits(map.transform[axis]).abs()
                    * texture.size[axis] as f32,
            );
        }
    }
    for texture in [bindings.albedo, bindings.gearstack, bindings.normal]
        .into_iter()
        .flatten()
    {
        for axis in 0..2 {
            density = density.max(span(primary, axis) * texture.size[axis] as f32);
        }
    }
    if let Some(dye) = bindings.dye {
        for (texture, transform) in [
            (bindings.detail, dye.transform),
            (bindings.detail_normal, dye.normal_transform),
        ] {
            let Some(texture) = texture else { continue };
            for (axis, scale) in transform.iter().enumerate().take(2) {
                density = density.max(span(detail, axis) * scale.abs() * texture.size[axis] as f32);
            }
        }
    }
    // Two samples per source texel retain bilinear transitions when the bounded
    // atlas can hold them. Reduced sampling is recorded in the GLB metadata.
    let required = density.ceil() * 2.0 + (MARGIN * 2 + 1) as f32;
    if !required.is_finite() {
        return Err("A native detail chart has a non-finite sampling density".into());
    }
    Ok(required)
}

pub(super) fn bake(
    model: &Model,
    dyes: &[Option<shader::Dye>; 6],
    plate: Plate,
    triangles: &[usize],
    budget: &mut Budget,
    backend: &mut Option<&mut dyn Baker>,
) -> Result<Vec<Layer>, String> {
    if triangles.is_empty() {
        return Ok(Vec::new());
    }
    let maximum = budget.edge()?;
    let mut groups = BTreeMap::<usize, Vec<(usize, f32)>>::new();
    for &triangle in triangles {
        let bindings = plate.bindings(model, triangle, dyes);
        let required = required(model, triangle, &bindings)?;
        let cell = (required.min(maximum as f32) as usize)
            .max(16)
            .next_power_of_two();
        groups.entry(cell).or_default().push((triangle, required));
    }
    let mut result = Vec::new();
    for (cell, triangles) in groups {
        let capacity = (EDGE / cell).pow(2);
        for page in triangles.chunks(capacity) {
            let columns = (page.len() as f64).sqrt().ceil() as usize;
            let size = [columns * cell, page.len().div_ceil(columns) * cell];
            budget.remaining = budget
                .remaining
                .checked_sub(size[0] * size[1])
                .ok_or("Repacked native detail exceeds the export texture budget")?;
            budget.pending = budget
                .pending
                .checked_sub(page.len())
                .ok_or("The detail chart plan does not match its geometry")?;
            let triangles: Vec<_> = page.iter().map(|&(triangle, _)| triangle).collect();
            let mut layer = paint(model, dyes, plate, &triangles, size, cell, backend)?;
            layer.detail_sampling = page
                .iter()
                .filter(|&&(_, required)| required > cell as f32)
                .map(|&(triangle, required)| (triangle, required, cell))
                .collect();
            result.push(layer);
        }
    }
    Ok(result)
}

fn paint(
    model: &Model,
    dyes: &[Option<shader::Dye>; 6],
    plate: Plate,
    triangles: &[usize],
    size: [usize; 2],
    cell: usize,
    backend: &mut Option<&mut dyn Baker>,
) -> Result<Layer, String> {
    let count = if backend.is_none() {
        size[0] * size[1]
    } else {
        0
    };
    let mut painted = Painted {
        color: vec![0; count * 4],
        channels: vec![255; count * 3],
        normal: vec![0; count * 3],
        emission: vec![[0.0; 3]; count],
    };
    let columns = size[0] / cell;
    let interior = (cell - 2 * MARGIN - 1) as f32;
    let mut coordinates = Vec::new();
    let mut claims = Vec::new();
    let mut surfaces = Vec::new();
    for (index, &triangle) in triangles.iter().enumerate() {
        let origin = [(index % columns) * cell, (index / columns) * cell];
        let indices = model.triangles[triangle];
        let uv = indices.map(|i| model.uvs[i as usize]);
        let detail = indices.map(|i| model.detail_uvs[i as usize]);
        let bindings = plate.bindings(model, triangle, dyes);
        let clip = model.triangle_clip.get(triangle).copied().unwrap_or(false)
            || model
                .triangle_dye_maps
                .get(triangle)
                .is_some_and(Option::is_some);
        coordinates.push([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]].map(|v| {
            std::array::from_fn(|axis| {
                (origin[axis] as f32 + MARGIN as f32 + 0.5 + v[axis] * interior) / size[axis] as f32
            })
        }));
        surfaces.push(Surface {
            triangle,
            dye: model
                .triangle_dyes
                .get(triangle)
                .copied()
                .unwrap_or(u8::MAX),
            clip: model.triangle_clip.get(triangle).copied().unwrap_or(false),
            cutoff: model
                .triangle_cutoff
                .get(triangle)
                .copied()
                .flatten()
                .unwrap_or(0.5),
            detail: None,
        });
        for y in 0..if backend.is_none() { cell } else { 0 } {
            for x in 0..cell {
                let mut weight =
                    [x, y].map(|v| ((v as f32 - MARGIN as f32) / interior).clamp(0.0, 1.0));
                let sum = weight[0] + weight[1];
                if sum > 1.0 {
                    weight = weight.map(|v| v / sum);
                }
                let interpolate = |values: [[f32; 2]; 3]| {
                    std::array::from_fn(|axis| {
                        values[0][axis]
                            + weight[0] * (values[1][axis] - values[0][axis])
                            + weight[1] * (values[2][axis] - values[0][axis])
                    })
                };
                let uv = interpolate(uv);
                let texel = material_at(&bindings, uv, Some(interpolate(detail)));
                let at = (origin[1] + y) * size[0] + origin[0] + x;
                let alpha = if clip {
                    bindings.coverage(uv).map_or(255, unorm)
                } else {
                    255
                };
                painted.color[at * 4..at * 4 + 4].copy_from_slice(&[
                    shader::encode(texel.albedo[0]),
                    shader::encode(texel.albedo[1]),
                    shader::encode(texel.albedo[2]),
                    alpha,
                ]);
                painted.channels[at * 3..at * 3 + 3].copy_from_slice(&texel.channels.map(unorm));
                painted.normal[at * 3..at * 3 + 3].copy_from_slice(&tangent(texel.packed));
                painted.emission[at] = texel.emission;
            }
        }
        claims.push(Claim {
            finish: Finish {
                dye: 0,
                clip,
                cutoff: None,
            },
            detail: None,
            triangles: vec![triangle],
            texels: Vec::new(),
        });
    }
    if let Some(backend) = backend.as_deref_mut() {
        painted = backend.paint(Request {
            size,
            plate,
            dyes: *dyes,
            surfaces,
            layout: Layout::Charts { cell },
        })?;
    }
    let members = (0..claims.len()).collect::<Vec<_>>();
    let mut layer = assemble(painted, plate, size, &members, &claims);
    layer.coordinates = Some(coordinates);
    Ok(layer)
}
