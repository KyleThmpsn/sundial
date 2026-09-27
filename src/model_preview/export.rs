//! Preview exports: a PNG writer for captured frames and a glTF 2.0 binary writer for the
//! posed geometry.
//!
//! Both encoders are written here rather than pulled in. Sundial emits exactly one PNG
//! variant and one single-mesh glB shape, so a codec crate would add a decoder surface and
//! a format matrix the preview never exercises.
//!
//! Gear materials go out baked. glTF cannot run the gear shader, so the shader's unlit result
//! is evaluated into standard metallic-roughness textures instead: dyed colour, occlusion,
//! roughness, metal, normal detail and emission. Alpha-clipped parts carry their coverage in
//! the colour's alpha. See `bake` for what cannot travel.
use super::{Model, shader};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Write};

mod bake;

const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Encodes RGBA8 pixels as a PNG byte stream.
pub(crate) fn png(pixels: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    image(pixels, width, height, 4)
}

/// Truecolour with alpha (4 channels) or without (3). Maps with no alpha go out as RGB,
/// a quarter less to deflate and to store.
fn image(pixels: &[u8], width: usize, height: usize, channels: usize) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("A PNG needs a width and height of at least one pixel".into());
    }
    let expected = width
        .checked_mul(height)
        .and_then(|count| count.checked_mul(channels))
        .ok_or("These image dimensions are too large to encode")?;
    if pixels.len() != expected {
        return Err(format!(
            "Expected {expected} bytes for {width}x{height}, got {}",
            pixels.len()
        ));
    }
    let wide = u32::try_from(width).map_err(|_| "The image width does not fit a PNG header")?;
    let tall = u32::try_from(height).map_err(|_| "The image height does not fit a PNG header")?;
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&wide.to_be_bytes());
    header.extend_from_slice(&tall.to_be_bytes());
    // Eight bits per channel, truecolour with or without alpha, deflate, adaptive filtering,
    // no interlace.
    let colour_type = if channels == 4 { 6 } else { 2 };
    header.extend_from_slice(&[8, colour_type, 0, 0, 0]);

    // Filter type 0 keeps every scanline verbatim. Predictors would only pay off on photographic
    // rows; preview frames are flat-shaded and deflate already carries the compression.
    let mut raw = Vec::with_capacity(expected + height);
    for row in pixels.chunks_exact(width * channels) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&raw)
        .map_err(|error| format!("Could not compress the image: {error}"))?;
    let data = encoder
        .finish()
        .map_err(|error| format!("Could not compress the image: {error}"))?;

    let mut out = Vec::with_capacity(SIGNATURE.len() + data.len() + 64);
    out.extend_from_slice(&SIGNATURE);
    chunk(&mut out, b"IHDR", &header)?;
    chunk(&mut out, b"IDAT", &data)?;
    chunk(&mut out, b"IEND", &[])?;
    Ok(out)
}

/// One PNG chunk: big-endian length, four-byte type, payload, then a CRC32 that covers the
/// type and the payload but not the length.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) -> Result<(), String> {
    let length = u32::try_from(data.len()).map_err(|_| "A PNG chunk exceeds the format limit")?;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
    Ok(())
}

const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const TRIANGLES: u32 = 4;

