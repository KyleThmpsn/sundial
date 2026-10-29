//! A level's baked lighting. Reach keeps it in the lightmap BSP data: a "dual VMF" intensity
//! array (DXT5, one 1792 square image with three layers for Hang 'Em High) and a direction map
//! (BC5), read through lightmap texture coordinates. Cluster meshes keep theirs in their second
//! vertex buffer slot, and placed instances in streams the lightmap BSP data names per instance.
//!
//! Reach's per-pixel lightmap pixel shader decodes a texel as follows. The three layers' alphas,
//! doubled less one, are a lobe's direction scaled by its bandwidth. Layer zero plus twice layer one
//! less one is the lobe's color, and a quarter of layer two the ambient color, both scaled by
//! `2^(-9 x)` of the direction map's first channel and by a compression constant. Irradiance over
//! pi is the lobe's color times a diffuse table read at the normal's cosine to the lobe and the
//! bandwidth, plus the ambient color. The direction map's second channel is the sun's visibility,
//! for the separate analytical light. The translation stores that decoded form in three images of
//! one atlas, which sets the lightmap image of every BSP a level loads on shelves, each ringed by
//! neutral texels, above the lighting probes and a last neutral strip that also lights surfaces
//! without lightmap coordinates.
//!
//! Instances without lightmap coordinates are lit per vertex or by one coloring row, and so are a
//! cluster mesh's parts lit per vertex. Per-vertex lighting is six bytes a vertex in a buffer the
//! lightmap BSP data names per instance and per cluster, which Reach's per-vertex vertex shader
//! decodes into a lobe direction from a lookup table, the sun's visibility, an ambient color and a
//! lobe color, scaled by the buffer's own scale. A coloring row is the four lighting constants of an
//! instance lit as one. Render vertices retain their decoded lobe, ambient and direction values.
//! Averaged probes remain available for atlas normalization and ground-light queries, while a
//! coloring row supplies a constant-lit instance's shared probe.
use super::super::{
    cache::{Cache, Tag},
    resource::Pages,
};
use anyhow::{Context, Result, ensure};

mod compute;

/// The lightmap BSP data's direction and intensity maps, and its per-instance lightmap texture
/// coordinate streams: one vertex buffer index per placed instance, `-1` for none.
const DIRECTION: usize = 0x1C;
const INTENSITY: usize = 0x2C;
const INSTANCE_TEXCOORDS: usize = 0xF4;
const INSTANCE_TEXCOORD_SIZE: usize = 2;
/// A bitmap's images and resources, each image's width, height, layer count and format.
const IMAGES: usize = 124;
const IMAGE_SIZE: usize = 56;
const IMAGE_LAYERS: usize = 4;
const IMAGE_FORMAT: usize = 8;
const RESOURCES: usize = 168;
const RESOURCE_SIZE: usize = 8;
/// DXT5 and unsigned BC5 blocks: 16 bytes for four by four texels.
const DXT5: i16 = 16;
const BC5: i16 = 44;
const BLOCK_BYTES: usize = 16;
/// The intensity array's layers.
const LAYERS: usize = 3;
/// The direction map's first channel scales a texel's colors by `2^(-EXPONENT x)`, the shader's
/// base-2 `exp`.
const EXPONENT: f32 = 9.000_001;
/// The lightmap BSP data's brightness, the compression constant its texels share.
const BRIGHTNESS: usize = 0x18;
/// The share of lit texels whose colors encode without clipping.
const PEAK_QUANTILE: f64 = 0.999;
/// Stored colors are logarithmic in their share `s` of the atlas's peak: a byte holds
/// `255 log2(1 + LOG_RANGE s) / log2(1 + LOG_RANGE)` ([`encode_share`], [`decode_share`]). Zero and
/// the peak store exactly, and the upper 16 stops step evenly by 4.4%, where the square root of `s`
/// stepped by 13% or more below its sixteenth byte, on 12% of Nightfall's lit texels, and its dark
/// baked light fell into contour bands. Below `s = 1 / LOG_RANGE` the steps turn linear.
pub(in crate::halo_reach) const LOG_RANGE: f32 = 65_536.;
/// Lightmap texture coordinate streams: two unsigned normalized 16-bit values per vertex, with
/// the declaration word's stride 4 above type 4.
const TEXCOORD_DECLARATION: u32 = 0x0004_0004;
/// The cluster mesh vertex buffer slot holding lightmap texture coordinates.
pub(super) const CLUSTER_SLOT: usize = 1;
/// Texels of neutral lighting around each source image and in the atlas's last strip, so filtering
/// at an image's edge reads neutral light rather than another image's.
const NEUTRAL_ROWS: usize = 4;
/// Texels on a side of the largest texture the atlas may take.
const MAX_SIDE: usize = 16_384;
/// The lightmap BSP data's per-vertex lighting buffers, an `i16` vertex buffer index and a half
/// float scale in each 4-byte row, and its instance rows, whose `i16` at `+0x02` names an
/// instance's per-vertex buffer row and at `+0x04` its coloring row.
const PER_VERTEX_BUFFERS: usize = 0x48;
const PER_VERTEX_BUFFER_SIZE: usize = 4;
const PER_VERTEX_SCALE: usize = 0x02;
const INSTANCE_ROWS: usize = 0x60;
const INSTANCE_ROW_SIZE: usize = 0xC;
const INSTANCE_PER_VERTEX: usize = 0x02;
const INSTANCE_COLORING: usize = 0x04;
/// Its cluster rows, one per structure cluster in cluster order: an `i16` at `+0x02` names the
/// cluster's per-vertex buffer row, and the 32-bit value at `+0x04` is the first mesh vertex that
/// buffer lights, the count of the lightmapped vertices before the parts lit per vertex.
const CLUSTER_ROWS: usize = 0x54;
const CLUSTER_ROW_SIZE: usize = 8;
const CLUSTER_PER_VERTEX: usize = 0x02;
const CLUSTER_FIRST_VERTEX: usize = 0x04;
/// The coloring rows: four half-float lighting constants, direction and sun visibility, lobe color
/// and bandwidth, a gel weight, ambient color, then a marker.
const COLORINGS: usize = 0x6C;
const COLORING_SIZE: usize = 0x24;
/// Coloring rows hold irradiance where the other sources hold its share over pi: divided by pi,
/// their average level matches the per-vertex probes' to within a third on each surveyed level.
const COLORING_SCALE: f32 = 1. / std::f32::consts::PI;
/// Bytes of per-vertex lighting each vertex takes.
const PER_VERTEX_BYTES: usize = 6;
/// The per-vertex lighting directions: four signed 16-bit values a texel, red first.
const DIRECTION_LUT: &str = r"rasterizer\direction_lut_1002";
const SIGNED_RGBA16: i16 = 34;
/// Texels on a side of the block that holds one lighting probe.
const PROBE_BLOCK: usize = 3;

