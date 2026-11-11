//! The map's lights: the structure lamps Reach allocates at runtime, the lights its scenario places
//! on their own, and the lights its placed objects carry at their markers. Lamps used only for the
//! bake remain in the lightmap and object-light samples. Each retained lamp becomes a Destiny light
//! that lights surfaces within its range, a spot light keeping its cone and gel.
//!
//! These names and layouts come from the field definitions built into the Reach tag build's
//! executable (`halo3_tag_play`), each read with MCC U13 data.
//!
//! A scenario structure BSP row names that BSP's structure lighting info (`stli`) at `+0x20`. Its
//! generic light definitions (`0x50` byte rows at `+0x04`) hold a type at `+0x00` (0 omni, 1 spot,
//! 2 directional), a color at `+0x08`, an intensity at `+0x14`, the hotspot size and cutoff at
//! `+0x18` and `+0x1C`, the hotspot falloff speed at `+0x20` and far attenuation bounds at `+0x2C`,
//! in world units. Its generic light instances (`0x84` byte rows at `+0x10`) hold a definition index
//! at `+0x00`, an origin at `+0x08`, forward and up at `+0x14` and `+0x20` and a gel bitmap at
//! `+0x64`. Directional instances are the sun, which the map's sun already takes. The hotspot and
//! cutoff are full cone angles in radians: they reach 2.79 (160 degrees), which a half angle cannot,
//! their values are whole degrees (60, 90, 120, 140, 160), and the definition's rectangle or circle
//! shape and aspect are 3ds Max's light cone parameters, whose hotspot and falloff are full angles.
//!
//! A light tag (`ligh`, `0xCC` bytes) holds its flags at `+0x02` (bit 2 renders it in first person
//! only), a type at `+0x04` (0 sphere, 1 frustum), its attenuation end and start at `+0x08` and
//! `+0x0C` in world units, its frustum field of view, the full horizontal angle, at `+0x1C`, its
//! angular hotspot, the inner falloff angle, at `+0x20` and its angular falloff speed at `+0x24`, its
//! color and intensity functions at `+0x38` and `+0x5C`, its gel bitmap at `+0x80` and the time after
//! which Reach destroys it at `+0x94`. Each function holds its input variable at `+0x00`, empty for a
//! constant, and its mapping's data at `+0x10`. A mapping's first byte is its curve kind (1 a
//! constant), its third the number of colors, zero for a scalar. A scalar keeps its bounds at `+0x04`
//! and `+0x08`, one color its packed color at `+0x04`, and more colors their first at `+0x04` and last
//! at `+0x10`, as `effect::property` decodes them. A constant takes its value. Any other curve,
//! most of them driven by an object function, takes the middle of its bounds or colors, as its
//! value at runtime is not known.
//!
//! The scenario's light volumes (`0x90` byte rows at `+0x210`, palette `+0x21C` of `ligh` tags) place
//! light tags as objects: position, rotation and scale as other placements, a parent object at
//! `+0x4A`, and at `+0x62` flags whose bit 0 gives the row a custom shape of its own. Placed crates,
//! scenery, controls, machines and effect scenery carry a light where their object's attachments
//! (`0x20` byte rows at object `+0x104`) name a light tag: at `+0x10` the marker it stands at, at
//! `+0x14` the change color that tints it, at `+0x15` flags whose bit 0 keeps it always on, and at
//! `+0x18` the object function whose value scales it. An attachment with such a function and without
//! that flag exists only while the function is active, which a static map cannot tell, so it stays
//! out. The light stands at the marker's place and faces along its x axis with z up, as the object's
//! default pose carries the marker's bone. With an empty marker it stands at the object's origin.
//! Lights on machines that move stand where the machine is placed.
//!
//! Reach shades a dynamic light (`rasterizer\shaders\light_apply`) as
//! `albedo * light_colour * saturate(N.L + 0.06) / pi` times `saturate((end - distance) /
//! (end - start))^2` and its cone, and the Xbox tag build's `render_screenspace_light` sets
//! `light_colour` to the light's color times the render exposure over `2^g_HDR_target_stops`,
//! the factor every surface's `g_exposure.x` holds. Its lights therefore share the unit of its
//! lightmapped surfaces. Destiny's light programs shade by the same diffuse form and falloff, and
//! light tags and structure lamps go through the same setter, so both take one scale. A
//! lightmapped surface emits its Reach radiance over the lightmap's average lit level times the
//! level's radiance and `native::material::REFERENCE_ILLUMINATION`, the share of native light
//! its finish holds. A light's Destiny color is its source color, clamped and raised to 2.2 as
//! Reach's analytical lights decode it, times its intensity and that same factor, and its range
//! spans its far attenuation bounds (Sword Base's definitions all hold flags `0xA`, linear far
//! attenuation without inverse-square falloff). That the converted surfaces present their true
//! albedo to native lights, and so match Reach's brightness, needs an in-game check.
//!
//! A spot keeps its cone as Destiny's projected lights shade it: full light within half its hotspot,
//! none beyond half its cutoff, `saturate((cos - cos outer) / (cos inner - cos outer))` raised to its
//! falloff speed between them. A cone wider than `WIDEST` lights all around it, a beam without
//! spread stays out, and a rectangle's aspect and a light tag's frustum near width and height scale
//! have no Destiny counterpart. A gel becomes the light's projected texture.
use crate::{
    halo_reach::{
        cache::{Cache, Tag},
        level::{objects, variant::Variant},
        material, model, native,
        resource::{Absent, Pages},
        rig::{self, METRES_PER_UNIT},
    },
    presentation::Graph,
    tiger::reader::Reader,
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const SCENARIO_BSPS: (usize, usize) = (0x50, 0xAC);
const BSP_LIGHTING: usize = 0x20;
const DEFINITIONS: (usize, usize) = (0x04, 0x50);
const INSTANCES: (usize, usize) = (0x10, 0x84);
const DEFINITION_TYPE: usize = 0x00;
const DEFINITION_FLAGS: usize = 0x02;
const DEFINITION_SHAPE: usize = 0x04;
const DEFINITION_COLOR: usize = 0x08;
const DEFINITION_INTENSITY: usize = 0x14;
const DEFINITION_HOTSPOT: usize = 0x18;
const DEFINITION_CUTOFF: usize = 0x1C;
const DEFINITION_FALLOFF: usize = 0x20;
const DEFINITION_FAR: usize = 0x2C;
const INSTANCE_DEFINITION: usize = 0x00;
const INSTANCE_ORIGIN: usize = 0x08;
const INSTANCE_FORWARD: usize = 0x14;
const INSTANCE_UP: usize = 0x20;
const INSTANCE_LIGHT_TYPE: usize = 0x2C;
const INSTANCE_FLAGS: usize = 0x2E;
/// `lights_add_max_light_in_lighting_info` and `lighting_info_light_place` in Reach's 2011-07-07
/// tag build skip light type zero unless either flag 2 or 3 is set. The same fields occupy +0x2C
/// and +0x2E in MCC U13. Keep the exceptions without assigning unverified names to their bits.
const LIGHTMAP_ONLY: u16 = 0;
const RUNTIME_LIGHT_FLAGS: u16 = (1 << 2) | (1 << 3);
const INSTANCE_GEL: usize = 0x64;
const DIRECTIONAL: i16 = 2;
const SPOT: i16 = 1;
const RECTANGLE: i16 = 0;
/// A light tag's fields.
const LIGHT_FLAGS: usize = 0x02;
const FIRST_PERSON_ONLY: u16 = 1 << 2;
const LIGHT_TYPE: usize = 0x04;
const FRUSTUM: u16 = 1;
const LIGHT_END: usize = 0x08;
const LIGHT_START: usize = 0x0C;
const LIGHT_FIELD_OF_VIEW: usize = 0x1C;
const LIGHT_HOTSPOT: usize = 0x20;
const LIGHT_FALLOFF: usize = 0x24;
const LIGHT_COLOR: usize = 0x38;
const LIGHT_INTENSITY: usize = 0x5C;
const FUNCTION_MAPPING: usize = 0x10;
const LIGHT_GEL: usize = 0x80;
const LIGHT_LIFETIME: usize = 0x94;
/// The scenario's light volumes and their palette.
const VOLUMES: (usize, usize) = (0x210, 0x90);
const VOLUME_PALETTE: (usize, usize) = (0x21C, 0x10);
const VOLUME_PARENT: usize = 0x4A;
const VOLUME_FLAGS: usize = 0x62;
const CUSTOM_GEOMETRY: u16 = 1;
/// An object's attachments, each naming a tag, its marker, change color, flags and scale function.
const ATTACHMENTS: (usize, usize) = (0x104, 0x20);
const ATTACHMENT_MARKER: usize = 0x10;
const ATTACHMENT_CHANGE_COLOR: usize = 0x14;
const ATTACHMENT_FLAGS: usize = 0x15;
const ALWAYS_ON: u8 = 1;
const ATTACHMENT_SCALE: usize = 0x18;
/// Placed effect scenery, whose rows share the other placements' leading fields.
const EFFECT_SCENERY: objects::Block = objects::Block {
    rows: 0x1F8,
    size: 0xB0,
    palette: 0x204,
    group: "efsc",
};
/// The widest cone half angle a spot keeps. Inspected stock projected lights reach 60 degrees, and
/// the volume's slope, the tangent of the half angle, grows without bound toward 90.
const WIDEST: f32 = 80. * std::f32::consts::PI / 180.;
/// The narrowest half angle a spot keeps, below which it is a beam a cone cannot hold.
const NARROWEST: f32 = 0.25 * std::f32::consts::PI / 180.;

/// A spot light's cone: its forward and up directions in the map's coordinates, the half angles
/// in radians within which it shines fully and beyond which it is dark, the power of its falloff
/// between them, and its projected texture's symbol in the map's graph.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Cone {
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub inner: f32,
    pub outer: f32,
    pub exponent: f32,
    pub gel: Option<String>,
}