/// Encodes the model as a self-contained binary glTF (.glb).
pub(crate) fn glb(model: &Model, seconds: f32) -> Result<Vec<u8>, String> {
    if model.triangles.is_empty() || model.vertices.is_empty() {
        return Err("This model has no geometry to export".into());
    }
    if model
        .triangles
        .iter()
        .flatten()
        .any(|&corner| corner as usize >= model.vertices.len())
    {
        return Err("This model has triangles that point outside its vertex list".into());
    }
    let posed = model
        .animation
        .as_ref()
        .map(|animation| animation.vertices(model, seconds.rem_euclid(animation.duration())));
    let points: Vec<[f32; 3]> = posed
        .as_deref()
        .unwrap_or(&model.vertices)
        .iter()
        .copied()
        .map(upright)
        .collect();
    if points.len() != model.vertices.len() {
        return Err("The posed vertex list does not match the model".into());
    }

    let mut buffer = Buffer::default();
    let position = positions(&mut buffer, &points);
    let normal = normals(&mut buffer, model);
    let texcoord = texcoords(&mut buffer, model);
    // Indices stay narrow while they can: a glB of a small part is often mailed around.
    let short = points.len() <= usize::from(u16::MAX) + 1;

    let dyes = shader::dyes(model, seconds);
    let mut plates: BTreeMap<bake::Plate, Vec<usize>> = BTreeMap::new();
    let mut flat = Vec::new();
    for triangle in 0..model.triangles.len() {
        match bake::Plate::of(model, triangle) {
            Some(plate) => plates.entry(plate).or_default().push(triangle),
            None => flat.push(triangle),
        }
    }
    let mut layers = Vec::new();
    for (plate, triangles) in &plates {
        layers.extend(bake::bake(model, &dyes, *plate, triangles)?);
    }
    let encoded = encode_layers(&layers)?;

    let mut gallery = Gallery::default();
    let mut primitives = Vec::new();
    let mut materials = Vec::new();
    let mut strong = false;
    for (layer, images) in layers.iter().zip(encoded) {
        let corners: Vec<u32> = layer
            .triangles
            .iter()
            .flat_map(|&triangle| model.triangles[triangle])
            .collect();
        primitives.push(json!({
            "attributes": attributes(position, normal, texcoord),
            "indices": indices(&mut buffer, &corners, short),
            "material": materials.len(),
            "mode": TRIANGLES,
        }));
        let textures = Placed {
            color: gallery.add(&mut buffer, &images.color),
            channels: images
                .channels
                .as_deref()
                .map(|png| gallery.add(&mut buffer, png)),
            normal: images
                .normal
                .as_deref()
                .map(|png| gallery.add(&mut buffer, png)),
            emission: images
                .emission
                .as_deref()
                .map(|png| gallery.add(&mut buffer, png)),
        };
        let (material, boosted) = baked_material(layer, &textures);
        strong |= boosted;
        materials.push(material);
    }
    for (key, list) in grouped(model, &flat) {
        // A plate is only worth embedding for a part that has coordinates to read it with.
        let texture = match key.albedo.filter(|_| texcoord.is_some()) {
            Some(index) => Some(gallery.texture(&mut buffer, model, index)?),
            None => None,
        };
        primitives.push(json!({
            "attributes": attributes(position, normal, texcoord),
            "indices": indices(&mut buffer, &list, short),
            "material": materials.len(),
            "mode": TRIANGLES,
        }));
        let (material, boosted) = material(key, &dyes, texture);
        strong |= boosted;
        materials.push(material);
    }

    let mut document = json!({
        "asset": {"version": "2.0", "generator": "Sundial model preview"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": primitives}],
        "materials": materials,
        "accessors": buffer.accessors,
        "bufferViews": buffer.views,
        "buffers": [{"byteLength": buffer.bin.len()}],
    });
    if !gallery.textures.is_empty() {
        // An empty table is invalid glTF, so the sampler appears only with something to sample.
        // The rasterizer wraps its UVs and does not mip, so the sampler says exactly that.
        document["samplers"] =
            json!([{"magFilter": 9729, "minFilter": 9729, "wrapS": 10497, "wrapT": 10497}]);
        document["images"] = json!(gallery.images);
        document["textures"] = json!(gallery.textures);
    }
    if strong {
        // Optional rather than required: a reader without it still gets the right colour,
        // only dimmer where the game drives emission past full scale.
        document["extensionsUsed"] = json!([EMISSIVE_STRENGTH]);
    }
    container(&document, &buffer.bin)
}

const EMISSIVE_STRENGTH: &str = "KHR_materials_emissive_strength";

/// A baked layer's maps, encoded.
struct Encoded {
    color: Vec<u8>,
    channels: Option<Vec<u8>>,
    normal: Option<Vec<u8>>,
    emission: Option<Vec<u8>>,
}

/// Deflating a plate's worth of maps dominates the export, and every map is independent, so
/// they are compressed side by side.
fn encode_layers(layers: &[bake::Layer]) -> Result<Vec<Encoded>, String> {
    let mut jobs: Vec<(&[u8], [usize; 2], usize)> = Vec::new();
    for layer in layers {
        jobs.push((&layer.color, layer.size, 4));
        if let bake::Channels::Texture(bytes) = &layer.channels {
            jobs.push((bytes, layer.size, 3));
        }
        if let Some(bytes) = &layer.normal {
            jobs.push((bytes, layer.size, 3));
        }
        if let Some((bytes, _)) = &layer.emission {
            jobs.push((bytes, layer.size, 3));
        }
    }
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(16));
    let mut done: Vec<Result<Vec<u8>, String>> = Vec::with_capacity(jobs.len());
    for batch in jobs.chunks(threads) {
        std::thread::scope(|scope| {
            let running: Vec<_> = batch
                .iter()
                .map(|&(pixels, [width, height], channels)| {
                    scope.spawn(move || image(pixels, width, height, channels))
                })
                .collect();
            for handle in running {
                done.push(
                    handle
                        .join()
                        .unwrap_or_else(|_| Err("A texture encoder stopped unexpectedly".into())),
                );
            }
        });
    }
    let mut done = done.into_iter();
    let mut next = || {
        done.next()
            .unwrap_or_else(|| Err("A baked texture went missing".into()))
    };
    layers
        .iter()
        .map(|layer| {
            Ok(Encoded {
                color: next()?,
                channels: match layer.channels {
                    bake::Channels::Texture(_) => Some(next()?),
                    bake::Channels::Uniform { .. } => None,
                },
                normal: layer.normal.as_ref().map(|_| next()).transpose()?,
                emission: layer.emission.as_ref().map(|_| next()).transpose()?,
            })
        })
        .collect()
}

