use super::{
    cache::{Cache, Tag},
    resource::Pages,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{collections::BTreeMap, fs::File, path::Path};

#[derive(Clone, Debug, Serialize)]
pub struct Mapping {
    pub role: String,
    pub bitmap: Option<Tag>,
    pub transform: [f32; 4],
    pub frame: usize,
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
    pub fn ammunition_place(&self) -> Option<u32> {
        let [function] = self.functions.as_slice() else {
            return None;
        };
        const DECIMAL: [u8; 32] = [
            0, 0x34, 0, 0, 0, 0, 0, 0, 0, 0, 0x20, 0x41, 0, 0, 0, 0, 0, 0, 0, 0, 0x48, 0xe1, 0xfa,
            0x3e, 0, 0, 0, 0x3f, 0, 0, 0, 0,
        ];
        if function.kind != 7 || !function.range.is_empty() || function.data != DECIMAL {
            return None;
        }
        match function.input.as_str() {
            "primary_ammunition_ones" => Some(1),
            "primary_ammunition_tens" => Some(10),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Property {
    pub template: Tag,
    pub constants: BTreeMap<String, [f32; 4]>,
    pub textures: Vec<Mapping>,
    pub functions: Vec<Function>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Material {
    pub tag: Option<Tag>,
    pub options: BTreeMap<String, String>,
    pub properties: Vec<Property>,
}

pub fn read(c: &Cache, tag: Option<Tag>) -> Result<Material> {
    let mut result = Material {
        tag: tag.clone(),
        options: BTreeMap::new(),
        properties: Vec::new(),
    };
    let Some(tag) = tag else { return Ok(result) };
    let at = tag.address()?;
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
            .into_iter()
            .zip(values.iter().copied())
            .collect::<BTreeMap<_, _>>();
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
                functions: selected.to_vec(),
            });
        }
        result.properties.push(Property {
            template,
            constants,
            textures,
            functions,
        });
    }
    Ok(result)
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
    pub fn alpha_mode(&self) -> &'static str {
        if self.options.get("alpha_test").is_some_and(|o| o != "none") {
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

impl Image {
    pub fn name(&self) -> String {
        format!(
            "texture-{:08X}-{}{}",
            self.tag.datum,
            self.image,
            if self.normal { "-normal" } else { "" }
        )
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

pub fn image(c: &Cache, pages: &mut Pages, tag: &Tag, image: usize, normal: bool) -> Result<Image> {
    let at = tag.address()?;
    let row = *c
        .block(at + 124, 56)?
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
        "Surface texture is not a 2D image"
    );
    let format = c.i16(row + 8)?;
    let resources = c.block(at + 168, 8)?;
    let interleaved = c.block(at + 180, 8)?;
    let r = if interleaved.is_empty() {
        *resources.get(image).context("Bitmap has no resource")?
    } else {
        *interleaved
            .get(c.u8(row + 18)? as usize)
            .context("Bitmap interleaved resource outside table")?
    };
    let resource = c.resource(tag, c.u32(r)?)?;
    let (unit, compressed) = match format {
        0..=2 => (1, false),
        3 | 6 | 8 | 9 | 22 => (2, false),
        10 | 11 => (4, false),
        14 | 31 | 40..=43 => (8, true),
        15 | 16 | 38 | 44 => (16, true),
        n => anyhow::bail!("Unsupported surface texture format {n} in {}", tag.path),
    };
    let length = if compressed {
        width.div_ceil(4) * height.div_ceil(4) * unit
    } else {
        width * height * unit
    };
    ensure!(
        length <= 256 * 1024 * 1024 && width * height <= 64 * 1024 * 1024,
        "Bitmap exceeds decode limit"
    );
    let raw = pages.bytes(c, &resource, 0, length)?;
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
        for (i, b) in raw.chunks_exact(unit).enumerate() {
            let p = match format {
                0 => [255, 255, 255, b[0]],
                1 => [b[0], b[0], b[0], 255],
                2 => {
                    let y = (b[0] & 15) * 17;
                    [y, y, y, (b[0] >> 4) * 17]
                }
                3 => [b[0], b[0], b[0], b[1]],
                10 | 11 => [b[2], b[1], b[0], if format == 10 { 255 } else { b[3] }],
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
    Ok(Image {
        tag: tag.clone(),
        image,
        width,
        height,
        format,
        curve: c.u8(row + 17)?,
        source_mips: c.u8(row + 16)?,
        normal,
        rgba,
    })
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
