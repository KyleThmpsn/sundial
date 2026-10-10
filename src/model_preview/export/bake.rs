//! Bakes the gear shader's unlit material into the textures a glTF metallic-roughness
//! material reads. The export then carries what the preview shows: dyes, wear, detail
//! textures, roughness, metal, occlusion, normal detail and emission, rather than the
//! undyed colour plate the shader starts from.
//!
//! One plate is usually shared by parts wearing different dye slots, each in its own region
//! of the plate. Every part is rasterized into the plate's texture space, so each texel is
//! evaluated with the dye of the part that owns it. Parts that overlap in texture space with
//! different dyes cannot share one texture, so they are split onto separate layers, and each
//! layer becomes its own material.
//!
//! Two things stay behind. Iridescence depends on the viewing angle, so it has no texture to
//! bake into. The preview's studio lighting is left to the application that opens the file.
use crate::model_preview::{Model, shader};
use std::collections::BTreeMap;
mod atlas;
pub(super) use atlas::Budget;
mod coordinates;
use coordinates::Coordinates;

/// Texels a part claims beyond its own edges, so filtering and mipmaps at a texture seam read
/// the part's own material instead of a neighbour's or the unclaimed background.
const MARGIN: usize = 4;

/// The three textures a part's material is evaluated from. Parts that share all three are
/// baked together.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Plate {
    pub albedo: usize,
    pub gearstack: Option<usize>,
    pub normal: Option<usize>,
    pub no_basis: bool,
    pub dye_map: Option<super::super::texture::DyeMap>,
    pub base_gain: Option<[u32; 3]>,
    pub base_metal: Option<u32>,
    pub paint: Option<[u32; 2]>,
    pub normal_decode: Option<[[u32; 2]; 4]>,
    pub grain: Option<[u32; 3]>,
    pub legacy_normal: Option<[u32; 3]>,
}

/// A planned material pass. The GPU consumes UV ownership, never CPU-shaded texels.
pub(crate) struct Request {
    pub size: [usize; 2],
    pub plate: Plate,
    pub dyes: [Option<shader::Dye>; 6],
    pub surfaces: Vec<Surface>,
    pub layout: Layout,
}

pub(crate) struct Surface {
    pub triangle: usize,
    pub dye: u8,
    pub clip: bool,
    pub cutoff: f32,
    pub detail: Option<[[f32; 3]; 2]>,
}

pub(crate) enum Layout {
    Plate(Vec<u32>),
    Charts { cell: usize },
}

pub(crate) trait Baker {
    fn paint(&mut self, request: Request) -> Result<Painted, String>;
}

impl Plate {
    pub(super) fn with_basis(mut self, available: bool) -> Self {
        self.no_basis = self.normal.is_some() && !available;
        self
    }

    pub(super) fn omits_normal(self) -> bool {
        self.no_basis
    }
    /// The plate a triangle bakes from. Parts without texture coordinates or a colour plate
    /// have nothing to bake, and emissive panels take a constant colour instead of a
    /// material, so the export keeps all three flat.
    pub(super) fn of(
        model: &Model,
        triangle: usize,
        frames: &[Option<super::super::effects::Frame>],
    ) -> Option<Self> {
        if model.uvs.is_empty()
            || model
                .triangle_constant
                .get(triangle)
                .copied()
                .flatten()
                .is_some()
        {
            return None;
        }
        let slot = |list: &[Option<usize>]| list.get(triangle).copied().flatten();
        let native = super::super::effects::index(model, triangle).and_then(|index| {
            Some((
                model.effects[index].native.as_ref()?,
                frames.get(index)?.as_ref()?,
            ))
        });
        Some(Self {
            albedo: slot(&model.triangle_textures)?,
            gearstack: slot(&model.triangle_gearstacks),
            normal: slot(&model.triangle_normals),
            no_basis: false,
            dye_map: model.triangle_dye_maps.get(triangle).copied().flatten(),
            base_gain: native
                .and_then(|(native, frame)| native.base_gain(frame).map(|v| v.map(f32::to_bits))),
            base_metal: native
                .and_then(|(native, frame)| native.base_metal(frame).map(f32::to_bits)),
            paint: native
                .and_then(|(native, frame)| native.paint(frame).map(|v| v.map(f32::to_bits))),
            normal_decode: native.and_then(|(native, frame)| {
                native.normal(frame).map(|v| v.map(|v| v.map(f32::to_bits)))
            }),
            grain: native
                .and_then(|(native, frame)| native.grain(frame).map(|v| v.map(f32::to_bits))),
            legacy_normal: super::super::effects::index(model, triangle).and_then(|index| {
                model.effects[index]
                    .normal
                    .as_ref()?
                    .frame(frames.get(index)?.as_ref()?)
                    .map(|v| v.map(f32::to_bits))
            }),
        })
    }
    fn bindings<'a>(
        self,
        model: &'a Model,
        triangle: usize,
        dyes: &'a [Option<shader::Dye>; 6],
    ) -> shader::Bindings<'a> {
        shader::Bindings::new(model, triangle, dyes)
            .with_normal_mapping(!self.no_basis)
            .with_gain(self.base_gain.map(|v| v.map(f32::from_bits)))
            .with_metal(self.base_metal.map(f32::from_bits))
            .with_paint(self.paint.map(|v| v.map(f32::from_bits)))
            .with_normal_decode(self.normal_decode.map(|v| v.map(|v| v.map(f32::from_bits))))
            .with_grain(self.grain.map(|v| v.map(f32::from_bits)))
            .with_legacy_normal(self.legacy_normal.map(|v| v.map(f32::from_bits)))
    }
}