/// Texture-table indices of a baked layer's maps.
struct Placed {
    color: usize,
    channels: Option<usize>,
    normal: Option<usize>,
    emission: Option<usize>,
}

/// The material for a baked layer, and whether it needs the emissive-strength extension.
fn baked_material(layer: &bake::Layer, textures: &Placed) -> (Value, bool) {
    let mut pbr = json!({"baseColorTexture": {"index": textures.color}});
    let mut material = json!({
        // The preview lights both faces and gear meshes are not reliably closed.
        "doubleSided": true,
    });
    match (&layer.channels, textures.channels) {
        (bake::Channels::Uniform { roughness, metal }, _) => {
            pbr["roughnessFactor"] = json!(f32::from(*roughness) / 255.0);
            pbr["metallicFactor"] = json!(f32::from(*metal) / 255.0);
        }
        (bake::Channels::Texture(_), Some(index)) => {
            // One map serves both slots: occlusion reads red, the others read green and blue.
            pbr["metallicRoughnessTexture"] = json!({"index": index});
            material["occlusionTexture"] = json!({"index": index});
        }
        (bake::Channels::Texture(_), None) => {}
    }
    material["pbrMetallicRoughness"] = pbr;
    if let Some(index) = textures.normal {
        material["normalTexture"] = json!({"index": index});
    }
    let mut strong = false;
    if let (Some(index), Some((_, strength))) = (textures.emission, &layer.emission) {
        material["emissiveTexture"] = json!({"index": index});
        material["emissiveFactor"] = json!([1.0, 1.0, 1.0]);
        strong = emissive_strength(&mut material, *strength);
    }
    if layer.masked {
        material["alphaMode"] = json!("MASK");
        material["alphaCutoff"] = json!(0.5);
    }
    (material, strong)
}

/// glTF's emissive factor stops at one. The game drives some emission well past that, so the
/// rest rides on the strength extension. Returns whether the extension was needed.
fn emissive_strength(material: &mut Value, strength: f32) -> bool {
    if strength <= 1.0 || !strength.is_finite() {
        return false;
    }
    material["extensions"] = json!({EMISSIVE_STRENGTH: {"emissiveStrength": strength}});
    true
}

/// glTF is right-handed with Y up and -Z forward; the engine's geometry is right-handed with
/// Z up and +Y forward. This is the quarter turn about X between them, a proper rotation, so
/// triangle winding and normals survive it untouched.
fn upright(point: [f32; 3]) -> [f32; 3] {
    [finite(point[0]), finite(point[2]), -finite(point[1])]
}

/// JSON has no encoding for NaN or infinity, so a stray coordinate would otherwise land in the
/// document as `null` and make it unreadable.
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn channel(value: f32) -> f32 {
    finite(value).clamp(0.0, 1.0)
}

/// glTF asks for unit normals, and a normal buffer that failed to decode is left zeroed.
fn unit(normal: [f32; 3]) -> [f32; 3] {
    let length = normal.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
    if length > 1e-6 {
        normal.map(|axis| axis / length)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// The binary chunk under construction, with the view and accessor tables that address it.
#[derive(Default)]
struct Buffer {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Buffer {
    /// Every view starts on a four-byte boundary, which satisfies glTF's alignment rule for
    /// any component type without tracking each accessor's own stride.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        let mut view = json!({
            "buffer": 0,
            "byteOffset": self.bin.len(),
            "byteLength": bytes.len(),
        });
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        self.bin.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    fn accessor(&mut self, view: usize, component: u32, kind: &str, count: usize) -> usize {
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": component,
            "count": count,
            "type": kind,
        }));
        self.accessors.len() - 1
    }
}

fn positions(buffer: &mut Buffer, points: &[[f32; 3]]) -> usize {
    let bytes: Vec<u8> = points
        .iter()
        .flatten()
        .flat_map(|axis| axis.to_le_bytes())
        .collect();
    let view = buffer.view(&bytes, Some(ARRAY_BUFFER));
    let accessor = buffer.accessor(view, FLOAT, "VEC3", points.len());
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    // glTF requires bounds on POSITION so an importer can frame the scene without a scan.
    buffer.accessors[accessor]["min"] = json!(low.map(finite));
    buffer.accessors[accessor]["max"] = json!(high.map(finite));
    accessor
}

fn normals(buffer: &mut Buffer, model: &Model) -> Option<usize> {
    // A posed export carries animated positions but bind-pose normals, which no longer belong
    // to the surface. The rasterizer drops vertex normals under animation for the same reason,
    // and importers recompute them from the triangles.
    if model.animation.is_some() || model.normals.len() != model.vertices.len() {
        return None;
    }
    let bytes: Vec<u8> = model
        .normals
        .iter()
        .flat_map(|&normal| unit(upright(normal)))
        .flat_map(f32::to_le_bytes)
        .collect();
    let view = buffer.view(&bytes, Some(ARRAY_BUFFER));
    Some(buffer.accessor(view, FLOAT, "VEC3", model.normals.len()))
}