/// A level's baked lighting in the decoded dual VMF form, as three linear RGBA8 images of one
/// atlas: each BSP's source image on shelves, ringed by neutral lighting, and then the lighting
/// probes above a last neutral strip. Texels outside those are black and never sampled. `lobe`
/// holds the lobe's color and its bandwidth, `ambient` the ambient color and the sun's visibility,
/// and `direction` the lobe's direction, halved and offset. Colors are stored as the logarithm of
/// their share of the shared peak (`LOG_RANGE`), and the other channels linearly. [`Atlas::texel`]
/// decodes one texel.
pub struct Atlas {
    pub width: usize,
    pub height: usize,
    pub lobe: Vec<u8>,
    pub ambient: Vec<u8>,
    pub direction: Vec<u8>,
    /// The scale of decoded lighting: the diffuse table's value times the lobe's decoded share
    /// (`decode_share`), plus the ambient's decoded share, times `scale` is irradiance over pi,
    /// with the average lit texel's at one.
    pub scale: f32,
    /// The average decoded level of every lit texel and probe, irradiance over pi in Reach's
    /// units. Decoded lighting is its share of this level, so a surface's light is its Reach
    /// radiance over it.
    pub level: f32,
    /// The lighting probes the atlas holds.
    pub probes: usize,
    /// The average decoded level of the lit image texels and of the probes, before the scale.
    pub averages: [f32; 2],
}

/// An atlas, each image's placement in it, and each BSP's probes' atlas coordinates.
pub(super) struct Packed {
    pub atlas: Atlas,
    pub placements: Vec<Option<Placement>>,
    pub probes: Vec<Vec<[f32; 2]>>,
}

/// Where a BSP's lightmap image sits in an atlas, in texels: its first row and column, width and
/// height.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    row: usize,
    column: usize,
    width: usize,
    height: usize,
}

/// One atlas texel as surfaces decode it: its lobe and ambient colors as irradiance over pi in the
/// atlas's units, where the average lit texel's level is one, the lobe's unit direction, and its
/// bandwidth and the sun's visibility, unsquared, from 0 to 1.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub lobe: [f32; 3],
    pub ambient: [f32; 3],
    pub direction: [f32; 3],
    pub bandwidth: f32,
    pub sun: f32,
}

impl Atlas {
    /// Atlas coordinates of lightmap coordinates `uv` in the source image at `placement`.
    pub fn coordinates(&self, placement: Placement, [u, v]: [f32; 2]) -> [f32; 2] {
        [
            (placement.column as f32 + u * placement.width as f32) / self.width as f32,
            (placement.row as f32 + v * placement.height as f32) / self.height as f32,
        ]
    }

    /// Atlas coordinates of the last neutral strip's middle.
    pub fn neutral(&self) -> [f32; 2] {
        let middle = (self.height - NEUTRAL_ROWS / 2) as f32;
        [0.5, middle / self.height as f32]
    }

    /// The texel holding atlas coordinates `uv`, unfiltered, decoded as surfaces decode it.
    pub fn texel(&self, [u, v]: [f32; 2]) -> Sample {
        let x = ((u * self.width as f32) as usize).min(self.width - 1);
        let y = ((v * self.height as f32) as usize).min(self.height - 1);
        self.texel_at(x, y)
    }

