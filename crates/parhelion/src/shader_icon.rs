//! A shader's inventory icon drawn from its dyes, in the style of the stock icons: the square split
//! by both diagonals into four triangles, each one dye surface with its detail textures on a
//! grained and scratched plate, all under one light and a reflected room.
//!
//! The stock icons show the weapon dyes: the first channel's primary at the top, the second
//! channel's primary at the left, and the third channel's primary and secondary at the right and
//! the bottom. They stretch each detail texture once across the icon whatever the dye's own
//! tiling, and draw every dye on the same plate, whose grain and scratches no dye carries. The
//! plate here is our own, worn through the way each dye wears on gear, and the light and room were
//! fitted to all 306 stock icons.

use std::sync::OnceLock;

use image::{Rgba, RgbaImage};

use crate::dye::linear_to_srgb;

/// Icons are 96 pixels square.
pub(crate) const EDGE: u32 = 96;

/// The surface each triangle shows, as a channel and its primary (0) or secondary (1) surface, for
/// the top, left, right and bottom triangles.
pub(crate) const TRIANGLES: [(usize, usize); 4] = [(0, 0), (1, 0), (2, 0), (2, 1)];

/// A texture's pixels as the package stores them.
pub(crate) struct IconTexture {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) rgba: Vec<u8>,
}

/// An iridescence row's linear colors across the row, as 0 to 255. An odd row tints the highlight,
/// an even row tints the color and makes it metal.
pub(crate) struct Iridescence<'a> {
    pub(crate) colors: &'a [[u8; 3]],
    pub(crate) highlight: bool,
}

/// A dye surface's finish, painted or worn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Finish {
    /// Linear color.
    pub(crate) albedo: [f32; 3],
    /// Detail color, detail normal and detail smoothness strengths, then metalness.
    pub(crate) params: [f32; 4],
    /// The smoothness remap: offset, scale, minimum and range.
    pub(crate) smoothness: [f32; 4],
}

/// One triangle's surface, as its dye's material describes it.
pub(crate) struct IconSurface<'a> {
    pub(crate) paint: Finish,
    /// What scratches wear through to.
    pub(crate) worn: Finish,
    /// How the plate's scratches become this surface's wear: offset, scale, minimum and range.
    pub(crate) wear: [f32; 4],
    pub(crate) iridescence: Option<Iridescence<'a>>,
    /// The detail texture: sRGB color, and smoothness in alpha.
    pub(crate) detail: Option<&'a IconTexture>,
    /// The detail normal texture: the normal in red and green, occlusion in blue.
    pub(crate) normal: Option<&'a IconTexture>,
}

/// The camera, lights and exposure the icons are drawn under. Distances are in half-widths of the
/// icon from its center: x right, y down, z toward the viewer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Studio {
    /// The camera's height over the icon's center. Reflections and the iridescence ramp shift
    /// across the icon because it is this close.
    pub(crate) camera: f32,
    /// The key light's position.
    pub(crate) light: [f32; 3],
    /// The key light's brightness, and its size as the least roughness its glint shows with.
    pub(crate) key: f32,
    pub(crate) key_size: f32,
    /// Light from every side, on paint.
    pub(crate) fill: f32,
    /// The reflected studio, laid out over the icon: its brightness at the dark upper left and
    /// the bright lower right.
    pub(crate) room: [f32; 2],
    /// A softbox in the studio: its brightness, where it sits over the icon, and its size.
    pub(crate) softbox: f32,
    pub(crate) softbox_at: [f32; 2],
    pub(crate) softbox_size: f32,
    /// Soft clouds in the studio's brightness: how strong, and how many across the icon.
    pub(crate) clouds: f32,
    pub(crate) cloud_scale: f32,
    /// How far a tilted normal moves the reflected point, in half-widths per unit of tilt.
    pub(crate) tilt: f32,
    /// How fast roughness blurs the studio's clouds and softbox away.
    pub(crate) blur: f32,
    pub(crate) exposure: f32,
    /// Detail texture repeats across the icon.
    pub(crate) detail_scale: f32,
    /// The smoothness under the detail texture, or the middle of each dye's own range.
    pub(crate) plate_smoothness: Option<f32>,
    /// How much stronger than in game the icons draw each dye's detail normal.
    pub(crate) normal_gain: f32,
    /// How much stronger than in game the icons draw each dye's detail color.
    pub(crate) detail_gain: f32,
    /// How strongly the plate's grain shows.
    pub(crate) grain: f32,
    /// How deep the plate's scratches go, from none to bare.
    pub(crate) scratches: f32,
}