fn texcoords(buffer: &mut Buffer, model: &Model) -> Option<usize> {
    if model.uvs.is_empty() {
        return None;
    }
    // The rasterizer reads a missing UV as the origin, so a short list pads the same way.
    let bytes: Vec<u8> = (0..model.vertices.len())
        .flat_map(|vertex| {
            model
                .uvs
                .get(vertex)
                .copied()
                .unwrap_or_default()
                .map(finite)
        })
        .flat_map(f32::to_le_bytes)
        .collect();
    let view = buffer.view(&bytes, Some(ARRAY_BUFFER));
    Some(buffer.accessor(view, FLOAT, "VEC2", model.vertices.len()))
}

fn indices(buffer: &mut Buffer, list: &[u32], short: bool) -> usize {
    let bytes: Vec<u8> = if short {
        // Only taken when the vertex count fits, so the narrow form cannot truncate.
        list.iter()
            .flat_map(|&corner| (corner as u16).to_le_bytes())
            .collect()
    } else {
        list.iter()
            .flat_map(|&corner| corner.to_le_bytes())
            .collect()
    };
    let view = buffer.view(&bytes, Some(ELEMENT_ARRAY_BUFFER));
    let component = if short { UNSIGNED_SHORT } else { UNSIGNED_INT };
    buffer.accessor(view, component, "SCALAR", list.len())
}

fn attributes(position: usize, normal: Option<usize>, texcoord: Option<usize>) -> Value {
    // Every primitive shares one vertex table and differs only in its index range, so a part
    // welded across several materials does not duplicate its vertices in the file.
    let mut attributes = json!({"POSITION": position});
    if let Some(normal) = normal {
        attributes["NORMAL"] = json!(normal);
    }
    if let Some(texcoord) = texcoord {
        attributes["TEXCOORD_0"] = json!(texcoord);
    }
    attributes
}

/// What splits the parts that are not baked into primitives: emissive panels, and parts with
/// no colour plate or no texture coordinates.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    albedo: Option<usize>,
    dye: u8,
    /// Bit patterns: the flat emissive colour is only ever compared for equality.
    constant: Option<[u32; 3]>,
}

fn key_of(model: &Model, triangle: usize) -> Key {
    Key {
        albedo: model
            .triangle_textures
            .get(triangle)
            .copied()
            .flatten()
            .filter(|_| !model.uvs.is_empty()),
        dye: model.triangle_dyes.get(triangle).copied().unwrap_or(0),
        constant: model
            .triangle_constant
            .get(triangle)
            .copied()
            .flatten()
            .map(|colour| colour.map(f32::to_bits)),
    }
}

/// Triangles sorted into runs of one material, each run carrying its own corner list.
fn grouped(model: &Model, triangles: &[usize]) -> Vec<(Key, Vec<u32>)> {
    let mut order = triangles.to_vec();
    order.sort_by_key(|&triangle| key_of(model, triangle));
    let mut groups: Vec<(Key, Vec<u32>)> = Vec::new();
    for triangle in order {
        let key = key_of(model, triangle);
        let corners = model.triangles[triangle];
        match groups.last_mut() {
            Some((last, list)) if *last == key => list.extend_from_slice(&corners),
            _ => groups.push((key, corners.to_vec())),
        }
    }
    groups
}

/// The image and texture tables, keyed by the model's own texture slot so a colour plate shared
/// by several parts is encoded and embedded once.
#[derive(Default)]
struct Gallery {
    images: Vec<Value>,
    textures: Vec<Value>,
    embedded: BTreeMap<usize, usize>,
}

impl Gallery {
    fn texture(
        &mut self,
        buffer: &mut Buffer,
        model: &Model,
        index: usize,
    ) -> Result<usize, String> {
        if let Some(&existing) = self.embedded.get(&index) {
            return Ok(existing);
        }
        let plate = model
            .textures
            .get(index)
            .ok_or("A part refers to a texture the model did not load")?;
        let encoded = png(&plate.rgba, plate.size[0], plate.size[1]).map_err(|error| {
            format!("Texture 0x{:08X} could not be encoded: {error}", plate.tag)
        })?;
        let texture = self.add(buffer, &encoded);
        self.embedded.insert(index, texture);
        Ok(texture)
    }

    /// Embeds an encoded PNG and returns its texture index.
    fn add(&mut self, buffer: &mut Buffer, encoded: &[u8]) -> usize {
        let view = buffer.view(encoded, None);
        self.images
            .push(json!({"bufferView": view, "mimeType": "image/png"}));
        self.textures
            .push(json!({"sampler": 0, "source": self.images.len() - 1}));
        self.textures.len() - 1
    }
}

