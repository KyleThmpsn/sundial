//! A level's sky. Reach draws the objects its scenario's skies block names in a pass of their own,
//! around the level and never stood on. A Shadowkeep map draws its sky from a sky object
//! collection: entity models placed by their own transforms, drawn in the transparent stage and
//! the light-shaft stage, unlit, in their collection's order. `add` turns the level's sky into
//! such models, each one a sky object, keeping its surfaces in the order Reach draws them so
//! blended layers keep their order. [`survey`] reports a level's skies with the sky, fog and
//! lighting tags its scenario and structure BSPs name.
use super::{MESH_CLASS, MODEL_CLASS, MODEL_EXTENTS, MODEL_RADIUS, bounds, sub, unit};
use crate::{
    halo_reach::{
        cache::{Cache, Tag},
        choices,
        material::{self as source_material, Material},
        model::{self, Primitive},
        native::material::{
            Fog, SKY_RADIANCE,
            sky::{
                SkyParallax, SkyScale, SkySurface,
                lens::{self, Lens, Reflection},
                patchy::{self, Patchy},
                surfaces, weather,
            },
        },
        object::Object as SourceObject,
        resource::Pages,
        rig,
    },
    presentation::{
        Graph,
        geometry::{self, Mesh},
        put,
    },
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

/// Native tags a map's sky copies from the donor's sky: the model reference (`80806F9B`) of its
/// first object, the entity model that reference names, whose first mesh carries each sky model's
/// geometry, and that model's transparent and light-shaft materials, whose envelopes the sky
/// surfaces copy.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Templates {
    pub reference: u32,
    pub model: u32,
    pub surface: u32,
    pub shafts: u32,
}

/// One sky object: its model reference by graph symbol, where it sits, the center of its bounds,
/// its world bounds in meters and the order it draws in among the map's sky objects.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Object {
    pub reference: String,
    pub position: [f32; 3],
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
    pub order: f32,
}

/// Vertices one sky model holds. Its indices are 16 bits wide, with the largest value reserved to
/// restart a strip.
const MAX_VERTICES: usize = 60_000;
/// The key bits of a sky surface's transparent and light-shaft materials, whose symbols
/// `material-{key:08X}` stay apart from the level's surfaces, keyed by material index alone.
const TRANSPARENT_KEY: u32 = 0x4000_0000;
const SHAFTS_KEY: u32 = 0x6000_0000;
/// The material index a sky surface without a shader takes in its keys.
const NO_MATERIAL: u32 = 0xFFFF;
/// The transparent stage and the light-shaft stage.
const TRANSPARENT_STAGE: u8 = 7;
const LIGHT_SHAFT_STAGE: u8 = 9;
/// A draw record's flags at `+0x18`, which every stock sky object's draws set to `0x10`.
const DRAW_FLAGS: usize = 0x18;
const SKY_DRAW_FLAGS: u16 = 0x10;
/// A model's draw records follow its mesh row.
pub(super) const DRAW_RECORDS: usize = 0x150;
pub(super) const DRAW_RECORD_SIZE: usize = 32;
/// The size of a sky model reference: its size, then its model.
const REFERENCE_SIZE: usize = 0x10;
const REFERENCE_MODEL: usize = 0x08;
/// The stride of a sky model's position and attribute streams, layout 139.
const POSITION_STRIDE: u16 = 8;
const ATTRIBUTE_STRIDE: u16 = 24;
/// The scenario's default camera effects. Their flags at `+0x00` mark automatic exposure with bit
/// 2, the target exposure in stops is at `+0x04`, the range automatic exposure keeps to in stops
/// at `+0x10`, and the screen brightness it aims for, in stops, at `+0x18`.
const DEFAULT_CAMERA_FX: usize = 0x6A8;
const CAMERA_FX_FLAGS: usize = 0x00;
const CAMERA_FX_EXPOSURE: usize = 0x04;
const CAMERA_FX_RANGE: usize = 0x10;
const CAMERA_FX_BRIGHTNESS: usize = 0x18;
const AUTO_EXPOSURE: u16 = 1 << 2;
/// The camera effects' inherent bloom (bloom off everything, bright and dark), bloom intensity and
/// large bloom color, each after a 16-bit flags word whose bit 0 takes the default. Hang 'Em High
/// sets 0.3, 0.8 and (0.809, 0.597, 0.254), so Reach lays a large amber bloom over its whole image.
/// Sword Base's large bloom color is near black.
const CAMERA_FX_INHERENT_BLOOM: usize = 0x38;
const CAMERA_FX_BLOOM_INTENSITY: usize = 0x48;
const CAMERA_FX_LARGE_BLOOM: usize = 0x58;
const CAMERA_FX_USE_DEFAULT: u16 = 1;
/// The average of the image the large bloom blurs, as a share of the level's average lit level: a
/// mid gray surface's albedo. The large bloom spreads across much of the screen, so the
/// translation takes it as one color over the whole image. Its medium and small blooms, white in
/// Hang 'Em High, brighten the image without tinting it, as the surfaces' radiance scale does.
const SCENE_AVERAGE: f32 = 0.18;
/// Reach's screen brightness, in stops, where a scenario names no camera effects.
const DEFAULT_BRIGHTNESS: f32 = -3.3;

/// One sky model's share of the sky: the primitives' triangles it draws, in order.
struct Chunk {
    /// Each run of triangles from one primitive, as that primitive and its triangles' corners.
    runs: Vec<(usize, Vec<[u32; 3]>)>,
    vertices: usize,
}

/// Splits the sky's triangles, in order, into chunks of at most `MAX_VERTICES` vertices.
fn chunks(primitives: &[(Primitive, Vec<[f32; 3]>)]) -> Result<Vec<Chunk>> {
    let mut out = Vec::new();
    let mut current = Chunk {
        runs: Vec::new(),
        vertices: 0,
    };
    let mut seen = std::collections::HashSet::<(usize, u32)>::new();
    for (index, (primitive, _)) in primitives.iter().enumerate() {
        ensure!(
            primitive.indices.len() % 3 == 0,
            "Sky primitive {index} is not a triangle list"
        );
        for face in primitive.indices.chunks_exact(3) {
            let face = [face[0], face[1], face[2]];
            let fresh = face
                .iter()
                .filter(|&&corner| !seen.contains(&(index, corner)))
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            if current.vertices + fresh > MAX_VERTICES {
                out.push(std::mem::replace(
                    &mut current,
                    Chunk {
                        runs: Vec::new(),
                        vertices: 0,
                    },
                ));
                seen.clear();
            }
            for corner in face {
                if seen.insert((index, corner)) {
                    current.vertices += 1;
                }
            }
            match current.runs.last_mut() {
                Some((primitive, faces)) if *primitive == index => faces.push(face),
                _ => current.runs.push((index, vec![face])),
            }
        }
    }
    if !current.runs.is_empty() {
        out.push(current);
    }
    Ok(out)
}

/// A render method's transparent sort layer, an `enum8` at `+0x5A`: invalid, pre-pass, normal and
/// post-pass from 0, per Assembly's MCC Reach shader definition.
const SORT_LAYER: usize = 0x5A;
const SORT_LAYER_NORMAL: u8 = 2;

/// The transparent sort layer of a sky surface's shader, by which Reach orders the sky's blended
/// parts. In the Xbox `halo3_tag_play` build, `s_render_type_default_opaque::render_object_mesh_parts`
/// (`825AC0A0`) adds an object's transparent parts between `begin_fixed_order_group` and
/// `end_fixed_order_group`, each through `add_transparent_mesh_part` (`825A9D58`) to
/// `c_transparency_renderer::add_element` with the shader's sort layer (`tag_get('rm  ')+0x5A`).
/// A part takes its own sorting position (render model `+0x80`, `0x30`-byte rows of a plane and a
/// point, indexed by the part's `int16` at `+0x02`) only when its mesh leaves flag bit 1 (`Use
/// Region Index For Sorting`) clear. Every sampled sky mesh sets mesh flags `0x0003`, so its parts
/// share the object's one point, offset only by the mesh's index, and their order comes from the
/// sort layer, then the mesh, then the part. A surface without a shader, or an invalid layer,
/// sorts as normal.
pub(super) fn sort_layer(cache: &Cache, material: Option<&Material>) -> u8 {
    material
        .and_then(|material| material.tag.as_ref())
        .and_then(|tag| tag.address().ok())
        .and_then(|at| cache.u8(at + SORT_LAYER).ok())
        .filter(|&layer| (1..=3).contains(&layer))
        .unwrap_or(SORT_LAYER_NORMAL)
}

/// An object's model (`hlmt`) reference, the model's flags and its sky parallax percent, with the
/// flag that makes a sky follow the camera ([`SkyParallax`]).
const OBJECT_MODEL: usize = 0x64;
const MODEL_FLAGS: usize = 0x15C;
const MODEL_SKY_PARALLAX: usize = 0x1B4;
const SKY_ATTACHES_TO_CAMERA: u32 = 1 << 6;