/// The studio fitted to the stock icons.
pub(crate) const STUDIO: Studio = Studio {
    camera: 2.61,
    light: [0.68, 0.8, 1.65],
    key: 1.07,
    key_size: 0.3,
    fill: 0.79,
    room: [0.25, 0.68],
    softbox: 1.6,
    softbox_at: [0.2, 0.3],
    softbox_size: 0.35,
    clouds: 0.8,
    cloud_scale: 3.0,
    tilt: 3.0,
    blur: 3.0,
    exposure: 0.894,
    detail_scale: 1.0,
    plate_smoothness: None,
    normal_gain: 1.0,
    detail_gain: 1.0,
    grain: 0.07,
    scratches: 0.8,
};

/// The icon for the top, left, right and bottom triangles' surfaces.
pub(crate) fn classic(surfaces: &[IconSurface<'_>; 4]) -> RgbaImage {
    draw(surfaces, &STUDIO)
}

/// The icon under a given studio.
pub(crate) fn draw(surfaces: &[IconSurface<'_>; 4], studio: &Studio) -> RgbaImage {
    let prepared = surfaces
        .each_ref()
        .map(|surface| Prepared::new(surface, studio, EDGE));
    let plate = Plate::shared();
    RgbaImage::from_fn(EDGE, EDGE, |x, y| {
        let at = (y * EDGE + x) as usize;
        let u = (x as f32 + 0.5) / EDGE as f32;
        let v = (y as f32 + 0.5) / EDGE as f32;
        let position = [u * 2.0 - 1.0, v * 2.0 - 1.0];
        let shade = |triangle: usize| {
            prepared[triangle].shade(
                &surfaces[triangle],
                studio,
                [u, v],
                (position, FACING),
                (plate.grain[at], plate.scratch[at]),
            )
        };
        // Where a diagonal crosses the pixel, each triangle counts for the share it covers.
        let mut coverage = [0.0_f32; 4];
        for sample in 0..EDGE_SAMPLES * EDGE_SAMPLES {
            let offset = |index: u32| (index as f32 + 0.5) / EDGE_SAMPLES as f32;
            let point = [
                (x as f32 + offset(sample % EDGE_SAMPLES)) / EDGE as f32 * 2.0 - 1.0,
                (y as f32 + offset(sample / EDGE_SAMPLES)) / EDGE as f32 * 2.0 - 1.0,
            ];
            coverage[triangle_at(point)] += 1.0;
        }
        let total = (EDGE_SAMPLES * EDGE_SAMPLES) as f32;
        let mut color = [0.0_f32; 3];
        for (triangle, covered) in coverage.into_iter().enumerate() {
            if covered > 0.0 {
                let shaded = shade(triangle);
                for (sum, channel) in color.iter_mut().zip(shaded) {
                    *sum += channel * covered / total;
                }
            }
        }
        let [red, green, blue] = color.map(linear_to_srgb);
        Rgba([red, green, blue, 255])
    })
}

/// Samples a side each pixel is split into to find how much of it each triangle covers.
const EDGE_SAMPLES: u32 = 4;

/// A flat surface's normal, facing the viewer.
const FACING: [f32; 3] = [0.0, 0.0, 1.0];

/// How far a ball's points sit from the icons' center, and how far its normals move a
/// reflection, so the studio spans the ball once. With the icons' own reach and tilt a sphere's
/// normals swept the whole studio in a few pixels and pinched the softbox into a dot.
const BALL_REACH: f32 = 0.3;
const BALL_TILT: f32 = 1.0;

/// A ball of one surface, `edge` pixels square with a clear surround, under the icons' studio.
/// Its textures stretch once across the ball, as on the icons.
pub(crate) fn swatch(surface: &IconSurface<'_>, edge: u32) -> RgbaImage {
    let studio = &Studio {
        tilt: BALL_TILT,
        ..STUDIO
    };
    let prepared = Prepared::new(surface, studio, edge);
    let center = edge as f32 / 2.0;
    let radius = center - 1.0;
    RgbaImage::from_fn(edge, edge, |x, y| {
        let offset = |index: u32| (index as f32 + 0.5) / EDGE_SAMPLES as f32;
        let covered = (0..EDGE_SAMPLES * EDGE_SAMPLES)
            .filter(|sample| {
                let across = (x as f32 + offset(sample % EDGE_SAMPLES) - center) / radius;
                let down = (y as f32 + offset(sample / EDGE_SAMPLES) - center) / radius;
                across * across + down * down < 1.0
            })
            .count();
        if covered == 0 {
            return Rgba([0, 0, 0, 0]);
        }
        // Shaded once at the pixel's center, drawn just inside the rim there. The icons' light
        // sits low on the right, so the ball is turned half round to read lit from the upper
        // left, as material balls do.
        let mut point = [
            (center - x as f32 - 0.5) / radius,
            (center - y as f32 - 0.5) / radius,
        ];
        let reach = (point[0] * point[0] + point[1] * point[1]).sqrt();
        if reach > 0.999 {
            point = point.map(|value| value / reach * 0.999);
        }
        let normal = [
            point[0],
            point[1],
            (1.0 - point[0] * point[0] - point[1] * point[1])
                .max(0.0)
                .sqrt(),
        ];
        let uv = point.map(|value| (value + 1.0) * 0.5);
        let position = point.map(|value| value * BALL_REACH);
        let [red, green, blue] = prepared
            .shade(surface, studio, uv, (position, normal), (0.0, 0.0))
            .map(linear_to_srgb);
        let alpha = covered as f32 / (EDGE_SAMPLES * EDGE_SAMPLES) as f32;
        Rgba([red, green, blue, (alpha * 255.0).round() as u8])
    })
}

/// The triangle a point on the icon falls in, from -1 to 1 across: top, left, right or bottom.
fn triangle_at([x, y]: [f32; 2]) -> usize {
    if y.abs() > x.abs() {
        if y < 0.0 { 0 } else { 3 }
    } else if x < 0.0 {
        1
    } else {
        2
    }
}

/// A surface's textures filtered to the icon's pixel size.
struct Prepared {
    detail: Option<Level>,
    normal: Option<Level>,
    plate_smoothness: f32,
}

impl Prepared {
    /// `size` is how many pixels the textures stretch across.
    fn new(surface: &IconSurface<'_>, studio: &Studio, size: u32) -> Self {
        let filtered = |texture: Option<&IconTexture>, color: bool| {
            texture
                .filter(|texture| {
                    texture.width > 0
                        && texture.height > 0
                        && texture.rgba.len() >= texture.width * texture.height * 4
                })
                .map(|texture| Level::new(texture, studio.detail_scale, size, color))
        };
        let [offset, scale, minimum, range] = surface.paint.smoothness;
        // With no gear plate under it, the swatch sits where the dye's remap puts the middle of
        // its range.
        let plate_smoothness = studio.plate_smoothness.unwrap_or_else(|| {
            if scale.abs() > f32::EPSILON {
                ((minimum + range * 0.5 - offset) / scale).clamp(0.0, 1.0)
            } else {
                0.5
            }
        });
        Self {
            detail: filtered(surface.detail, true),
            normal: filtered(surface.normal, false),
            plate_smoothness,
        }
    }

    /// The surface's linear color at `uv` on the icon, whose centered position is `position` and
    /// whose own normal is `base`, over the plate's grain and scratch there.
    fn shade(
        &self,
        surface: &IconSurface<'_>,
        studio: &Studio,
        uv: [f32; 2],
        (position, base): ([f32; 2], [f32; 3]),
        (grain, scratch): (f32, f32),
    ) -> [f32; 3] {
        // The game wears paint where the plate's wear mask runs low, by each dye's own remap.
        let intact = saturate(remap(1.0 - scratch * studio.scratches, surface.wear));
        let params: [f32; 4] = std::array::from_fn(|channel| {
            mix(
                surface.worn.params[channel],
                surface.paint.params[channel],
                intact,
            )
        });
        let [detail_strength, _, smoothness_strength, metal] = params.map(saturate);
        let detail_strength = saturate(detail_strength * studio.detail_gain);
        let uv = uv.map(|value| value * studio.detail_scale);
        // The plate's color under the dye is a quarter, which the dye's blend leaves unchanged,
        // plus its grain.
        let plate = 0.25 + grain * studio.grain;
        let tone: [f32; 3] = std::array::from_fn(|channel| {
            let color = mix(
                surface.worn.albedo[channel],
                surface.paint.albedo[channel],
                intact,
            );
            overlay(plate, color.max(0.0))
        });
        let mut albedo = tone;
        let mut smoothness = saturate(self.plate_smoothness + grain * studio.grain * 0.5);
        if let Some(level) = &self.detail {
            let texel = level.sample(uv);
            albedo = [0, 1, 2].map(|channel| {
                mix(
                    albedo[channel],
                    overlay(texel[channel], albedo[channel]),
                    detail_strength,
                )
            });
            smoothness = mix(
                smoothness,
                overlay(smoothness, texel[3]),
                smoothness_strength,
            );
        }
        let smoothness = mix(
            remap(smoothness, surface.worn.smoothness),
            remap(smoothness, surface.paint.smoothness),
            intact,
        );
        let mut normal = base;
        let mut occlusion = 1.0;
        if let Some(level) = &self.normal {
            let strength = (params[1] * studio.normal_gain).clamp(0.0, 4.0);
            let texel = level.sample(uv);
            let x = (mix(0.5, texel[0], strength) * 2.0 - 1.0).clamp(-1.0, 1.0);
            let y = (mix(0.5, texel[1], strength) * 2.0 - 1.0).clamp(-1.0, 1.0);
            let tilted = normalize([x, y, (1.0 - x * x - y * y).max(0.0).sqrt()]);
            normal = turned(base, tilted);
            occlusion = mix(1.0, texel[2], strength.min(1.0));
        }
        let to_viewer = normalize([-position[0], -position[1], studio.camera]);
        let facing_viewer = dot(normal, to_viewer).max(1.0e-4);
        let mut metal = metal;
        let mut tint = [1.0; 3];
        if let Some(iridescence) = surface
            .iridescence
            .as_ref()
            .filter(|row| !row.colors.is_empty())
        {
            // The game reads the row by how squarely the surface faces the viewer.
            let color = ramp(iridescence.colors, facing_viewer);
            let strength = 1.0 - luminance(tone);
            if iridescence.highlight {
                tint = color.map(|channel| mix(1.0, channel, strength));
            } else {
                albedo = [0, 1, 2].map(|channel| mix(albedo[channel], color[channel], strength));
                metal = mix(metal, 1.0, strength);
            }
        }
        let to_light = normalize([
            studio.light[0] - position[0],
            studio.light[1] - position[1],
            studio.light[2],
        ]);
        let half = normalize([
            to_light[0] + to_viewer[0],
            to_light[1] + to_viewer[1],
            to_light[2] + to_viewer[2],
        ]);
        let facing_light = dot(normal, to_light).max(0.0);
        // Cook-Torrance with a GGX lobe for the key light, no sharper than the light is large.
        let roughness = (1.0 - smoothness).clamp(0.04, 1.0);
        let glint = roughness.max(studio.key_size);
        let alpha = (glint * glint).max(0.002);
        let spread = dot(normal, half).max(0.0).powi(2) * (alpha * alpha - 1.0) + 1.0;
        let distribution = alpha * alpha / (std::f32::consts::PI * spread * spread);
        let k = (glint + 1.0).powi(2) / 8.0;
        let shadowing = facing_light / (facing_light * (1.0 - k) + k)
            * (facing_viewer / (facing_viewer * (1.0 - k) + k));
        let grazing = (1.0 - dot(half, to_viewer).max(0.0)).powi(5);
        // The studio the surface reflects, laid out over the icon. A tilted normal reflects a point
        // further along, and roughness blurs the clouds and softbox into the gradient under them.
        let reflected = [
            position[0] + normal[0] * studio.tilt,
            position[1] + normal[1] * studio.tilt,
        ];
        let gradient = saturate(0.5 + (reflected[0] + reflected[1]) * 0.35);
        let base = mix(studio.room[0], studio.room[1], gradient);
        let offset = [
            reflected[0] - studio.softbox_at[0],
            reflected[1] - studio.softbox_at[1],
        ];
        let softbox = studio.softbox
            * (-(offset[0] * offset[0] + offset[1] * offset[1])
                / (studio.softbox_size * studio.softbox_size))
                .exp();
        let clouds = (value_noise(
            (reflected[0] + 1.0) * studio.cloud_scale,
            (reflected[1] + 1.0) * studio.cloud_scale,
            3,
        ) - 0.5)
            * 2.0
            * studio.clouds;
        // Clouds brighten and darken around the studio's middle brightness, so even its dark
        // side shows them.
        let middle = (studio.room[0] + studio.room[1]) * 0.5;
        let sharp = (base + softbox + clouds * middle).max(0.0);
        let room = mix(
            sharp,
            base + softbox * 0.3,
            saturate(roughness * studio.blur),
        );
        [0, 1, 2].map(|channel| {
            let base = albedo[channel];
            let reflectance = mix(0.04, base, metal);
            let fresnel = reflectance + (1.0 - reflectance) * grazing;
            let specular = distribution * shadowing * fresnel / (4.0 * facing_viewer);
            let diffuse = base * (1.0 - metal) / std::f32::consts::PI * facing_light;
            let direct = (diffuse + specular * tint[channel]) * studio.key;
            let ambient = base * (1.0 - metal) * studio.fill * occlusion
                + reflectance * tint[channel] * room * occlusion;
            (direct + ambient) * studio.exposure
        })
    }
}

/// The plate every icon is drawn on: a grain from -0.5 to 0.5 and a scratch coverage from 0 to 1
/// for each pixel. Seeded, so every icon gets the same plate.
struct Plate {
    grain: Vec<f32>,
    scratch: Vec<f32>,
}

impl Plate {
    fn shared() -> &'static Self {
        static PLATE: OnceLock<Plate> = OnceLock::new();
        PLATE.get_or_init(Self::new)
    }

    fn new() -> Self {
        let edge = EDGE as usize;
        let grain = (0..edge * edge)
            .map(|at| {
                let (x, y) = ((at % edge) as f32, (at / edge) as f32);
                0.6 * value_noise(x / 1.5, y / 1.5, 1) + 0.4 * value_noise(x / 4.0, y / 4.0, 2)
                    - 0.5
            })
            .collect();
        let mut random = Random(0x5EED_1C0E);
        let mut strokes = Vec::new();
        for _ in 0..SCRATCHES {
            let mut point = [random.next() * EDGE as f32, random.next() * EDGE as f32];
            let mut angle = random.next() * std::f32::consts::TAU;
            let bend = (random.next() - 0.5) * 0.08;
            let length = 6.0 + random.next() * 30.0;
            let width = 0.5 + random.next() * 0.6;
            let depth = 0.5 + random.next() * 0.5;
            let mut travelled = 0.0;
            while travelled < length {
                let next = [point[0] + angle.cos() * 2.0, point[1] + angle.sin() * 2.0];
                strokes.push((point, next, width, depth));
                point = next;
                angle += bend;
                travelled += 2.0;
            }
        }
        let scratch = (0..edge * edge)
            .map(|at| {
                let pixel = [(at % edge) as f32 + 0.5, (at / edge) as f32 + 0.5];
                strokes
                    .iter()
                    .map(|&(from, to, width, depth)| {
                        let distance = segment_distance(pixel, from, to);
                        depth * saturate(1.0 - distance / width)
                    })
                    .fold(0.0, f32::max)
            })
            .collect();
        Self { grain, scratch }
    }
}

/// Scratches on the plate.
const SCRATCHES: usize = 28;

/// A small seeded generator, so the plate never changes between runs.
struct Random(u32);

impl Random {
    /// The next value from 0 to 1.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1 << 24) as f32
    }
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut value = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0xCB1A_B31F);
    value ^= value >> 13;
    value = value.wrapping_mul(0x5BD1_E995);
    value ^= value >> 15;
    (value & 0xFFFF) as f32 / 65_535.0
}