    /// Texel `(x, y)`, decoded as surfaces decode it. A surface filters the stored values, the
    /// logarithms, before decoding, so a mean of decoded texels is slightly brighter than its
    /// filtered sample.
    pub fn texel_at(&self, x: usize, y: usize) -> Sample {
        let at = (y * self.width + x) * 4;
        let unit = |bytes: &[u8], k: usize| f32::from(bytes[at + k]) / 255.;
        let color = |bytes: &[u8]| -> [f32; 3] {
            std::array::from_fn(|k| decode_share(unit(bytes, k)) * self.scale)
        };
        let axis: [f32; 3] = std::array::from_fn(|k| unit(&self.direction, k) * 2. - 1.);
        let length = axis.iter().map(|v| v * v).sum::<f32>().sqrt();
        Sample {
            lobe: color(&self.lobe),
            ambient: color(&self.ambient),
            direction: if length > 1e-5 {
                axis.map(|v| v / length)
            } else {
                [0., 0., 1.]
            },
            bandwidth: unit(&self.lobe, 3),
            sun: unit(&self.ambient, 3),
        }
    }
}

/// One decoded lightmap texel or lighting probe. Its colors stay linear until an atlas encodes them
/// against a peak, and its direction, bandwidth and sun visibility are already the bytes an atlas
/// stores.
pub(super) struct Texel {
    lobe: [f32; 3],
    ambient: [f32; 3],
    direction: [u8; 3],
    bandwidth: u8,
    sun: u8,
}

impl Texel {
    /// A texel of colors `lobe` and `ambient`, unit `direction`, and `bandwidth` and `sun` from 0 to
    /// 1.
    fn new(
        lobe: [f32; 3],
        ambient: [f32; 3],
        direction: [f32; 3],
        bandwidth: f32,
        sun: f32,
    ) -> Self {
        let byte = |value: f32| (value.clamp(0., 1.) * 255. + 0.5) as u8;
        Self {
            lobe,
            ambient,
            direction: direction.map(|v| byte(v * 0.5 + 0.5)),
            bandwidth: byte(bandwidth),
            sun: byte(sun),
        }
    }

    /// The texel of intensity layers `layers` and direction map channels `map`, as Reach's per-pixel
    /// lightmap shader decodes it, without the compression constant every texel shares.
    fn decode(layers: [[u8; 4]; LAYERS], map: [f32; 2]) -> Self {
        let unit = |value: u8| f32::from(value) / 255.;
        let axis = [0, 1, 2].map(|layer| unit(layers[layer][3]) * 2. - 1.);
        let length = axis.iter().map(|v| v * v).sum::<f32>().sqrt();
        let direction = if length > 1e-5 {
            axis.map(|v| v / length)
        } else {
            [0., 0., 1.]
        };
        let exposure = (-EXPONENT * map[0]).exp2();
        Self::new(
            [0, 1, 2]
                .map(|k| ((unit(layers[0][k]) + 2. * unit(layers[1][k]) - 1.) * exposure).max(0.)),
            [0, 1, 2].map(|k| 0.25 * unit(layers[2][k]) * exposure),
            direction,
            length,
            map[1],
        )
    }

    /// A probe of ambient light `color` without a lobe, and the sun's visibility `sun` from 0 to 1,
    /// as a decorator's own light is.
    pub(super) fn ambient(color: [f32; 3], sun: f32) -> Self {
        Self::new([0.; 3], color, [0., 0., 1.], 0., sun)
    }

    /// The brightest color channel.
    pub(super) fn peak(&self) -> f32 {
        self.lobe.into_iter().chain(self.ambient).fold(0., f32::max)
    }

    /// Luminance of irradiance over pi with half the lobe, a stand-in for an average normal.
    pub(super) fn level(&self) -> f32 {
        let luminance = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        0.5 * luminance(self.lobe) + luminance(self.ambient)
    }
}

/// One vertex's per-vertex lighting from its six bytes `c`, as Reach's per-vertex vertex shader
/// decodes it: lobe color from `c3` to `c5` and ambient color from the 5-bit fields of `c1 >> 1`
/// and `c2`, each squared, with the ambient's quarter the pixel shader takes, direction
/// `lut[c0 & 127]`, and sun visibility, squared, from `c0 >> 7` and `c1 & 1`.
fn vertex_lighting(c: &[u8], lut: &[[f32; 3]]) -> ([f32; 3], [f32; 3], [f32; 3], f32) {
    let lobe = [c[3], c[4], c[5]].map(|v| (f32::from(v) / 255.).powi(2));
    let five = [(c[1] >> 1) & 31, (c[1] >> 6) | ((c[2] & 7) << 2), c[2] >> 3];
    let ambient = five.map(|v| 0.25 * (f32::from(v) / 31.).powi(2));
    let direction = lut
        .get(usize::from(c[0] & 127))
        .copied()
        .unwrap_or([0., 0., 1.]);
    let sun = (f32::from(2 * (c[1] & 1) + (c[0] >> 7)) / 3.).powi(2);
    (lobe, ambient, direction, sun)
}

/// Decoded vertex lighting before interpolation, in the level's space and source radiance units.
pub(super) fn vertex(
    bytes: &[u8],
    lighting: PerVertex,
    source: u32,
    lut: &[[f32; 3]],
) -> Option<[[f32; 4]; 3]> {
    let index = usize::try_from(source.checked_sub(lighting.first)?).ok()?;
    let at = index.checked_mul(PER_VERTEX_BYTES)?;
    let c = bytes.get(at..at + PER_VERTEX_BYTES)?;
    let (lobe, ambient, direction, sun) = vertex_lighting(c, lut);
    Some([
        [
            lobe[0] * lighting.scale,
            lobe[1] * lighting.scale,
            lobe[2] * lighting.scale,
            1.,
        ],
        [
            ambient[0] * lighting.scale,
            ambient[1] * lighting.scale,
            ambient[2] * lighting.scale,
            sun,
        ],
        [direction[0], direction[1], direction[2], 1.],
    ])
}

