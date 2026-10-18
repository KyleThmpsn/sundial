//! Checked surface roles adapted to the native deferred surface contract.
use super::{
    cache::Cache,
    material::{self, LINEAR_CURVE, Material},
    resource::Pages,
};
use crate::{
    presentation::{Graph, append, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};
pub(super) mod sky;
mod surface;

pub(super) struct Binding<'a> {
    pub palette: &'a [u8],
    pub rigid: bool,
    pub objects: &'a BTreeMap<String, u8>,
    pub overrides: &'a BTreeMap<String, BTreeMap<String, [f32; 4]>>,
    pub lenses: &'a BTreeMap<u32, Lens>,
    pub paint: &'a BTreeMap<u32, super::paint::Surface>,
    /// A level's decoded baked lighting, sampled with the lightmap coordinates its meshes carry in
    /// the detail coordinates.
    pub lightmap: Option<&'a Lightmap>,
}

/// The map slot of a level material's diffuse table, which Reach's lightmap shaders read at the
/// cosine between the normal and a lightmap lobe and at the lobe's bandwidth.
const VMF_TABLE_SLOT: usize = 15;
/// The map slot of a glass material's opacity map, whose alpha is its coverage.
const GLASS_OPACITY_SLOT: usize = 10;
/// The texture registers a level surface may bind. Its maps and lighting take consecutive registers
/// from zero. Distant Shore's 100 stock surfaces bind none above eleven.
const MAX_TEXTURE_REGISTERS: usize = 16;

/// The shell's pixel sampler table row holding its anisotropic clamping sampler, `80C6B566` in
/// Shadowkeep (filter 0x55, address mode 3, anisotropy 4, bias -0.5).
const CLAMPED_SAMPLER: usize = 2;
/// The first pixel sampler register a surface binds. Its map samplers follow in order, then the
/// clamping sampler for its lightmap and palette lookups.
const FIRST_SAMPLER_REGISTER: usize = 3;
/// D3D11's pixel sampler registers.
const SAMPLER_REGISTERS: usize = 16;
/// D3D11 anisotropic filtering, address modes and the anisotropy surfaces sample with.
const ANISOTROPIC: u32 = 0x55;
const WRAP: u32 = 1;
const MIRROR: u32 = 2;
const CLAMP: u32 = 3;
const BORDER: u32 = 4;
const MIRROR_ONCE: u32 = 5;
const MAX_ANISOTROPY: u32 = 16;

/// The scale of a detail map sample. A neutral detail map reads 0.5^2.2, either stored so on the
/// linear curve or as middle gray on a gamma curve, so its sample scales by 2^2.2.
const DETAIL_MULTIPLIER: f32 = 4.594_79;
/// Destiny's scene radiance for Reach radiance equal to a level's average lit level. A lightmapped
/// level surface emits its Reach radiance over the lightmap's average lit level times this. At
/// 30, taken from the scale of Distant Shore's distant scenery cards, Hang 'Em High's surfaces and
/// sky drew white in game. At 3 its sunlit sand still drew white and its walls two to four times
/// brighter than in Reach captures of the level, and at 1 its large frames about one and a half
/// times as bright. At 0.6 the frames matched the captures until their environment reflection was
/// translated, which brightened them to about 1.4 times the captures' values again. At 0.45 the
/// interior corridors' frames measured means of 120 to 140 against the captures' frames at 105 to
/// 124, and Reach's nine press screenshots of the level hold median luminances of 58 to 98 against
/// the corridors' 103 to 123. At 0.4 the corridors drew a median of about 81 and the arena views
/// 76 to 103 against the screenshots' median of 75, before the large bloom's floor added light to
/// the shade.
pub(super) const LIGHT_RADIANCE: f32 = 0.35;
/// The same scale for the sky. Reach captures of Hang 'Em High show its sky two to five times as
/// bright as its lit walls. At the surfaces' scale of 3 the sky drew darker than the walls in game,
/// so the sky kept that scale while the surfaces took a fifth of it, and it stays five times the
/// surfaces' scale. Reach's own sky programs scale their self illumination and their result by
/// runtime exposure constants that were not recovered.
pub(super) const SKY_RADIANCE: f32 = 5. * LIGHT_RADIANCE;
/// The frame constants at `b13`, which a baked surface's pixel program reads for its exposure.
const FRAME: &str = "cbuffer Frame : register(b13) {float4 frameData[2];};";

#[derive(Clone)]
pub(super) struct Lens {
    pub center: [f32; 3],
    pub rear: f32,
    pub radius: f32,
    pub reticle: bool,
}

const VERTEX: &str = r#"
$SOURCE_PALETTE$
$SHADOW_RESOURCE$
cbuffer Object : register(b11) {float4 objectData[776];};
cbuffer View : register(b12) {float4 viewData[14];};
void main(float4 position:POSITION0,float4 weight:BLENDWEIGHT0,uint4 bone:BLENDINDICES0,float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,float2 detail:TEXCOORD2,
out float4 o0:TEXCOORD0,out float4 o1:TEXCOORD1,out float4 o2:TEXCOORD2,out float4 o3:TEXCOORD3,out float3 o4:TEXCOORD4,out float4 projected:SV_POSITION0) {
float4 p=float4(position.xyz*objectData[5].www+objectData[5].xyz,1);
$MOTION$
float3 world=float3(dot(r0,p),dot(r1,p),dot(r2,p));
float3 n=normalize(float3(dot(r0.xyz,normal.xyz),dot(r1.xyz,normal.xyz),dot(r2.xyz,normal.xyz)));
float3 t=normalize(float3(dot(r0.xyz,tangent.xyz),dot(r1.xyz,tangent.xyz),dot(r2.xyz,tangent.xyz)));
float3 relative=world-viewData[7].xyz;
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
$SHADOW_PROJECTION$
o0=float4(n,saturate(objectData[7].w+saturate(objectData[7].z*n.z)));
o1=float4(t,p.z);o2=float4(cross(n,t)*tangent.w,dot(n,normalize(-relative)));
o3=float4(uv*objectData[6].xy+objectData[6].zw,detail);o4=p.xyz;
}
"#;

pub(super) fn program(
    native: &mut Reader,
    g: &mut Graph,
    symbol: &str,
    template: u32,
    text: &str,
    vertex: bool,
) -> Result<()> {
    let (bytes, warnings) = crate::tiger::shader::compile(text, vertex)?;
    fs::write(g.root.join(format!("{symbol}.hlsl")), text)?;
    fs::write(g.root.join(format!("{symbol}.log")), warnings)?;
    let mut header = native.tag(template, None)?.0.clone();
    put(&mut header, 8, &u32::try_from(bytes.len())?.to_le_bytes())?;
    let data = format!("{symbol}-data");
    g.add(symbol, template, &header, Some(&data), vec![])?;
    g.add(
        &data,
        native.reference(template)?,
        &bytes,
        Some(symbol),
        vec![],
    )
}