/// Smoothly interpolated lattice noise from 0 to 1.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (left, top) = (x.floor(), y.floor());
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (fx, fy) = (smooth(x - left), smooth(y - top));
    let (column, row) = (left as i32, top as i32);
    mix(
        mix(hash(column, row, seed), hash(column + 1, row, seed), fx),
        mix(
            hash(column, row + 1, seed),
            hash(column + 1, row + 1, seed),
            fx,
        ),
        fy,
    )
}

fn segment_distance(point: [f32; 2], from: [f32; 2], to: [f32; 2]) -> f32 {
    let along = [to[0] - from[0], to[1] - from[1]];
    let offset = [point[0] - from[0], point[1] - from[1]];
    let length = along[0] * along[0] + along[1] * along[1];
    let t = if length > 0.0 {
        saturate((offset[0] * along[0] + offset[1] * along[1]) / length)
    } else {
        0.0
    };
    let nearest = [from[0] + along[0] * t, from[1] + along[1] * t];
    ((point[0] - nearest[0]).powi(2) + (point[1] - nearest[1]).powi(2)).sqrt()
}

/// A texture box-filtered down to about one texel per icon pixel, so a finely repeated detail
/// shows its average rather than noise. Color channels are filtered in linear light.
struct Level {
    width: usize,
    height: usize,
    texels: Vec<[f32; 4]>,
}