/// The lighting probe of the mesh vertices of a part lit per vertex by `bytes`, the contents of the
/// buffer `lighting` names, each given as its source vertex and its normal in the level. Reach's
/// per-vertex programs pass each vertex's lobe direction in the level's space, at full bandwidth,
/// so a vertex's irradiance is its lobe times its normal's clamped cosine to the lobe, plus its
/// ambient color. The probe holds the average of that irradiance as ambient light without a lobe,
/// and the average sun visibility. With one lobe at the lobes' averaged direction instead, Hang 'Em
/// High's tombstones drew near black in game. `None` when `bytes` covers none of the vertices.
pub(super) fn probe(
    bytes: &[u8],
    lighting: PerVertex,
    vertices: impl Iterator<Item = (u32, [f32; 3])>,
    lut: &[[f32; 3]],
) -> Option<Texel> {
    let scale = lighting.scale;
    let mut irradiance = [0f32; 3];
    let (mut sun, mut count) = (0f32, 0usize);
    for (source, normal) in vertices {
        let Some(vertex) = source.checked_sub(lighting.first) else {
            continue;
        };
        let at = usize::try_from(vertex).ok()? * PER_VERTEX_BYTES;
        let Some(c) = bytes.get(at..at + PER_VERTEX_BYTES) else {
            continue;
        };
        let (l, a, d, s) = vertex_lighting(c, lut);
        let length = normal.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
        let cosine = (0..3).map(|k| normal[k] * d[k]).sum::<f32>() / length;
        for k in 0..3 {
            irradiance[k] += cosine.max(0.) * l[k] + a[k];
        }
        sun += s;
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let n = count as f32;
    Some(Texel::new(
        [0.; 3],
        irradiance.map(|v| v * scale / n),
        [0., 0., 1.],
        1.,
        sun / n,
    ))
}

/// Each placed instance's coloring row as a lighting probe, in instance order, `None` for an
/// instance without one. A present zero coloring remains black. The constant-lit pixel shader takes the row's first
/// half4 as the lobe's direction and the sun's visibility, its second as the lobe's color and
/// bandwidth, and a quarter of its fourth's color as the ambient color.
pub(super) struct Coloring {
    pub analytical_light: u32,
    pub texel: Texel,
}

pub(super) fn colorings(cache: &Cache, lightmap: &Tag) -> Result<Vec<Option<Coloring>>> {
    let at = lightmap.address()?;
    let rows = cache.block(at + COLORINGS, COLORING_SIZE)?;
    cache
        .block(at + INSTANCE_ROWS, INSTANCE_ROW_SIZE)?
        .into_iter()
        .map(|row| {
            let Some(&coloring) = usize::try_from(cache.i16(row + INSTANCE_COLORING)?)
                .ok()
                .and_then(|index| rows.get(index))
            else {
                return Ok(None);
            };
            let bytes = cache.bytes(coloring, 32)?;
            let h = std::array::from_fn::<f32, 16, _>(|k| {
                half::f16::from_le_bytes([bytes[k * 2], bytes[k * 2 + 1]]).to_f32()
            });
            let analytical_light = cache.u32(coloring + 0x20)?;
            ensure!(
                h.iter().all(|v| v.is_finite()),
                "Nonfinite instance coloring"
            );
            let direction = [h[0], h[1], h[2]];
            let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
            Ok(Some(Coloring {
                analytical_light,
                texel: Texel::new(
                    [h[4], h[5], h[6]].map(|v| v.max(0.) * COLORING_SCALE),
                    [h[12], h[13], h[14]].map(|v| 0.25 * v.max(0.) * COLORING_SCALE),
                    if length > 1e-5 {
                        direction.map(|v| v / length)
                    } else {
                        [0., 0., 1.]
                    },
                    h[7],
                    h[3],
                ),
            }))
        })
        .collect()
}

/// The sum and count of the lit levels of `texels`.
pub(super) fn lit_levels<'a>(texels: impl Iterator<Item = &'a Texel>) -> (f64, usize) {
    texels
        .map(Texel::level)
        .filter(|&level| level > 1e-6)
        .fold((0., 0), |(sum, lit), level| {
            (sum + f64::from(level), lit + 1)
        })
}