pub(super) fn texture(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    mapping: &material::Mapping,
    normal: bool,
    srgb: bool,
) -> Result<String> {
    let mut image = material::image(
        c,
        pages,
        mapping.bitmap.as_ref().context("bitmap")?,
        mapping.static_frame(c)?,
        normal,
    )?;
    // A bitmap stored on the linear curve already holds linear values, which an sRGB view would
    // darken. The default detail map stores 0.5^2.2 this way.
    let srgb = srgb && image.curve != LINEAR_CURVE;
    let animated = mapping.ammunition_place(c)?.is_some();
    if animated {
        let tag = mapping.bitmap.as_ref().context("Animated bitmap")?;
        let frames = (0..10)
            .map(|frame| material::image(c, pages, tag, frame, normal))
            .collect::<Result<Vec<_>>>()?;
        let width = image.width;
        let height = image.height;
        ensure!(
            frames
                .iter()
                .all(|f| f.width == width && f.height == height && f.format == image.format),
            "Decimal bitmap frame dimensions or formats differ"
        );
        image.width *= 10;
        image.rgba.resize(image.width * height * 4, 0);
        for (index, frame) in frames.iter().enumerate() {
            for y in 0..height {
                let start = (y * image.width + index * width) * 4;
                image.rgba[start..start + width * 4]
                    .copy_from_slice(&frame.rgba[y * width * 4..(y + 1) * width * 4]);
            }
        }
    }
    let symbol = format!(
        "{}-{}{}",
        image.name(),
        if srgb { "srgb" } else { "linear" },
        if animated { "-decimal" } else { "" }
    );
    if g.nodes.iter().any(|n| n["symbol"] == symbol) {
        return Ok(symbol);
    }
    // Smaller levels of a digit strip would blend neighboring digits, and palettes and diffuse
    // tables are lookup tables read at exact coordinates.
    let mips = if animated || matches!(mapping.role.as_str(), "palette" | "g_sample_vmf_diffuse") {
        Mips::None
    } else if normal {
        Mips::Normal
    } else if srgb {
        Mips::Color
    } else {
        Mips::Data
    };
    rgba_texture(
        native,
        g,
        &symbol,
        image.width,
        image.height,
        image.rgba,
        srgb,
        mips,
    )?;
    Ok(symbol)
}

/// How a texture's smaller mip levels are filtered from its top level.
#[derive(Clone, Copy, PartialEq)]
enum Mips {
    /// The top level alone.
    None,
    /// sRGB color, averaged as linear light. Alpha is averaged as stored.
    Color,
    /// Values averaged as stored.
    Data,
    /// Tangent-space normals in RGB, averaged and renormalized.
    Normal,
}

