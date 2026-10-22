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
        material::Material,
        model::{self, Primitive},
        native_material::{
            SKY_RADIANCE,
            sky::{SkyScale, SkySurface, surfaces},
        },
        object::Object as SourceObject,
        resource::Pages,
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
/// which the level's surfaces' light is a share of. Returns the sky objects, in draw order, and a
/// report.
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
    )?;
    let template = native.tag(templates.model, Some(MODEL_CLASS))?;
    let row = *template
        .array(16, 136, Some(MESH_CLASS))?
        .first()
        .context("The donor's sky model has no mesh")?;
    let mut objects = Vec::new();
    let mut models = Vec::new();
    for (index, chunk) in chunks(primitives)?.into_iter().enumerate() {
        let name = format!("sky{index}");
        let mut mesh = chunk_mesh(primitives, &chunk, offset, vertex_color)?;
        let (low, high) = bounds(mesh.positions.iter().copied());
        // The object sits at the center of its bounds, and its geometry is relative to it.
        let center: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
        for position in &mut mesh.positions {
            *position = sub(*position, center);
        }
        let used_stages = mesh
            .groups
            .iter()
            .map(|(key, _)| (*key, stages[key]))
            .collect::<BTreeMap<_, _>>();
        let mut encoded = geometry::encode_source_passes(&mesh, &template, row, &used_stages)?;
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
        let patches = add_streams(native, graph, &template, row, &name, &encoded)?;
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
        let world_low = std::array::from_fn(|k| low[k]);
        let world_high = std::array::from_fn(|k| high[k]);
        models.push(json!({
            "model": model,
            "vertices": mesh.positions.len(),
            "triangles": chunk.runs.iter().map(|(_, faces)| faces.len()).sum::<usize>(),
            "primitives": chunk.runs.iter().map(|(index, _)| index).collect::<Vec<_>>(),
            "surfaces": chunk
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
                .collect::<Result<Vec<_>>>()?,
            "position": center,
            "bounds": [world_low, world_high],
            "draws": records,
        }));
        objects.push(Object {
            reference: symbol,
            position: center,
            minimum: world_low,
            maximum: world_high,
            order: index as f32,
        });
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
            "templates": {
                "reference": format!("{:08X}", templates.reference),
                "model": format!("{:08X}", templates.model),
                "surface": format!("{:08X}", templates.surface),
                "shafts": format!("{:08X}", templates.shafts),
            },
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
        let mut header = native.tag(tag, None)?.0.clone();
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