impl Level {
    fn new(texture: &IconTexture, scale: f32, size: u32, color: bool) -> Self {
        let footprint = texture.width.max(texture.height) as f32 * scale.abs() / size as f32;
        let mut factor = 1;
        while (factor * 2) as f32 <= footprint && factor * 2 <= texture.width.max(texture.height) {
            factor *= 2;
        }
        let width = texture.width.div_ceil(factor);
        let height = texture.height.div_ceil(factor);
        let decode = |value: u8, channel: usize| {
            let value = f32::from(value) / 255.0;
            if color && channel < 3 {
                srgb_decode(value)
            } else {
                value
            }
        };
        let mut texels = Vec::with_capacity(width * height);
        for row in 0..height {
            for column in 0..width {
                let mut sum = [0.0_f32; 4];
                let mut count = 0.0_f32;
                for y in row * factor..((row + 1) * factor).min(texture.height) {
                    for x in column * factor..((column + 1) * factor).min(texture.width) {
                        let at = (y * texture.width + x) * 4;
                        for (channel, total) in sum.iter_mut().enumerate() {
                            *total += decode(texture.rgba[at + channel], channel);
                        }
                        count += 1.0;
                    }
                }
                texels.push(sum.map(|total| total / count));
            }
        }
        Self {
            width,
            height,
            texels,
        }
    }