/// One of the level's lights: its position in the map's coordinates, where its falloff starts and
/// its range in meters, its color times its intensity in Destiny's units and, for a spot, its cone.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MapLight {
    pub position: [f32; 3],
    #[serde(default)]
    pub start: f32,
    pub range: f32,
    pub color: [f32; 3],
    #[serde(default)]
    pub cone: Option<Cone>,
}

/// Each object and model variant's model with the default pose of its bones, read once.
type Models = BTreeMap<(String, String), Option<(model::Model, Vec<[f32; 16]>)>>;

/// What the reader counts and the conversion state it shares across the level's light sources.
struct Lights<'a> {
    cache: &'a Cache,
    pages: &'a mut Pages,
    native: &'a mut Reader,
    graph: &'a mut Graph,
    offset: [f32; 3],
    scale: f32,
    lights: Vec<MapLight>,
    kinds: Vec<Value>,
    /// Each gel bitmap's texture symbol, or none when it could not be converted.
    gels: BTreeMap<String, Option<String>>,
    counts: BTreeMap<String, usize>,
}

/// A light tag's color and intensity function at `at`: its mapping's middle value, and whether an
/// input drives it. A scalar takes the middle of its bounds unless it is a constant, a color its
/// packed colors averaged in bytes over 255.
fn function(cache: &Cache, at: usize, color: bool) -> Result<([f32; 3], bool)> {
    let data = at + FUNCTION_MAPPING;
    let size = usize::try_from(cache.i32(data)?).context("Light function size")?;
    anyhow::ensure!(
        (32..=4096).contains(&size),
        "Unsupported light function size"
    );
    let mapping = cache.meta(cache.expand(cache.u32(data + 12)?)?, size)?;
    let word = |k: usize| u32::from_le_bytes(mapping[k..k + 4].try_into().unwrap_or([0; 4]));
    let driven = cache.u32(at)? != 0;
    let constant = mapping[0] == 1 && mapping[1] & 1 == 0;
    let colors = mapping[2];
    anyhow::ensure!(colors == 0 || color, "A light intensity holds colors");
    let value = if colors == 0 {
        let (lower, upper) = (f32::from_bits(word(4)), f32::from_bits(word(8)));
        [if constant {
            lower
        } else {
            (lower + upper) * 0.5
        }; 3]
    } else {
        let rgb = |packed: u32| [16, 8, 0].map(|shift| f32::from((packed >> shift) as u8));
        let packed: Vec<[f32; 3]> = if constant || colors == 1 {
            vec![rgb(word(4))]
        } else {
            let mut list = (0..usize::from(colors) - 1)
                .map(|i| rgb(word(4 + i * 4)))
                .collect::<Vec<_>>();
            list.push(rgb(word(16)));
            list
        };
        let count = packed.len() as f32;
        std::array::from_fn(|k| packed.iter().map(|c| c[k]).sum::<f32>() / count / 255.)
    };
    anyhow::ensure!(
        value.iter().all(|v| v.is_finite()),
        "Nonfinite light function"
    );
    Ok((value, driven && !constant))
}