/// The material for an unbaked group, and whether it needs the emissive-strength extension.
fn material(key: Key, dyes: &[Option<shader::Dye>; 6], texture: Option<usize>) -> (Value, bool) {
    let mut material = json!({
        // The preview lights both faces and gear meshes are not reliably closed.
        "doubleSided": true,
    });
    let Some(constant) = key.constant else {
        // No plate to bake from, so the shader's own fallback surface stands in rather than
        // an invented metalness and roughness.
        material["pbrMetallicRoughness"] = json!({
            "baseColorFactor": base_color(key, dyes),
            "metallicFactor": 0.0,
            "roughnessFactor": 0.6,
        });
        return (material, false);
    };
    // An emissive panel is unlit: the constant tints the plate's colour, and the plate's
    // one-bit alpha cuts the gaps between its segments. Black base colour keeps the lit
    // surface from adding to it.
    let colour = constant.map(|bits| finite(f32::from_bits(bits)).max(0.0));
    let strength = colour.iter().copied().fold(1.0_f32, f32::max);
    let mut pbr = json!({
        "baseColorFactor": [0.0, 0.0, 0.0, 1.0],
        "metallicFactor": 0.0,
        "roughnessFactor": 1.0,
    });
    material["emissiveFactor"] = json!(colour.map(|value| value / strength));
    if let Some(index) = texture {
        pbr["baseColorTexture"] = json!({"index": index});
        material["emissiveTexture"] = json!({"index": index});
        material["alphaMode"] = json!("MASK");
        material["alphaCutoff"] = json!(0.5);
    }
    material["pbrMetallicRoughness"] = pbr;
    let strong = emissive_strength(&mut material, strength);
    (material, strong)
}

/// The flat colour the rasterizer draws where no colour plate is bound: the dye's albedo, else
/// its untextured fill. Both are already linear, which is what `baseColorFactor` expects.
fn base_color(key: Key, dyes: &[Option<shader::Dye>; 6]) -> [f32; 4] {
    let rgb = if let Some(dye) = dyes.get(usize::from(key.dye)).and_then(Option::as_ref) {
        dye.surface.albedo.map(channel)
    } else {
        [205.0_f32, 216.0, 230.0].map(|value| shader::linear(value / 255.0))
    };
    [rgb[0], rgb[1], rgb[2], 1.0]
}