/// How the level's sky follows the camera, from the first sky row active on the `loaded` BSPs,
/// as Reach moves every sky by the first visible one's parallax, and a report. The sky object
/// stands at the level's origin, which the map moves by `offset`. In the U13 caches only The
/// Package's and Long Night of Solace's space skies (`sky_wafer`, `sky_corvette`, 0.6) and
/// `space_alpha_halo` of `m70_a` and `m70_bonus` (0, held at the camera) set the flag. Every other
/// sky of the U13 caches leaves it clear and stays where it is.
fn parallax(cache: &Cache, loaded: u32, offset: [f32; 3]) -> Result<(Option<SkyParallax>, Value)> {
    let scenario = cache.only("scnr")?.address()?;
    for row in cache.block(scenario + SKIES, SKY_SIZE)? {
        if u32::from(cache.u16(row + SKY_BSPS)?) & loaded == 0 {
            continue;
        }
        let Some(sky) = cache.reference(row, Some("scen"))? else {
            continue;
        };
        let Some(model) = cache.reference(sky.address()? + OBJECT_MODEL, Some("hlmt"))? else {
            return Ok((None, json!({"sky": sky.path, "model": null})));
        };
        let at = model.address()?;
        let flags = cache.u32(at + MODEL_FLAGS)?;
        let percent = cache.f32(at + MODEL_SKY_PARALLAX)?;
        ensure!(
            percent.is_finite(),
            "Sky model {} holds a nonfinite parallax",
            model.path
        );
        // `get_object_values` truncates the percent times 255 to a byte.
        let byte = (percent * 255.).clamp(0., 255.).trunc();
        let parallax = (flags & SKY_ATTACHES_TO_CAMERA != 0).then_some(SkyParallax {
            origin: offset,
            follow: 1. - byte / 255.,
        });
        return Ok((
            parallax,
            json!({
                "sky": sky.path,
                "model": model.path,
                "flags": format!("{flags:08X}"),
                "parallax_percent": percent,
                "follow": parallax.map(|p| p.follow),
            }),
        ));
    }
    Ok((None, json!({"sky": null})))
}

/// The key bits of a lens flare's reflections and of the patchy fog's sheets, apart from the
/// sky's own surfaces.
const FLARE_KEY: u32 = 0x5000_0000;
const PATCHY_KEY: u32 = 0x5100_0000;
/// A scenario object's attachments: `0x20`-byte rows naming a tag at `+0x00` and a marker's string
/// id at `+0x10`, as Assembly's MCC Reach object definitions lay them out.
const OBJECT_ATTACHMENTS: (usize, usize) = (0x104, 0x20);
const ATTACHMENT_MARKER: usize = 0x10;
/// A lens flare tag (`lens`, Assembly's Reach definition): the occlusion inner radius scale, the
/// bitmap, the flags (bit 1, no occlusion test) and the reflections, `0x58`-byte rows of flags, a
/// bitmap index, a rotation offset in degrees, an axis offset, radius and brightness functions, a
/// modulation factor, a color and a tint power.
const LENS_OCCLUSION_SCALE: usize = 0x10;
const LENS_BITMAP: usize = 0x24;
const LENS_FLAGS: usize = 0x34;
const LENS_NO_OCCLUSION: u16 = 1 << 1;
const LENS_REFLECTIONS: (usize, usize) = (0x44, 0x58);
const REFLECTION_BITMAP: usize = 0x02;
const REFLECTION_ROTATION: usize = 0x04;
const REFLECTION_AXIS: usize = 0x08;
const REFLECTION_RADIUS: usize = 0x1C;
const REFLECTION_BRIGHTNESS: usize = 0x30;
const REFLECTION_MODULATION: usize = 0x44;
const REFLECTION_COLOR: usize = 0x48;
const REFLECTION_TINT_POWER: usize = 0x54;
/// A bitmap's images, `0x38`-byte rows at `+0x7C`.
const BITMAP_IMAGES: (usize, usize) = (0x7C, 0x38);
/// A sampler address byte that clamps both coordinates, as Reach draws flare sprites.
const CLAMP_ADDRESS: u8 = 0x11;
/// A fog tag's flags, with patchy fog at bit 3, its patchy fog parameters (the tag build's
/// `Patchy Fog Per-Cluster Parameters`): sheet density, tint, inner tint, intensity, full and half
/// intensity heights, wind direction and reference plane height, and the atmosphere globals'
/// (`atgf`) noise bitmap, texture repeat rate, distance between sheets and depth fade factor.
const FOG_PATCHY: u16 = 1 << 3;
const PATCHY_DENSITY: usize = 0x74;
const PATCHY_TINT: usize = 0x78;
const PATCHY_INNER: usize = 0x84;
const PATCHY_INTENSITY: usize = 0x90;
const PATCHY_FULL: usize = 0x94;
const PATCHY_HALF: usize = 0x98;
const PATCHY_WIND: usize = 0x9C;
const PATCHY_REFERENCE: usize = 0xA8;
const GLOBALS_REPEAT: usize = 0x10;
const GLOBALS_SPACING: usize = 0x14;
const GLOBALS_DEPTH_FADE: usize = 0x18;
/// How far the patchy fog's sheets reach past the level's pieces, in meters.
const SHEET_MARGIN: f32 = 64.;

/// The value of the Reach function whose data reference is at `field`, at an input of one and a
/// range input of zero, as a sky attachment's lens flare evaluates its curves: the minimum for a
/// constant (kind 1), the maximum for an identity (kind 0). Other kinds take their maximum, with a
/// note.
fn function_value(cache: &Cache, field: usize, notes: &mut Vec<String>) -> Result<f32> {
    let size = usize::try_from(cache.i32(field)?)?;
    if size < 12 {
        return Ok(0.);
    }
    let data = cache.meta(cache.expand(cache.u32(field + 0x0C)?)?, size)?;
    ensure!(data[2] == 0, "Lens flare function holds colors");
    let low = f32::from_le_bytes(data[4..8].try_into()?);
    let high = f32::from_le_bytes(data[8..12].try_into()?);
    let weight = match data[0] {
        0 => 1.,
        1 => 0.,
        kind => {
            notes.push(format!("Function kind {kind} takes its maximum"));
            1.
        }
    };
    let mut value = low + (high - low) * weight;
    if data[1] & 4 != 0 {
        value = value.clamp(0., 1.);
    }
    ensure!(value.is_finite(), "Lens flare function is not finite");
    Ok(value)
}

/// A lens flare and the point it is attached at.
type Placed = (Lens, [f32; 3]);

/// The lens flare the first sky active on the `loaded` BSPs attaches at a marker of its model, in
/// the map's space (`offset`), with a report. Reach's sky attachments name the flare at the
/// `sun_flare` marker. Reflections whose bitmap index lies past the bitmap's images are left out,
/// as Reach draws none of them: Breakneck's flare keeps only its first.
fn read_lens(
    cache: &Cache,
    pages: &mut Pages,
    loaded: u32,
    offset: [f32; 3],
) -> Result<(Option<Placed>, Value)> {
    let scenario = cache.only("scnr")?.address()?;
    let Some(sky) = cache
        .block(scenario + SKIES, SKY_SIZE)?
        .into_iter()
        .map(|row| -> Result<Option<Tag>> {
            if u32::from(cache.u16(row + SKY_BSPS)?) & loaded == 0 {
                return Ok(None);
            }
            cache.reference(row, Some("scen"))
        })
        .find_map(|sky| sky.transpose())
        .transpose()?
    else {
        return Ok((None, json!({"lens": null})));
    };
    let at = sky.address()?;
    let mut attachment = None;
    for row in cache.block(at + OBJECT_ATTACHMENTS.0, OBJECT_ATTACHMENTS.1)? {
        if let Some(tag) = cache.reference(row, Some("lens"))? {
            attachment = Some((tag, cache.string_id(cache.u32(row + ATTACHMENT_MARKER)?)?));
            break;
        }
    }
    let Some((tag, marker_name)) = attachment else {
        return Ok((None, json!({"sky": sky.path, "lens": null})));
    };
    let object = SourceObject::read(cache, &sky)?;
    let render = object
        .world
        .as_ref()
        .and_then(|branch| branch.model.as_ref())
        .context("Sky has no render model")?;
    let selection = choices(&object, None, &BTreeMap::new())?;
    let model = model::read(cache, pages, render, &selection)?;
    let worlds = rig::world(&model.bones)?;
    let mut marker = None;
    for candidate in model.markers.iter().filter(|m| m.name == marker_name) {
        if model.marker_selected(candidate)? {
            marker = Some(candidate);
            break;
        }
    }
    let Some(marker) = marker else {
        return Ok((
            None,
            json!({"sky": sky.path, "lens": tag.path, "marker": marker_name, "reason": "No selected marker"}),
        ));
    };
    let local = match marker.bone {
        Some(bone) => rig::point(
            worlds.get(bone).context("Lens flare marker bone")?,
            marker.translation,
            true,
        ),
        None => marker.translation,
    };
    let position: [f32; 3] = std::array::from_fn(|k| local[k] + offset[k]);
    let lens_at = tag.address()?;
    let bitmap = cache
        .reference(lens_at + LENS_BITMAP, Some("bitm"))?
        .context("Lens flare has no bitmap")?;
    let images = cache
        .block(bitmap.address()? + BITMAP_IMAGES.0, BITMAP_IMAGES.1)?
        .len();
    let mut notes = Vec::new();
    let mut reflections = Vec::new();
    let mut skipped = 0;
    for row in cache.block(lens_at + LENS_REFLECTIONS.0, LENS_REFLECTIONS.1)? {
        let Ok(frame) = usize::try_from(cache.i16(row + REFLECTION_BITMAP)?) else {
            skipped += 1;
            continue;
        };
        if frame >= images {
            skipped += 1;
            continue;
        }
        let radius = function_value(cache, row + REFLECTION_RADIUS, &mut notes)?;
        let brightness = function_value(cache, row + REFLECTION_BRIGHTNESS, &mut notes)?;
        if radius <= 0. || brightness <= 0. {
            skipped += 1;
            continue;
        }
        let color = cache.floats::<3>(row + REFLECTION_COLOR)?;
        let values = [
            cache.f32(row + REFLECTION_ROTATION)?,
            cache.f32(row + REFLECTION_AXIS)?,
            cache.f32(row + REFLECTION_MODULATION)?,
            cache.f32(row + REFLECTION_TINT_POWER)?,
        ];
        ensure!(
            values.iter().chain(&color).all(|v| v.is_finite()),
            "Lens flare {} holds a nonfinite reflection",
            tag.path
        );
        reflections.push(Reflection {
            flags: cache.u8(row)?,
            sprite: source_material::Mapping {
                role: "lens_flare".into(),
                bitmap: Some(bitmap.clone()),
                transform: [1., 1., 0., 0.],
                frame,
                address: CLAMP_ADDRESS,
                filter: 0,
                functions: Vec::new(),
            },
            rotation: values[0].to_radians(),
            axis: values[1],
            radius,
            brightness,
            modulation: values[2],
            color,
            tint_power: values[3],
        });
    }
    let flags = cache.u16(lens_at + LENS_FLAGS)?;
    let occlusion_scale = cache.f32(lens_at + LENS_OCCLUSION_SCALE)?;
    let report = json!({
        "sky": sky.path,
        "lens": tag.path,
        "marker": marker_name,
        "position": position,
        "bitmap": bitmap.path,
        "images": images,
        "reflections": reflections.len(),
        "skipped_reflections": skipped,
        "flags": format!("{flags:04X}"),
        "occlusion_scale": occlusion_scale,
        "notes": notes,
    });
    if reflections.is_empty() {
        return Ok((None, report));
    }
    Ok((
        Some((
            Lens {
                reflections,
                occlusion_scale: if occlusion_scale.is_finite() {
                    occlusion_scale.clamp(0., 1.)
                } else {
                    0.
                },
                occluded: flags & LENS_NO_OCCLUSION == 0,
            },
            position,
        )),
        report,
    ))
}