/// Light tag `tag`'s linear color, its color function's value clamped and raised to 2.2 as Reach's
/// analytical lights decode it, and the seconds after which Reach destroys it, zero for none.
pub(in crate::halo_reach) fn color(cache: &Cache, tag: &Tag) -> Result<([f32; 3], f32)> {
    let at = tag.address()?;
    let (color, _) = function(cache, at + LIGHT_COLOR, true)?;
    let lifetime = cache.f32(at + LIGHT_LIFETIME)?;
    anyhow::ensure!(
        lifetime.is_finite() && lifetime >= 0.,
        "Invalid light lifetime"
    );
    Ok((color.map(|v| v.clamp(0., 1.).powf(2.2)), lifetime))
}

/// Cheap light tag (`gldf`) `tag`'s linear color, decoded as [`color`] decodes a light's, and its
/// radius in world units. The tag holds color, intensity and radius functions of 0x24 bytes at
/// `+0x0`, `+0x24` and `+0x48`, laid out as a light's, then its falloff and close-up fading
/// distance. A radius an input drives takes the middle of its bounds.
pub(in crate::halo_reach) fn cheap(cache: &Cache, tag: &Tag) -> Result<([f32; 3], f32)> {
    let at = tag.address()?;
    let (color, _) = function(cache, at, true)?;
    let ([radius, ..], _) = function(cache, at + 0x48, false)?;
    anyhow::ensure!(
        radius.is_finite() && radius > 0.,
        "Invalid cheap light radius"
    );
    Ok((color.map(|v| v.clamp(0., 1.).powf(2.2)), radius))
}