/// What a part wears on its plate. Parts with the same finish can share texels freely.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Finish {
    dye: u8,
    clip: bool,
    cutoff: Option<u32>,
}

/// Occlusion, roughness and metal, either as a texture or, when every texel agrees and none
/// is occluded, as the two factors glTF can state without one.
pub(super) enum Channels {
    Texture(Vec<u8>),
    Uniform { roughness: u8, metal: u8 },
}

/// One material's worth of a plate: the triangles drawn with it and its textures.
pub(super) struct Layer {
    pub triangles: Vec<usize>,
    pub size: [usize; 2],
    /// RGBA with sRGB colour. Alpha is coverage where a part is alpha-clipped, else opaque.
    pub color: Vec<u8>,
    /// RGB with linear occlusion, roughness and metal in red, green and blue, as glTF packs
    /// them.
    pub channels: Channels,
    /// RGB tangent-space normals in glTF's convention, when the plate has a normal map.
    pub normal: Option<Vec<u8>>,
    /// RGB sRGB emission scaled into range, with the strength that scales it back.
    pub emission: Option<(Vec<u8>, f32)>,
    pub masked: bool,
    /// Repacked charts have a separate UV and vertex for each triangle corner.
    pub coordinates: Option<Vec<[[f32; 2]; 3]>>,
    pub detail_sampling: Vec<(usize, f32, usize)>,
}

enum Mapping {
    Ordinary,
    Native(Coordinates),
    Chart,
}

fn detail_scale(model: &Model, dye: Option<&shader::Dye>) -> f32 {
    dye.map_or(1.0, |d| {
        [(d.detail, d.transform), (d.normal, d.normal_transform)]
            .into_iter()
            .filter_map(|(index, transform)| Some((model.textures.get(index?)?, transform)))
            .flat_map(|(texture, transform)| {
                [
                    texture.size[0] as f32 * transform[0].abs(),
                    texture.size[1] as f32 * transform[1].abs(),
                ]
            })
            .fold(1.0, f32::max)
    })
}

fn mapping(model: &Model, triangle: usize, dye: Option<&shader::Dye>) -> Result<Mapping, String> {
    if !model
        .triangle_detail_uv
        .get(triangle)
        .copied()
        .unwrap_or(false)
        || !dye.is_some_and(|d| d.detail.is_some() || d.normal.is_some())
    {
        return Ok(Mapping::Ordinary);
    }
    Ok(match Coordinates::read(model, triangle)? {
        Some(map) if map.agrees(&map, 0.125 / detail_scale(model, dye)) => Mapping::Native(map),
        _ => Mapping::Chart,
    })
}

pub(super) fn chart_count(
    model: &Model,
    dyes: &[Option<shader::Dye>; 6],
    triangles: impl Iterator<Item = usize>,
) -> Result<usize, String> {
    let mut count = 0;
    for triangle in triangles {
        let slot = model.triangle_dyes.get(triangle).copied().unwrap_or(0) as usize;
        if matches!(
            mapping(model, triangle, dyes.get(slot).and_then(Option::as_ref))?,
            Mapping::Chart
        ) {
            count += 1;
        }
    }
    Ok(count)
}