/// The flare's mesh: one quad per reflection at `position`, keyed from [`FLARE_KEY`], whose
/// surfaces draw both faces since their programs turn and mirror it on screen. Its size only
/// bounds the object for culling, the largest reflection's radius at the flare's distance.
fn flare_mesh(lens: &Lens, position: [f32; 3], offset: [f32; 3]) -> Result<Mesh> {
    let distance = (0..3)
        .map(|k| (position[k] - offset[k]).powi(2))
        .sum::<f32>()
        .sqrt();
    let size = lens
        .reflections
        .iter()
        .map(|reflection| {
            if reflection.flags & 2 != 0 {
                reflection.radius * distance
            } else {
                reflection.radius * 3.048
            }
        })
        .fold(1f32, f32::max);
    let mut mesh = Mesh {
        positions: Vec::new(),
        attributes: Vec::new(),
        groups: Vec::new(),
        weights: Vec::new(),
        bones: 0,
    };
    for index in 0..lens.reflections.len() {
        let first = u32::try_from(mesh.positions.len())?;
        for (u, v) in [(0., 0.), (1., 0.), (1., 1.), (0., 1.)] {
            mesh.positions.push([
                position[0] + (u * 2. - 1.) * size,
                position[1] + (v * 2. - 1.) * size,
                position[2],
            ]);
            mesh.attributes.push([u, v, 0., 0., 1., 1., 1., 1., 1., 1.]);
        }
        let key = FLARE_KEY | u32::try_from(index)?;
        mesh.groups.push((
            key,
            vec![[first, first + 1, first + 2], [first, first + 2, first + 3]],
        ));
    }
    Ok(mesh)
}

/// The level's patchy fog, from its fog tag `fog_tag` when the tag enables it with a sheet density
/// above zero, and the atmosphere globals' noise, with a report.
#[allow(clippy::cognitive_complexity)]
fn read_patchy(
    cache: &Cache,
    fog_tag: Option<&str>,
    offset: [f32; 3],
) -> Result<(Option<Patchy>, Value)> {
    let Some(path) = fog_tag else {
        return Ok((None, json!({"reason": "The level has no fog tag"})));
    };
    let fog = cache.find("fogg", path)?;
    let at = fog.address()?;
    let flags = cache.u16(at)?;
    let density = cache.f32(at + PATCHY_DENSITY)?;
    if flags & FOG_PATCHY == 0 || density.is_nan() || density <= 0. {
        return Ok((
            None,
            json!({"fog": path, "flags": format!("{flags:04X}"), "density": density, "reason": "Patchy fog is off"}),
        ));
    }
    let Some(globals) = cache.tags.iter().flatten().find(|tag| tag.group == "atgf") else {
        return Ok((
            None,
            json!({"fog": path, "reason": "No atmosphere globals"}),
        ));
    };
    let globals_at = globals.address()?;
    let Some(noise) = cache.reference(globals_at, Some("bitm"))? else {
        return Ok((
            None,
            json!({"fog": path, "globals": globals.path, "reason": "No noise bitmap"}),
        ));
    };
    let floats = |at: usize| -> Result<f32> {
        let value = cache.f32(at)?;
        ensure!(
            value.is_finite(),
            "Patchy fog of {path} holds a nonfinite value"
        );
        Ok(value)
    };
    let patchy = Patchy {
        noise: source_material::Mapping {
            role: "patchy_fog".into(),
            bitmap: Some(noise.clone()),
            transform: [1., 1., 0., 0.],
            frame: 0,
            address: 0,
            filter: 0,
            functions: Vec::new(),
        },
        density,
        tint: cache.floats::<3>(at + PATCHY_TINT)?,
        inner: cache.floats::<3>(at + PATCHY_INNER)?,
        intensity: floats(at + PATCHY_INTENSITY)?,
        full_height: floats(at + PATCHY_FULL)?,
        half_height: floats(at + PATCHY_HALF)?,
        wind: cache.floats::<3>(at + PATCHY_WIND)?,
        reference: floats(at + PATCHY_REFERENCE)?,
        repeat: floats(globals_at + GLOBALS_REPEAT)?.max(0.001),
        spacing: floats(globals_at + GLOBALS_SPACING)?.max(0.01),
        depth_fade: floats(globals_at + GLOBALS_DEPTH_FADE)?.max(0.),
        origin: offset,
    };
    ensure!(
        patchy
            .tint
            .iter()
            .chain(&patchy.inner)
            .chain(&patchy.wind)
            .all(|v| v.is_finite()),
        "Patchy fog of {path} holds a nonfinite color or wind"
    );
    let report = json!({"fog": path, "globals": globals.path, "noise": noise.path});
    Ok((Some(patchy), report))
}

/// The patchy fog's sheets over the level's `area` (in the level's meters, moved by `offset`):
/// [`patchy::SHEETS`] horizontal quads, each carrying its order in the stack in `normal.w`, whose
/// vertex program sets their heights around the camera. Their corners span the level's height,
/// which only bounds the object. Returns the band of heights above the reference plane where the
/// fog's height fade is above zero within the level, or none where it lies outside the level.
fn sheet_mesh(
    patchy: &Patchy,
    area: ([f32; 3], [f32; 3]),
    offset: [f32; 3],
) -> Result<Option<(Mesh, (f32, f32))>> {
    let (low, high) = area;
    let height = |z: f32| z / 3.048 - patchy.reference;
    let Some(band) = patchy.band(height(low[2]), height(high[2])) else {
        return Ok(None);
    };
    let mut mesh = Mesh {
        positions: Vec::new(),
        attributes: Vec::new(),
        groups: Vec::new(),
        weights: Vec::new(),
        bones: 0,
    };
    let mut faces = Vec::new();
    for sheet in 0..patchy::SHEETS {
        let order = sheet as f32 / patchy::SHEETS as f32;
        let z = low[2] + (high[2] - low[2]) * order + offset[2];
        let first = u32::try_from(mesh.positions.len())?;
        for (u, v) in [(0., 0.), (1., 0.), (1., 1.), (0., 1.)] {
            mesh.positions.push([
                if u == 0. {
                    low[0] - SHEET_MARGIN
                } else {
                    high[0] + SHEET_MARGIN
                } + offset[0],
                if v == 0. {
                    low[1] - SHEET_MARGIN
                } else {
                    high[1] + SHEET_MARGIN
                } + offset[1],
                z,
            ]);
            mesh.attributes
                .push([u, v, 0., 0., 1., order, 1., 1., 1., 1.]);
        }
        faces.extend([[first, first + 1, first + 2], [first, first + 2, first + 3]]);
    }
    mesh.groups.push((PATCHY_KEY, faces));
    Ok(Some((mesh, band)))
}