/// The per-vertex lighting directions, read from the lookup table bitmap the per-vertex vertex
/// shader samples, as that shader takes them: each texel's alpha, blue and green. `None` when the
/// cache holds no table.
pub(super) fn direction_lut(cache: &Cache, pages: &mut Pages) -> Result<Option<Vec<[f32; 3]>>> {
    let Some(tag) = cache
        .tags
        .iter()
        .flatten()
        .find(|tag| tag.group == "bitm" && tag.path == DIRECTION_LUT)
        .cloned()
    else {
        return Ok(None);
    };
    let bitmap = tag.address()?;
    let image = *cache
        .block(bitmap + IMAGES, IMAGE_SIZE)?
        .first()
        .context("Direction table has no image")?;
    let width = usize::from(cache.u16(image)?);
    ensure!(
        cache.i16(image + IMAGE_FORMAT)? == SIGNED_RGBA16
            && cache.u16(image + 2)? == 1
            && width > 0,
        "Direction table {} is not one row of signed 16-bit texels",
        tag.path
    );
    let handle = cache.u32(
        *cache
            .block(bitmap + RESOURCES, RESOURCE_SIZE)?
            .first()
            .context("Direction table has no resource")?,
    )?;
    let resource = cache.resource(&tag, handle)?;
    let bytes = pages.bytes(cache, &resource, 0, width * 8)?;
    Ok(Some(
        bytes
            .chunks_exact(8)
            .map(|texel| {
                let value = |k: usize| {
                    (f32::from(i16::from_le_bytes([texel[k * 2], texel[k * 2 + 1]])) / 32767.)
                        .max(-1.)
                };
                [value(3), value(2), value(1)]
            })
            .collect(),
    ))
}

/// A per-vertex lighting buffer: its vertex buffer, its scale and the first vertex of the mesh it
/// lights. The per-vertex vertex shader reads vertex `n`'s lighting at `n - first`.
#[derive(Clone, Copy)]
pub(super) struct PerVertex {
    pub buffer: usize,
    pub scale: f32,
    pub first: u32,
}

/// For each row of the block at `rows` in lightmap BSP data `lightmap`, the per-vertex lighting
/// buffer that the row's `i16` at `field` names, and the first vertex `first` reads from the row,
/// `None` for a row that names no buffer.
fn per_vertex_rows(
    cache: &Cache,
    lightmap: &Tag,
    (rows, size): (usize, usize),
    field: usize,
    first: impl Fn(usize) -> Result<u32>,
) -> Result<Vec<Option<PerVertex>>> {
    let at = lightmap.address()?;
    let buffers = cache
        .block(at + PER_VERTEX_BUFFERS, PER_VERTEX_BUFFER_SIZE)?
        .into_iter()
        .map(|row| {
            let scale = half::f16::from_bits(cache.u16(row + PER_VERTEX_SCALE)?).to_f32();
            Ok(usize::try_from(cache.i16(row)?)
                .ok()
                .filter(|_| scale.is_finite() && scale > 0.)
                .map(|buffer| (buffer, scale)))
        })
        .collect::<Result<Vec<_>>>()?;
    cache
        .block(at + rows, size)?
        .into_iter()
        .map(|row| {
            let Some((buffer, scale)) = usize::try_from(cache.i16(row + field)?)
                .ok()
                .and_then(|index| buffers.get(index).copied().flatten())
            else {
                return Ok(None);
            };
            Ok(Some(PerVertex {
                buffer,
                scale,
                first: first(row)?,
            }))
        })
        .collect()
}

/// Each placed instance's per-vertex lighting buffer, in instance order, `None` for an instance the
/// lightmap BSP data lights another way. An instance's buffer lights its mesh from the first vertex.
pub(super) fn per_vertex_buffers(cache: &Cache, lightmap: &Tag) -> Result<Vec<Option<PerVertex>>> {
    per_vertex_rows(
        cache,
        lightmap,
        (INSTANCE_ROWS, INSTANCE_ROW_SIZE),
        INSTANCE_PER_VERTEX,
        |_| Ok(0),
    )
}

/// Each structure cluster's per-vertex lighting buffer, in cluster order, `None` for a cluster
/// without parts lit per vertex.
pub(super) fn cluster_buffers(cache: &Cache, lightmap: &Tag) -> Result<Vec<Option<PerVertex>>> {
    per_vertex_rows(
        cache,
        lightmap,
        (CLUSTER_ROWS, CLUSTER_ROW_SIZE),
        CLUSTER_PER_VERTEX,
        |row| cache.u32(row + CLUSTER_FIRST_VERTEX),
    )
}

/// The blocks of bitmap `tag`'s first image, one buffer per layer, with its width and height.
fn layers(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    format: i16,
    count: usize,
) -> Result<(usize, usize, Vec<Vec<u8>>)> {
    let bitmap = tag.address()?;
    let image = *cache
        .block(bitmap + IMAGES, IMAGE_SIZE)?
        .first()
        .context("Lightmap bitmap has no image")?;
    let width = usize::from(cache.u16(image)?);
    let height = usize::from(cache.u16(image + 2)?);
    let layers = usize::from(cache.u8(image + IMAGE_LAYERS)?).max(1);
    ensure!(
        cache.i16(image + IMAGE_FORMAT)? == format
            && width > 0
            && height > 0
            && width <= 16_384
            && height <= 16_384
            && width.is_multiple_of(4)
            && height.is_multiple_of(4)
            && count <= layers,
        "Lightmap bitmap {} is not a format {format} image of {count} layers",
        tag.path
    );
    let handle = cache.u32(
        *cache
            .block(bitmap + RESOURCES, RESOURCE_SIZE)?
            .first()
            .context("Lightmap bitmap has no resource")?,
    )?;
    let resource = cache.resource(tag, handle)?;
    let layer_bytes = width / 4 * (height / 4) * BLOCK_BYTES;
    let blocks = (0..count)
        .map(|layer| pages.bytes(cache, &resource, layer * layer_bytes, layer_bytes))
        .collect::<Result<Vec<_>>>()?;
    Ok((width, height, blocks))
}