/// One part finish's claim on the plate.
struct Claim {
    finish: Finish,
    detail: Option<Coordinates>,
    triangles: Vec<usize>,
    texels: Vec<u32>,
}

pub(super) fn bake(
    model: &Model,
    dyes: &[Option<shader::Dye>; 6],
    plate: Plate,
    triangles: &[usize],
    remaining_charts: &mut Budget,
    backend: &mut Option<&mut dyn Baker>,
) -> Result<Vec<Layer>, String> {
    let texture = model
        .textures
        .get(plate.albedo)
        .ok_or("A part refers to a texture the model did not load")?;
    let size = texture.size;
    let [width, height] = size;
    if width == 0 || height == 0 || texture.rgba.len() != width * height * 4 {
        return Err(format!(
            "Texture 0x{:08X} has no pixels to bake",
            texture.tag
        ));
    }
    let mut finishes: BTreeMap<Finish, Vec<Claim>> = BTreeMap::new();
    let mut repacked = Vec::new();
    for &triangle in triangles {
        let finish = Finish {
            cutoff: model
                .triangle_cutoff
                .get(triangle)
                .copied()
                .flatten()
                .map(f32::to_bits),
            dye: model.triangle_dyes.get(triangle).copied().unwrap_or(0),
            clip: model.triangle_clip.get(triangle).copied().unwrap_or(false)
                || model
                    .triangle_dye_maps
                    .get(triangle)
                    .is_some_and(Option::is_some),
        };
        let dye = dyes.get(finish.dye as usize).and_then(Option::as_ref);
        let detail = match mapping(model, triangle, dye)? {
            Mapping::Ordinary => None,
            Mapping::Native(map) => Some(map),
            Mapping::Chart => {
                repacked.push(triangle);
                continue;
            }
        };
        let scale = detail_scale(model, dye);
        let claims = finishes.entry(finish).or_default();
        let matching = claims
            .iter_mut()
            .find(|claim| match (&claim.detail, &detail) {
                (None, None) => true,
                (Some(a), Some(b)) => a.agrees(b, 0.125 / scale),
                _ => false,
            });
        if let Some(claim) = matching {
            claim.triangles.push(triangle);
        } else {
            claims.push(Claim {
                finish,
                detail,
                triangles: vec![triangle],
                texels: Vec::new(),
            });
        }
    }
    let mut claims: Vec<Claim> = finishes
        .into_values()
        .flatten()
        .map(|mut entry| {
            entry.texels = claim(model, &entry.triangles, size);
            entry.texels.sort_unstable();
            entry.texels.dedup();
            entry
        })
        .collect();
    // The largest claim anchors the first layer and fills whatever no part reaches.
    claims.sort_by_key(|claim| std::cmp::Reverse(claim.texels.len()));
    let bindings: Vec<shader::Bindings<'_>> = claims
        .iter()
        .map(|claim| plate.bindings(model, claim.triangles[0], dyes))
        .collect();

    let mut layers: Vec<(Vec<u32>, Vec<usize>)> = Vec::new();
    for (index, claim) in claims.iter().enumerate() {
        let mark =
            u32::try_from(index + 1).map_err(|_| "Too many material mappings share one plate")?;
        // Parts that merely touch at a seam overlap by a sliver, which is not a conflict.
        let tolerance = (claim.texels.len() / 100).max(8);
        let layer = layers.iter().position(|(owner, _)| {
            let taken = |&&texel: &&u32| {
                let other = owner[texel as usize];
                other != 0 && other != mark
            };
            let overlap = claim.texels.iter().filter(taken).count();
            overlap <= tolerance && (overlap == 0 || overlap * 2 < claim.texels.len())
        });
        let layer = layer.unwrap_or_else(|| {
            layers.push((vec![0; width * height], Vec::new()));
            layers.len() - 1
        });
        let (owner, members) = &mut layers[layer];
        for &texel in &claim.texels {
            let slot = &mut owner[texel as usize];
            if *slot == 0 {
                *slot = mark;
            }
        }
        members.push(index);
    }

    let finishes: Vec<Finish> = claims.iter().map(|claim| claim.finish).collect();
    let coordinates: Vec<_> = claims.iter().map(|claim| claim.detail.as_ref()).collect();
    let mut result: Vec<_> = layers
        .into_iter()
        .map(|(mut owner, members)| {
            // Marks were checked when they were handed out.
            spread(&mut owner, size, members[0] as u32 + 1);
            let painted = if let Some(backend) = backend.as_deref_mut() {
                backend.paint(Request {
                    size,
                    plate,
                    dyes: *dyes,
                    surfaces: claims
                        .iter()
                        .map(|claim| Surface {
                            triangle: claim.triangles[0],
                            dye: model
                                .triangle_dyes
                                .get(claim.triangles[0])
                                .copied()
                                .unwrap_or(u8::MAX),
                            clip: model
                                .triangle_clip
                                .get(claim.triangles[0])
                                .copied()
                                .unwrap_or(false),
                            cutoff: claim.finish.cutoff.map(f32::from_bits).unwrap_or(0.5),
                            detail: claim.detail.as_ref().map(Coordinates::matrix),
                        })
                        .collect(),
                    layout: Layout::Plate(owner),
                })?
            } else {
                paint(&owner, &bindings, &finishes, &coordinates, size)
            };
            Ok(assemble(painted, plate, size, &members, &claims))
        })
        .collect::<Result<Vec<_>, String>>()?;
    result.extend(atlas::bake(
        model,
        dyes,
        plate,
        &repacked,
        remaining_charts,
        backend,
    )?);
    Ok(result)
}