/// The key bits of the weather effect's particle fields.
const WEATHER_KEY: u32 = 0x5200_0000;
/// A fog tag's weather effect reference.
const FOG_WEATHER: usize = 0xAC;
/// An effect emitter's properties (`effect::source` offsets): radius, maximum count, rate, life,
/// velocity, size, scale, tint and alpha. A property holds its inputs, its function's size at
/// `+0x04` and data at `+0x10`, a compiled constant at `+0x18` and flags at `+0x1C`, bit 5 marking
/// the constant valid.
const EMITTER_RADIUS: usize = 0xB8;
const EMITTER_MAXIMUM_COUNT: usize = 0x138;
const EMITTER_RATE: usize = 0x158;
const EMITTER_LIFE: usize = 0x178;
const EMITTER_VELOCITY: usize = 0x200;
const EMITTER_SIZE: usize = 0x260;
const EMITTER_SCALE: usize = 0x280;
const EMITTER_TINT: usize = 0x2A0;
const EMITTER_ALPHA: usize = 0x2C0;
/// A particle's (`prt3`) embedded material and the property read as its sprite's aspect, the
/// length over the width: 4 and 5 on Sword Base's raindrops, 1 and 1.5 on Breakneck's ash.
const PARTICLE_MATERIAL: usize = 0x40;
const PARTICLE_ASPECT: usize = 0xD8;
/// A particle's model (`pmdf`), which it draws in place of a sprite, as Hang 'Em High's space junk
/// draws debris. A sprite field cannot stand for it, as the effect importer's particles cannot.
const PARTICLE_MODEL: usize = 0x1BC;
/// The slowest a weather particle is taken to fall, in meters per second, so ash with no
/// velocity of its own still drifts.
const SLOWEST_FALL: f32 = 0.2;

/// The low and high values of the effect property at `at`: its compiled constant, a constant
/// function's minimum, or its function's minimum and maximum. Colors are read by
/// [`property_color`].
fn property_range(cache: &Cache, at: usize) -> Result<[f32; 2]> {
    if cache.u16(at + 0x1C)? & 0x20 != 0 {
        let value = cache.f32(at + 0x18)?;
        ensure!(value.is_finite(), "Effect property constant is not finite");
        return Ok([value; 2]);
    }
    let size = usize::try_from(cache.i32(at + 4)?)?;
    if size < 12 {
        return Ok([0.; 2]);
    }
    let data = cache.meta(cache.expand(cache.u32(at + 0x10)?)?, size)?;
    let low = f32::from_le_bytes(data[4..8].try_into()?);
    let high = f32::from_le_bytes(data[8..12].try_into()?);
    ensure!(
        low.is_finite() && high.is_finite(),
        "Effect property range is not finite"
    );
    Ok(if data[0] == 1 && data[1] & 1 == 0 {
        [low; 2]
    } else {
        [low.min(high), low.max(high)]
    })
}

/// The first color of the color property at `at`, a packed `0xAARRGGBB` value over 255, or none
/// when the property holds no colors.
fn property_color(cache: &Cache, at: usize) -> Result<Option<[f32; 3]>> {
    let size = usize::try_from(cache.i32(at + 4)?)?;
    if size < 12 {
        return Ok(None);
    }
    let data = cache.meta(cache.expand(cache.u32(at + 0x10)?)?, size)?;
    if data[2] == 0 {
        return Ok(None);
    }
    let packed = u32::from_le_bytes(data[4..8].try_into()?);
    Ok(Some(
        [16, 8, 0].map(|shift| ((packed >> shift) & 0xFF) as f32 / 255.),
    ))
}

/// The particle fields of the weather effect fog tag `fog_tag` names, one per emitter that draws a
/// sprite, with a report.
fn read_weather(cache: &Cache, fog_tag: Option<&str>) -> Result<(Vec<weather::Particles>, Value)> {
    let Some(path) = fog_tag else {
        return Ok((Vec::new(), json!({"reason": "The level has no fog tag"})));
    };
    let fog = cache.find("fogg", path)?;
    let Some(effect) = cache.reference(fog.address()? + FOG_WEATHER, Some("effe"))? else {
        return Ok((Vec::new(), json!({"fog": path, "effect": null})));
    };
    let source = crate::halo_reach::effect::source::read(cache, effect.address()?)?;
    let mut fields = Vec::new();
    let mut skipped = Vec::new();
    for event in source["events"].as_array().context("Weather events")? {
        for system in event["particles"].as_array().context("Weather systems")? {
            if system["tag"].is_null() {
                continue;
            }
            let particle: Tag = serde_json::from_value(system["tag"].clone())?;
            let particle_at = particle.address()?;
            if let Some(model) = cache.reference(particle_at + PARTICLE_MODEL, None)? {
                skipped.push(json!({"particle": particle.path, "model": model.path,
                    "reason": "Draws a particle model"}));
                continue;
            }
            let material = source_material::embedded(
                cache,
                particle.clone(),
                particle_at + PARTICLE_MATERIAL,
            )?;
            let Some(sprite) = material.mapping(&["base_map"]).cloned() else {
                skipped.push(json!({"particle": particle.path, "reason": "No sprite"}));
                continue;
            };
            let additive = material
                .options
                .get("blend_mode")
                .is_some_and(|mode| mode == "additive");
            let aspect = property_range(cache, particle_at + PARTICLE_ASPECT)?[1].max(0.1);
            for emitter in system["emitters"].as_array().context("Weather emitters")? {
                let at = usize::try_from(emitter["offset"].as_u64().context("Weather emitter")?)?;
                let mean = |[low, high]: [f32; 2]| (low + high) / 2.;
                let radius = property_range(cache, at + EMITTER_RADIUS)?;
                let maximum = property_range(cache, at + EMITTER_MAXIMUM_COUNT)?[1];
                let rate = mean(property_range(cache, at + EMITTER_RATE)?);
                let life = mean(property_range(cache, at + EMITTER_LIFE)?);
                let velocity = mean(property_range(cache, at + EMITTER_VELOCITY)?).abs();
                let size = property_range(cache, at + EMITTER_SIZE)?[1];
                let scale = property_range(cache, at + EMITTER_SCALE)?[1];
                let alpha = mean(property_range(cache, at + EMITTER_ALPHA)?);
                let color = property_color(cache, at + EMITTER_TINT)?.unwrap_or([1.; 3]);
                let count = (rate * life).min(if maximum > 0. { maximum } else { f32::MAX });
                let width = size * if scale > 0. { scale } else { 1. } * 3.048;
                if !(count >= 1. && width > 0. && radius[1] > 0. && alpha > 0.) {
                    skipped.push(
                        json!({"particle": particle.path, "reason": "No steady particles",
                        "count": count, "width": width, "radius": radius, "alpha": alpha}),
                    );
                    continue;
                }
                fields.push(weather::Particles {
                    sprite: sprite.clone(),
                    additive,
                    color,
                    alpha: alpha.min(1.),
                    width,
                    aspect,
                    count,
                    radii: [radius[0].max(0.1) * 3.048, radius[1] * 3.048],
                    fall: (velocity * 3.048).max(SLOWEST_FALL),
                });
            }
        }
    }
    Ok((
        fields,
        json!({"fog": path, "effect": effect.path, "location": source["locations"], "skipped": skipped}),
    ))
}

/// The weather fields' shell: the corners of the level's `area`, moved by `offset`, as a closed
/// box of triangles, one group per field keyed by `keys`. Its vertex program only
/// takes the corners' directions from the object's center, so the shell stays around the camera,
/// while its bounds cover the level and the object is never culled there.
fn shell_mesh(area: ([f32; 3], [f32; 3]), offset: [f32; 3], keys: &[u32]) -> Result<Mesh> {
    let (low, high) = area;
    let mut mesh = Mesh {
        positions: Vec::new(),
        attributes: Vec::new(),
        groups: Vec::new(),
        weights: Vec::new(),
        bones: 0,
    };
    for corner in 0..8 {
        let pick = |axis: usize| {
            if corner >> axis & 1 == 0 {
                low[axis] - SHEET_MARGIN
            } else {
                high[axis] + SHEET_MARGIN
            }
        };
        mesh.positions
            .push(std::array::from_fn(|k| pick(k) + offset[k]));
        mesh.attributes
            .push([0., 0., 0., 0., 1., 1., 1., 1., 1., 1.]);
    }
    // The cube's six faces, each as two triangles. The surfaces draw both faces.
    let quads: [[u32; 4]; 6] = [
        [0, 1, 3, 2],
        [4, 6, 7, 5],
        [0, 4, 5, 1],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 5, 7, 3],
    ];
    let mut faces = Vec::new();
    for [a, b, c, d] in quads {
        faces.extend([[a, b, c], [a, c, d]]);
    }
    for &key in keys {
        mesh.groups.push((key, faces.clone()));
    }
    Ok(mesh)
}

/// The keys of a sky primitive's transparent and light-shaft materials.
fn keys(primitive: &Primitive) -> Result<(u32, u32)> {
    let index = match primitive.material {
        Some(index) => u32::try_from(index)?,
        None => NO_MATERIAL,
    };
    ensure!(
        index <= NO_MATERIAL,
        "Sky material {index} is outside the sky's keys"
    );
    Ok((TRANSPARENT_KEY | index, SHAFTS_KEY | index))
}