/// Calls `visit` with each texel's index and its decoded value from 16-byte blocks laid out row
/// by row across `width`.
fn texels<T>(
    blocks: &[u8],
    width: usize,
    decode: impl Fn(&[u8]) -> [T; 16],
    mut visit: impl FnMut(usize, T),
) {
    for (index, block) in blocks.chunks_exact(BLOCK_BYTES).enumerate() {
        let (bx, by) = (index % (width / 4), index / (width / 4));
        for (texel, value) in decode(block).into_iter().enumerate() {
            visit((by * 4 + texel / 4) * width + bx * 4 + texel % 4, value);
        }
    }
}

/// A color's stored byte from its share of the atlas's peak ([`LOG_RANGE`]). The hardware encoder
/// (`compute/encode.hlsl`) evaluates the same expression in the same order.
pub(in crate::halo_reach) fn encode_share(share: f32) -> u8 {
    ((1. + share.clamp(0., 1.) * LOG_RANGE).log2() / (1. + LOG_RANGE).log2() * 255. + 0.5) as u8
}

/// A stored color channel's share of the atlas's peak, from the channel as a texture sample reads
/// it, from 0 to 1. Filtering between texels therefore interpolates their logarithms.
pub(in crate::halo_reach) fn decode_share(stored: f32) -> f32 {
    ((stored * (1. + LOG_RANGE).log2()).exp2() - 1.) / LOG_RANGE
}

/// [`decode_share`] as an HLSL expression of the stored channels `stored`, a float or vector.
pub(in crate::halo_reach) fn decode_share_hlsl(stored: &str) -> String {
    format!(
        "((exp2(({stored})*{:.9})-1)*{:e})",
        (1. + LOG_RANGE).log2(),
        1. / LOG_RANGE
    )
}

fn encode_texel(texel: &Texel, peak: f32) -> [u8; 12] {
    let [r, g, b] = texel.lobe.map(|c| encode_share(c / peak));
    let [ar, ag, ab] = texel.ambient.map(|c| encode_share(c / peak));
    let [x, y, z] = texel.direction;
    [
        r,
        g,
        b,
        texel.bandwidth,
        ar,
        ag,
        ab,
        texel.sun,
        x,
        y,
        z,
        255,
    ]
}

/// A BSP's lightmap image, decoded: `width` by `height` texels, row by row.
pub(super) struct Lighting {
    width: usize,
    height: usize,
    texels: Vec<Texel>,
}

impl Lighting {
    /// The sum and count of the image's lit levels.
    pub(super) fn levels(&self) -> (f64, usize) {
        lit_levels(self.texels.iter())
    }
}

/// The decoded lightmap image of lightmap BSP data `lightmap`, or `None` when it names no
/// intensity map.
pub(super) fn decode(cache: &Cache, pages: &mut Pages, lightmap: &Tag) -> Result<Option<Lighting>> {
    let at = lightmap.address()?;
    let Some(intensity) = cache.reference(at + INTENSITY, Some("bitm"))? else {
        return Ok(None);
    };
    let direction = cache
        .reference(at + DIRECTION, Some("bitm"))?
        .context("Lightmap intensity has no direction map")?;
    let (width, height, intensity_layers) = layers(cache, pages, &intensity, DXT5, LAYERS)?;
    let (map_width, map_height, map) = layers(cache, pages, &direction, BC5, 1)?;
    ensure!(
        (map_width, map_height) == (width, height),
        "Lightmap direction map {} is {map_width} by {map_height}, its intensity {width} by {height}",
        direction.path
    );
    let brightness = cache.f32(at + BRIGHTNESS)?;
    ensure!(brightness.is_finite(), "Lightmap brightness is not finite");
    let constant = if brightness > 0. { brightness } else { 1. };
    ensure!(
        constant <= f32::MAX / 2.,
        "Lightmap brightness overflows decoded lighting"
    );
    if let Some(texels) = compute::decode(width, height, &intensity_layers, &map[0], constant)? {
        return Ok(Some(Lighting {
            width,
            height,
            texels,
        }));
    }
    let mut intensities = vec![[[0u8; 4]; LAYERS]; width * height];
    for (layer, blocks) in intensity_layers.iter().enumerate() {
        texels(
            blocks,
            width,
            |block| {
                let mut rgba = [0u8; 64];
                bcdec_rs::bc3(block, &mut rgba, 16);
                std::array::from_fn(|t| {
                    [
                        rgba[t * 4],
                        rgba[t * 4 + 1],
                        rgba[t * 4 + 2],
                        rgba[t * 4 + 3],
                    ]
                })
            },
            |index, rgba| intensities[index][layer] = rgba,
        );
    }
    let mut channels = vec![[0f32; 2]; width * height];
    texels(
        &map[0],
        width,
        |block| {
            let mut xy = [0f32; 32];
            bcdec_rs::bc5_float(block, &mut xy, 8, false);
            std::array::from_fn(|t| [xy[t * 2], xy[t * 2 + 1]])
        },
        |index, xy| channels[index] = xy,
    );
    let texels = intensities
        .into_iter()
        .zip(channels)
        .map(|(layers, map)| {
            let mut texel = Texel::decode(layers, map);
            texel.lobe = texel.lobe.map(|c| c * constant);
            texel.ambient = texel.ambient.map(|c| c * constant);
            texel
        })
        .collect();
    Ok(Some(Lighting {
        width,
        height,
        texels,
    }))
}