/// The texels whose centres lie inside a finish's triangles, in the plate's own texel grid.
/// Texture coordinates wrap, as the sampler does.
fn claim(model: &Model, triangles: &[usize], size: [usize; 2]) -> Vec<u32> {
    let [width, height] = size;
    let mut texels = Vec::new();
    for &triangle in triangles {
        let Some(corners) = model.triangles.get(triangle) else {
            continue;
        };
        let corners = corners.map(|corner| {
            let uv = model.uvs.get(corner as usize).copied().unwrap_or_default();
            [uv[0] * width as f32, uv[1] * height as f32]
        });
        if corners.iter().flatten().all(|value| value.is_finite()) {
            scan(corners, size, &mut texels);
        }
    }
    texels
}

/// One triangle's texels, already in texel units.
fn scan(corners: [[f32; 2]; 3], size: [usize; 2], texels: &mut Vec<u32>) {
    let [width, height] = size;
    let wrap = |x: i64, y: i64| {
        (y.rem_euclid(height as i64) as usize * width + x.rem_euclid(width as i64) as usize) as u32
    };
    let low = [0, 1].map(|axis| {
        corners
            .iter()
            .map(|c| c[axis])
            .fold(f32::INFINITY, f32::min)
    });
    let high = [0, 1].map(|axis| {
        corners
            .iter()
            .map(|c| c[axis])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    // A triangle wider than the plate covers all of it once, so the scan never needs more.
    let x0 = low[0].floor() as i64;
    let y0 = low[1].floor() as i64;
    let x1 = (high[0].ceil() as i64).min(x0 + width as i64);
    let y1 = (high[1].ceil() as i64).min(y0 + height as i64);
    let area = edge(corners[0], corners[1], corners[2]);
    let before = texels.len();
    if area.abs() > 1e-9 {
        for y in y0..y1 {
            for x in x0..x1 {
                let centre = [x as f32 + 0.5, y as f32 + 0.5];
                let weights = [
                    edge(corners[1], corners[2], centre),
                    edge(corners[2], corners[0], centre),
                    edge(corners[0], corners[1], centre),
                ];
                if weights.iter().all(|&weight| weight * area.signum() >= 0.0) {
                    texels.push(wrap(x, y));
                }
            }
        }
    }
    if texels.len() == before {
        // Too small to cover a texel centre, so it takes the texel under its centroid.
        let centre = [0, 1].map(|axis| corners.iter().map(|c| c[axis]).sum::<f32>() / 3.0);
        texels.push(wrap(centre[0].floor() as i64, centre[1].floor() as i64));
    }
}

fn edge(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Grows every claim outward by `MARGIN` texels, then gives whatever is still unclaimed to
/// the layer's largest finish.
fn spread(owner: &mut [u32], size: [usize; 2], fallback: u32) {
    let [width, height] = size;
    for _ in 0..MARGIN {
        if !owner.contains(&0) {
            return;
        }
        let before = owner.to_vec();
        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                if before[index] != 0 {
                    continue;
                }
                let neighbour = [
                    ((x + width - 1) % width, y),
                    ((x + 1) % width, y),
                    (x, (y + height - 1) % height),
                    (x, (y + 1) % height),
                ]
                .into_iter()
                .map(|(nx, ny)| before[ny * width + nx])
                .find(|&mark| mark != 0);
                if let Some(mark) = neighbour {
                    owner[index] = mark;
                }
            }
        }
    }
    for slot in owner.iter_mut().filter(|slot| **slot == 0) {
        *slot = fallback;
    }
}

/// Raw per-texel output before the layer decides which maps it needs.
pub(crate) struct Painted {
    pub color: Vec<u8>,
    pub channels: Vec<u8>,
    pub normal: Vec<u8>,
    pub emission: Vec<[f32; 3]>,
}

/// Evaluates the material at every texel centre with its owner's dye. Evaluation is per
/// texel and CPU bound, so rows are split across every core as the rasterizer does.
fn paint(
    owner: &[u32],
    bindings: &[shader::Bindings<'_>],
    finishes: &[Finish],
    coordinates: &[Option<&Coordinates>],
    size: [usize; 2],
) -> Painted {
    let [width, height] = size;
    let texels = width * height;
    let mut painted = Painted {
        color: vec![0; texels * 4],
        channels: vec![0; texels * 3],
        normal: vec![0; texels * 3],
        emission: vec![[0.0; 3]; texels],
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(16));
    let rows = height.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (band, (((color, channels), normal), emission)) in painted
            .color
            .chunks_mut(rows * width * 4)
            .zip(painted.channels.chunks_mut(rows * width * 3))
            .zip(painted.normal.chunks_mut(rows * width * 3))
            .zip(painted.emission.chunks_mut(rows * width))
            .enumerate()
        {
            scope.spawn(move || {
                let first = band * rows * width;
                for (local, glow) in emission.iter_mut().enumerate() {
                    let index = first + local;
                    let (x, y) = (index % width, index / width);
                    let mark = (owner[index] as usize).saturating_sub(1);
                    let (Some(bindings), Some(finish)) = (bindings.get(mark), finishes.get(mark))
                    else {
                        continue;
                    };
                    let uv = [
                        (x as f32 + 0.5) / width as f32,
                        (y as f32 + 0.5) / height as f32,
                    ];
                    let detail = coordinates
                        .get(mark)
                        .copied()
                        .flatten()
                        .map(|map| map.sample(uv));
                    let texel = material_at(bindings, uv, detail);
                    let alpha = if finish.clip {
                        bindings.coverage(uv).map_or(255, unorm)
                    } else {
                        255
                    };
                    color[local * 4..local * 4 + 4].copy_from_slice(&[
                        shader::encode(texel.albedo[0]),
                        shader::encode(texel.albedo[1]),
                        shader::encode(texel.albedo[2]),
                        alpha,
                    ]);
                    channels[local * 3..local * 3 + 3].copy_from_slice(&texel.channels.map(unorm));
                    normal[local * 3..local * 3 + 3].copy_from_slice(&tangent(texel.packed));
                    *glow = texel.emission;
                }
            });
        }
    });
    painted
}