/// The light Reach's large bloom adds to every pixel of the level, as a share of its average lit
/// level: inherent bloom times bloom intensity times the large bloom color, over the image's
/// average ([`SCENE_AVERAGE`]). It is black where the scenario names no camera effects or any of the
/// three takes its default. Hang 'Em High's, about (0.035, 0.026, 0.011), is a warm floor that
/// dominates its shade, whose baked light is a pale lavender, as the level's press screenshots show
/// warm gray shade.
pub(in crate::halo_reach) fn veil(cache: &Cache) -> Result<[f32; 3]> {
    let scenario = cache.only("scnr")?.address()?;
    let Some(effects) = cache.reference(scenario + DEFAULT_CAMERA_FX, Some("cfxs"))? else {
        return Ok([0.; 3]);
    };
    let at = effects.address()?;
    for field in [
        CAMERA_FX_INHERENT_BLOOM,
        CAMERA_FX_BLOOM_INTENSITY,
        CAMERA_FX_LARGE_BLOOM,
    ] {
        if cache.u16(at + field)? & CAMERA_FX_USE_DEFAULT != 0 {
            return Ok([0.; 3]);
        }
    }
    let inherent = cache.f32(at + CAMERA_FX_INHERENT_BLOOM + 4)?;
    let intensity = cache.f32(at + CAMERA_FX_BLOOM_INTENSITY + 4)?;
    let color = cache.floats::<3>(at + CAMERA_FX_LARGE_BLOOM + 4)?;
    if ![inherent, intensity]
        .into_iter()
        .chain(color)
        .all(|v| v.is_finite() && v >= 0.)
    {
        return Ok([0.; 3]);
    }
    Ok(color.map(|c| inherent * intensity * c * SCENE_AVERAGE))
}

/// The exposure Reach shows the level at, as a scale, and the screen brightness its automatic
/// exposure aims for, from the scenario's default camera effects. With automatic exposure the
/// level settles inside its range, taken at the range's middle.
fn exposure(cache: &Cache) -> Result<(f32, f32, Value)> {
    let scenario = cache.only("scnr")?.address()?;
    let Some(effects) = cache.reference(scenario + DEFAULT_CAMERA_FX, Some("cfxs"))? else {
        return Ok((1., DEFAULT_BRIGHTNESS.exp2(), json!({"camera_fx": null})));
    };
    let at = effects.address()?;
    let flags = cache.u16(at + CAMERA_FX_FLAGS)?;
    let target = cache.f32(at + CAMERA_FX_EXPOSURE)?;
    let [low, high] = cache.floats::<2>(at + CAMERA_FX_RANGE)?;
    let brightness = cache.f32(at + CAMERA_FX_BRIGHTNESS)?;
    ensure!(
        [target, low, high, brightness]
            .iter()
            .all(|v| v.is_finite())
            && low <= high,
        "Camera effects {} hold an invalid exposure",
        effects.path
    );
    let stops = if flags & AUTO_EXPOSURE != 0 {
        (low + high) / 2.
    } else {
        target.clamp(low, high)
    };
    Ok((
        stops.exp2(),
        brightness.exp2(),
        json!({
            "camera_fx": effects.path,
            "automatic": flags & AUTO_EXPOSURE != 0,
            "target_stops": target,
            "range_stops": [low, high],
            "screen_brightness_stops": brightness,
            "exposure_stops": stops,
        }),
    ))
}

/// Carry the authored camera target through the calibrated native radiance scale. Automatic
/// exposure still belongs to Shadowkeep. This preserves relative source targets, rather than
/// claiming to reproduce Reach's view-dependent luminance reduction and adaptation history.
pub(super) fn radiance(cache: &Cache, level: Option<f32>) -> Result<(f32, Value)> {
    let (shown, brightness, mut report) = exposure(cache)?;
    // The existing native calibration used a source automatic target of 0.09 linear light.
    const REFERENCE_BRIGHTNESS: f32 = 0.09;
    let target = if report["automatic"].as_bool() == Some(false) {
        level.unwrap_or(brightness / shown) * shown
    } else {
        brightness
    };
    let radiance = SKY_RADIANCE * target / REFERENCE_BRIGHTNESS;
    ensure!(
        radiance.is_finite() && radiance > 0.,
        "Invalid translated camera radiance"
    );
    report["native_radiance"] = json!(radiance);
    report["reference_brightness"] = json!(REFERENCE_BRIGHTNESS);
    report["method"] = json!(
        "Source camera target relative to the native calibration. Native exposure adaptation remains active."
    );
    Ok((radiance, report))
}

/// Adds the level's sky to `graph`: one native model and model reference for each chunk of the
/// sky's triangles, each drawing its surfaces in the transparent and light-shaft stages with the
/// materials `templates` lends their envelopes. `primitives` are the sky's, about the level's
/// origin and moved by `offset`, with materials among `materials`, each with its vertex colors.
/// The colors ride in the tangent stream, as square roots over the sky's largest color, since
/// the sky's surfaces are unlit and need no tangents. `level` is the lightmap's average lit level,
/// which the level's surfaces' light is a share of, and `fog` the level's fog, which the sky takes
/// as Reach's does. A sky that follows the camera in Reach, by the first sky active on the
/// `loaded` BSPs, follows it here ([`parallax`]). That sky's lens flare draws as one more sky
/// object at its marker ([`read_lens`]), the patchy fog of fog tag `fog_tag` as soft sheets over
/// the level's `area` ([`read_patchy`]), and its weather effect's sprite emitters as particle fields
/// around the camera ([`read_weather`]), all after the sky. Returns the sky objects, in draw order,
/// and a report.
#[allow(clippy::too_many_arguments)]
pub(super) fn add(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    materials: &[Material],
    primitives: &[(Primitive, Vec<[f32; 3]>)],
    offset: [f32; 3],
    level: Option<f32>,
    fog: Option<Fog>,
    loaded: u32,
    fog_tag: Option<&str>,
    area: Option<([f32; 3], [f32; 3])>,
) -> Result<(Vec<Object>, Value)> {
    if primitives.is_empty() {
        return Ok((Vec::new(), json!({"objects": 0})));
    }
    for (primitive, colors) in primitives {
        ensure!(
            colors.len() == primitive.vertices.len(),
            "Sky vertex colors differ from its vertices"
        );
    }
    let largest = primitives
        .iter()
        .flat_map(|(_, colors)| colors.iter().flatten())
        .fold(0f32, |largest, &value| largest.max(value));
    let vertex_color = largest.max(1e-6);
    let (shown, brightness, _) = exposure(cache)?;
    let (radiance, exposure_report) = radiance(cache, level)?;
    // The sky takes its Reach radiance over the average lit level, as the level surfaces do, at its
    // own scale. Without a lightmap, the average lit level is taken where Reach's automatic
    // exposure aims its average lit surface: the screen brightness over the exposure.
    let level = level
        .filter(|level| level.is_finite() && *level > 0.)
        .unwrap_or(brightness / shown);
    let scale = SkyScale {
        vertex_color,
        radiance: radiance / level,
        white: 1. / shown,
    };
    let placeholder = Material {
        decal: None,
        tag: None,
        options: BTreeMap::new(),
        properties: Vec::new(),
    };
    let mut used = BTreeMap::<u32, (u32, &Material)>::new();
    let mut stages = BTreeMap::<u32, u8>::new();
    for (primitive, _) in primitives {
        let (transparent, shafts) = keys(primitive)?;
        let material = match primitive.material {
            Some(index) => materials
                .get(index)
                .with_context(|| format!("Sky material {index} is missing"))?,
            None => &placeholder,
        };
        used.insert(transparent, (shafts, material));
        stages.insert(transparent, TRANSPARENT_STAGE);
        stages.insert(shafts, LIGHT_SHAFT_STAGE);
    }
    let (parallax, parallax_report) = parallax(cache, loaded, offset)?;
    let sky_surfaces = used
        .iter()
        .map(|(&transparent, &(shafts, material))| SkySurface {
            material,
            transparent,
            shafts,
        })
        .collect::<Vec<_>>();
    let material_report = surfaces(
        cache,
        pages,
        native,
        graph,
        &sky_surfaces,
        [templates.surface, templates.shafts],
        scale,
        fog.as_ref(),
        parallax,
    )?;
    let template = native
        .tag(templates.model, Some(MODEL_CLASS))
        .context("Sky model template")?;
    let row = *template
        .array(16, 136, Some(MESH_CLASS))?
        .first()
        .context("The donor's sky model has no mesh")?;
    let mut objects = Vec::new();
    let mut models = Vec::new();
    for (index, chunk) in chunks(primitives)?.into_iter().enumerate() {
        let mesh = chunk_mesh(primitives, &chunk, offset, vertex_color)?;
        let (object, mut model) = add_model(
            native,
            graph,
            templates,
            &template,
            row,
            &format!("sky{index}"),
            mesh,
            &stages,
            index as f32,
        )?;
        model["triangles"] = json!(
            chunk
                .runs
                .iter()
                .map(|(_, faces)| faces.len())
                .sum::<usize>()
        );
        model["primitives"] = json!(
            chunk
                .runs
                .iter()
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        );
        model["surfaces"] = json!(
            chunk
                .runs
                .iter()
                .map(|(index, faces)| {
                    keys(&primitives[*index].0).map(|(transparent, shafts)| {
                        json!({
                            "primitive": index,
                            "triangles": faces.len(),
                            "transparent": format!("{transparent:08X}"),
                            "shafts": format!("{shafts:08X}"),
                        })
                    })
                })
                .collect::<Result<Vec<_>>>()?
        );
        models.push(model);
        objects.push(object);
    }
    let (lens_found, mut lens_report) = read_lens(cache, pages, loaded, offset)?;
    if let Some((lens, position)) = lens_found {
        let keys = (0..lens.reflections.len())
            .map(|index| Ok(FLARE_KEY | u32::try_from(index)?))
            .collect::<Result<Vec<_>>>()?;
        lens_report["surfaces"] = json!(lens::surfaces(
            cache,
            pages,
            native,
            graph,
            &lens,
            &keys,
            templates.surface,
            scale,
            parallax,
        )?);
        let stages = keys
            .iter()
            .map(|&key| (key, TRANSPARENT_STAGE))
            .collect::<BTreeMap<_, _>>();
        let (object, model) = add_model(
            native,
            graph,
            templates,
            &template,
            row,
            "sky-flare",
            flare_mesh(&lens, position, offset)?,
            &stages,
            objects.len() as f32,
        )?;
        lens_report["model"] = model;
        objects.push(object);
    }
    let (patchy_found, mut patchy_report) = read_patchy(cache, fog_tag, offset)?;
    if let (Some(patchy), Some(area)) = (patchy_found, area)
        && let Some((mesh, band)) = sheet_mesh(&patchy, area, offset)?
    {
        patchy_report["surface"] = patchy::surface(
            cache,
            pages,
            native,
            graph,
            &patchy,
            PATCHY_KEY,
            templates.surface,
            scale,
        )?;
        let (object, model) = add_model(
            native,
            graph,
            templates,
            &template,
            row,
            "sky-patchy-fog",
            mesh,
            &BTreeMap::from([(PATCHY_KEY, TRANSPARENT_STAGE)]),
            objects.len() as f32,
        )?;
        patchy_report["band_above_reference"] = json!(band);
        patchy_report["model"] = model;
        objects.push(object);
    }
    let (fields, mut weather_report) = read_weather(cache, fog_tag)?;
    if let (false, Some(area)) = (fields.is_empty(), area) {
        let keys = (0..fields.len())
            .map(|index| Ok(WEATHER_KEY | u32::try_from(index)?))
            .collect::<Result<Vec<_>>>()?;
        weather_report["surfaces"] = json!(weather::surfaces(
            cache,
            pages,
            native,
            graph,
            &fields,
            &keys,
            templates.surface,
            scale,
        )?);
        let stages = keys
            .iter()
            .map(|&key| (key, TRANSPARENT_STAGE))
            .collect::<BTreeMap<_, _>>();
        let (object, model) = add_model(
            native,
            graph,
            templates,
            &template,
            row,
            "sky-weather",
            shell_mesh(area, offset, &keys)?,
            &stages,
            objects.len() as f32,
        )?;
        weather_report["model"] = model;
        objects.push(object);
    }
    Ok((
        objects,
        json!({
            "objects": models,
            "materials": material_report,
            "exposure": exposure_report,
            "vertex_color_scale": vertex_color,
            "radiance_scale": scale.radiance,
            "white": scale.white,
            "sky_radiance": radiance,
            "lit_level": level,
            "fogged": fog.is_some(),
            "parallax": parallax_report,
            "lens_flare": lens_report,
            "patchy_fog": patchy_report,
            "weather": weather_report,
            "templates": {
                "reference": format!("{:08X}", templates.reference),
                "model": format!("{:08X}", templates.model),
                "surface": format!("{:08X}", templates.surface),
                "shafts": format!("{:08X}", templates.shafts),
            },
        }),
    ))
}