/// Packs `images`, each loaded BSP's decoded lightmap image when it has one, and `probes`, each
/// BSP's lighting probes, into one atlas ([`arrange`]). Each image sits on a shelf ringed by
/// neutral lighting, the probes follow in blocks of `PROBE_BLOCK` texels above a last neutral strip,
/// and every color is encoded against the 99.9th percentile peak of all of them. Returns the atlas
/// with each image's placement and each probe's coordinates, or `None` when no BSP has an image or
/// a probe.
pub(super) fn pack(
    images: Vec<Option<Lighting>>,
    probes: Vec<Vec<Texel>>,
) -> Result<Option<Packed>> {
    let probe_count = probes.iter().map(Vec::len).sum::<usize>();
    if images.iter().all(Option::is_none) && probe_count == 0 {
        return Ok(None);
    }
    let image_texels = || images.iter().flatten().flat_map(|image| &image.texels);
    let texels = || image_texels().chain(probes.iter().flatten());
    let mut peaks = texels()
        .map(Texel::peak)
        .filter(|&peak| peak > 1e-6)
        .collect::<Vec<_>>();
    ensure!(!peaks.is_empty(), "Every lightmap texel and probe is dark");
    let quantile = ((peaks.len() - 1) as f64 * PEAK_QUANTILE) as usize;
    let peak = *peaks.select_nth_unstable_by(quantile, f32::total_cmp).1;
    drop(peaks);
    let average = |levels: &mut dyn Iterator<Item = f32>| {
        let (sum, lit) = levels
            .filter(|&level| level > 1e-6)
            .fold((0f64, 0usize), |(sum, lit), level| {
                (sum + f64::from(level), lit + 1)
            });
        (sum / lit.max(1) as f64) as f32
    };
    let all = average(&mut texels().map(Texel::level));
    ensure!(all > 0., "Every lightmap texel and probe is dark");
    let averages = [
        average(&mut image_texels().map(Texel::level)),
        average(&mut probes.iter().flatten().map(Texel::level)),
    ];
    let scale = peak / all;
    let sizes = images
        .iter()
        .flatten()
        .map(|image| (image.width, image.height))
        .collect::<Vec<_>>();
    let layout = arrange(&sizes, probe_count)?;
    let (width, height) = (layout.width, layout.height);
    let per_row = (width / PROBE_BLOCK).max(1);
    // Neutral texels have no lobe and the average lit texel's irradiance as their ambient color.
    let neutral = encode_share(1. / scale);
    // Neutral rings around the images and the last strip. Other texels stay black: no coordinates
    // reach them. The open sky light (`occlusion::sky_light`) reads the last strip's neutral texel.
    let mut lobe = [0, 0, 0, 0].repeat(width * height);
    let mut ambient = [0, 0, 0, 255].repeat(width * height);
    let mut direction = [128, 128, 255, 255].repeat(width * height);
    let mut fill = |columns: std::ops::Range<usize>, rows: std::ops::Range<usize>| {
        for y in rows {
            for x in columns.clone() {
                let at = (y * width + x) * 4;
                ambient[at..at + 3].fill(neutral);
            }
        }
    };
    for (&(column, row), &(w, h)) in layout.origins.iter().zip(&sizes) {
        fill(
            column.saturating_sub(NEUTRAL_ROWS)..(column + w + NEUTRAL_ROWS).min(width),
            row.saturating_sub(NEUTRAL_ROWS)..(row + h + NEUTRAL_ROWS).min(height),
        );
    }
    fill(0..width, height - NEUTRAL_ROWS..height);
    let mut write = |at: usize, bytes: &[u8]| {
        lobe[at..at + 4].copy_from_slice(&bytes[..4]);
        ambient[at..at + 4].copy_from_slice(&bytes[4..8]);
        direction[at..at + 4].copy_from_slice(&bytes[8..12]);
    };
    let mut placements = Vec::with_capacity(images.len());
    let mut origins = layout.origins.iter().copied();
    for image in &images {
        let Some(image) = image else {
            placements.push(None);
            continue;
        };
        let (column, row) = origins
            .next()
            .context("Lightmap image outside its layout")?;
        let encoded = compute::encode(&image.texels, peak)?;
        for (index, texel) in image.texels.iter().enumerate() {
            let cpu;
            let bytes = if let Some(encoded) = &encoded {
                &encoded[index * 12..index * 12 + 12]
            } else {
                cpu = encode_texel(texel, peak);
                &cpu
            };
            write(
                ((row + index / image.width) * width + column + index % image.width) * 4,
                bytes,
            );
        }
        placements.push(Some(Placement {
            row,
            column,
            width: image.width,
            height: image.height,
        }));
    }
    // Each probe fills a block, and its coordinates are the block's middle, so filtering within the
    // block reads the probe alone.
    let mut coordinates = Vec::with_capacity(probes.len());
    let mut next = 0;
    for bsp in &probes {
        let mut placed = Vec::with_capacity(bsp.len());
        for texel in bsp {
            let encoded = encode_texel(texel, peak);
            let (x0, y0) = (
                next % per_row * PROBE_BLOCK,
                layout.probe_row + next / per_row * PROBE_BLOCK,
            );
            for y in y0..y0 + PROBE_BLOCK {
                for x in x0..x0 + PROBE_BLOCK {
                    write((y * width + x) * 4, &encoded);
                }
            }
            let middle = PROBE_BLOCK as f32 / 2.;
            placed.push([
                (x0 as f32 + middle) / width as f32,
                (y0 as f32 + middle) / height as f32,
            ]);
            next += 1;
        }
        coordinates.push(placed);
    }
    Ok(Some(Packed {
        atlas: Atlas {
            width,
            height,
            lobe,
            ambient,
            direction,
            scale,
            level: all,
            probes: probe_count,
            averages,
        },
        placements,
        probes: coordinates,
    }))
}