/// Unit `forward` and an `up` made perpendicular to it.
fn basis(forward: [f32; 3], up: [f32; 3]) -> Option<([f32; 3], [f32; 3])> {
    let forward = rig::normalize(forward).ok()?;
    let along = (0..3).map(|k| up[k] * forward[k]).sum::<f32>();
    let up = rig::normalize(std::array::from_fn(|k| up[k] - forward[k] * along))
        .or_else(|_| {
            let side = if forward[2].abs() < 0.9 {
                [0., 0., 1.]
            } else {
                [1., 0., 0.]
            };
            let along = (0..3).map(|k| side[k] * forward[k]).sum::<f32>();
            rig::normalize(std::array::from_fn(|k| side[k] - forward[k] * along))
        })
        .ok()?;
    Some((forward, up))
}

impl Lights<'_> {
    fn count(&mut self, key: &str) {
        *self.counts.entry(key.to_owned()).or_default() += 1;
    }

    /// The texture symbol of gel bitmap `tag`, converted once, or none when its pixels are absent.
    fn gel(&mut self, tag: &Tag) -> Result<Option<String>> {
        if let Some(symbol) = self.gels.get(&tag.path) {
            return Ok(symbol.clone());
        }
        let mapping = material::Mapping {
            role: "gel".into(),
            bitmap: Some(tag.clone()),
            transform: [1., 1., 0., 0.],
            frame: 0,
            address: 0,
            filter: 0,
            functions: Vec::new(),
        };
        let symbol = match native::material::texture(
            self.cache,
            self.pages,
            self.native,
            self.graph,
            &mapping,
            false,
            true,
        ) {
            Ok(symbol) => Some(symbol),
            Err(error) if error.downcast_ref::<Absent>().is_some() => None,
            Err(error) => return Err(error.context(format!("Gel {}", tag.path))),
        };
        self.gels.insert(tag.path.clone(), symbol.clone());
        Ok(symbol)
    }

    /// Adds a light at `origin` in meters, from `start` to `end`
    /// in world units, with linear `color` and `intensity`, and for a spot its directions, full
    /// cone angles, falloff speed and gel. `kind` describes it in the report.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        origin: [f32; 3],
        [start, end]: [f32; 2],
        color: [f32; 3],
        intensity: f32,
        spot: Option<Spotlight>,
        kind: Value,
    ) -> Result<()> {
        let color = color.map(|v| v.clamp(0., 1.).powf(2.2) * intensity * self.scale);
        let range = end * METRES_PER_UNIT;
        let start = (start * METRES_PER_UNIT).clamp(0., range * 0.99);
        let position = std::array::from_fn(|k| origin[k] + self.offset[k]);
        let finite = color.iter().chain(&position).all(|v| v.is_finite()) && range.is_finite();
        if !finite || range <= 0. || color.iter().all(|&c| c <= 0.) {
            self.count("unlit_skipped");
            return Ok(());
        }
        let mut cone = None;
        if let Some((forward, up, [hotspot, cutoff], speed, gel)) = spot {
            let outer = cutoff * 0.5;
            if outer.is_nan() || outer <= NARROWEST {
                self.count("beams_skipped");
                return Ok(());
            }
            match basis(forward, up) {
                Some((forward, up)) if outer <= WIDEST => {
                    let gel = match gel {
                        Some(tag) => self.gel(&tag)?,
                        None => None,
                    };
                    self.count(if gel.is_some() {
                        "spots_with_gels"
                    } else {
                        "spots"
                    });
                    cone = Some(Cone {
                        forward,
                        up,
                        inner: (hotspot * 0.5).clamp(0., outer * 0.999),
                        outer,
                        exponent: if speed > 0. { speed } else { 1. },
                        gel,
                    });
                }
                Some(_) => self.count("wide_spots_as_points"),
                None => self.count("spots_without_direction_as_points"),
            }
        }
        self.lights.push(MapLight {
            position,
            start,
            range,
            color,
            cone,
        });
        self.kinds.push(kind);
        Ok(())
    }

    /// The lamps of the structure BSPs in `loaded`.
    fn structure(&mut self, loaded: u32) -> Result<()> {
        let cache = self.cache;
        let scenario = cache.only("scnr")?.address()?;
        for (bsp, row) in cache
            .block(scenario + SCENARIO_BSPS.0, SCENARIO_BSPS.1)?
            .into_iter()
            .enumerate()
        {
            if bsp >= 32 || (loaded >> bsp) & 1 == 0 {
                continue;
            }
            let Some(tag) = cache.reference(row + BSP_LIGHTING, Some("stli"))? else {
                continue;
            };
            let at = tag.address()?;
            let definitions = cache.block(at + DEFINITIONS.0, DEFINITIONS.1)?;
            for instance in cache.block(at + INSTANCES.0, INSTANCES.1)? {
                let light_type = cache.u16(instance + INSTANCE_LIGHT_TYPE)?;
                let instance_flags = cache.u16(instance + INSTANCE_FLAGS)?;
                if light_type == LIGHTMAP_ONLY && instance_flags & RUNTIME_LIGHT_FLAGS == 0 {
                    self.count("baked_lights_skipped");
                    continue;
                }
                let index = usize::try_from(cache.i32(instance + INSTANCE_DEFINITION)?)?;
                let definition = *definitions
                    .get(index)
                    .context("Light instance definition outside its stli")?;
                let kind = cache.i16(definition + DEFINITION_TYPE)?;
                if kind == DIRECTIONAL {
                    self.count("directional_skipped");
                    continue;
                }
                let spot = if kind == SPOT {
                    if cache.i16(definition + DEFINITION_SHAPE)? == RECTANGLE {
                        self.count("rectangular_spots_as_circles");
                    }
                    Some((
                        cache.floats::<3>(instance + INSTANCE_FORWARD)?,
                        cache.floats::<3>(instance + INSTANCE_UP)?,
                        [
                            cache.f32(definition + DEFINITION_HOTSPOT)?,
                            cache.f32(definition + DEFINITION_CUTOFF)?,
                        ],
                        cache.f32(definition + DEFINITION_FALLOFF)?,
                        cache.reference(instance + INSTANCE_GEL, Some("bitm"))?,
                    ))
                } else {
                    None
                };
                let origin = cache
                    .floats::<3>(instance + INSTANCE_ORIGIN)?
                    .map(|v| v * METRES_PER_UNIT);
                self.add(
                    origin,
                    cache.floats::<2>(definition + DEFINITION_FAR)?,
                    cache.floats::<3>(definition + DEFINITION_COLOR)?,
                    cache.f32(definition + DEFINITION_INTENSITY)?,
                    spot,
                    json!({
                        "source": "structure",
                        "flags": cache.u16(definition + DEFINITION_FLAGS)?,
                        "light_type": light_type,
                        "instance_flags": instance_flags,
                    }),
                )?;
            }
        }
        Ok(())
    }

    /// Adds light tag `tag` placed by `transform`, a column-major transform in meters whose x axis
    /// faces forward and z axis up, tinted by `tint`.
    fn tag(
        &mut self,
        tag: &Tag,
        transform: &[f32; 16],
        tint: [f32; 3],
        source: &str,
    ) -> Result<()> {
        let cache = self.cache;
        let at = tag.address()?;
        if cache.u16(at + LIGHT_FLAGS)? & FIRST_PERSON_ONLY != 0 {
            self.count("first_person_lights_skipped");
            return Ok(());
        }
        if cache.f32(at + LIGHT_LIFETIME)? > 0. {
            self.count("transient_lights_skipped");
            return Ok(());
        }
        let (color, driven_color) = function(cache, at + LIGHT_COLOR, true)?;
        let (intensity, driven_intensity) = function(cache, at + LIGHT_INTENSITY, false)?;
        if driven_color || driven_intensity {
            self.count("function_driven_lights_at_middle");
        }
        let spot = if cache.u16(at + LIGHT_TYPE)? == FRUSTUM {
            Some((
                rig::point(transform, [1., 0., 0.], false),
                rig::point(transform, [0., 0., 1.], false),
                [
                    cache.f32(at + LIGHT_HOTSPOT)?,
                    cache.f32(at + LIGHT_FIELD_OF_VIEW)?,
                ],
                cache.f32(at + LIGHT_FALLOFF)?,
                cache.reference(at + LIGHT_GEL, Some("bitm"))?,
            ))
        } else {
            None
        };
        self.add(
            rig::point(transform, [0.; 3], true),
            [cache.f32(at + LIGHT_START)?, cache.f32(at + LIGHT_END)?],
            std::array::from_fn(|k| color[k] * tint[k]),
            intensity[0],
            spot,
            json!({"source": source, "tag": tag.path}),
        )
    }

    /// The light volumes the scenario places in the structure BSPs in `loaded`.
    fn volumes(&mut self, loaded: u32) -> Result<()> {
        let cache = self.cache;
        let scenario = cache.only("scnr")?.address()?;
        let palette = cache
            .block(scenario + VOLUME_PALETTE.0, VOLUME_PALETTE.1)?
            .into_iter()
            .map(|row| cache.reference(row, Some("ligh")))
            .collect::<Result<Vec<_>>>()?;
        for row in cache.block(scenario + VOLUMES.0, VOLUMES.1)? {
            let Ok(index) = usize::try_from(cache.i16(row)?) else {
                continue;
            };
            if !objects::placed_in(cache, row, loaded)? {
                continue;
            }
            let Some(tag) = palette
                .get(index)
                .context("A light volume palette index lies outside its palette")?
                .clone()
            else {
                continue;
            };
            if cache.i16(row + VOLUME_PARENT)? >= 0 {
                self.count("attached_volumes_skipped");
                continue;
            }
            if cache.u16(row + VOLUME_FLAGS)? & CUSTOM_GEOMETRY != 0 {
                self.count("custom_shape_volumes_skipped");
                continue;
            }
            let transform = objects::placement_transform(cache, row)?;
            self.tag(&tag, &transform, [1.; 3], "volume")?;
        }
        Ok(())
    }

    /// The lights the objects placed in the structure BSPs in `loaded` carry, or with a map
    /// `variant`, the variant's objects in place of the scenario's placements of every object its
    /// palette offers.
    fn attached(&mut self, loaded: u32, variant: Option<&Variant>) -> Result<()> {
        let cache = self.cache;
        let scenario = cache.only("scnr")?.address()?;
        let mut models = BTreeMap::new();
        for block in [
            &objects::CRATES,
            &objects::SCENERY,
            &objects::CONTROLS,
            &objects::MACHINES,
            &EFFECT_SCENERY,
        ] {
            let palette = cache
                .block(scenario + block.palette, objects::PALETTE_SIZE)?
                .into_iter()
                .map(|row| cache.reference(row, Some(block.group)))
                .collect::<Result<Vec<_>>>()?;
            for row in cache.block(scenario + block.rows, block.size)? {
                let Ok(index) = usize::try_from(cache.i16(row)?) else {
                    continue;
                };
                if !objects::placed_in(cache, row, loaded)? {
                    continue;
                }
                let Some(tag) = palette.get(index).with_context(|| {
                    format!("A {} palette index lies outside its palette", block.group)
                })?
                else {
                    continue;
                };
                if tag.path.starts_with(objects::MARKERS)
                    || variant.is_some_and(|variant| variant.replaces(tag))
                {
                    continue;
                }
                // Effect scenery rows end before the permutation data that names a variant.
                let name = if block.group == EFFECT_SCENERY.group {
                    String::new()
                } else {
                    objects::variant_name(cache, row)?
                };
                let transform = objects::placement_transform(cache, row)?;
                self.object(tag, Some(row), &name, &transform, &mut models)?;
            }
        }
        for placement in variant.map_or(&[][..], |variant| variant.placements.as_slice()) {
            let tag = &placement.tag;
            if !objects::VARIANT_GROUPS.contains(&tag.group.as_str())
                || tag.path.starts_with(objects::MARKERS)
            {
                continue;
            }
            let transform = objects::transform(placement.position, placement.axes, 1.);
            self.object(tag, None, &placement.variant, &transform, &mut models)?;
        }
        Ok(())
    }

    /// The lights object `tag` with model variant `name` carries where `transform` places it, its
    /// change colors set by scenario row `row` when one places it.
    fn object(
        &mut self,
        tag: &Tag,
        row: Option<usize>,
        name: &str,
        transform: &[f32; 16],
        models: &mut Models,
    ) -> Result<()> {
        let cache = self.cache;
        let mut attached = Vec::new();
        for attachment in cache.block(tag.address()? + ATTACHMENTS.0, ATTACHMENTS.1)? {
            if let Some(light) = cache.reference(attachment, None)?
                && light.group == "ligh"
            {
                attached.push((attachment, light));
            }
        }
        if attached.is_empty() {
            return Ok(());
        }
        let key = (tag.path.clone(), name.to_owned());
        if !models.contains_key(&key) {
            let model = match objects::model_of(cache, self.pages, tag, name, None) {
                Ok(Some(model)) => {
                    let worlds = rig::world(&model.bones)?;
                    Some((model, worlds))
                }
                Ok(None) => None,
                Err(error) if error.downcast_ref::<Absent>().is_some() => None,
                Err(error) => return Err(error),
            };
            models.insert(key.clone(), model);
        }
        let colors = objects::change_colors(cache, tag, row, name)?;
        for (attachment, light) in attached {
            let flags = cache.u8(attachment + ATTACHMENT_FLAGS)?;
            if flags & ALWAYS_ON == 0 && cache.u32(attachment + ATTACHMENT_SCALE)? != 0 {
                let function = cache.string_id(cache.u32(attachment + ATTACHMENT_SCALE)?)?;
                self.count(&format!("gated_by_function/{function}"));
                continue;
            }
            let tint = match cache.u8(attachment + ATTACHMENT_CHANGE_COLOR)? {
                0 => [1.; 3],
                index => colors
                    .get(usize::from(index) - 1)
                    .copied()
                    .flatten()
                    .unwrap_or([1.; 3]),
            };
            let marker = cache.string_id(cache.u32(attachment + ATTACHMENT_MARKER)?)?;
            if marker.is_empty() {
                self.tag(&light, transform, tint, "object")?;
                continue;
            }
            let Some((model, worlds)) = &models[&key] else {
                self.count("lights_on_objects_without_models");
                continue;
            };
            let mut places = Vec::new();
            for found in model.markers.iter().filter(|m| m.name == marker) {
                if !model.marker_selected(found)? {
                    continue;
                }
                let bone = found
                    .bone
                    .and_then(|bone| worlds.get(bone))
                    .copied()
                    .unwrap_or(rig::IDENTITY);
                let local = rig::transform(found.translation, found.rotation);
                places.push(rig::multiply(transform, &rig::multiply(&bone, &local)));
            }
            if places.is_empty() {
                self.count("lights_at_missing_markers");
            }
            for place in places {
                self.tag(&light, &place, tint, "object")?;
            }
        }
        Ok(())
    }
}