/// Appends the smaller mip levels of `rgba`, a `width` by `height` top level, each a 2x2 box filter
/// of the level above, down to one pixel. Returns the level count.
fn mip_chain(width: usize, height: usize, rgba: &mut Vec<u8>, mips: Mips) -> u8 {
    let linear: [f32; 256] = std::array::from_fn(|value| {
        let c = value as f32 / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    let (mut w, mut h, mut start, mut levels) = (width, height, 0, 1);
    while mips != Mips::None && (w > 1 || h > 1) {
        let next = downsample(w, h, &rgba[start..], mips, &linear);
        start += w * h * 4;
        rgba.extend(next);
        (w, h, levels) = ((w / 2).max(1), (h / 2).max(1), levels + 1);
    }
    levels
}

/// The level below `level`, a `w` by `h` RGBA8 image, each pixel filtered from a 2x2 block. An odd
/// side repeats its last row or column.
fn downsample(w: usize, h: usize, level: &[u8], mips: Mips, linear: &[f32; 256]) -> Vec<u8> {
    let (next_w, next_h) = ((w / 2).max(1), (h / 2).max(1));
    let mut next = Vec::with_capacity(next_w * next_h * 4);
    for y in 0..next_h {
        for x in 0..next_w {
            let block = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| {
                let p = ((2 * y + dy).min(h - 1) * w + (2 * x + dx).min(w - 1)) * 4;
                [level[p], level[p + 1], level[p + 2], level[p + 3]]
            });
            next.extend(filter(&block, mips, linear));
        }
    }
    next
}

/// The mean of four RGBA8 pixels as `mips` filters them, with `linear` decoding sRGB values.
fn filter(block: &[[u8; 4]; 4], mips: Mips, linear: &[f32; 256]) -> [u8; 4] {
    let unorm = |value: f32| (value.clamp(0., 1.) * 255. + 0.5) as u8;
    let mean = |channel: usize, read: &dyn Fn(u8) -> f32| {
        block.iter().map(|pixel| read(pixel[channel])).sum::<f32>() / 4.
    };
    let alpha = unorm(mean(3, &|v| f32::from(v) / 255.));
    let [r, g, b] = match mips {
        Mips::Color => [0, 1, 2].map(|channel| {
            let l = mean(channel, &|v| linear[usize::from(v)]);
            let c = if l <= 0.003_130_8 {
                l * 12.92
            } else {
                1.055 * l.powf(1. / 2.4) - 0.055
            };
            unorm(c)
        }),
        Mips::Normal => {
            let n = [0, 1, 2].map(|channel| mean(channel, &|v| f32::from(v) / 127.5 - 1.));
            let length = n.iter().map(|v| v * v).sum::<f32>().sqrt();
            let n = if length > 1e-6 {
                n.map(|v| v / length)
            } else {
                [0., 0., 1.]
            };
            n.map(|v| unorm(v * 0.5 + 0.5))
        }
        Mips::None | Mips::Data => {
            [0, 1, 2].map(|channel| unorm(mean(channel, &|v| f32::from(v) / 255.)))
        }
    };
    [r, g, b, alpha]
}

/// Adds texture `symbol`, uncompressed RGBA8 pixels with the mip levels `mips` filters, and its
/// data.
#[allow(clippy::too_many_arguments)]
fn rgba_texture(
    native: &mut Reader,
    g: &mut Graph,
    symbol: &str,
    width: usize,
    height: usize,
    mut rgba: Vec<u8>,
    srgb: bool,
    mips: Mips,
) -> Result<()> {
    ensure!(rgba.len() == width * height * 4, "Texture size differs");
    let levels = mip_chain(width, height, &mut rgba, mips);
    let template = if srgb { 0x80ec31b9 } else { 0x80ec31bc };
    let mut header = native.tag(template, None)?.0.clone();
    put(&mut header, 0, &u32::try_from(rgba.len())?.to_le_bytes())?;
    put(
        &mut header,
        4,
        &(if srgb { 29u32 } else { 28u32 }).to_le_bytes(),
    )?;
    put(&mut header, 14, &u16::try_from(width)?.to_le_bytes())?;
    put(&mut header, 16, &u16::try_from(height)?.to_le_bytes())?;
    put(&mut header, 18, &1u16.to_le_bytes())?;
    put(&mut header, 20, &1u16.to_le_bytes())?;
    header[22] = 0;
    header[23] = levels;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::tiger::texture::resident(&mut header, rgba.len())?;
    let data = format!("{symbol}-data");
    g.add(symbol, template, &header, Some(&data), vec![])?;
    g.add(
        &data,
        native.reference(template)?,
        &rgba,
        Some(symbol),
        vec![],
    )?;
    Ok(())
}

/// A level's decoded baked lighting: its three atlas textures, the scale of their lighting and the
/// average lit level that lighting is a share of, as [`super::level::lightmap::Atlas`] describes
/// them, and the light its opaque surfaces add for the level's large bloom, as a share of that
/// level.
pub(super) struct Lightmap {
    pub lobe: String,
    pub ambient: String,
    pub direction: String,
    pub scale: f32,
    pub level: f32,
    pub veil: [f32; 3],
}

/// Adds a level's decoded baked lighting as three linear RGBA8 textures. Each keeps one level, as
/// smaller levels would blend neighboring charts of its atlas.
pub(super) fn lightmap_textures(
    native: &mut Reader,
    g: &mut Graph,
    atlas: &super::level::lightmap::Atlas,
    veil: [f32; 3],
) -> Result<Lightmap> {
    let mut add = |name: &str, rgba: &[u8]| -> Result<String> {
        let symbol = format!("lightmap-{name}-linear");
        rgba_texture(
            native,
            g,
            &symbol,
            atlas.width,
            atlas.height,
            rgba.to_vec(),
            false,
            Mips::None,
        )?;
        Ok(symbol)
    };
    Ok(Lightmap {
        lobe: add("lobe", &atlas.lobe)?,
        ambient: add("ambient", &atlas.ambient)?,
        direction: add("direction", &atlas.direction)?,
        scale: atlas.scale,
        level: atlas.level,
        veil,
    })
}

/// The D3D11 address modes, U then V, of a source mapping's packed address byte. Its low four bits
/// address U and its high four bits V, each wrap, clamp, mirror, black border, mirror once or
/// mirror once with a border.
fn address_modes(packed: u8) -> Result<[u32; 2]> {
    let mode = |value: u8| match value {
        0 => Ok(WRAP),
        1 => Ok(CLAMP),
        2 => Ok(MIRROR),
        3 => Ok(BORDER),
        4 | 5 => Ok(MIRROR_ONCE),
        _ => anyhow::bail!("Unsupported source sampler address mode {value}"),
    };
    Ok([mode(packed & 15)?, mode(packed >> 4)?])
}

/// The shader name of a surface's sampler `index`. The first always wraps both axes.
fn sampler_name(index: usize) -> String {
    if index == 0 {
        "SurfaceSampler".into()
    } else {
        format!("Sampler{index}")
    }
}

/// Adds a sampler surfaces read their maps with: the native anisotropic sampler `template` with the
/// U and V address `modes` of the source mappings that use it.
fn sampler(native: &mut Reader, g: &mut Graph, template: u32, modes: [u32; 2]) -> Result<String> {
    let symbol = format!("sampler-{}-{}", modes[0], modes[1]);
    if g.nodes.iter().any(|n| n["symbol"] == symbol) {
        return Ok(symbol);
    }
    let header = native.tag(template, None)?.0.clone();
    let data = native.reference(template)?;
    let mut description = native.tag(data, None)?.0.clone();
    let filter = Payload(description.clone()).u32(0)?;
    ensure!(
        header == [0; 8] && description.len() == 52 && filter == ANISOTROPIC,
        "Native anisotropic sampler differs"
    );
    for (offset, mode) in [(4, modes[0]), (8, modes[1]), (12, WRAP)] {
        put(&mut description, offset, &mode.to_le_bytes())?;
    }
    put(&mut description, 20, &MAX_ANISOTROPY.to_le_bytes())?;
    let raw = format!("{symbol}-data");
    g.add(&symbol, template, &header, Some(&raw), vec![])?;
    g.add(&raw, data, &description, Some(&symbol), vec![])?;
    Ok(symbol)
}

/// A surface's pixel samplers: one per address mode pair its maps use, then the shell's clamping
/// sampler for its lightmap and palette lookups, bound to consecutive registers from
/// `FIRST_SAMPLER_REGISTER`.
struct Samplers {
    /// Sampler table rows. The map samplers' rows leave their hash for their symbol.
    table: Vec<u8>,
    /// The map samplers' symbols, by table row.
    symbols: Vec<String>,
    /// Bytecode that pushes each table row and binds it to its register.
    expression: Vec<u8>,
    /// The pixel program's sampler declarations.
    declarations: String,
}

/// The samplers of a surface whose maps use address `modes`, then its table's copy of the shell's
/// clamping sampler row `clamped`.
fn samplers(
    native: &mut Reader,
    g: &mut Graph,
    clamped: [u8; 16],
    modes: &[[u32; 2]],
) -> Result<Samplers> {
    let template = u32::from_le_bytes(clamped[..4].try_into()?);
    ensure!(
        FIRST_SAMPLER_REGISTER + modes.len() < SAMPLER_REGISTERS,
        "A surface needs more samplers than a pixel program binds"
    );
    let mut result = Samplers {
        table: Vec::new(),
        symbols: Vec::new(),
        expression: Vec::new(),
        declarations: String::new(),
    };
    for row in 0..=modes.len() {
        let name = if row < modes.len() {
            sampler_name(row)
        } else {
            "ClampSampler".into()
        };
        let register = FIRST_SAMPLER_REGISTER + row;
        result.expression.extend([
            0x4c,
            u8::try_from(row)?,
            0x49,
            0x20 | u8::try_from(register)?,
        ]);
        result
            .declarations
            .push_str(&format!("SamplerState {name}:register(s{register});"));
    }
    for modes in modes {
        result.symbols.push(sampler(native, g, template, *modes)?);
        result.table.extend(u32::MAX.to_le_bytes());
        result.table.extend(1u32.to_le_bytes());
        result.table.extend(0u64.to_le_bytes());
    }
    result.table.extend(clamped);
    Ok(result)
}

fn sample(c: &Cache, slot: usize, mapping: &material::Mapping, sampler: &str) -> Result<String> {
    let transform = mapping.transform;
    let uv = format!(
        "v3.xy*float2({:.9},{:.9})+float2({:.9},{:.9})",
        transform[0], transform[1], transform[2], transform[3]
    );
    Ok(if let Some(place) = mapping.ammunition_place(c)? {
        format!("Map{slot}.Sample({sampler},decimalUv({uv},{place}.0))")
    } else {
        format!("Map{slot}.Sample({sampler},{uv})")
    })
}

fn decimal_display(c: &Cache, pages: &mut Pages, material: &Material) -> Result<Option<String>> {
    let mut selected = None;
    for mapping in material.properties.iter().flat_map(|p| &p.textures) {
        if mapping.ammunition_place(c)?.is_some() {
            selected = Some(mapping);
            break;
        }
    }
    let Some(mapping) = selected else {
        return Ok(None);
    };
    let frame = material::image(
        c,
        pages,
        mapping.bitmap.as_ref().context("Decimal bitmap")?,
        0,
        false,
    )?;
    Ok(Some(format!(
        "cbuffer Magazine : register(b0) {{ float4 magazine; }};\nfloat2 decimalUv(float2 uv,float place) {{ float digit=fmod(floor(max(magazine.x,0)/place),10); return float2((digit+clamp(uv.x,{:.9},{:.9}))/10,clamp(uv.y,{:.9},{:.9})); }}",
        0.5 / frame.width as f32,
        1. - 0.5 / frame.width as f32,
        0.5 / frame.height as f32,
        1. - 0.5 / frame.height as f32
    )))
}

struct TextureBindings {
    /// Each bound texture's register and symbol.
    bindings: Vec<(usize, String)>,
    samples: BTreeMap<usize, String>,
    /// The address modes of each sampler the samples name, by sampler index.
    samplers: Vec<[u32; 2]>,
    /// Each bound map's register, by map slot.
    registers: BTreeMap<usize, usize>,
    /// The sampler of the bound diffuse table, in map slot `VMF_TABLE_SLOT`.
    table: Option<String>,
}

/// A terrain shader's blend map and its second to fourth layers' base maps take slots 6 to 9,
/// which terrain shaders leave free. The first layer keeps slots 0 and 1, and the layers' detail
/// maps take slots 11 to 14.
const TERRAIN_BLEND: usize = 6;
const TERRAIN_LAYERS: [usize; 4] = [0, 7, 8, 9];
const TERRAIN_DETAIL: [usize; 4] = [11, 12, 13, 14];

/// Whether `material` is a terrain shader, which blends up to four layers by its blend map.
fn terrain(material: &Material) -> bool {
    material.tag.as_ref().is_some_and(|tag| tag.group == "rmtr")
}

/// A terrain shader's blend weights: the blend map's channels for its active layers, the first
/// layer alone where the blend map is absent.
fn terrain_weights(material: &Material, samples: &BTreeMap<usize, String>) -> String {
    let Some(blend) = samples.get(&TERRAIN_BLEND) else {
        return "float4(1,0,0,0)".into();
    };
    let active = TERRAIN_LAYERS
        .iter()
        .enumerate()
        .map(|(layer, slot)| {
            let off = material
                .options
                .get(&format!("material_{layer}"))
                .is_some_and(|option| option == "off");
            if off || !samples.contains_key(slot) {
                "0"
            } else {
                "1"
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("terrainWeights(({blend})*float4({active}))")
}

/// Shader functions terrain blending uses: weights summing to one, falling back to the first
/// layer where the blend map gives none.
const TERRAIN_FUNCTIONS: &str = "float4 terrainWeights(float4 w){float total=dot(w,float4(1,1,1,1));return total>0.0001?w/total:float4(1,0,0,0);}";

/// The maps `material` binds and their samples. With `dense`, as for a lightmapped level material,
/// they take consecutive registers from zero and the material's diffuse table binds too.
fn textures(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    material: &Material,
    paint: Option<&super::paint::Surface>,
    dense: bool,
) -> Result<TextureBindings> {
    let mut bindings = Vec::new();
    let mut samples = BTreeMap::new();
    let mut registers = BTreeMap::new();
    let mut samplers = vec![[WRAP, WRAP]];
    let layers: &[(usize, &[&str], bool, bool)] = &[
        (TERRAIN_BLEND, &["blend_map"][..], false, false),
        (TERRAIN_LAYERS[1], &["base_map_m_1"][..], false, true),
        (TERRAIN_LAYERS[2], &["base_map_m_2"][..], false, true),
        (TERRAIN_LAYERS[3], &["base_map_m_3"][..], false, true),
        (TERRAIN_DETAIL[0], &["detail_map_m_0"][..], false, true),
        (TERRAIN_DETAIL[1], &["detail_map_m_1"][..], false, true),
        (TERRAIN_DETAIL[2], &["detail_map_m_2"][..], false, true),
        (TERRAIN_DETAIL[3], &["detail_map_m_3"][..], false, true),
    ];
    let standard: [(usize, &[&str], bool, bool); 10] = [
        (0, &["base_map", "base_map_m_0"][..], false, true),
        (1, &["bump_map", "bump_map_m_0"][..], true, false),
        (2, &["material_texture"][..], false, false),
        (
            3,
            &["alpha_mask_map", "alpha_test_map", "alpha_map"][..],
            false,
            false,
        ),
        (
            4,
            &["self_illum_map", "meter_map"][..],
            false,
            material
                .options
                .get("self_illumination")
                .is_none_or(|v| v != "3_channel_self_illum"),
        ),
        (5, &["detail_map"][..], false, true),
        (6, &["noise_map_a"][..], false, false),
        (7, &["noise_map_b"][..], false, false),
        (8, &["palette"][..], false, true),
        (9, &["change_color_map"][..], false, false),
    ];
    let table = if terrain(material) {
        standard[..2]
            .iter()
            .chain(layers)
            .copied()
            .collect::<Vec<_>>()
    } else if material.glass() {
        let mut table = standard.to_vec();
        table.push((GLASS_OPACITY_SLOT, &["opacity_texture"][..], false, false));
        table
    } else {
        standard.to_vec()
    };
    for (slot, roles, normal, srgb) in table {
        if slot == 9
            && !terrain(material)
            && paint.is_none_or(|paint| !matches!(paint.mask, super::paint::PaintMask::ChangeColor))
        {
            continue;
        }
        if let Some(mapping) = material.mapping(roles) {
            let target = if dense {
                bindings.len()
            } else if paint.is_some() && slot >= 3 {
                slot + 6
            } else {
                slot
            };
            bindings.push((target, texture(c, pages, native, g, mapping, normal, srgb)?));
            registers.insert(slot, target);
            let index = sampler_index(&mut samplers, address_modes(mapping.address)?);
            samples.insert(slot, sample(c, slot, mapping, &sampler_name(index))?);
        }
    }
    let mut table = None;
    if dense && let Some(mapping) = material.mapping(&["g_sample_vmf_diffuse"]) {
        let target = bindings.len();
        bindings.push((target, texture(c, pages, native, g, mapping, false, false)?));
        registers.insert(VMF_TABLE_SLOT, target);
        let index = sampler_index(&mut samplers, address_modes(mapping.address)?);
        table = Some(sampler_name(index));
    }
    Ok(TextureBindings {
        bindings,
        samples,
        samplers,
        registers,
        table,
    })
}

/// The index of the sampler with address `modes` in `samplers`, which gains it when absent.
fn sampler_index(samplers: &mut Vec<[u32; 2]>, modes: [u32; 2]) -> usize {
    samplers
        .iter()
        .position(|existing| *existing == modes)
        .unwrap_or_else(|| {
            samplers.push(modes);
            samplers.len() - 1
        })
}

fn dye_tint(
    paint: Option<&super::paint::Surface>,
    samples: &BTreeMap<usize, String>,
) -> Result<String> {
    let tinting = if let Some(paint) = paint {
        let mask = match paint.mask {
            super::paint::PaintMask::Surface => "1.0".to_owned(),
            super::paint::PaintMask::ChangeColor => format!(
                "saturate(dot(({}).rgb,float3(1,1,1)))",
                samples.get(&9).context("Source paint mask sample")?
            ),
        };
        // Generic model draws can run without an equipped gear scope. An
        // unbound D3D constant buffer reads as zero, including dye metadata.
        // Check the complete record, not its color, so an intentional black
        // dye with valid parameters remains black.
        format!(
            "float4 dyeData=0;[unroll]for(uint dyeRow=0;dyeRow<27;dyeRow++){{dyeData=max(dyeData,abs(dye[dyeRow]));}}float3 paintColor=any(dyeData>0)?max(dye[9].rgb,0):float3(1,1,1);albedo.rgb*=lerp(float3(1,1,1),paintColor,{mask});"
        )
    } else {
        String::new()
    };
    Ok(tinting)
}

/// The environment reflection a lightmapped surface adds to its albedo before its baked light
/// multiplies both, as an HLSL expression, and its per-channel scale for the report. Reach's
/// deferred lightmap programs for `two_lobe_phong` and `cook_torrance` surfaces add, to the
/// diffuse `(lobe * table + ambient) / pi * albedo`, the environment map sampled at the reflected
/// view times `(lobe * 3 * specular_table + ambient) * tint`, the specular mask, the specular
/// coefficient, `environment_map_specular_contribution` and `env_tint_color`. The tint blends from
/// the model's normal tint (`normal_specular_tint`, or `specular_tint`) toward the albedo by its
/// blend (`albedo_specular_tint_blend`, or `albedo_blend`), and toward the glancing tint by a
/// fresnel term. The translation has no view direction, so it takes the cube map's mean, the
/// normal tint's blend without the fresnel term, and the diffuse response for the specular table's.
/// Its reflection is then the term over the diffuse light, times pi.
fn reflection(
    c: &Cache,
    pages: &mut Pages,
    material: &Material,
    surface: &surface::Surface<'_>,
    means: &mut BTreeMap<u32, [f32; 3]>,
) -> Result<Option<(String, [f32; 3])>> {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    if option("environment_mapping") != Some("per_pixel") {
        return Ok(None);
    }
    let (normal_tint, blend) = match option("material_model") {
        Some("two_lobe_phong") => ("normal_specular_tint", "albedo_specular_tint_blend"),
        Some("cook_torrance") => ("specular_tint", "albedo_blend"),
        _ => return Ok(None),
    };
    let mask = match option("specular_mask") {
        Some("specular_mask_from_diffuse") => "albedo.a",
        Some("no_specular_mask") => "1",
        _ => return Ok(None),
    };
    let Some(bitmap) = material
        .properties
        .iter()
        .flat_map(|property| &property.textures)
        .find(|mapping| mapping.role == "environment_map")
        .and_then(|mapping| mapping.bitmap.as_ref())
    else {
        return Ok(None);
    };
    let mean = match means.get(&bitmap.datum) {
        Some(&mean) => mean,
        None => {
            let mean = material::cube_mean(c, pages, bitmap)
                .with_context(|| format!("Environment map {}", bitmap.path))?;
            means.insert(bitmap.datum, mean);
            mean
        }
    };
    let first = |name: &str, default: f32| surface.constant(name).map_or(default, |v| v[0]);
    let strength = first("environment_map_specular_contribution", 0.)
        * first("specular_coefficient", 1.)
        * std::f32::consts::PI;
    let tint = surface.constant("env_tint_color").unwrap_or([1.; 4]);
    let scale: [f32; 3] = std::array::from_fn(|k| (mean[k] * tint[k] * strength).max(0.));
    if scale.iter().all(|&v| v <= 0.) {
        return Ok(None);
    }
    let normal = surface.constant(normal_tint).unwrap_or([1.; 4]);
    Ok(Some((
        format!(
            "lerp(float3({:.9},{:.9},{:.9}),albedo.rgb,{:.9})*float3({:.9},{:.9},{:.9})*{mask}",
            normal[0],
            normal[1],
            normal[2],
            first(blend, 0.),
            scale[0],
            scale[1],
            scale[2]
        ),
        scale,
    )))
}

/// A lightmapped level surface's lighting, applied once its normal is known, and its lighting
/// textures' declarations. The three lightmap textures take the next registers after `bindings`,
/// and `table` names the sampler of the material's diffuse table when it binds one. As Reach's
/// per-pixel lightmap shader does, irradiance is the lobe's color times the diffuse table read at
/// the normal's cosine to the lobe and at its bandwidth, plus the ambient color. Without a table,
/// the cosine response blends from flat to clamped with the bandwidth. The irradiance lights the
/// albedo and the surface's environment `reflection` together.
fn level_lighting(
    lightmap: &Lightmap,
    bindings: &mut Vec<(usize, String)>,
    table: Option<&str>,
    reflection: Option<&str>,
) -> Result<(String, String)> {
    let first = bindings.len();
    ensure!(
        first + 3 <= MAX_TEXTURE_REGISTERS,
        "A level surface needs {} texture registers",
        first + 3
    );
    for (offset, symbol) in [&lightmap.lobe, &lightmap.ambient, &lightmap.direction]
        .into_iter()
        .enumerate()
    {
        bindings.push((first + offset, symbol.clone()));
    }
    let cosine = "saturate(dot(n,lightDirection)*0.5+0.5)";
    let response = match table {
        Some(sampler) => {
            format!("Map{VMF_TABLE_SLOT}.SampleLevel({sampler},float2({cosine},lightLobe.a),0).w")
        }
        None => "lerp(0.5,saturate(dot(n,lightDirection)),lightLobe.a)".to_owned(),
    };
    let lit = reflection.map_or_else(|| "albedo.rgb".to_owned(), |r| format!("(albedo.rgb+{r})"));
    let lighting = format!(
        "float4 lightLobe=LightLobe.Sample(ClampSampler,v3.zw);float4 lightAmbient=LightAmbient.Sample(ClampSampler,v3.zw);float3 lightDirection=normalize(LightDirection.Sample(ClampSampler,v3.zw).xyz*2-1+float3(0,0,0.00001));float lightResponse={response};albedo.rgb={lit}*(lightResponse*lightLobe.rgb*lightLobe.rgb+lightAmbient.rgb*lightAmbient.rgb)*{:.9};",
        lightmap.scale
    );
    let declarations = format!(
        "Texture2D<float4> LightLobe:register(t{first});Texture2D<float4> LightAmbient:register(t{});Texture2D<float4> LightDirection:register(t{});",
        first + 1,
        first + 2
    );
    Ok((lighting, declarations))
}

/// A lightmapped opaque level surface's G-buffer writes. Reach lights such a surface by its baked
/// light alone, which its albedo already holds as a share of the lightmap's average lit `level`,
/// plus its self illumination. The surface emits that radiance at [`LIGHT_RADIANCE`] through
/// Destiny's packed emission and lets none of Destiny's own light reach it: the ambient term's
/// visibility, target two's `w`, is zero, and the normal points straight down, away from a sun
/// that stays above the horizon. Destiny's deferred program decodes target two's `y` as
/// `exp2(13 * saturate(2 * y - (1 + 2/255)) - 7) - 1/128` and scales target zero's color by it.
/// The surface takes the eight-bit step at or above its brightest channel, and target zero holds
/// the radiance over what that step decodes to. That program scales emission by the frame's
/// exposure scale and relative illumination (`cb13[1].x * cb13[1].w`), while a sky surface's
/// transparent stage takes the exposure scale for shading instead (`cb13[1].z * cb13[1].w`). The
/// radiance is therefore written times `z / x`, which brings the level to the sky's scale. It
/// also adds the level's large bloom `veil`, a share of the average lit level, before that scale.
/// Reach adds the bloom over the finished image, so the surface's own light no longer decides the
/// color of its shade.
fn baked_finish(level: f32, veil: [f32; 3]) -> String {
    format!(
        "float3 radiance=(max(albedo.rgb+emission*{inverse:.9},0)+float3({veil_r:.9},{veil_g:.9},{veil_b:.9}))*{LIGHT_RADIANCE:.9}*frameData[1].z/max(frameData[1].x,0.000001);float strength=max(radiance.r,max(radiance.g,radiance.b));float encoded=0.5*(1+2.0/255.0+saturate((log2(strength+1.0/128.0)+7)/13));float packed=min(ceil(encoded*255)/255,1);float decoded=exp2(13*saturate(2*packed-(1+2.0/255.0))-7)-1.0/128.0;o0=float4(saturate(radiance/max(decoded,0.000001)),0.04);o1=float4(saturate(float3(0,0,-1)*(roughness*0.125+0.375)+0.5),0);o2=float4(0,packed,0,0);",
        inverse = 1. / level,
        veil_r = veil[0],
        veil_g = veil[1],
        veil_b = veil[2],
    )
}

/// A lightmapped level's transparent surface's output, as a sky surface's transparent stage writes
/// its own: its lit color plus its self illumination over the average lit `level`, times the
/// surfaces' [`LIGHT_RADIANCE`] and the frame's exposure (`cb13[1].z * cb13[1].w`), blended by its
/// blend mode. The opaque surface behind it already holds the large bloom's floor. Additive
/// surfaces add their self illumination. A multiplying surface, such as a dirt decal, darkens what
/// lies behind it by its unlit albedo's mean faded by its coverage, the nearest a premultiplied
/// blend comes to a product.
fn baked_transparent_finish(material: &Material, level: f32) -> String {
    let scale = format!("{LIGHT_RADIANCE:.9}*frameData[1].z*frameData[1].w");
    let emission = format!("emission*{:.9}", 1. / level);
    match material.options.get("blend_mode").map(String::as_str) {
        Some("additive") => format!("o0=float4({emission}*{scale},0);"),
        Some("multiply") => "float3 product=lerp(float3(1,1,1),unlit.rgb,saturate(unlit.a));o0=float4(0,0,0,1-saturate(dot(product,float3(1,1,1))/3));".to_owned(),
        _ => format!(
            "float coverage=saturate(albedo.a);o0=float4((albedo.rgb+{emission})*{scale}*coverage,coverage);"
        ),
    }
}

#[allow(clippy::cognitive_complexity)]
pub(super) fn build(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    materials: &BTreeMap<u32, Material>,
    binding: Binding<'_>,
) -> Result<Vec<Value>> {
    let shell = native.tag(0x80ec270d, Some(0x808071e8))?;
    let transparent_shell = native.tag(0x815246aa, Some(0x808071e8))?;
    ensure!(
        transparent_shell.0[32] == 0x88,
        "Native premultiplied surface state differs"
    );
    let palette = binding.palette;
    ensure!(
        !palette.is_empty() && palette.len() <= 256,
        "Invalid source motion palette"
    );
    ensure!(
        binding.lightmap.is_none() || binding.paint.is_empty(),
        "Lightmapped surfaces cannot also take gear paint"
    );
    let lookup = format!(
        "static const uint motionPalette[{}] = {{{}}};",
        palette.len(),
        palette
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let motion = if binding.rigid {
        "float4 r0=float4(objectData[0].x,objectData[1].x,objectData[2].x,objectData[3].x);float4 r1=float4(objectData[0].y,objectData[1].y,objectData[2].y,objectData[3].y);float4 r2=float4(objectData[0].z,objectData[1].z,objectData[2].z,objectData[3].z);"
    } else {
        "float4 r0=0,r1=0,r2=0;[unroll] for(uint i=0;i<4;i++){uint row=8+motionPalette[bone[i]]*3;r0+=objectData[row]*weight[i];r1+=objectData[row+1]*weight[i];r2+=objectData[row+2]*weight[i];}"
    };
    let vertex = VERTEX
        .replace("$SOURCE_PALETTE$", &lookup)
        .replace("$MOTION$", motion);
    program(
        native,
        g,
        "vertex",
        shell.u32(0x48)?,
        &vertex
            .replace("$SHADOW_RESOURCE$", "")
            .replace("$SHADOW_PROJECTION$", ""),
        true,
    )?;
    // Surfaces read their maps through a wrapping copy of the shell's anisotropic sampler, and a
    // lightmap atlas through the clamping original.
    let sampler_rows = shell.array(0x308, 16, None)?;
    let clamped = shell.bytes::<16>(
        *sampler_rows
            .get(CLAMPED_SAMPLER)
            .context("Native sampler table is missing")?,
    )?;
    let sampler_class = shell.u32(sampler_rows[0] - 8)?;
    let mut report = Vec::new();
    let mut cube_means = BTreeMap::new();
    for (&key, material) in materials {
        let surface = surface::Surface::new(material, &binding, key);
        let paint = binding.paint.get(&key);
        let display = decimal_display(c, pages, material)?;
        let animated = display.is_some();
        let display = display.unwrap_or_default();
        let dense = binding.lightmap.is_some();
        let TextureBindings {
            mut bindings,
            samples,
            samplers: modes,
            registers,
            table,
        } = textures(c, pages, native, g, material, paint, dense)?;
        let samplers = samplers(native, g, clamped, &modes)?;
        let reflected = match binding.lightmap {
            Some(_) if !surface.transparent() => {
                reflection(c, pages, material, &surface, &mut cube_means)?
            }
            _ => None,
        };
        let (lighting, lighting_declarations) = match binding.lightmap {
            Some(lightmap) => level_lighting(
                lightmap,
                &mut bindings,
                table.as_deref(),
                reflected
                    .as_ref()
                    .map(|(expression, _)| expression.as_str()),
            )?,
            None => (String::new(), String::new()),
        };
        let tint = surface.constant("albedo_color").unwrap_or([1.; 4]);
        let weights = terrain(material).then(|| terrain_weights(material, &samples));
        let mut color = albedo(material, &samples, tint, weights.as_deref());
        // Glass replaces its lighting with the blend its compiled programs write.
        let lighting = if material.glass() {
            let (albedo, shading) =
                surface.glass(&samples, binding.lightmap.map(|lightmap| lightmap.scale));
            color = albedo;
            shading
        } else {
            lighting
        };
        let emission_tint = surface
            .constant("self_illum_color")
            .or_else(|| surface.constant("self_illum_tint_color"))
            .unwrap_or([1.; 4]);
        let emission_scale = surface
            .constant("self_illum_intensity")
            .map_or(1., |v| v[0])
            .max(0.);
        let emission = surface.emission(&samples, &color);
        let normal = samples
            .get(&1)
            .map(|s| match &weights {
                // The first layer's bump fades where other layers cover it.
                Some(weights) => format!("lerp(float3(0,0,1),({s}).xyz*2-1,({weights}).x)"),
                None => format!("({s}).xyz*2-1"),
            })
            .unwrap_or_else(|| "float3(0,0,1)".into());
        let rough = samples
            .get(&2)
            .map(|s| format!("saturate(1-({s}).r)"))
            .unwrap_or_else(|| "0.48".into());
        let clip = if material.alpha_mode() == "MASK" {
            format!(
                "clip({}-0.5);",
                samples
                    .get(&3)
                    .map(|s| format!("({s}).a"))
                    .unwrap_or_else(|| "albedo.a".into())
            )
        } else {
            String::new()
        };
        let output = surface.output();
        let (finish, frame) = match binding.lightmap {
            Some(lightmap) if !surface.transparent() => {
                (baked_finish(lightmap.level, lightmap.veil), FRAME)
            }
            Some(lightmap) => (baked_transparent_finish(material, lightmap.level), FRAME),
            None => (surface.finish(), ""),
        };
        let detail_slots = weights.as_ref().map_or(&[][..], |_| &TERRAIN_DETAIL[..]);
        let declarations = if dense {
            registers
                .iter()
                .map(|(slot, register)| {
                    format!("Texture2D<float4> Map{slot}:register(t{register});")
                })
                .collect::<String>()
        } else {
            (0..=9)
                .chain(detail_slots.iter().copied())
                .chain(material.glass().then_some(GLASS_OPACITY_SLOT))
                .map(|slot| {
                    let target = if paint.is_some() && slot >= 3 {
                        slot + 6
                    } else {
                        slot
                    };
                    format!("Texture2D<float4> Map{slot}:register(t{target});")
                })
                .collect::<String>()
        };
        let dye = paint.map_or_else(String::new, |paint| {
            format!(
                "cbuffer GearPaint:register(b{}) {{float4 dye[27];}};",
                paint.slot
            )
        });
        let tinting = dye_tint(paint, &samples)?;
        let functions = weights.as_ref().map_or("", |_| TERRAIN_FUNCTIONS);
        let sampler_declarations = &samplers.declarations;
        let text = format!(
            r#"
{declarations}
{lighting_declarations}
{frame}
{dye}
{display}
{functions}
{sampler_declarations}
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
{output}) {{
float4 albedo={color};{tinting}{clip}float3 n={normal};n=normalize(n.x*v1.xyz+n.y*v2.xyz+n.z*v0.xyz);float4 unlit=albedo;{lighting}
float3 emission={emission};float roughness={rough};{finish}
}}
"#
        );
        let shader = format!("pixel-{key:08X}");
        program(native, g, &shader, shell.u32(0x2c8)?, &text, false)?;
        let (template, envelope, stage) = if surface.transparent() {
            (0x815246aa, &transparent_shell, 7)
        } else {
            (0x80ec270d, &shell, 0)
        };
        let mut bytes = fresh_surface(envelope)?;
        if let Some(paint) = paint {
            for offset in [24, 28] {
                let current = Payload(bytes.clone()).u32(offset)?;
                put(&mut bytes, offset, &(current | paint.scopes).to_le_bytes())?;
            }
        }
        let mut fixed = Vec::new();
        for (slot, _) in &bindings {
            fixed.extend((*slot as u32).to_le_bytes());
            fixed.extend(u32::MAX.to_le_bytes());
        }
        append(&mut bytes, 0x2d0, 0x80807211, &fixed, 8)?;
        append(
            &mut bytes,
            0x308,
            u64::from(sampler_class),
            &samplers.table,
            16,
        )?;
        let mut expression = if bindings.is_empty() {
            vec![]
        } else {
            samplers.expression.clone()
        };
        if animated {
            let input = *binding
                .objects
                .get("32CC5402")
                .context("Native current-magazine model input")?;
            expression.extend([0x4d, input, 0x43, 0]);
            bytes[0x33c] = 0x10;
            put(
                &mut bytes,
                0x338,
                &u32::try_from(binding.objects.len())?.to_le_bytes(),
            )?;
            put(&mut bytes, 0x348, &0u32.to_le_bytes())?;
        }
        append(
            &mut bytes,
            0x318,
            0x80800090,
            if animated { &[0; 16] } else { &[] },
            16,
        )?;
        put(&mut bytes, 0x34c, &u32::MAX.to_le_bytes())?;
        append(&mut bytes, 0x2e8, 0x80800009, &expression, 1)?;
        let patches = surface_patches(&bytes, &shader, &bindings, &samplers.symbols)?;
        g.add(
            &format!("material-{key:08X}"),
            template,
            &bytes,
            None,
            patches,
        )?;
        report.push(json!({"source":material.tag,"source_properties":material.properties,"effective_constants":surface.constants,"options":material.options,"bound_textures":bindings,"albedo_color":tint,"emission_color":emission_tint,"emission_scale":emission_scale,"alpha":material.alpha_mode(),"native_stage":stage,"baked_emission":binding.lightmap.is_some()&&!surface.transparent(),"environment_reflection":reflected.as_ref().map(|(_, scale)| scale),"paint":paint.map(|paint| json!({"channel":paint.channel+10,"mask":paint.mask,"constant_buffer":paint.slot,"scope_mask":paint.scopes,"method":"Native equipped dye color multiplied through the authored surface mask. A missing or entirely empty dye record preserves source color."})),"limits":["Opaque surfaces use the native deferred approximation. Transparent surfaces use premultiplied output. Decimal ammunition frame functions are bound to the live magazine input. Other source functions, activation events, plasma depth fading and layered overlays are not translated. Explicit overrides select a static appearance state."]}));
    }
    shadow(native, g, &vertex)?;
    Ok(report)
}

/// A surface's albedo: its base map times its `tint`. The default albedo multiplies the base map
/// by its detail map, as detail options do. A terrain shader instead mixes its layers by their
/// blend `weights`, each at its own tiling and each multiplied by its own detail map.
fn albedo(
    material: &Material,
    samples: &BTreeMap<usize, String>,
    tint: [f32; 4],
    weights: Option<&str>,
) -> String {
    let base = samples
        .get(&0)
        .cloned()
        .unwrap_or_else(|| "float4(1,1,1,1)".into());
    let mut color = format!(
        "({base})*float4({:.9},{:.9},{:.9},{:.9})",
        tint[0], tint[1], tint[2], tint[3]
    );
    if material
        .options
        .get("albedo")
        .is_some_and(|m| m == "default" || m.contains("detail"))
        && let Some(detail) = samples.get(&5)
    {
        color = format!("({color})*float4({DETAIL_MULTIPLIER:.6}*({detail}).rgb,1)");
    }
    // A decal's `diffuse_plus_alpha_mask` albedo takes its coverage from its alpha map.
    if material
        .options
        .get("albedo")
        .is_some_and(|m| m == "diffuse_plus_alpha_mask")
        && let Some(mask) = samples.get(&3)
    {
        color = format!("float4(({color}).rgb,({mask}).a*{:.9})", tint[3]);
    }
    if let Some(weights) = weights {
        let [a, b, c, d] = std::array::from_fn(|layer| {
            let base = samples
                .get(&TERRAIN_LAYERS[layer])
                .cloned()
                .unwrap_or_else(|| "float4(0,0,0,1)".into());
            match samples.get(&TERRAIN_DETAIL[layer]) {
                Some(detail) => {
                    format!("float4(({base}).rgb*{DETAIL_MULTIPLIER:.6}*({detail}).rgb,({base}).a)")
                }
                None => base,
            }
        });
        color = format!("mul({weights},float4x4({a},{b},{c},{d}))");
    }
    color
}

/// The patches of surface record `bytes`, whose pixel program is `shader`: its two programs, then
/// each bound texture's row in its texture table and each map sampler's row in its sampler table.
fn surface_patches(
    bytes: &[u8],
    shader: &str,
    bindings: &[(usize, String)],
    sampler_symbols: &[String],
) -> Result<Vec<Value>> {
    let record = Payload(bytes.to_vec());
    let mut patches = vec![
        json!({"offset":0x48,"symbol":"vertex"}),
        json!({"offset":0x2c8,"symbol":shader}),
    ];
    for (row, (_, symbol)) in record.array(0x2d0, 8, None)?.into_iter().zip(bindings) {
        patches.push(json!({"offset":row+4,"symbol":symbol}));
    }
    for (row, symbol) in record
        .array(0x308, 16, None)?
        .into_iter()
        .zip(sampler_symbols)
    {
        patches.push(json!({"offset":row,"symbol":symbol}));
    }
    Ok(patches)
}

fn fresh_surface(envelope: &Payload) -> Result<Vec<u8>> {
    let mut bytes = envelope.0.clone();
    for at in [24, 28] {
        put(
            &mut bytes,
            at,
            &(envelope.u32(at)? & !0x1e000000).to_le_bytes(),
        )?;
    }
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    // Fresh programs use only the native object/view buffers and explicit bindings.
    for stage in [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368] {
        bytes[stage..stage + 0xa0].fill(0);
        put(&mut bytes, stage, &u32::MAX.to_le_bytes())?;
        if [0x48, 0x2c8].contains(&stage) {
            put(&mut bytes, stage + 0x80, &u32::MAX.to_le_bytes())?;
        }
        put(&mut bytes, stage + 0x84, &u32::MAX.to_le_bytes())?;
    }
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x2c8, &u32::MAX.to_le_bytes())?;
    Ok(bytes)
}

fn shadow(native: &mut Reader, g: &mut Graph, vertex: &str) -> Result<()> {
    let template = native.tag(0x80ec271d, Some(0x808071e8))?;
    // The native shadow pass supplies the light's atlas rectangle at t2.
    // Its projection is distinct from the full-view surface projection even
    // though both use the same object pose and view constants.
    let vertex = vertex
        .replace(
            "$SHADOW_RESOURCE$",
            "Texture2D<float4> ShadowAtlas : register(t2);",
        )
        .replace(
            "$SHADOW_PROJECTION$",
            "float4 atlas=ShadowAtlas.Load(int3(0,0,0));\nprojected.xy=projected.xy*atlas.zw+projected.w*float2(2*atlas.x+atlas.z-1,1-2*atlas.y-atlas.w);",
        );
    program(
        native,
        g,
        "shadow-vertex",
        template.u32(0x48)?,
        &vertex,
        true,
    )?;
    // The donor's dissolve program reads weapon object channels. A projectile
    // has no such channel array, so retaining that program faults on its first
    // shadow draw. These imported surfaces have no translated dissolve state.
    program(
        native,
        g,
        "shadow-pixel",
        template.u32(0x2c8)?,
        "void main() {}",
        false,
    )?;
    let mut bytes = template.0.clone();
    // Keep the native pixel-bearing shadow envelope, with a fresh stage that
    // writes depth and needs no material expressions or constant resources.
    bytes[0x2c8..0x368].fill(0);
    put(&mut bytes, 0x348, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x34c, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x2c8, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    g.add(
        "material-shadow",
        0x80ec271d,
        &bytes,
        None,
        vec![
            json!({"offset":0x48,"symbol":"shadow-vertex"}),
            json!({"offset":0x2c8,"symbol":"shadow-pixel"}),
        ],
    )
}