/// The unlit material at one texel, in the terms a PBR material reads.
struct Baked {
    albedo: [f32; 3],
    /// Occlusion, roughness and metal.
    channels: [f32; 3],
    packed: Option<[f32; 2]>,
    emission: [f32; 3],
}

fn material_at(bindings: &shader::Bindings<'_>, uv: [f32; 2], detail: Option<[f32; 2]>) -> Baked {
    if let Some(shader::Texel { surface, normal }) = bindings.texel_at(uv, detail) {
        let mut ao = surface.ao;
        if let Some(map) = &normal {
            ao *= map.occlusion[0];
            ao *= map.occlusion[1];
        }
        return Baked {
            albedo: surface.albedo,
            channels: [ao, surface.roughness, surface.metal],
            packed: normal.map(|map| map.packed),
            emission: surface.emission,
        };
    }
    // No gear material: the plate under the part's dye tint, as the rasterizer draws it, on
    // the shader's own fallback surface.
    let base = bindings.albedo.map_or([1.0; 3], |texture| {
        let [r, g, b, _] = texture.sample_color(uv);
        [r, g, b]
    });
    let tint = bindings.tint().unwrap_or([1.0; 3]);
    Baked {
        albedo: std::array::from_fn(|i| base[i] * tint[i]),
        channels: [1.0, 0.6, 0.0],
        packed: None,
        emission: [0.0; 3],
    }
}