/// The level's lights in the structure BSPs in `loaded`, a mask by scenario index, with a map
/// `variant`'s objects, moved by `offset` meters, at `radiance` over a lightmap whose average lit
/// level is `level`, with their gels added to `graph`, and a report.
#[allow(clippy::too_many_arguments)]
pub(super) fn read(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    loaded: u32,
    variant: Option<&Variant>,
    offset: [f32; 3],
    radiance: f32,
    level: f32,
) -> Result<(Vec<MapLight>, Value)> {
    let scale = radiance * native::material::REFERENCE_ILLUMINATION / level;
    let mut lights = Lights {
        cache,
        pages,
        native,
        graph,
        offset,
        scale,
        lights: Vec::new(),
        kinds: Vec::new(),
        gels: BTreeMap::new(),
        counts: BTreeMap::new(),
    };
    lights.structure(loaded)?;
    let structure = lights.lights.len();
    lights.volumes(loaded)?;
    let volumes = lights.lights.len() - structure;
    lights.attached(loaded, variant)?;
    let attached = lights.lights.len() - structure - volumes;
    let report = json!({
        "lights": lights.lights.len(),
        "structure": structure,
        "volumes": volumes,
        "attached": attached,
        "counts": lights.counts,
        "gels": lights.gels,
        "scale": scale,
        "ranges": lights.lights.iter().map(|light| [light.start, light.range]).collect::<Vec<_>>(),
        "colors": lights.lights.iter().map(|light| light.color).collect::<Vec<_>>(),
        "cones": lights.lights.iter().map(|light| light.cone.as_ref().map(|cone| {
            json!([cone.inner, cone.outer, cone.exponent, cone.gel])
        })).collect::<Vec<_>>(),
        "kinds": lights.kinds,
    });
    Ok((lights.lights, report))
}

type Spotlight = ([f32; 3], [f32; 3], [f32; 2], f32, Option<Tag>);