/// The glB envelope: a header, then a JSON chunk and a BIN chunk, each padded to four bytes with
/// the filler its chunk type requires so a reader can seek straight to the binary payload.
fn container(document: &Value, bin: &[u8]) -> Result<Vec<u8>, String> {
    let mut json = serde_json::to_vec(document)
        .map_err(|error| format!("Could not write the glTF document: {error}"))?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let padding = (4 - bin.len() % 4) % 4;
    let total = u32::try_from(28 + json.len() + bin.len() + padding)
        .map_err(|_| "This model is too large for a single glB file")?;
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    // Chunk lengths were bounded by the total above, so neither cast can lose a byte.
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&((bin.len() + padding) as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(bin);
    out.resize(out.len() + padding, 0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_preview::texture::Texture;
    use std::io::Write;

    /// Walks the chunk stream, verifying each checksum, and hands back the payloads in order.
    fn chunks(encoded: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut chunks = Vec::new();
        let mut at = 8;
        while at + 12 <= encoded.len() {
            let length =
                u32::from_be_bytes(encoded[at..at + 4].try_into().expect("length")) as usize;
            let body = &encoded[at + 4..at + 8 + length];
            let stored = u32::from_be_bytes(
                encoded[at + 8 + length..at + 12 + length]
                    .try_into()
                    .expect("checksum"),
            );
            let mut crc = flate2::Crc::new();
            crc.update(body);
            assert_eq!(crc.sum(), stored, "chunk checksum at {at}");
            chunks.push((
                String::from_utf8_lossy(&body[..4]).into_owned(),
                body[4..].to_vec(),
            ));
            at += 12 + length;
        }
        assert_eq!(at, encoded.len(), "trailing bytes after the last chunk");
        chunks
    }

    #[test]
    fn png_signs_every_chunk_and_keeps_them_in_order() {
        let encoded = png(&[9u8; 2 * 3 * 4], 2, 3).expect("encode");
        assert_eq!(encoded[..8], SIGNATURE);
        let chunks = chunks(&encoded);
        let kinds: Vec<&str> = chunks.iter().map(|(kind, _)| kind.as_str()).collect();
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
        assert_eq!(chunks[0].1, [0u8, 0, 0, 2, 0, 0, 0, 3, 8, 6, 0, 0, 0]);
        assert!(chunks[2].1.is_empty());
    }

    #[test]
    fn png_deflates_each_row_behind_a_zero_filter_byte() {
        let pixels: Vec<u8> = (0u8..16).collect();
        let encoded = png(&pixels, 2, 2).expect("encode");
        let chunks = chunks(&encoded);
        let mut decoder = flate2::write::ZlibDecoder::new(Vec::new());
        decoder.write_all(&chunks[1].1).expect("inflate");
        let raw = decoder.finish().expect("inflate");
        let mut expected: Vec<u8> = vec![0];
        expected.extend_from_slice(&pixels[..8]);
        expected.push(0);
        expected.extend_from_slice(&pixels[8..]);
        assert_eq!(raw, expected);
    }

    #[test]
    fn png_rejects_a_buffer_that_does_not_match_its_size() {
        assert!(png(&[0u8; 3], 1, 1).is_err());
        assert!(png(&[0u8; 4], 0, 1).is_err());
        assert!(png(&[0u8; 8], 1, 1).is_err());
    }

    /// Two triangles lying in the engine's XZ plane, one textured and one bare, so the export
    /// has to split them into separate primitives.
    fn quad() -> Model {
        Model {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 2.0],
                [0.0, 0.0, 2.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            normals: vec![[0.0, -1.0, 0.0]; 4],
            triangle_textures: vec![Some(0), None],
            textures: vec![Texture {
                tag: 0x1234_5678,
                size: [1, 1],
                rgba: vec![255, 0, 0, 255],
            }],
            ..Default::default()
        }
    }

    /// Splits a glB into its document and its binary chunk, checking the envelope on the way.
    fn parse(bytes: &[u8]) -> (Value, Vec<u8>) {
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().expect("version")),
            2
        );
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().expect("length")) as usize,
            bytes.len()
        );
        let json_length =
            u32::from_le_bytes(bytes[12..16].try_into().expect("json length")) as usize;
        assert_eq!(&bytes[16..20], b"JSON");
        assert_eq!(
            json_length % 4,
            0,
            "the JSON chunk is not four-byte aligned"
        );
        let document: Value =
            serde_json::from_slice(&bytes[20..20 + json_length]).expect("the document parses");
        let at = 20 + json_length;
        let bin_length =
            u32::from_le_bytes(bytes[at..at + 4].try_into().expect("bin length")) as usize;
        assert_eq!(&bytes[at + 4..at + 8], b"BIN\0");
        assert_eq!(bin_length % 4, 0, "the BIN chunk is not four-byte aligned");
        assert_eq!(at + 8 + bin_length, bytes.len());
        (document, bytes[at + 8..].to_vec())
    }

    #[test]
    fn glb_writes_one_primitive_per_material_group() {
        let (document, bin) = parse(&glb(&quad(), 0.0).expect("export"));
        let primitives = document["meshes"][0]["primitives"]
            .as_array()
            .expect("primitives");
        assert_eq!(primitives.len(), 2);
        assert_eq!(document["materials"].as_array().map(Vec::len), Some(2));
        // The groups share one vertex table and differ only in their index accessor.
        assert_eq!(
            primitives[0]["attributes"], primitives[1]["attributes"],
            "primitives should share the vertex accessors"
        );
        assert_ne!(primitives[0]["indices"], primitives[1]["indices"]);
        for primitive in primitives {
            let accessor = primitive["indices"].as_u64().expect("indices") as usize;
            assert_eq!(document["accessors"][accessor]["count"], 3);
            assert_eq!(
                document["accessors"][accessor]["componentType"],
                UNSIGNED_SHORT
            );
        }
        assert_eq!(document["accessors"][0]["count"], 4);
        assert_eq!(document["accessors"][0]["type"], "VEC3");
        assert_eq!(document["accessors"].as_array().map(Vec::len), Some(5));
        // The buffer stops before the chunk's alignment filler, which glTF allows to be up to
        // three bytes longer than the buffer it carries.
        let declared = document["buffers"][0]["byteLength"]
            .as_u64()
            .expect("byteLength") as usize;
        assert!(declared <= bin.len() && bin.len() - declared < 4);
    }

    #[test]
    fn glb_embeds_each_used_plate_as_a_png() {
        let (document, _) = parse(&glb(&quad(), 0.0).expect("export"));
        assert_eq!(document["images"].as_array().map(Vec::len), Some(1));
        assert_eq!(document["images"][0]["mimeType"], "image/png");
        assert_eq!(document["textures"][0]["source"], 0);
        assert_eq!(document["samplers"].as_array().map(Vec::len), Some(1));
        let materials = document["materials"].as_array().expect("materials");
        let textured = materials
            .iter()
            .filter(|material| !material["pbrMetallicRoughness"]["baseColorTexture"].is_null())
            .count();
        assert_eq!(textured, 1);
        // The bare group falls back to the rasterizer's untextured fill, in linear space.
        let bare = materials
            .iter()
            .find(|material| material["pbrMetallicRoughness"]["baseColorTexture"].is_null())
            .expect("an untextured group");
        assert_eq!(
            bare["pbrMetallicRoughness"]["baseColorFactor"]
                .as_array()
                .map(Vec::len),
            Some(4)
        );
    }

    /// One dyed, alpha-clipped quad with a gearstack and a normal map, the way gear arrives.
    fn dyed() -> Model {
        let flat = |rgba: [u8; 4]| Texture {
            tag: 0,
            size: [2, 2],
            rgba: rgba.repeat(4),
        };
        let dye = shader::Dye {
            surface: crate::weapon_dyes::material::Surface {
                albedo: [1.0, 0.0, 0.0],
                worn_albedo: [1.0, 0.0, 0.0],
                params: [0.0, 0.0, 0.0, 1.0],
                worn_params: [0.0, 0.0, 0.0, 1.0],
                wear: [0.0, 1.0, 0.0, 1.0],
                iridescence: -1.0,
                ..Default::default()
            },
            detail: None,
            transform: [1.0, 1.0, 0.0, 0.0],
            normal: None,
            normal_transform: [1.0, 1.0, 0.0, 0.0],
            vectors: [[0.0; 4]; 27],
        };
        Model {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            normals: vec![[0.0, -1.0, 0.0]; 4],
            // Grey plate, a dyeable gearstack with no emission, and a map leaning along V
            // whose blue channel carries half occlusion.
            textures: vec![
                flat([128, 128, 128, 255]),
                flat([255, 128, 32, 255]),
                flat([128, 200, 128, 255]),
            ],
            triangle_textures: vec![Some(0); 2],
            triangle_gearstacks: vec![Some(1); 2],
            triangle_normals: vec![Some(2); 2],
            triangle_dyes: vec![0; 2],
            triangle_clip: vec![true; 2],
            dyes: [Some(dye), None, None, None, None, None],
            ..Default::default()
        }
    }

    /// The PNG behind a material's texture reference, found through the document's own tables.
    fn embedded<'a>(document: &Value, bin: &'a [u8], slot: &Value) -> &'a [u8] {
        let index = |value: &Value| value.as_u64().expect("an index") as usize;
        let image = index(&document["textures"][index(&slot["index"])]["source"]);
        let view = &document["bufferViews"][index(&document["images"][image]["bufferView"])];
        let start = index(&view["byteOffset"]);
        &bin[start..start + index(&view["byteLength"])]
    }

    /// The pixels of one embedded texture.
    fn texels(document: &Value, bin: &[u8], slot: &Value, channels: usize) -> Vec<u8> {
        let chunks = chunks(embedded(document, bin, slot));
        let width = u32::from_be_bytes(chunks[0].1[..4].try_into().expect("width")) as usize;
        let data = &chunks
            .iter()
            .find(|(kind, _)| kind == "IDAT")
            .expect("image data")
            .1;
        let mut decoder = flate2::write::ZlibDecoder::new(Vec::new());
        decoder.write_all(data).expect("inflate");
        let raw = decoder.finish().expect("inflate");
        raw.chunks_exact(1 + width * channels)
            .flat_map(|row| row[1..].to_vec())
            .collect()
    }

    #[test]
    fn glb_bakes_the_gear_shader_into_standard_maps() {
        let (document, bin) = parse(&glb(&dyed(), 0.0).expect("export"));
        let materials = document["materials"].as_array().expect("materials");
        assert_eq!(materials.len(), 1);
        let material = &materials[0];
        let pbr = &material["pbrMetallicRoughness"];
        assert!(!pbr["metallicRoughnessTexture"].is_null());
        assert_eq!(
            material["occlusionTexture"],
            pbr["metallicRoughnessTexture"]
        );
        assert!(!material["normalTexture"].is_null());
        assert_eq!(material["alphaMode"], "MASK");
        // The plate is grey, and the dye paints it red.
        let colour = texels(&document, &bin, &pbr["baseColorTexture"], 4);
        assert!(
            colour[0] > colour[1] + 100,
            "the dye should tint the plate: {:?}",
            &colour[..4]
        );
        assert_eq!(colour[3], 255, "full coverage stays opaque");
        // The map leans along increasing V, which glTF reads as down the image.
        let normal = texels(&document, &bin, &material["normalTexture"], 3);
        assert!(
            normal[1] < 128,
            "green should be mirrored for glTF: {:?}",
            &normal[..3]
        );
        // Occlusion comes from the map's blue channel, and the dye asks for full metal.
        let channels = texels(&document, &bin, &pbr["metallicRoughnessTexture"], 3);
        assert!(channels[0] < 200, "occlusion: {:?}", &channels[..3]);
        assert_eq!(channels[2], 255);
    }

    #[test]
    fn glb_keeps_an_emissive_panel_unlit_and_at_full_strength() {
        let mut model = quad();
        model.triangle_constant = vec![Some([3.0, 1.5, 0.0]), None];
        let (document, _) = parse(&glb(&model, 0.0).expect("export"));
        let panel = document["materials"]
            .as_array()
            .expect("materials")
            .iter()
            .find(|material| !material["emissiveFactor"].is_null())
            .expect("an emissive panel");
        assert_eq!(
            panel["pbrMetallicRoughness"]["baseColorFactor"],
            json!([0.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(panel["emissiveFactor"], json!([1.0, 0.5, 0.0]));
        assert_eq!(
            panel["extensions"][EMISSIVE_STRENGTH]["emissiveStrength"],
            json!(3.0)
        );
        assert_eq!(panel["alphaMode"], "MASK");
        assert_eq!(document["extensionsUsed"], json!([EMISSIVE_STRENGTH]));
    }

    /// Exports two real weapons and writes every baked map beside the raw colour plates, so a
    /// reader can see the dyes arrive in the file. Age-Old Bond wears two dyed finishes and
    /// Better Devils carries emissive panels.
    #[test]
    #[ignore = "requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT"]
    fn real_weapons_export_their_dyed_materials() {
        let setting = |name: &str| {
            std::path::PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name}")))
        };
        let packages = setting("SUNDIAL_PREVIEW_PACKAGES");
        let out = setting("SUNDIAL_PROBE_OUT");
        std::fs::create_dir_all(&out).expect("output folder");
        let catalog = crate::investment::InvestmentCatalog::load(
            packages.parent().expect("install folder"),
            false,
            |_| {},
        )
        .expect("catalog");
        let mut report = String::new();
        for name in ["Age-Old Bond", "Better Devils"] {
            let donor = catalog
                .weapon_donors()
                .into_iter()
                .find(|donor| donor.name == name)
                .expect("donor");
            let loadout = catalog.preview_loadout(donor.hash).expect("loadout");
            let appearance = catalog.preview_appearance(&loadout);
            let model = crate::model_preview::weapon::load_reported(
                &packages,
                &appearance,
                &crate::model_preview::Load::default(),
            )
            .expect("model");
            let slug = name.to_lowercase().replace(' ', "-");
            let plates: std::collections::BTreeSet<usize> =
                model.triangle_textures.iter().flatten().copied().collect();
            for plate in plates {
                let texture = &model.textures[plate];
                let raw = png(&texture.rgba, texture.size[0], texture.size[1]).expect("raw");
                std::fs::write(out.join(format!("{slug}-raw-{plate}.png")), raw).expect("raw");
            }
            let started = std::time::Instant::now();
            let bytes = glb(&model, 0.0).expect("export");
            let elapsed = started.elapsed();
            std::fs::write(out.join(format!("{slug}.glb")), &bytes).expect("write glb");
            let (document, bin) = parse(&bytes);
            let materials = document["materials"].as_array().expect("materials");
            report.push_str(&format!(
                "{name}: {} bytes in {elapsed:?}, {} materials, {} images, {} triangles\n",
                bytes.len(),
                materials.len(),
                document["images"].as_array().map_or(0, Vec::len),
                model.triangles.len(),
            ));
            for (index, material) in materials.iter().enumerate() {
                report.push_str(&format!("  material {index}: {material}\n"));
                let pbr = &material["pbrMetallicRoughness"];
                for (slot, reference) in [
                    ("color", &pbr["baseColorTexture"]),
                    ("channels", &pbr["metallicRoughnessTexture"]),
                    ("normal", &material["normalTexture"]),
                    ("emission", &material["emissiveTexture"]),
                ] {
                    if !reference.is_null() {
                        let map = embedded(&document, &bin, reference);
                        std::fs::write(out.join(format!("{slug}-{index}-{slot}.png")), map)
                            .expect("write map");
                    }
                }
            }
        }
        std::fs::write(out.join("export-report.txt"), &report).expect("write report");
        println!("{report}");
    }

    #[test]
    fn glb_stands_the_engines_up_axis_upright() {
        assert_eq!(upright([1.0, 2.0, 3.0]), [1.0, 3.0, -2.0]);
        let (document, _) = parse(&glb(&quad(), 0.0).expect("export"));
        let bounds = |name: &str| -> Vec<f64> {
            document["accessors"][0][name]
                .as_array()
                .expect("bounds")
                .iter()
                .map(|value| value.as_f64().expect("a finite bound"))
                .collect()
        };
        // Engine Z spans 0..2 and becomes the glTF up axis; engine Y is flat and becomes -Z.
        assert_eq!(bounds("min"), [0.0, 0.0, 0.0]);
        assert_eq!(bounds("max"), [1.0, 2.0, 0.0]);
    }

    #[test]
    fn glb_rejects_geometry_it_cannot_address() {
        assert!(glb(&Model::default(), 0.0).is_err());
        let broken = Model {
            vertices: vec![[0.0; 3]],
            triangles: vec![[0, 1, 2]],
            ..Default::default()
        };
        assert!(glb(&broken, 0.0).is_err());
    }
}