/// Adds sky model `name` drawing `mesh`, each group in its stage of `stages`, with its model
/// reference, as one sky object at the center of its bounds with draw order `order`. The donor's
/// model `template` lends its mesh `row` and stream headers. Returns the object and a report.
#[allow(clippy::too_many_arguments)]
fn add_model(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    template: &Payload,
    row: usize,
    name: &str,
    mut mesh: Mesh,
    stages: &BTreeMap<u32, u8>,
    order: f32,
) -> Result<(Object, Value)> {
    let (low, high) = bounds(mesh.positions.iter().copied());
    // The object sits at the center of its bounds, and its geometry is relative to it.
    let center: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
    for position in &mut mesh.positions {
        *position = sub(*position, center);
    }
    let used_stages = mesh
        .groups
        .iter()
        .map(|(key, _)| {
            stages
                .get(key)
                .map(|&stage| (*key, stage))
                .with_context(|| format!("Sky surface {key:08X} has no stage"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut encoded = geometry::encode_source_passes(&mesh, template, row, &used_stages)?;
    let radius = Payload(encoded.header.clone()).f32(MODEL_RADIUS)?;
    for offset in MODEL_EXTENTS {
        put(&mut encoded.header, offset, &radius.to_le_bytes())?;
    }
    let records = (encoded.header.len() - DRAW_RECORDS) / DRAW_RECORD_SIZE;
    for record in 0..records {
        put(
            &mut encoded.header,
            DRAW_RECORDS + record * DRAW_RECORD_SIZE + DRAW_FLAGS,
            &SKY_DRAW_FLAGS.to_le_bytes(),
        )?;
    }
    let patches = add_streams(native, graph, template, row, name, &encoded)?;
    let model = format!("{name}-model");
    graph.add(&model, templates.model, &encoded.header, None, patches)?;
    if let Some(node) = graph.nodes.iter_mut().find(|node| node["symbol"] == model) {
        node["model"] = json!(true);
    }
    let mut reference = vec![0u8; REFERENCE_SIZE];
    put(&mut reference, 0, &(REFERENCE_SIZE as u64).to_le_bytes())?;
    put(&mut reference, REFERENCE_MODEL, &u32::MAX.to_le_bytes())?;
    let symbol = format!("{name}-reference");
    graph.add(
        &symbol,
        templates.reference,
        &reference,
        None,
        vec![json!({"offset": REFERENCE_MODEL, "symbol": model})],
    )?;
    let minimum = std::array::from_fn(|k| low[k]);
    let maximum = std::array::from_fn(|k| high[k]);
    Ok((
        Object {
            reference: symbol,
            position: center,
            minimum,
            maximum,
            order,
        },
        json!({
            "model": model,
            "vertices": mesh.positions.len(),
            "position": center,
            "bounds": [minimum, maximum],
            "draws": records,
        }),
    ))
}

/// The mesh of `chunk`, a share of the sky's `primitives`, with its vertices moved by `offset` and
/// each vertex color kept as its square root over `vertex_color`, the sky's largest color. Each run
/// of triangles draws in the transparent and the light-shaft stage.
fn chunk_mesh(
    primitives: &[(Primitive, Vec<[f32; 3]>)],
    chunk: &Chunk,
    offset: [f32; 3],
    vertex_color: f32,
) -> Result<Mesh> {
    let mut mesh = Mesh {
        positions: Vec::new(),
        attributes: Vec::new(),
        groups: Vec::new(),
        weights: Vec::new(),
        bones: 0,
    };
    let mut corners = HashMap::<(usize, u32), u32>::new();
    for (primitive_index, faces) in &chunk.runs {
        let (primitive, colors) = &primitives[*primitive_index];
        let (transparent, shafts) = keys(primitive)?;
        let mut out = Vec::with_capacity(faces.len());
        for face in faces {
            let points = face.map(|c| primitive.vertices[c as usize].position);
            let normal = unit(super::cross(
                sub(points[1], points[0]),
                sub(points[2], points[0]),
            ))
            .unwrap_or([0., 0., 1.]);
            let mut corner_indices = [0u32; 3];
            for (slot, &corner) in corner_indices.iter_mut().zip(face) {
                *slot = match corners.get(&(*primitive_index, corner)) {
                    Some(&at) => at,
                    None => {
                        let vertex = &primitive.vertices[corner as usize];
                        let n = unit(vertex.normal).unwrap_or(normal);
                        let color = colors[corner as usize]
                            .map(|value| (value.max(0.) / vertex_color).sqrt().min(1.));
                        let at = u32::try_from(mesh.positions.len())?;
                        mesh.positions
                            .push(std::array::from_fn(|k| vertex.position[k] + offset[k]));
                        mesh.attributes.push([
                            vertex.uv[0],
                            vertex.uv[1],
                            n[0],
                            n[1],
                            n[2],
                            1.,
                            color[0],
                            color[1],
                            color[2],
                            1.,
                        ]);
                        corners.insert((*primitive_index, corner), at);
                        at
                    }
                };
            }
            out.push(corner_indices);
        }
        mesh.groups.push((transparent, out.clone()));
        mesh.groups.push((shafts, out));
    }
    Ok(mesh)
}

/// Adds sky model `name`'s position, attribute and index streams: copies of the stream headers the
/// donor's mesh `row` of `template` names, sized for the streams `encoded` holds, each over its
/// data. Returns the encoder's patches pointed at the new headers.
fn add_streams(
    native: &mut Reader,
    graph: &mut Graph,
    template: &Payload,
    row: usize,
    name: &str,
    encoded: &geometry::Encoded,
) -> Result<Vec<Value>> {
    let mut patches = encoded.patches.clone();
    for (stream, kind) in ["positions", "attributes", "indices"]
        .into_iter()
        .enumerate()
    {
        let tag = template.u32(row + [0, 4, 16][stream])?;
        let mut header = native
            .tag(tag, None)
            .with_context(|| format!("Sky buffer {tag:08X}"))?
            .0
            .clone();
        put(
            &mut header,
            if stream == 2 { 8 } else { 0 },
            &u32::try_from(encoded.streams[stream].len())?.to_le_bytes(),
        )?;
        if stream == 2 {
            // Unweighted meshes index with 16 bits.
            header[1] = 0;
        } else {
            put(
                &mut header,
                4,
                &(if stream == 0 {
                    POSITION_STRIDE
                } else {
                    ATTRIBUTE_STRIDE
                })
                .to_le_bytes(),
            )?;
        }
        let symbol = format!("{name}-{kind}-header");
        let data = format!("{name}-{kind}-data");
        graph.add(&symbol, tag, &header, Some(&data), vec![])?;
        graph.add(
            &data,
            native.reference(tag)?,
            &encoded.streams[stream],
            Some(&symbol),
            vec![],
        )?;
        for patch in &mut patches {
            if patch["symbol"] == format!("{kind}-header") {
                patch["symbol"] = json!(symbol);
            }
        }
    }
    Ok(patches)
}

/// The scenario's skies: the sky object, cloud scale, speed and direction, the cloud texture, the
/// name index and the structure BSPs each is active on.
const SKIES: usize = 0x88;
const SKY_SIZE: usize = 0x30;
const SKY_CLOUDS: usize = 0x10;
const SKY_CLOUD_TEXTURE: usize = 0x1C;
const SKY_NAME: usize = 0x2C;
const SKY_BSPS: usize = 0x2E;
/// The scenario's structure BSPs, each naming its structure lighting and local structure lighting,
/// its flags and its default sky.
const SCENARIO_BSPS: usize = 0x50;
const SCENARIO_BSP_SIZE: usize = 0xAC;
const BSP_LIGHTING: usize = 0x20;
const BSP_LOCAL_LIGHTING: usize = 0x30;
const BSP_FLAGS: usize = 0x50;
const BSP_DEFAULT_SKY: usize = 0x52;
const BSP_CUBEMAP: usize = 0x58;
/// The scenario's default camera effects, sky, fog, global lighting and lightmap references.
const SCENARIO_REFERENCES: [(&str, usize); 5] = [
    ("default_camera_fx", 0x6A8),
    ("sky_parameters", 0x6D8),
    ("fog_parameters", 0x6E8),
    ("global_lighting", 0x6F8),
    ("lightmap", 0x708),
];
/// The scenario's camera effects by zone set, each naming its camera effects at `+0x04`.
const CAMERA_FX: (usize, usize) = (0x540, 0x30);
/// A global lighting tag's lighting variables by object type, `0x14` byte rows.
const LIGHTING_VARIABLES: (usize, usize) = (0x04, 0x14);
/// A structure lighting tag's two blocks, of `0x50` and `0x84` byte rows.
const LIGHT_DEFINITIONS: (usize, usize) = (0x4, 0x50);
const LIGHT_INSTANCES: (usize, usize) = (0x10, 0x84);
const LIGHT_INSTANCE_REFERENCES: [usize; 4] = [0x44, 0x54, 0x64, 0x74];
/// A sky atmosphere tag's fog bitmap, its atmosphere properties and its underwater settings.
const ATMOSPHERE_PROPERTIES: (usize, usize) = (0x2C, 0xAC);
const ATMOSPHERE_WEATHER: usize = 0x94;
/// Meters per Reach world unit.
const METRES_PER_UNIT: f32 = 3.048;

fn reference(cache: &Cache, at: usize) -> Result<Value> {
    Ok(cache.reference(at, None)?.map_or(
        Value::Null,
        |tag| json!({"group": tag.group, "path": tag.path}),
    ))
}

/// Each word of the `size` bytes at `at`, as hexadecimal and as a float, by offset.
fn words(cache: &Cache, at: usize, size: usize) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for offset in (0..size).step_by(4) {
        let word = cache.u32(at + offset)?;
        out.insert(
            format!("{offset:#04x}"),
            json!([format!("{word:08X}"), f32::from_bits(word)]),
        );
    }
    Ok(Value::Object(out))
}

/// A block's rows as words, with the tag references at `references` resolved.
fn rows(
    cache: &Cache,
    at: usize,
    (offset, size): (usize, usize),
    references: &[usize],
) -> Result<Value> {
    cache
        .block(at + offset, size)?
        .into_iter()
        .map(|row| {
            let mut value = words(cache, row, size)?;
            for &field in references {
                value[format!("ref{field:#04x}")] = reference(cache, row + field)?;
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}

fn lighting(cache: &Cache, tag: Option<Tag>) -> Result<Value> {
    let Some(tag) = tag else {
        return Ok(Value::Null);
    };
    let at = tag.address()?;
    Ok(json!({
        "group": tag.group,
        "path": tag.path,
        "header": words(cache, at, 0x4C)?,
        "definitions": rows(cache, at, LIGHT_DEFINITIONS, &[])?,
        "instances": rows(cache, at, LIGHT_INSTANCES, &LIGHT_INSTANCE_REFERENCES)?,
    }))
}

/// The render geometry and materials of a sky object's default variant, in meters about the
/// level's origin.
fn sky_model(cache: &Cache, pages: &mut Pages, tag: &Tag) -> Result<Value> {
    let object = SourceObject::read(cache, tag)?;
    let Some(render) = object
        .world
        .as_ref()
        .and_then(|branch| branch.model.as_ref())
    else {
        return Ok(json!({"model": null}));
    };
    let selection = choices(&object, None, &BTreeMap::new())?;
    let model = model::read(cache, pages, render, &selection)
        .with_context(|| format!("Sky {}", tag.path))?;
    // Each section's flags, vertex type, PRT type and lighting policy, and the streams its vertex
    // buffer slots name, with the first vertices' bytes.
    let geometry = model::Geometry::read(cache, render, render.address()? + 100)?;
    let mut sections = Vec::new();
    for (index, &row) in geometry.sections.iter().enumerate() {
        let mut slots = Vec::new();
        for slot in 0..8 {
            let Some(vb) = geometry.section_buffer(cache, index, slot)? else {
                continue;
            };
            let (count, declaration, data) = geometry.vertex_buffer(cache, pages, vb)?;
            let stride = (declaration >> 16) as usize;
            let head = data
                .chunks(stride.max(1))
                .take(4)
                .map(|bytes| bytes.iter().map(|b| format!("{b:02X}")).collect::<String>())
                .collect::<Vec<_>>();
            slots.push(json!({
                "slot": slot,
                "buffer": vb,
                "count": count,
                "declaration": format!("{declaration:08X}"),
                "bytes": data.len(),
                "head": head,
            }));
        }
        sections.push(json!({
            "section": index,
            "flags": format!("{:04X}", cache.u16(row + 0x2C)?),
            "vertex_type": cache.u8(row + 0x2F)?,
            "prt_type": cache.u8(row + 0x30)?,
            "lighting_policy": cache.u8(row + 0x31)?,
            "index_type": cache.u8(row + 0x32)?,
            "slots": slots,
        }));
    }
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    let mut vertices = 0;
    let mut triangles = 0;
    let mut primitives = Vec::new();
    for primitive in &model.primitives {
        let mut plow = [f32::INFINITY; 3];
        let mut phigh = [f32::NEG_INFINITY; 3];
        for vertex in &primitive.vertices {
            for k in 0..3 {
                let value = vertex.position[k];
                plow[k] = plow[k].min(value);
                phigh[k] = phigh[k].max(value);
            }
        }
        for k in 0..3 {
            low[k] = low[k].min(plow[k]);
            high[k] = high[k].max(phigh[k]);
        }
        vertices += primitive.vertices.len();
        triangles += primitive.indices.len() / 3;
        primitives.push(json!({
            "material": primitive.material,
            "section": primitive.section,
            "part": primitive.part,
            "flags": primitive.flags,
            "vertices": primitive.vertices.len(),
            "triangles": primitive.indices.len() / 3,
            "bounds": [plow, phigh],
            "first_vertex": primitive.vertices.first(),
        }));
    }
    Ok(json!({
        "object": tag.path,
        "model": model.tag.path,
        "selected": model.selected,
        "vertices": vertices,
        "triangles": triangles,
        "bounds": [low, high],
        "primitives": primitives,
        "sections": sections,
        "materials": model.materials,
    }))
}

/// A report of `map`'s skies, its structure BSPs' default skies and lighting, and the sky, fog and
/// lighting tags its scenario names. Positions are in meters.
pub fn survey(maps: &Path, map: &str) -> Result<Value> {
    let cache = Cache::open(&maps.join(map))?;
    let mut pages = Pages::default();
    let scenario = cache.only("scnr")?.address()?;
    let mut skies = Vec::new();
    for (index, row) in cache
        .block(scenario + SKIES, SKY_SIZE)?
        .into_iter()
        .enumerate()
    {
        let tag = cache.reference(row, None)?;
        skies.push(json!({
            "index": index,
            "object": tag.as_ref().map(|tag| json!({"group": tag.group, "path": tag.path})),
            "clouds": cache.floats::<3>(row + SKY_CLOUDS)?,
            "cloud_texture": reference(&cache, row + SKY_CLOUD_TEXTURE)?,
            "name_index": cache.i16(row + SKY_NAME)?,
            "active_on_bsps": format!("{:04X}", cache.u16(row + SKY_BSPS)?),
            "geometry": match &tag {
                Some(tag) => sky_model(&cache, &mut pages, tag)?,
                None => Value::Null,
            },
        }));
    }
    let mut bsps = Vec::new();
    for row in cache.block(scenario + SCENARIO_BSPS, SCENARIO_BSP_SIZE)? {
        // Each cluster's bounds and the rows of its block at `+0x80`: two 16-bit values and a
        // block of 16-byte rows whose first three floats read as points.
        let clusters = match cache.reference(row, Some("sbsp"))? {
            Some(bsp) => {
                let at = bsp.address()?;
                cache
                    .block(at + 0x138, 0x8C)?
                    .into_iter()
                    .map(|cluster| {
                        let entries = cache
                            .block(cluster + 0x80, 0x10)?
                            .into_iter()
                            .map(|entry| {
                                let points = cache
                                    .block(entry + 4, 0x10)?
                                    .into_iter()
                                    .map(|point| {
                                        Ok(json!([
                                            cache.f32(point)?,
                                            cache.f32(point + 4)?,
                                            cache.f32(point + 8)?,
                                            format!("{:08X}", cache.u32(point + 12)?)
                                        ]))
                                    })
                                    .collect::<Result<Vec<_>>>()?;
                                Ok(json!({
                                    "a": cache.i16(entry)?,
                                    "b": cache.i16(entry + 2)?,
                                    "points": points,
                                }))
                            })
                            .collect::<Result<Vec<_>>>()?;
                        Ok(json!({
                            "bounds": cache.floats::<6>(cluster)?,
                            "entries": entries,
                        }))
                    })
                    .collect::<Result<Vec<_>>>()?
            }
            None => Vec::new(),
        };
        bsps.push(json!({
            "clusters": clusters,
            "bsp": reference(&cache, row)?,
            "flags": format!("{:04X}", cache.u16(row + BSP_FLAGS)?),
            "default_sky": cache.i16(row + BSP_DEFAULT_SKY)?,
            "cubemap": reference(&cache, row + BSP_CUBEMAP)?,
            "structure_lighting": lighting(&cache, cache.reference(row + BSP_LIGHTING, None)?)?,
            "local_structure_lighting": lighting(&cache, cache.reference(row + BSP_LOCAL_LIGHTING, None)?)?,
        }));
    }
    let mut references = serde_json::Map::new();
    for (name, offset) in SCENARIO_REFERENCES {
        let tag = cache.reference(scenario + offset, None)?;
        let value = match tag {
            Some(tag) => {
                let at = tag.address()?;
                let size = if tag.group == "cfxs" { 0xDC } else { 0x44 };
                let mut value = json!({"group": tag.group, "path": tag.path, "header": words(&cache, at, size)?});
                if tag.group == "chmt" {
                    value["lighting_variables"] = rows(&cache, at, LIGHTING_VARIABLES, &[])?;
                }
                if tag.group == "skya" {
                    value["fog_bitmap"] = reference(&cache, at)?;
                    value["atmospheres"] =
                        rows(&cache, at, ATMOSPHERE_PROPERTIES, &[ATMOSPHERE_WEATHER])?;
                }
                value
            }
            None => Value::Null,
        };
        references.insert(name.to_owned(), value);
    }
    let camera_fx = cache
        .block(scenario + CAMERA_FX.0, CAMERA_FX.1)?
        .into_iter()
        .map(|row| {
            let tag = cache.reference(row + 0x04, None)?;
            Ok(match tag {
                Some(tag) => json!({
                    "group": tag.group,
                    "path": tag.path,
                    "header": words(&cache, tag.address()?, 0xDC)?,
                }),
                None => Value::Null,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    references.insert("camera_fx".to_owned(), Value::Array(camera_fx));
    // Every atmosphere fog (`fogg`) and atmosphere globals (`atgf`) tag with its leading words, and
    // the count of tags in each group.
    let mut groups = BTreeMap::<String, usize>::new();
    let mut fog = Vec::new();
    // Every bitmap's images, each its width, height and mip count, by datum.
    let mut bitmaps = serde_json::Map::new();
    for tag in cache.tags.iter().flatten() {
        if tag.group == "bitm"
            && let Ok(at) = tag.address()
            && let Ok(rows) = cache.block(at + 124, 56)
        {
            let images = rows
                .iter()
                .map(|&row| {
                    Ok(json!([
                        cache.u16(row)?,
                        cache.u16(row + 2)?,
                        cache.u8(row + 16)?,
                        cache.i16(row + 8)?,
                        cache.u8(row + 17)?,
                        cache.u8(row + 6)?
                    ]))
                })
                .collect::<Result<Vec<_>>>()?;
            bitmaps.insert(
                format!("{:08X}", tag.datum),
                json!({"path": tag.path, "images": images}),
            );
        }
        *groups.entry(tag.group.clone()).or_default() += 1;
        if ["fogg", "atgf"].contains(&tag.group.as_str()) {
            fog.push(json!({
                "group": tag.group,
                "path": tag.path,
                "header": match tag.address() {
                    Ok(at) => words(&cache, at, 0x100).unwrap_or(Value::Null),
                    Err(_) => Value::Null,
                },
            }));
        }
    }
    // The scenario's player spawn points' count and bounds, in meters, or why there are none. A
    // scenario without spawns still has a sky to report.
    let spawns = match crate::halo_reach::level::spawn::read(&cache, None, 0) {
        Ok(spawns) => {
            let (low, high) = spawns.iter().fold(
                ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
                |(low, high), spawn| {
                    (
                        std::array::from_fn(|k| low[k].min(spawn.position[k])),
                        std::array::from_fn(|k| high[k].max(spawn.position[k])),
                    )
                },
            );
            json!({"count": spawns.len(), "low": low, "high": high})
        }
        Err(error) => json!({"error": format!("{error:#}")}),
    };
    Ok(json!({
        "map": map,
        "metres_per_unit": METRES_PER_UNIT,
        "skies": skies,
        "bsps": bsps,
        "references": references,
        "groups": groups,
        "fog": fog,
        "bitmaps": bitmaps,
        "spawns": spawns,
    }))
}

/// Writes the compiled shaders of every sky surface's template of `map` into `output`, one folder
/// per template: its pixel shader's compiled shaders as `pixel-<index>-<slot>.bin`, its vertex
/// shader's as `vertex-<index>-<slot>.bin`, and `shaders.json` with the template, the sky
/// materials using it, both shaders' entry points and the template's float constant and texture
/// names.
pub fn export_shaders(maps: &Path, map: &str, output: &Path) -> Result<()> {
    let cache = Cache::open(&maps.join(map))?;
    let mut pages = Pages::default();
    let scenario = cache.only("scnr")?.address()?;
    let mut templates = BTreeMap::<String, (Tag, Vec<Value>)>::new();
    for row in cache.block(scenario + SKIES, SKY_SIZE)? {
        let Some(tag) = cache.reference(row, Some("scen"))? else {
            continue;
        };
        let object = SourceObject::read(&cache, &tag)?;
        let Some(render) = object
            .world
            .as_ref()
            .and_then(|branch| branch.model.as_ref())
        else {
            continue;
        };
        let selection = choices(&object, None, &BTreeMap::new())?;
        let model = model::read(&cache, &mut pages, render, &selection)?;
        for material in &model.materials {
            let Some(property) = material.properties.first() else {
                continue;
            };
            templates
                .entry(property.template.path.clone())
                .or_insert_with(|| (property.template.clone(), Vec::new()))
                .1
                .push(json!({"material": material.tag, "options": material.options}));
        }
    }
    for (path, (template, materials)) in templates {
        let folder = output.join(path.rsplit('\\').next().unwrap_or(&path));
        std::fs::create_dir_all(&folder)?;
        let template_at = template.address()?;
        let pixel = cache
            .reference(template_at + 0x10, Some("pixl"))?
            .context("Template has no pixel shader")?;
        let pixel_at = pixel.address()?;
        let pixel_entry_points = cache
            .block(pixel_at + 0x04, 2)?
            .into_iter()
            .map(|row| Ok([cache.u8(row)?, cache.u8(row + 1)?]))
            .collect::<Result<Vec<_>>>()?;
        let pixel_shaders =
            crate::halo_reach::level::write_compiled_shaders(&cache, pixel_at, "pixel-", &folder)?;
        let vertex = cache
            .reference(template_at, Some("vtsh"))?
            .context("Template has no vertex shader")?;
        let vertex_at = vertex.address()?;
        let vertex_entry_points = cache
            .block(vertex_at + 0x04, 0x0C)?
            .into_iter()
            .map(|row| {
                cache
                    .block(row, 2)?
                    .into_iter()
                    .map(|types| Ok([cache.u8(types)?, cache.u8(types + 1)?]))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        let vertex_shaders = crate::halo_reach::level::write_compiled_shaders(
            &cache, vertex_at, "vertex-", &folder,
        )?;
        let names = |at: usize| -> Result<Vec<String>> {
            cache
                .block(template_at + at, 4)?
                .into_iter()
                .map(|row| cache.string_id(cache.u32(row)?))
                .collect()
        };
        std::fs::write(
            folder.join("shaders.json"),
            serde_json::to_string_pretty(&json!({
                "template": template,
                "materials": materials,
                "pixel_shader": pixel,
                "pixel_entry_points": pixel_entry_points,
                "pixel_shaders": pixel_shaders,
                "vertex_shader": vertex,
                "vertex_entry_points": vertex_entry_points,
                "vertex_shaders": vertex_shaders,
                "float_constants": names(0x48)?,
                "textures": names(0x6C)?,
            }))?,
        )?;
    }
    Ok(())
}