    /// Bilinear sample with the texture repeating, as detail textures do.
    fn sample(&self, uv: [f32; 2]) -> [f32; 4] {
        let x = uv[0] * self.width as f32 - 0.5;
        let y = uv[1] * self.height as f32 - 0.5;
        let (left, top) = (x.floor(), y.floor());
        let (fx, fy) = (x - left, y - top);
        let at = |column: f32, row: f32| {
            let column = (column as i64).rem_euclid(self.width as i64) as usize;
            let row = (row as i64).rem_euclid(self.height as i64) as usize;
            self.texels[row * self.width + column]
        };
        let (a, b, c, d) = (
            at(left, top),
            at(left + 1.0, top),
            at(left, top + 1.0),
            at(left + 1.0, top + 1.0),
        );
        [0, 1, 2, 3].map(|channel| {
            mix(
                mix(a[channel], b[channel], fx),
                mix(c[channel], d[channel], fx),
                fy,
            )
        })
    }
}

/// The dye shader's detail blend: below a quarter the detail darkens, above it the detail adds.
fn overlay(detail: f32, base: f32) -> f32 {
    base * saturate(detail * 4.0) + saturate(detail - 0.25)
}

/// The dye shader's smoothness remap: offset, scale, then clamped to minimum plus range.
fn remap(value: f32, [offset, scale, minimum, range]: [f32; 4]) -> f32 {
    let end = minimum + range;
    (value * scale + offset).clamp(minimum.min(end), minimum.max(end))
}

