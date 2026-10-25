use super::{
    cache::{Cache, Tag},
    resource::Pages,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{collections::BTreeMap, fs::File, path::Path};
pub(super) mod array;
pub(super) mod lookup;

#[derive(Clone, Debug, Serialize)]
pub struct Mapping {
    pub role: String,
    pub bitmap: Option<Tag>,
    pub transform: [f32; 4],
    pub frame: usize,
    /// The sampler address mode byte stored after the frame.
    pub address: u8,
    /// The sampler filter mode, the low four bits of the byte after the address mode, in the order
    /// of TagTool's Gen3 texture constants: 0 trilinear, 1 point, 2 bilinear, 3 through 6 anisotropic.
    /// Across the eight converted maps' level materials the byte reads 0xA0, 0xA2 and 0xA4 for
    /// most maps, and 0x80, 0xB0, 0xC0 and 0x40 with the same low values. What the high bits
    /// select was not traced.
    pub filter: u8,
    pub functions: Vec<Function>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Function {
    pub kind: u32,
    pub input: String,
    pub range: String,
    pub period: f32,
    pub data: Vec<u8>,
}
impl Mapping {
    /// Animated frame selectors replace the stored index. Static exports use the
    /// first image until a supported native input supplies the selected frame.
    /// A single-image bitmap has only one possible stored surface.
    pub fn static_frame(&self, cache: &Cache) -> Result<usize> {
        let bitmap = self.bitmap.as_ref().context("Missing mapped bitmap")?;
        let images = cache.block(bitmap.address()? + 124, 56)?.len();
        ensure!(images > 0, "Mapped bitmap has no images");
        Ok(
            if images == 1 || self.functions.iter().any(|function| function.kind == 7) {
                0
            } else {
                self.frame
            },
        )
    }

    /// The checked MCC decimal frame function maps its named digit to ten frames.
    /// Other functions remain in the source receipt without a runtime claim.
    pub fn ammunition_place(&self, cache: &Cache) -> Result<Option<u32>> {
        let [function] = self.functions.as_slice() else {
            return Ok(None);
        };
        const DECIMAL: [u8; 32] = [
            0, 0x34, 0, 0, 0, 0, 0, 0, 0, 0, 0x20, 0x41, 0, 0, 0, 0, 0, 0, 0, 0, 0x48, 0xe1, 0xfa,
            0x3e, 0, 0, 0, 0x3f, 0, 0, 0, 0,
        ];
        if function.kind != 7 || !function.range.is_empty() || function.data != DECIMAL {
            return Ok(None);
        }
        let place = match function.input.as_str() {
            "primary_ammunition_ones" => 1,
            "primary_ammunition_tens" => 10,
            _ => return Ok(None),
        };
        let bitmap = self.bitmap.as_ref().context("Decimal bitmap")?;
        let images = cache.block(bitmap.address()? + 124, 56)?.len();
        ensure!(
            matches!(images, 1 | 10),
            "Decimal frame function requires one static image or ten digit images"
        );
        // A one-image display plate can carry the same selector as digit art.
        // It always resolves to its sole surface and needs no runtime input.
        Ok((images == 10).then_some(place))
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Property {
    pub template: Tag,
    pub constants: BTreeMap<String, [f32; 4]>,
    /// Named bits from the MCC property Boolean mask, separate from real parameter rows.
    pub booleans: BTreeMap<String, bool>,
    pub textures: Vec<Mapping>,
    pub functions: Vec<Function>,
    /// Constant row names and overlay indices selected by the compiled real-parameter routes.
    pub overlays: BTreeMap<String, Vec<usize>>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Decal {
    pub layer: u16,
    /// The cached sprite offset followed by scale. Projection clipping uses the original UV.
    pub sprite: [f32; 4],
    pub render_pass: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Material {
    pub decal: Option<Decal>,
    pub tag: Option<Tag>,
    pub options: BTreeMap<String, String>,
    pub properties: Vec<Property>,
}

pub fn read(c: &Cache, tag: Option<Tag>) -> Result<Material> {
    let mut result = Material {
        decal: None,
        tag: tag.clone(),
        options: BTreeMap::new(),
        properties: Vec::new(),
    };
    let Some(tag) = tag else { return Ok(result) };
    let at = tag.address()?;
    read_at(c, &mut result, at)?;
    Ok(result)
}

pub(super) fn embedded(c: &Cache, owner: Tag, at: usize) -> Result<Material> {
    let mut result = Material {
        decal: None,
        tag: Some(owner),
        options: BTreeMap::new(),
        properties: Vec::new(),
    };
    read_at(c, &mut result, at)?;
    Ok(result)
}

fn read_at(c: &Cache, result: &mut Material, at: usize) -> Result<()> {
    let definition = c
        .reference(at, Some("rmdf"))?
        .context("Material has no definition")?;
    let categories = c.block(definition.address()? + 16, 24)?;
    for (i, row) in c.block(at + 32, 2)?.into_iter().enumerate() {
        let option = c.i16(row)?;
        if option < 0 {
            continue;
        }
        let category = *categories
            .get(i)
            .context("Shader category outside definition")?;
        let options = c.block(category + 4, 28)?;
        let selected = *options
            .get(option as usize)
            .context("Shader option outside category")?;
        let name = c.string_id(c.u32(category)?)?;
        ensure!(
            result
                .options
                .insert(name, c.string_id(c.u32(selected)?)?)
                .is_none(),
            "Duplicate shader category"
        );
    }
    for row in c.block(at + 56, 180)? {
        let template = c
            .reference(row, Some("rmt2"))?
            .context("Missing shader template")?;
        let args = c
            .block(template.address()? + 72, 4)?
            .into_iter()
            .map(|r| c.string_id(c.u32(r)?))
            .collect::<Result<Vec<_>>>()?;
        let roles = c
            .block(template.address()? + 108, 4)?
            .into_iter()
            .map(|r| c.string_id(c.u32(r)?))
            .collect::<Result<Vec<_>>>()?;
        let values = c
            .block(row + 28, 16)?
            .into_iter()
            .map(|r| c.floats::<4>(r))
            .collect::<Result<Vec<_>>>()?;
        ensure!(values.len() >= args.len(), "Missing material constants");
        let constants = args
            .iter()
            .cloned()
            .zip(values.iter().copied())
            .collect::<BTreeMap<_, _>>();
        let boolean_names = c.block(template.address()? + 0x60, 4)?;
        ensure!(
            boolean_names.len() <= 32,
            "Material Boolean names exceed mask width"
        );
        let boolean_mask = c.u32(row + 0x34)?;
        let booleans = boolean_names
            .into_iter()
            .enumerate()
            .map(|(index, name)| Ok((c.string_id(c.u32(name)?)?, boolean_mask & (1 << index) != 0)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut textures = Vec::new();
        let functions = c
            .block(row + 0x5c, 36)?
            .into_iter()
            .map(|at| {
                let length = usize::try_from(c.u32(at + 0x10)?)?;
                ensure!(
                    length <= 1024 * 1024,
                    "Material function exceeds the source limit"
                );
                Ok(Function {
                    kind: c.u32(at)?,
                    input: c.string_id(c.u32(at + 4)?)?,
                    range: c.string_id(c.u32(at + 8)?)?,
                    period: c.f32(at + 12)?,
                    data: if length == 0 {
                        Vec::new()
                    } else {
                        c.meta(c.expand(c.u32(at + 0x1c)?)?, length)?.to_vec()
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        // MCC 00262840 uses routing byte two as the overlay index and byte three
        // as the destination float4 row. The u16 destination is the later GPU
        // binding, not an index into the named source constants.
        let routes = c.block(row + 0x50, 4)?;
        let mut overlays = BTreeMap::<String, Vec<usize>>::new();
        for pass in c.block(row + 0x44, 6)? {
            for offset in [2, 4] {
                let packed = usize::from(c.u16(pass + offset)?);
                let first = packed & 1023;
                let count = packed >> 10;
                for &route in routes
                    .get(first..first + count)
                    .context("Material real-parameter route range outside table")?
                {
                    let function = usize::from(c.u8(route + 2)?);
                    ensure!(
                        function < functions.len(),
                        "Material route overlay outside table"
                    );
                    let name = args
                        .get(usize::from(c.u8(route + 3)?))
                        .context("Material route constant outside template")?;
                    let selected = overlays.entry(name.clone()).or_default();
                    if !selected.contains(&function) {
                        selected.push(function);
                    }
                }
            }
        }
        let maps = c.block(row + 16, 24)?;
        ensure!(maps.len() >= roles.len(), "Missing shader texture slots");
        for (i, mapping) in maps.into_iter().enumerate() {
            let role = roles
                .get(i)
                .with_context(|| format!("Unnamed material texture slot {i}"))?
                .clone();
            // Template argument names are authoritative. The map's packed tiling
            // selector is retained by the source metadata, not guessed as a UV slot.
            let transform = constants.get(&role).copied().unwrap_or([1., 1., 0., 0.]);
            let packed = usize::from(c.u16(mapping + 22)?);
            let first = packed & 1023;
            let count = packed >> 10;
            let selected = functions
                .get(first..first + count)
                .context("Texture animation range outside material functions")?;
            let frame = c.i16(mapping + 16)?;
            ensure!(frame >= 0, "Negative source bitmap frame");
            textures.push(Mapping {
                role,
                bitmap: c.reference(mapping, Some("bitm"))?,
                transform,
                frame: usize::try_from(frame)?,
                address: c.u8(mapping + 18)?,
                filter: c.u8(mapping + 19)? & 0x0F,
                functions: selected.to_vec(),
            });
        }
        result.properties.push(Property {
            template,
            constants,
            booleans,
            textures,
            functions,
            overlays,
        });
    }
    Ok(())
}

impl Material {
    pub fn mapping(&self, roles: &[&str]) -> Option<&Mapping> {
        self.properties
            .first()?
            .textures
            .iter()
            .find(|m| m.bitmap.is_some() && roles.contains(&m.role.as_str()))
    }
    pub fn constant(&self, name: &str) -> Option<[f32; 4]> {
        self.properties.first()?.constants.get(name).copied()
    }
    /// Whether the material is a glass shader (`rmgl`). Reach always blends glass by an opacity of
    /// its own, which its options do not name.
    pub fn glass(&self) -> bool {
        self.tag.as_ref().is_some_and(|tag| tag.group == "rmgl")
    }
    /// Whether the material is a water shader (`rmw `). Reach draws water over the refracted scene
    /// behind it, so it blends.
    pub fn water(&self) -> bool {
        self.tag.as_ref().is_some_and(|tag| tag.group == "rmw ")
    }
    /// Whether the material is a decal system's (`decs`). A preplaced decal is projected onto whole
    /// surface triangles, so its triangles reach past the decal, and Reach's decal pixel program
    /// discards every pixel whose decal coordinates leave 0 to 1
    /// (`clip(float4(texcoord, 1 - texcoord))` in `effects\decal.fx`).
    pub fn decal(&self) -> bool {
        self.tag.as_ref().is_some_and(|tag| tag.group == "decs")
    }
    pub fn alpha_mode(&self) -> &'static str {
        if self.glass()
            || self.water()
            || self.decal()
                && self
                    .options
                    .get("blend_mode")
                    .is_some_and(|mode| mode == "double_multiply")
        {
            "BLEND"
        } else if self.options.get("alpha_test").is_some_and(|o| o != "none") {
            "MASK"
        } else if self.options.get("blend_mode").is_some_and(|o| {
            [
                "additive",
                "multiply",
                "alpha_blend",
                "pre_multiplied_alpha",
            ]
            .contains(&o.as_str())
        }) {
            "BLEND"
        } else {
            "OPAQUE"
        }
    }
    /// Whether the material never draws: alpha blended by a constant albedo whose alpha is zero, as
    /// `fx\fx_anchor\shaders\invisible` is, or alpha tested against a map whose every texel's alpha
    /// lies below half, so the alpha test discards every pixel, as Reach's other `invisible` shaders
    /// do with `alpha_black`. The map is the one a native surface's alpha test reads, and terrain,
    /// which tests its albedo's alpha, is never judged so.
    pub fn never_drawn(&self, cache: &Cache, pages: &mut Pages) -> Result<bool> {
        let option = |name: &str| self.options.get(name).map(String::as_str);
        if option("blend_mode") == Some("alpha_blend")
            && option("albedo") == Some("constant_color")
            && option("alpha_blend_source")
                .is_some_and(|value| value.starts_with("from_albedo_alpha"))
            && self
                .constant("albedo_color")
                .is_some_and(|color| color[3] <= 0.)
            && !self
                .properties
                .first()
                .is_some_and(|property| property.overlays.contains_key("albedo_color"))
        {
            return Ok(true);
        }
        let terrain = self.tag.as_ref().is_some_and(|tag| tag.group == "rmtr");
        let Some(mapping) = self
            .mapping(&["alpha_mask_map", "alpha_test_map", "alpha_map"])
            .filter(|_| self.alpha_mode() == "MASK" && !terrain)
        else {
            return Ok(false);
        };
        let bitmap = mapping.bitmap.as_ref().context("Missing mapped bitmap")?;
        let image = image(cache, pages, bitmap, mapping.static_frame(cache)?, false)?;
        Ok(image.rgba.chunks_exact(4).all(|texel| texel[3] < 128))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Image {
    pub tag: Tag,
    pub image: usize,
    pub width: usize,
    pub height: usize,
    pub format: i16,
    pub curve: u8,
    pub source_mips: u8,
    pub normal: bool,
    #[serde(skip)]
    pub rgba: Vec<u8>,
}

/// Checked bitmap metadata and source extent, before any pixels are decoded.
pub(super) struct Bitmap {
    tag: Tag,
    image: usize,
    pub width: usize,
    pub height: usize,
    pub format: i16,
    pub curve: u8,
    source_mips: u8,
    normal: bool,
    resource: super::resource::Resource,
    top_bytes: usize,
}

impl Bitmap {
    pub(super) fn read(c: &Cache, tag: &Tag, image: usize, normal: bool) -> Result<Self> {
        let row = *c
            .block(tag.address()? + 124, 56)?
            .get(image)
            .context("Bitmap image outside table")?;
        let width = c.u16(row)? as usize;
        let height = c.u16(row + 2)? as usize;
        ensure!(
            width > 0 && height > 0 && width <= 16384 && height <= 16384,
            "Invalid bitmap dimensions"
        );
        ensure!(
            c.u8(row + 5)? & 8 == 0,
            "Swizzled MCC bitmap needs a layout translation"
        );
        ensure!(
            c.u8(row + 6)? == 0 && c.u8(row + 4)? <= 1,
            "Surface texture {} image {image} is not 2D: kind {}, depth {}",
            tag.path,
            c.u8(row + 6)?,
            c.u8(row + 4)?
        );
        let format = c.i16(row + 8)?;
        let resource = image_resource(c, tag, row, image)?;
        let (unit, compressed) = texel_layout(format, tag)?;
        let top_bytes = image_length(width, height, unit, compressed);
        ensure!(
            top_bytes <= 256 * 1024 * 1024 && width * height <= 64 * 1024 * 1024,
            "Bitmap exceeds decode limit"
        );
        Ok(Self {
            tag: tag.clone(),
            image,
            width,
            height,
            format,
            curve: c.u8(row + 17)?,
            source_mips: c.u8(row + 16)?,
            normal,
            resource,
            top_bytes,
        })
    }

    pub(super) fn decode(&self, c: &Cache, pages: &mut Pages) -> Result<Image> {
        let raw = pages.bytes(c, &self.resource, 0, self.top_bytes)?;
        Ok(Image {
            tag: self.tag.clone(),
            image: self.image,
            width: self.width,
            height: self.height,
            format: self.format,
            curve: self.curve,
            source_mips: self.source_mips,
            normal: self.normal,
            rgba: decode(&raw, self.width, self.height, self.format, self.normal)?,
        })
    }

    pub(super) fn name(&self) -> String {
        image_name(&self.tag, self.image, self.normal)
    }

    /// Retain linear MCC block storage and its authored mips where native sampling has the same
    /// channel contract. BC5 normals need reconstruction after sampling in the pixel program.
    pub(super) fn blocks(&self, c: &Cache, pages: &mut Pages) -> Result<Option<Blocks>> {
        let format = match self.format {
            14 => 71,
            15 => 74,
            16 => 77,
            38 if self.normal => 84,
            44 if self.normal => 83,
            _ => return Ok(None),
        };
        let levels = self
            .source_mips
            .checked_add(1)
            .context("Bitmap mip count overflow")?;
        ensure!(
            u32::from(levels) <= self.width.max(self.height).ilog2() + 1,
            "Bitmap mip count exceeds its dimensions"
        );
        let unit = if self.format == 14 { 8 } else { 16 };
        let length = (0..levels)
            .map(|level| {
                image_length(
                    (self.width >> level).max(1),
                    (self.height >> level).max(1),
                    unit,
                    true,
                )
            })
            .sum::<usize>();
        ensure!(
            length <= 256 * 1024 * 1024,
            "Bitmap mip chain exceeds decode limit"
        );
        let row = c.block(self.tag.address()? + 124, 56)?[self.image];
        let declared = c.u32(row + 24)? as usize;
        ensure!(
            declared == 0 || declared == length,
            "Bitmap mip extent differs from its declared size"
        );
        let resource = &self.resource;
        let available = resource.page_bytes - resource.page_offset;
        let data = if !resource.secondary {
            pages.bytes(c, resource, 0, length)?
        } else {
            let top = image_length(self.width, self.height, unit, true);
            ensure!(
                available == top,
                "Bitmap {} image {} streamed page is not exactly its top mip",
                self.tag.path,
                self.image
            );
            let mut data = pages.bytes(c, resource, 0, top)?;
            if length > top {
                let primary = c.primary_resource(&self.tag, resource.handle)?;
                ensure!(
                    (
                        primary.cache.as_str(),
                        primary.file_offset,
                        primary.page_offset
                    ) != (
                        resource.cache.as_str(),
                        resource.file_offset,
                        resource.page_offset
                    ),
                    "Bitmap {} image {} has no separate lower mip resource",
                    self.tag.path,
                    self.image
                );
                data.extend(pages.bytes(c, &primary, 0, length - top).with_context(|| {
                    format!(
                        "Bitmap {} image {} lower mip chain",
                        self.tag.path, self.image
                    )
                })?);
            }
            data
        };
        Ok(Some(Blocks {
            format,
            levels,
            data,
        }))
    }
}

fn image_name(tag: &Tag, image: usize, normal: bool) -> String {
    format!(
        "texture-{:08X}-{image}{}",
        tag.datum,
        if normal { "-normal" } else { "" }
    )
}

impl Image {
    pub fn name(&self) -> String {
        image_name(&self.tag, self.image, self.normal)
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        let name = self.name();
        std::fs::write(root.join(format!("{name}.rgba")), &self.rgba)?;
        let mut encoder = png::Encoder::new(
            File::create(root.join(format!("{name}.png")))?,
            u32::try_from(self.width)?,
            u32::try_from(self.height)?,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)?;
        writer.finish()?;
        Ok(())
    }
}

pub(super) struct Blocks {
    pub format: u32,
    pub levels: u8,
    pub data: Vec<u8>,
}

pub fn image(c: &Cache, pages: &mut Pages, tag: &Tag, image: usize, normal: bool) -> Result<Image> {
    Bitmap::read(c, tag, image, normal)?.decode(c, pages)
}

/// MCC water's signed BC5 array, with each slice followed by its complete mip chain.
pub(super) struct Slopes {
    pub width: usize,
    pub height: usize,
    pub levels: u8,
    pub frames: Vec<Vec<u8>>,
}

pub(super) fn slopes(c: &Cache, pages: &mut Pages, tag: &Tag, image: usize) -> Result<Slopes> {
    let row = *c
        .block(tag.address()? + 124, 56)?
        .get(image)
        .context("Wave image outside bitmap table")?;
    let width = usize::from(c.u16(row)?);
    let height = usize::from(c.u16(row + 2)?);
    let layers = usize::from(c.u8(row + 4)?);
    ensure!(
        width > 0 && height > 0 && width <= 4096 && height <= 4096,
        "Wave array dimensions are invalid"
    );
    ensure!(
        c.u8(row + 5)? & 8 == 0 && c.u8(row + 6)? == 3 && c.i16(row + 8)? == 38,
        "Wave slopes require a linear MCC BC5_SNORM array"
    );
    ensure!(
        (1..=32).contains(&layers),
        "Wave array layer count is invalid"
    );
    let levels = c
        .u8(row + 16)?
        .checked_add(1)
        .context("Wave mip count overflow")?;
    ensure!(
        u32::from(levels) <= width.max(height).ilog2() + 1,
        "Wave mip count exceeds dimensions"
    );
    let chain = (0..levels)
        .map(|level| image_length((width >> level).max(1), (height >> level).max(1), 16, true))
        .sum::<usize>();
    let length = chain
        .checked_mul(layers)
        .context("Wave array byte count overflow")?;
    ensure!(
        length <= 256 * 1024 * 1024 && usize::try_from(c.u32(row + 24)?)? == length,
        "Wave array payload does not match its slices and mips"
    );
    let resource = image_resource(c, tag, row, image)?;
    let raw = pages.bytes(c, &resource, 0, length)?;
    Ok(Slopes {
        width,
        height,
        levels,
        frames: raw.chunks_exact(chain).map(<[u8]>::to_vec).collect(),
    })
}

/// Accumulates each texel's color times its alpha, linear unless `gamma`.
fn add_texels(sum: &mut [f64; 3], rgba: &[u8], gamma: bool) {
    for texel in rgba.chunks_exact(4) {
        let alpha = f64::from(texel[3]) / 255.;
        for (total, &value) in sum.iter_mut().zip(texel) {
            let value = f64::from(value) / 255.;
            *total += if gamma { value.powf(2.2) } else { value } * alpha;
        }
    }
}

/// The mean color of a cube map's six faces at full size ([`cube_faces`]), each texel's color times
/// its alpha, as Reach's environment lighting multiplies them, and linear unless the bitmap's curve
/// stores gamma encoded values. A 2D image in the environment slot, as Breakneck's 256 by 192
/// `cex_headlong_city`, gives the mean of its full-size image.
pub fn cube_mean(c: &Cache, pages: &mut Pages, tag: &Tag) -> Result<[f32; 3]> {
    let at = tag.address()?;
    let row = *c
        .block(at + 124, 56)?
        .first()
        .context("Cube map has no image")?;
    let size = c.u16(row)? as usize;
    let height = c.u16(row + 2)? as usize;
    let gamma = c.u8(row + 17)? != LINEAR_CURVE;
    if size != height && c.u8(row + 6)? == 0 {
        let image = image(c, pages, tag, 0, false)?;
        let mut sum = [0f64; 3];
        add_texels(&mut sum, &image.rgba, gamma);
        let count = (image.width * image.height) as f64;
        return Ok(sum.map(|total| (total / count) as f32));
    }
    let (size, faces) = cube_faces(c, pages, tag, 0)?;
    let mut sum = [0f64; 3];
    for face in &faces {
        add_texels(&mut sum, face, gamma);
    }
    let count = (6 * size * size) as f64;
    Ok(sum.map(|total| (total / count) as f32))
}

/// Decoded source cube mips, each containing six RGBA8 faces in source face order.
pub struct Cube {
    pub size: usize,
    pub mips: Vec<[Vec<u8>; 6]>,
}

/// The six full-size faces of cube image `image`, decoded to RGBA8, and their size.
pub fn cube_faces(
    c: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    image: usize,
) -> Result<(usize, [Vec<u8>; 6])> {
    let mut cube = cube(c, pages, tag, image)?;
    Ok((cube.size, cube.mips.remove(0)))
}

/// MCC cube resources store all six faces of each mip together, unlike the layer-major wave
/// arrays. Keep authored lower mips, which a box reduction of the top face cannot recover.
pub fn cube(c: &Cache, pages: &mut Pages, tag: &Tag, image: usize) -> Result<Cube> {
    let at = tag.address()?;
    let row = *c
        .block(at + 124, 56)?
        .get(image)
        .context("Cube map image outside table")?;
    let size = c.u16(row)? as usize;
    let height = c.u16(row + 2)? as usize;
    ensure!(
        size.is_power_of_two() && size <= 4096 && height == size && c.u8(row + 6)? == 2,
        "Cube map faces are not square: {size} by {height}, image type {}",
        c.u8(row + 6)?
    );
    ensure!(
        c.u8(row + 5)? & 8 == 0,
        "Swizzled MCC bitmap needs a layout translation"
    );
    let format = c.i16(row + 8)?;
    let (unit, compressed) = texel_layout(format, tag)?;
    let levels = usize::from(c.u8(row + 16)?) + 1;
    ensure!(
        levels <= size.ilog2() as usize + 1,
        "Cube has too many mip levels"
    );
    let lengths = (0..levels)
        .map(|level| {
            let side = (size >> level).max(1);
            image_length(side, side, unit, compressed)
        })
        .collect::<Vec<_>>();
    let chain = lengths.iter().sum::<usize>();
    ensure!(
        c.u32(row + 24)? as usize == chain * 6,
        "Cube pixel extent differs from its declared mip faces"
    );
    let resource = image_resource(c, tag, row, image)?;
    let raw = pages
        .bytes(c, &resource, 0, chain * 6)
        .with_context(|| format!("Cube map {} holds fewer than six faces", tag.path))?;
    let mut mips = Vec::with_capacity(levels);
    let mut offset = 0;
    for (level, length) in lengths.into_iter().enumerate() {
        let side = (size >> level).max(1);
        let mut faces: [Vec<u8>; 6] = Default::default();
        for (index, out) in faces.iter_mut().enumerate() {
            let start = offset + index * length;
            *out = decode(&raw[start..start + length], side, side, format, false)?;
        }
        mips.push(faces);
        offset += length * 6;
    }
    Ok(Cube { size, mips })
}

/// The source bitmap curve whose values are stored linear, not gamma encoded.
pub const LINEAR_CURVE: u8 = 3;

/// The resource holding image `image` of bitmap `tag`, whose image row is at `row`.
fn image_resource(
    c: &Cache,
    tag: &Tag,
    row: usize,
    image: usize,
) -> Result<super::resource::Resource> {
    let at = tag.address()?;
    let resources = c.block(at + 168, 8)?;
    let interleaved = c.block(at + 180, 8)?;
    let r = if interleaved.is_empty() {
        *resources.get(image).context("Bitmap has no resource")?
    } else {
        *interleaved
            .get(c.u8(row + 18)? as usize)
            .context("Bitmap interleaved resource outside table")?
    };
    c.resource(tag, c.u32(r)?)
}

/// The bytes of one texel, or of one 4 by 4 block for a compressed `format`, and whether it is
/// compressed. Uncompressed formats 24, 25, 28 and 30 store four channels red first: 32-bit floats,
/// 16-bit floats, signed bytes and unsigned 16-bit values.
fn layout(format: i16) -> Option<(usize, bool)> {
    Some(match format {
        0..=2 => (1, false),
        3 | 6 | 8 | 9 | 22 => (2, false),
        10 | 11 | 28 => (4, false),
        25 | 30 => (8, false),
        24 => (16, false),
        14 | 31 | 40..=43 => (8, true),
        15 | 16 | 38 | 44 => (16, true),
        _ => return None,
    })
}

fn texel_layout(format: i16, tag: &Tag) -> Result<(usize, bool)> {
    layout(format).with_context(|| {
        format!(
            "Unsupported surface texture format {format} in {}",
            tag.path
        )
    })
}

fn image_length(width: usize, height: usize, unit: usize, compressed: bool) -> usize {
    if compressed {
        width.div_ceil(4) * height.div_ceil(4) * unit
    } else {
        width * height * unit
    }
}

/// Decodes `raw`, a `width` by `height` image of `format`, into RGBA8.
fn decode(raw: &[u8], width: usize, height: usize, format: i16, normal: bool) -> Result<Vec<u8>> {
    let (unit, compressed) =
        layout(format).with_context(|| format!("Unsupported surface texture format {format}"))?;
    ensure!(
        raw.len() >= image_length(width, height, unit, compressed),
        "Bitmap data ends early"
    );
    let mut rgba = vec![0; width * height * 4];
    if compressed {
        for by in 0..height.div_ceil(4) {
            for bx in 0..width.div_ceil(4) {
                let at = (by * width.div_ceil(4) + bx) * unit;
                let b = &raw[at..at + unit];
                let pixels = decode_block(b, format, normal)?;
                for y in 0..4 {
                    for x in 0..4 {
                        if bx * 4 + x < width && by * 4 + y < height {
                            let p = ((by * 4 + y) * width + bx * 4 + x) * 4;
                            rgba[p..p + 4]
                                .copy_from_slice(&pixels[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4]);
                        }
                    }
                }
            }
        }
    } else {
        for (i, b) in raw[..width * height * unit].chunks_exact(unit).enumerate() {
            let p = match format {
                0 => [255, 255, 255, b[0]],
                1 => [b[0], b[0], b[0], 255],
                2 => {
                    let y = (b[0] & 15) * 17;
                    [y, y, y, (b[0] >> 4) * 17]
                }
                3 => [b[0], b[0], b[0], b[1]],
                10 | 11 => [b[2], b[1], b[0], if format == 10 { 255 } else { b[3] }],
                24 => std::array::from_fn(|k| {
                    unorm(f32::from_le_bytes(b[k * 4..k * 4 + 4].try_into().unwrap()))
                }),
                25 => std::array::from_fn(|k| {
                    unorm(half::f16::from_le_bytes([b[k * 2], b[k * 2 + 1]]).to_f32())
                }),
                28 => {
                    std::array::from_fn(|k| unorm((b[k] as i8 as f32 / 127.).max(-1.) * 0.5 + 0.5))
                }
                30 => std::array::from_fn(|k| {
                    unorm(f32::from(u16::from_le_bytes([b[k * 2], b[k * 2 + 1]])) / 65535.)
                }),
                22 => {
                    let x = (b[0] as i8 as f32 / 127.).max(-1.);
                    let y = (b[1] as i8 as f32 / 127.).max(-1.);
                    [
                        unorm(x * 0.5 + 0.5),
                        unorm(y * 0.5 + 0.5),
                        unorm((1. - x * x - y * y).max(0.).sqrt() * 0.5 + 0.5),
                        255,
                    ]
                }
                6 | 8 | 9 => {
                    let v = u16::from_le_bytes(b.try_into().unwrap());
                    match format {
                        6 => [
                            expand(v >> 11, 31),
                            expand((v >> 5) & 63, 63),
                            expand(v & 31, 31),
                            255,
                        ],
                        8 => [
                            expand((v >> 10) & 31, 31),
                            expand((v >> 5) & 31, 31),
                            expand(v & 31, 31),
                            if v & 0x8000 != 0 { 255 } else { 0 },
                        ],
                        _ => [
                            expand((v >> 8) & 15, 15),
                            expand((v >> 4) & 15, 15),
                            expand(v & 15, 15),
                            expand(v >> 12, 15),
                        ],
                    }
                }
                _ => unreachable!(),
            };
            rgba[i * 4..i * 4 + 4].copy_from_slice(&p);
        }
    }
    Ok(rgba)
}
fn decode_block(b: &[u8], format: i16, normal: bool) -> Result<[u8; 64]> {
    let mut pixels = [0u8; 64];
    match format {
        14 => bcdec_rs::bc1(b, &mut pixels, 16),
        15 => bcdec_rs::bc2(b, &mut pixels, 16),
        16 => bcdec_rs::bc3(b, &mut pixels, 16),
        38 | 44 => {
            let mut xy = [0f32; 32];
            bcdec_rs::bc5_float(b, &mut xy, 8, format == 38);
            for i in 0..16 {
                if normal {
                    let x = if format == 38 {
                        xy[i * 2]
                    } else {
                        xy[i * 2] * 2. - 1.
                    };
                    let y = if format == 38 {
                        xy[i * 2 + 1]
                    } else {
                        xy[i * 2 + 1] * 2. - 1.
                    };
                    let z = (1. - x * x - y * y).max(0.).sqrt();
                    pixels[i * 4..i * 4 + 4].copy_from_slice(&[
                        unorm(x * 0.5 + 0.5),
                        unorm(y * 0.5 + 0.5),
                        unorm(z * 0.5 + 0.5),
                        255,
                    ]);
                } else {
                    pixels[i * 4..i * 4 + 4].copy_from_slice(&[
                        unorm(xy[i * 2]),
                        unorm(xy[i * 2 + 1]),
                        0,
                        255,
                    ]);
                }
            }
        }
        31 | 42 | 43 => {
            let mut values = [0u8; 16];
            bcdec_rs::bc4(b, &mut values, 4, false);
            for (i, v) in values.into_iter().enumerate() {
                pixels[i * 4..i * 4 + 4].copy_from_slice(&if format == 42 {
                    [255, 255, 255, v]
                } else {
                    [v, v, v, 255]
                });
            }
        }
        40 | 41 => {
            for i in 0..16 {
                let v = ((b[i / 2] >> ((i % 2) * 4)) & 15) * 17;
                pixels[i * 4..i * 4 + 4].copy_from_slice(&if format == 40 {
                    [255, 255, 255, v]
                } else {
                    [v, v, v, 255]
                });
            }
        }
        _ => unreachable!(),
    }
    Ok(pixels)
}

fn unorm(v: f32) -> u8 {
    (v * 255.).round().clamp(0., 255.) as u8
}
fn expand(v: u16, max: u16) -> u8 {
    ((u32::from(v) * 255 + u32::from(max) / 2) / u32::from(max)) as u8
}