/// Where [`pack`] puts its images and probes: the atlas's width and height, each image's column
/// and row in the order of the sizes laid out, and the first row of the probe blocks.
struct Layout {
    width: usize,
    height: usize,
    origins: Vec<(usize, usize)>,
    probe_row: usize,
}

/// Lays out images of `sizes`, each a width and height, and `probes` probe blocks across `width`:
/// the images on shelves, tallest first, each `NEUTRAL_ROWS` texels from the next on either axis,
/// then the probe blocks in rows, then a last neutral strip.
fn layout(sizes: &[(usize, usize)], probes: usize, width: usize) -> Layout {
    let mut order = (0..sizes.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| std::cmp::Reverse((sizes[index].1, sizes[index].0)));
    let mut origins = vec![(0, 0); sizes.len()];
    let (mut x, mut y, mut shelf) = (0, 0, 0);
    for index in order {
        let (w, h) = sizes[index];
        if x > 0 && x + w > width {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        origins[index] = (x, y);
        x += w + NEUTRAL_ROWS;
        shelf = shelf.max(h + NEUTRAL_ROWS);
    }
    let probe_row = y + shelf;
    let probe_rows = probes.div_ceil((width / PROBE_BLOCK).max(1)) * PROBE_BLOCK;
    Layout {
        width,
        height: probe_row
            + if probes > 0 {
                probe_rows + NEUTRAL_ROWS
            } else {
                0
            },
        origins,
        probe_row,
    }
}

/// The squarest [`layout`] of images of `sizes` and `probes` probe blocks, the one whose longer
/// side is shortest, among widths of a whole number of the widest images and widths near the
/// square of the texels laid out. Coordinates keep a fixed step across each side, so the longer
/// side sets their precision: stacked in one column, Nightfall's images and probes took an atlas
/// 1,024 by 7,118 texels.
fn arrange(sizes: &[(usize, usize)], probes: usize) -> Result<Layout> {
    let widest = sizes.iter().map(|size| size.0).max().unwrap_or(PROBE_BLOCK);
    let area = sizes
        .iter()
        .map(|(w, h)| (w + NEUTRAL_ROWS) * (h + NEUTRAL_ROWS))
        .sum::<usize>()
        + probes * PROBE_BLOCK * PROBE_BLOCK;
    let side = (area as f64).sqrt().ceil() as usize;
    let mut widths = sizes.iter().map(|size| size.0).collect::<Vec<_>>();
    widths.sort_unstable_by(|a, b| b.cmp(a));
    let mut candidates = vec![widest, side, side * 9 / 8, side * 5 / 4, side * 3 / 2];
    let mut run = 0;
    for width in widths {
        candidates.push(run + width);
        run += width + NEUTRAL_ROWS;
    }
    candidates
        .into_iter()
        .map(|width| width.max(widest).next_multiple_of(4).min(MAX_SIDE))
        .filter(|&width| width >= widest)
        .map(|width| layout(sizes, probes, width))
        .filter(|candidate| candidate.height <= MAX_SIDE)
        .min_by_key(|candidate| {
            (
                candidate.width.max(candidate.height),
                candidate.width * candidate.height,
            )
        })
        .with_context(|| {
            format!(
                "{} lightmap images and {probes} probes exceed a {MAX_SIDE} texel atlas",
                sizes.len()
            )
        })
}

/// Lightmap texture coordinates from a stream of two unsigned normalized 16-bit values per vertex.
pub(super) fn texcoords(declaration: u32, count: usize, bytes: &[u8]) -> Result<Vec<[f32; 2]>> {
    ensure!(
        declaration == TEXCOORD_DECLARATION && bytes.len() == count * 4,
        "Lightmap texture coordinates have declaration {declaration:#x} and {} bytes for {count} vertices",
        bytes.len()
    );
    Ok(bytes
        .chunks_exact(4)
        .map(|v| [0, 2].map(|at| f32::from(u16::from_le_bytes([v[at], v[at + 1]])) / 65535.))
        .collect())
}

/// Each placed instance's lightmap texture coordinate vertex buffer, in instance order.
pub(super) fn instance_buffers(cache: &Cache, lightmap: &Tag) -> Result<Vec<Option<usize>>> {
    cache
        .block(
            lightmap.address()? + INSTANCE_TEXCOORDS,
            INSTANCE_TEXCOORD_SIZE,
        )?
        .into_iter()
        .map(|row| Ok(usize::try_from(cache.i16(row)?).ok()))
        .collect()
}