/// The ramp's linear color at `position`, from 0 at its start to 1 at its end.
fn ramp(colors: &[[u8; 3]], position: f32) -> [f32; 3] {
    let scaled = saturate(position) * (colors.len() - 1) as f32;
    let index = (scaled.floor() as usize).min(colors.len() - 1);
    let next = (index + 1).min(colors.len() - 1);
    let weight = scaled - index as f32;
    [0, 1, 2].map(|channel| {
        mix(
            f32::from(colors[index][channel]) / 255.0,
            f32::from(colors[next][channel]) / 255.0,
            weight,
        )
    })
}

fn srgb_decode(value: f32) -> f32 {
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(color: [f32; 3]) -> f32 {
    0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]
}

fn saturate(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn mix(from: f32, to: f32, weight: f32) -> f32 {
    from + (to - from) * weight
}

/// `tilted`, a normal in the frame around `base`, turned to where `base` faces. The frame's first
/// axis runs right across the image and its second down it, so for a surface facing the viewer
/// `tilted` comes back unchanged.
fn turned(base: [f32; 3], tilted: [f32; 3]) -> [f32; 3] {
    if base == FACING {
        return tilted;
    }
    let across = if base[0].abs() + base[2].abs() > 1.0e-4 {
        normalize([base[2], 0.0, -base[0]])
    } else {
        [1.0, 0.0, 0.0]
    };
    let down = [
        base[1] * across[2] - base[2] * across[1],
        base[2] * across[0] - base[0] * across[2],
        base[0] * across[1] - base[1] * across[0],
    ];
    normalize(
        [0, 1, 2]
            .map(|axis| across[axis] * tilted[0] + down[axis] * tilted[1] + base[axis] * tilted[2]),
    )
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn normalize(vector: [f32; 3]) -> [f32; 3] {
    let length = dot(vector, vector).sqrt().max(f32::EPSILON);
    vector.map(|channel| channel / length)
}