/// The preview reads the map's green along increasing V. glTF reads it up the image, which
/// is decreasing V in the same texture coordinates, so green is mirrored on the way out. The
/// map's blue channel holds occlusion, so the third axis is rebuilt from the other two.
fn tangent(packed: Option<[f32; 2]>) -> [u8; 3] {
    let Some(packed) = packed else {
        return [128, 128, 255];
    };
    let [x, y] = packed.map(|v| (v * 2.0 - 1.0).clamp(-1.0, 1.0));
    let z = (1.0 - x * x - y * y).max(0.0).sqrt();
    shader::normal::normalize([x, -y, z])
        .unwrap_or([0.0, 0.0, 1.0])
        .map(|v| unorm(v * 0.5 + 0.5))
}

fn unorm(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn assemble(
    painted: Painted,
    plate: Plate,
    size: [usize; 2],
    members: &[usize],
    claims: &[Claim],
) -> Layer {
    let Painted {
        color,
        channels,
        normal,
        emission,
    } = painted;
    let first = [channels[0], channels[1], channels[2]];
    let channels = if first[0] == 255 && channels.chunks_exact(3).all(|texel| texel == first) {
        Channels::Uniform {
            roughness: first[1],
            metal: first[2],
        }
    } else {
        Channels::Texture(channels)
    };
    let peak = emission
        .iter()
        .flatten()
        .filter(|value| value.is_finite())
        .fold(0.0_f32, |peak, &value| peak.max(value));
    // Below half a step of an 8-bit channel nothing would survive encoding anyway.
    let emission = (peak > 0.5 / 255.0).then(|| {
        let strength = peak.max(1.0);
        let bytes = emission
            .iter()
            .flat_map(|glow| glow.map(|value| shader::encode(value / strength)))
            .collect();
        (bytes, strength)
    });
    Layer {
        triangles: members
            .iter()
            .flat_map(|&member| claims[member].triangles.iter().copied())
            .collect(),
        size,
        color,
        channels,
        normal: plate.normal.filter(|_| !plate.no_basis).map(|_| normal),
        emission,
        masked: plate.gearstack.is_some() && members.iter().any(|&m| claims[m].finish.clip),
        coordinates: None,
        detail_sampling: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_neutral_map_points_straight_out_and_green_is_mirrored() {
        assert_eq!(tangent(None), [128, 128, 255]);
        assert_eq!(tangent(Some([0.5, 0.5])), [128, 128, 255]);
        // Green above one half leans along increasing V here, which is down the image.
        let [red, green, blue] = tangent(Some([0.5, 0.8]));
        assert_eq!(red, 128);
        assert!(green < 128, "green should point up the image for glTF");
        assert!(blue > 128);
    }

    #[test]
    fn claims_cover_their_triangle_and_wrap_with_the_sampler() {
        let model = Model {
            vertices: vec![[0.0; 3]; 3],
            triangles: vec![[0, 1, 2]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            ..Default::default()
        };
        let texels = claim(&model, &[0], [4, 4]);
        // Four by four texels, and the centres on or under the diagonal belong to it.
        assert_eq!(texels.len(), 10);
        let shifted = Model {
            uvs: vec![[1.0, 1.0], [2.0, 1.0], [1.0, 2.0]],
            ..model
        };
        let mut wrapped = claim(&shifted, &[0], [4, 4]);
        let mut original = texels;
        wrapped.sort_unstable();
        original.sort_unstable();
        assert_eq!(wrapped, original);
    }

    #[test]
    fn a_sliver_still_claims_the_texel_under_it() {
        let model = Model {
            vertices: vec![[0.0; 3]; 3],
            triangles: vec![[0, 1, 2]],
            uvs: vec![[0.1, 0.1], [0.11, 0.1], [0.1, 0.11]],
            ..Default::default()
        };
        assert_eq!(claim(&model, &[0], [4, 4]), vec![0]);
    }

    #[test]
    fn spreading_fills_every_texel_and_prefers_a_neighbour() {
        let mut owner = vec![0, 0, 0, 0, 0, 0, 0, 0, 2];
        spread(&mut owner, [3, 3], 1);
        assert!(owner.iter().all(|&mark| mark == 2));
        let mut empty = vec![0; 9];
        spread(&mut empty, [3, 3], 1);
        assert!(empty.iter().all(|&mark| mark == 1));
    }
}
