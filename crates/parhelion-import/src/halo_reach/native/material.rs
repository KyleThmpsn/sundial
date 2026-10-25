//! Checked surface roles adapted to the native deferred surface contract.
use crate::halo_reach::{
    cache::Cache,
    level::lighting::Analytical,
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
mod animation;
mod decal;
mod environment;
mod fog;
mod mux;
pub(in crate::halo_reach) mod sky;
mod specular;
mod surface;
mod terrain;
pub(in crate::halo_reach) mod vertex_data;
mod water;
pub(in crate::halo_reach) use fog::{Band, Fog, Light};

pub(in crate::halo_reach) struct Binding<'a> {
    pub palette: &'a [u8],
    pub rigid: bool,
    pub objects: &'a BTreeMap<String, u8>,
    pub overrides: &'a BTreeMap<String, BTreeMap<String, [f32; 4]>>,
    pub lenses: &'a BTreeMap<u32, Lens>,
    pub paint: &'a BTreeMap<u32, crate::halo_reach::paint::Surface>,
    /// A level's decoded baked lighting, sampled with the lightmap coordinates its meshes carry in
    /// the detail coordinates.
    pub lightmap: Option<&'a Lightmap>,
    /// Optional vertex-stage data texture, independent of the pixel texture bindings.
    pub vertex_data: Option<&'a str>,
    /// Whether the surfaces draw static instances ([`STATIC_SCOPE`]), whose transform and texture
    /// coordinate scale come from the instance scope rather than from an object's.
    pub statics: bool,
    /// Prefixes the symbols of the programs and surfaces this build adds, so a second build with
    /// another binding can share the graph. Textures and samplers are shared across builds.
    pub prefix: &'a str,
}

/// The scope bits a static instance's surfaces take in place of an object's: stock static
/// surfaces (`80F56C45`, `80BFDCCD`) hold `0x200` at `+0x18` and `+0x1C` where entity surfaces
/// hold `0x80`.
const STATIC_SCOPE: u32 = 0x200;
const OBJECT_SCOPE: u32 = 0x80;

/// The vertex stage register of a static surface's vertex data texture. A static instance's
/// scope binds the per-vertex occlusion buffer stock static programs read at `t0`.
const STATIC_VERTEX_DATA_REGISTER: u32 = 1;

/// [`VERTEX`] for a static instance. The stock static program (`815B86C9`) reads instance
/// `SV_InstanceID`'s three transform rows and its texture coordinate scale and offset as four rows
/// of `b11`, the transform already holding the static's position scale and offset. Its first
/// output's `w`, the stock per-vertex occlusion, stays 1, since the baked lighting holds the
/// level's occlusion.
fn static_vertex(text: &str) -> Result<String> {
    let mut text = text.to_owned();
    for (from, to) in [
        (
            "cbuffer Object : register(b11) {float4 objectData[776];};",
            "cbuffer Instances : register(b11) {float4 instanceData[64];};",
        ),
        (
            "float2 detail:TEXCOORD2,",
            "float2 detail:TEXCOORD2,uint instance:SV_InstanceID,",
        ),
        (
            "float4 p=float4(position.xyz*objectData[5].www+objectData[5].xyz,1);",
            "float4 p=float4(position.xyz,1);uint row=instance*4;",
        ),
        (
            RIGID_MOTION,
            "float4 r0=instanceData[row],r1=instanceData[row+1],r2=instanceData[row+2];",
        ),
        // The stock static depth and shadow programs (`815B86E8`, `815B86E6`) take the camera
        // from each row's translation before the dot product, then project with a multiply, two
        // multiply-adds and an add. Surfaces project in the same order so every pass writes the
        // same depth. World space first rounds differently a kilometre from the origin, and the
        // surface then lost its depth test at range and shadowed itself in patches.
        (
            "projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];",
            "float3 view=float3(dot(float4(r0.xyz,r0.w-viewData[7].x),p),dot(float4(r1.xyz,r1.w-viewData[7].y),p),dot(float4(r2.xyz,r2.w-viewData[7].z),p));projected=viewData[1]*view.y;projected=viewData[0]*view.x+projected;projected=viewData[2]*view.z+projected;projected=projected+viewData[13];",
        ),
        (
            "o0=float4(n,saturate(objectData[7].w+saturate(objectData[7].z*n.z)));",
            "o0=float4(n,1);",
        ),
        (
            "o3=float4(uv*objectData[6].xy+objectData[6].zw,detail);",
            "o3=float4(uv*instanceData[row+3].x+instanceData[row+3].yz,detail);",
        ),
    ] {
        ensure!(
            text.matches(from).count() == 1,
            "The static vertex program's source differs at {from}"
        );
        text = text.replace(from, to);
    }
    ensure!(
        !text.contains("objectData"),
        "The static vertex program still reads object data"
    );
    Ok(text)
}

const RIGID_MOTION: &str = "float4 r0=float4(objectData[0].x,objectData[1].x,objectData[2].x,objectData[3].x);float4 r1=float4(objectData[0].y,objectData[1].y,objectData[2].y,objectData[3].y);float4 r2=float4(objectData[0].z,objectData[1].z,objectData[2].z,objectData[3].z);";

/// The map slot of a level material's diffuse table, which Reach's lightmap shaders read at the
/// cosine between the normal and a lightmap lobe and at the lobe's bandwidth.
const VMF_TABLE_SLOT: usize = 15;
/// The map slot of a glass material's opacity map, whose alpha is its coverage.
const GLASS_OPACITY_SLOT: usize = 10;
/// Translation budget for fixed texture bindings, separate from the sixteen sampler registers.
/// Four terrain layers can require seventeen surface maps before the three lighting maps.
const MAX_TEXTURE_REGISTERS: usize = 32;

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
const TRILINEAR: u32 = 0x15;
const WRAP: u32 = 1;
const MIRROR: u32 = 2;
const CLAMP: u32 = 3;
const BORDER: u32 = 4;
const MIRROR_ONCE: u32 = 5;
const MAX_ANISOTROPY: u32 = 16;
/// A sampler description's mip bias, a float after its filter and three address modes.
const MIP_BIAS: usize = 16;

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
pub(in crate::halo_reach) const LIGHT_RADIANCE: f32 = 0.35;
/// Sky and level surfaces share one radiance conversion. Their final programs already compensate
/// for the different native exposure paths, so an additional sky multiplier amplifies every
/// overlapping cloud and light-shaft layer. Source vertex lighting and self illumination retain
/// the authored brightness differences. Reach's runtime exposure constants are not reproduced.
pub(in crate::halo_reach) const SKY_RADIANCE: f32 = LIGHT_RADIANCE;
/// The frame constants at `b13`, which a baked surface's pixel program reads for its exposure.
const FRAME: &str = "cbuffer Frame : register(b13) {float4 frameData[2];};";
/// The relative illumination (`cb13[1].w`) at which a baked surface keeps the brightness it had
/// when it followed the deferred emission path. Native light rows scale with the frame's
/// exposure (`cb13[1].x`, `Frame+0x1C`), and the deferred pass scales emission by `x * w`, where
/// `w` read `x^(-2/3)` on Nightfall (x 7.98, w 0.249) and Breakneck (x 0.192, w 3.03). Baked
/// light written as emission therefore followed `x^(1/3)` and drove automatic exposure to
/// `radiance^-3`: Breakneck's world, about 3.5 times Nightfall's, set its exposure 40 times lower,
/// and the native sun and ambient light on its weapons and players with it. Baked surfaces divide
/// by `w` and take this constant instead, so they scale with exposure as native light does.
/// Nightfall's value keeps its look.
const REFERENCE_ILLUMINATION: f32 = 0.25;

#[derive(Clone)]
pub(in crate::halo_reach) struct Lens {
    pub center: [f32; 3],
    pub rear: f32,
    pub radius: f32,
    pub reticle: bool,
}

const VERTEX: &str = r#"
$SOURCE_PALETTE$
$SHADOW_RESOURCE$
$VERTEX_LIGHT_DECL$
cbuffer Object : register(b11) {float4 objectData[776];};
cbuffer View : register(b12) {float4 viewData[14];};
void main(float4 position:POSITION0,$SKIN_INPUTS$float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,float2 detail:TEXCOORD2,
out float4 o0:TEXCOORD0,out float4 o1:TEXCOORD1,out float4 o2:TEXCOORD2,out float4 o3:TEXCOORD3,out float3 o4:TEXCOORD4,out float4 projected:SV_POSITION0,out float4 o5:TEXCOORD5,out float4 o6:TEXCOORD6,out float4 o7:TEXCOORD7,out float4 o8:TEXCOORD8,out float4 o9:TEXCOORD9) {
float4 p=float4(position.xyz*objectData[5].www+objectData[5].xyz,1);
$MOTION$
float3 world=float3(dot(r0,p),dot(r1,p),dot(r2,p));
float3 n=normalize(float3(dot(r0.xyz,normal.xyz),dot(r1.xyz,normal.xyz),dot(r2.xyz,normal.xyz)));
float3 t=normalize(float3(dot(r0.xyz,tangent.xyz),dot(r1.xyz,tangent.xyz),dot(r2.xyz,tangent.xyz)));
float3 relative=world-viewData[7].xyz;
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
$SHADOW_PROJECTION$
o0=float4(n,saturate(objectData[7].w+saturate(objectData[7].z*n.z)));
o1=float4(t,length(relative)/3.048);o2=float4(cross(n,t)*tangent.w,dot(n,normalize(-relative)));
o3=float4(uv*objectData[6].xy+objectData[6].zw,detail);o4=$PIXEL_POSITION$;o5=float4(-relative,world.z);
o9=float4(o3.xy,1/max(o1.w,0.01),0);
$VERTEX_LIGHT_READ$
}
"#;

pub(in crate::halo_reach) fn program(
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

pub(in crate::halo_reach) fn texture(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    mapping: &material::Mapping,
    normal: bool,
    srgb: bool,
) -> Result<String> {
    Ok(bound_texture(c, pages, native, g, mapping, normal, srgb)?.0)
}

/// The optional flag identifies signed or unsigned BC5 normal samples.
fn bound_texture(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    mapping: &material::Mapping,
    normal: bool,
    srgb: bool,
) -> Result<(String, Option<bool>)> {
    let bitmap = material::Bitmap::read(
        c,
        mapping.bitmap.as_ref().context("bitmap")?,
        mapping.static_frame(c)?,
        normal,
    )
    .with_context(|| format!("Surface bitmap role {}", mapping.role))?;
    // A bitmap stored on the linear curve already holds linear values, which an sRGB view would
    // darken. The default detail map stores 0.5^2.2 this way.
    let srgb = srgb && bitmap.curve != LINEAR_CURVE;
    let animated = mapping.ammunition_place(c)?.is_some();
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
    let symbol = format!(
        "{}-{}{}{}",
        bitmap.name(),
        if srgb { "srgb" } else { "linear" },
        if animated { "-decimal" } else { "" },
        if mips == Mips::None && !animated {
            "-lookup"
        } else {
            ""
        }
    );
    let bc5 = (mips != Mips::None && normal && matches!(bitmap.format, 38 | 44))
        .then_some(bitmap.format == 38);
    // Repeated bindings need only the checked identity. Compressed passthrough reads the authored
    // mip chain directly, so neither path needs an intermediate RGBA decode.
    if g.nodes.iter().any(|n| n["symbol"] == symbol) {
        return Ok((symbol, bc5));
    }
    if mips != Mips::None
        && let Some(blocks) = bitmap.blocks(c, pages)?
    {
        let format = blocks.format + u32::from(srgb && matches!(blocks.format, 71 | 74 | 77));
        texture_data(
            native,
            g,
            &symbol,
            [bitmap.width, bitmap.height],
            format,
            blocks.levels,
            &blocks.data,
        )?;
        return Ok((symbol, bc5));
    }
    let mut image = bitmap
        .decode(c, pages)
        .with_context(|| format!("Surface bitmap role {}", mapping.role))?;
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
    Ok((symbol, None))
}

/// Distant Shore's interior cube texture header (`80EB038A`), type 32:2, whose six faces, cube flags
/// (`0xFF` at `+0x1B`, 2 at `+0x20`) and resident data tag a map's cube textures copy.
const CUBE_TEMPLATE: u32 = 0x80EB_038A;
/// DXGI's four 16-bit floats per texel.
const RGBA16_FLOAT: u32 = 10;

/// Native cube payloads are mip-major. Alpha remains separate until the shader samples it.
pub(in crate::halo_reach) fn cube_mips(
    native: &mut Reader,
    g: &mut Graph,
    symbol: &str,
    size: usize,
    mips: &[[Vec<[f32; 4]>; 6]],
) -> Result<()> {
    ensure!(
        size.is_power_of_two() && !mips.is_empty() && mips.len() <= size.ilog2() as usize + 1,
        "Invalid native cube mip extent"
    );
    let mut data = Vec::new();
    for (level, faces) in mips.iter().enumerate() {
        let side = (size >> level).max(1);
        for face in faces {
            ensure!(
                face.len() == side * side,
                "Native cube mip face extent differs"
            );
            for texel in face {
                for &value in texel {
                    ensure!(value.is_finite(), "Nonfinite cube channel");
                    let value = half::f16::from_f32(value.max(0.));
                    ensure!(value.is_finite(), "Cube channel exceeds half-float range");
                    data.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    let mut header = native.tag(CUBE_TEMPLATE, None)?.0.clone();
    put(&mut header, 0, &u32::try_from(data.len())?.to_le_bytes())?;
    put(&mut header, 4, &RGBA16_FLOAT.to_le_bytes())?;
    put(&mut header, 14, &u16::try_from(size)?.to_le_bytes())?;
    put(&mut header, 16, &u16::try_from(size)?.to_le_bytes())?;
    put(&mut header, 18, &1u16.to_le_bytes())?;
    put(&mut header, 20, &6u16.to_le_bytes())?;
    header[23] = u8::try_from(mips.len())?;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::tiger::texture::resident(&mut header, data.len())?;
    let data_symbol = format!("{symbol}-data");
    g.add(symbol, CUBE_TEMPLATE, &header, Some(&data_symbol), vec![])?;
    g.add(
        &data_symbol,
        native.reference(CUBE_TEMPLATE)?,
        &data,
        Some(symbol),
        vec![],
    )?;
    Ok(())
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
    texture_data(
        native,
        g,
        symbol,
        [width, height],
        if srgb { 29 } else { 28 },
        levels,
        &rgba,
    )
}

fn texture_data(
    native: &mut Reader,
    g: &mut Graph,
    symbol: &str,
    [width, height]: [usize; 2],
    format: u32,
    levels: u8,
    pixels: &[u8],
) -> Result<()> {
    let srgb = matches!(format, 29 | 72 | 75 | 78);
    let template = if srgb { 0x80ec31b9 } else { 0x80ec31bc };
    let mut header = native.tag(template, None)?.0.clone();
    put(&mut header, 0, &u32::try_from(pixels.len())?.to_le_bytes())?;
    put(&mut header, 4, &format.to_le_bytes())?;
    put(&mut header, 14, &u16::try_from(width)?.to_le_bytes())?;
    put(&mut header, 16, &u16::try_from(height)?.to_le_bytes())?;
    put(&mut header, 18, &1u16.to_le_bytes())?;
    put(&mut header, 20, &1u16.to_le_bytes())?;
    header[22] = 0;
    header[23] = levels;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::tiger::texture::resident(&mut header, pixels.len())?;
    let data = format!("{symbol}-data");
    g.add(symbol, template, &header, Some(&data), vec![])?;
    g.add(
        &data,
        native.reference(template)?,
        pixels,
        Some(symbol),
        vec![],
    )?;
    Ok(())
}

/// A level's decoded baked lighting: its three atlas textures, the scale of their lighting and the
/// average lit level that lighting is a share of, as [`crate::halo_reach::level::lightmap::Atlas`] describes
/// them, the light its opaque surfaces add for the level's large bloom, as a share of that level,
/// and the level's fog.
pub(in crate::halo_reach) struct Lightmap {
    pub lobe: String,
    pub ambient: String,
    pub direction: String,
    pub scale: f32,
    pub level: f32,
    pub radiance: f32,
    pub veil: [f32; 3],
    pub sun: Option<Analytical>,
    pub analytical: BTreeMap<u32, Analytical>,
    pub origin: [f32; 3],
    pub fog: Option<Fog>,
}

/// Adds a level's decoded baked lighting as three linear RGBA8 textures. Each keeps one level, as
/// smaller levels would blend neighboring charts of its atlas.
pub(in crate::halo_reach) fn lightmap_textures(
    native: &mut Reader,
    g: &mut Graph,
    atlas: &crate::halo_reach::level::lightmap::Atlas,
    veil: [f32; 3],
    sun: Option<Analytical>,
    fog: Option<Fog>,
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
        radiance: LIGHT_RADIANCE,
        veil,
        sun,
        analytical: BTreeMap::new(),
        origin: [0.; 3],
        fog,
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

/// Adds a sampler surfaces read their maps with: the native sampler `template` with the
/// U and V address and filter `modes` of the mappings that use it, and no
/// mip bias. The shell's sampler biases mips by -0.5, a sharpening Shadowkeep's own maps are made
/// for. Reach's maps tile far more densely, base maps up to 256 times and detail maps up to 250
/// times, and Reach sampled them unbiased.
fn sampler(native: &mut Reader, g: &mut Graph, template: u32, modes: [u32; 3]) -> Result<String> {
    let symbol = format!("sampler-{}-{}-{}", modes[0], modes[1], modes[2]);
    if g.nodes.iter().any(|n| n["symbol"] == symbol) {
        return Ok(symbol);
    }
    let header = native.tag(template, None)?.0.clone();
    let data = native.reference(template)?;
    let mut description = native.tag(data, None)?.0.clone();
    let filter = Payload(description.clone()).u32(0)?;
    ensure!(
        header == [0; 8] && description.len() == 52 && matches!(filter, ANISOTROPIC | TRILINEAR),
        "Native linear sampler differs"
    );
    for (offset, mode) in [(4, modes[0]), (8, modes[1]), (12, WRAP)] {
        put(&mut description, offset, &mode.to_le_bytes())?;
    }
    put(&mut description, 0, &modes[2].to_le_bytes())?;
    put(&mut description, MIP_BIAS, &0f32.to_le_bytes())?;
    let anisotropy = if modes[2] == ANISOTROPIC {
        MAX_ANISOTROPY
    } else {
        1
    };
    put(&mut description, 20, &anisotropy.to_le_bytes())?;
    let raw = format!("{symbol}-data");
    g.add(&symbol, template, &header, Some(&raw), vec![])?;
    g.add(&raw, data, &description, Some(&symbol), vec![])?;
    Ok(symbol)
}

/// Effect textures use the same checked source addressing and unbiased filtering as surfaces.
pub(in crate::halo_reach) fn texture_sampler(
    native: &mut Reader,
    graph: &mut Graph,
    template: u32,
    mapping: &material::Mapping,
) -> Result<String> {
    sampler(native, graph, template, sampler_modes(mapping)?)
}

/// A surface's pixel samplers: one per address and filter combination, then the shell's clamping
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
    modes: &[[u32; 3]],
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

/// The sample of the map in `slot` through its source sampler and mapping transform.
/// With `animated`, as for a level surface,
/// the transform's translation and scale animations run on the game clock, as the sky's do, so
/// Forge World's waterfalls scroll their detail maps down over 20 and 30 seconds.
#[allow(clippy::too_many_arguments)]
fn sample(
    c: &Cache,
    material: &Material,
    slot: usize,
    mapping: &material::Mapping,
    sampler: &str,
    animated: bool,
    notes: &mut Vec<String>,
) -> Result<String> {
    let transform = mapping.transform;
    let base = if material.decal() {
        decal::base(material, slot)
    } else if matches!(slot, WATER_COLOR_SLOT | WATER_SHAPE_SLOT) {
        "v9.xy"
    } else {
        "v3.xy"
    };
    let uv = if material.decal() && slot == decal::VECTOR {
        base.to_owned()
    } else if animated {
        animation::coordinates(mapping, base, notes)
    } else {
        format!(
            "{base}*float2({:.9},{:.9})+float2({:.9},{:.9})",
            transform[0], transform[1], transform[2], transform[3]
        )
    };
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
    notes: Vec<String>,
    /// Each bound texture's register and symbol.
    bindings: Vec<(usize, String)>,
    samples: BTreeMap<usize, String>,
    /// Address U, address V and filter of each sampler, by sampler index.
    samplers: Vec<[u32; 3]>,
    /// Each bound map's register, by map slot.
    registers: BTreeMap<usize, usize>,
    /// The sampler of the bound diffuse table, in map slot `VMF_TABLE_SLOT`.
    table: Option<String>,
    /// Resource declarations and selection helpers for source texture arrays.
    functions: String,
}

/// A terrain shader's blend map and its second to fourth layers' base maps take slots 6 to 9,
/// which terrain shaders leave free. The first layer keeps slots 0 and 1, and the layers' detail
/// maps take slots 11 to 14.
const TERRAIN_BLEND: usize = 6;
const TERRAIN_LAYERS: [usize; 4] = [0, 7, 8, 9];
const TERRAIN_DETAIL: [usize; 4] = [11, 12, 13, 14];
const TERRAIN_BUMP: [usize; 4] = [1, 20, 21, 22];
const TERRAIN_DETAIL_BUMP: [usize; 4] = [23, 24, 25, 26];
/// A standard level surface's detail normal map, for `bump_mapping` `detail`. Terrain keeps slots 11
/// to 14 for its layers' detail maps, and other surfaces leave them free. A lightmapped surface
/// binds its maps to consecutive registers whatever their slots, so slots past 15 serve as well.
const BUMP_DETAIL_SLOT: usize = 11;
/// A standard level surface's second and third detail maps, for the `two_detail`, `detail_blend` and
/// `three_detail_blend` albedos, and its second and third detail normal maps, for
/// `three_detail_blend` bump mapping.
const DETAIL2_SLOT: usize = 12;
const DETAIL3_SLOT: usize = 13;
const BUMP_DETAIL2_SLOT: usize = 14;
const BUMP_DETAIL3_SLOT: usize = 16;
/// A standard level surface's color mask, for the `color_mask` albedo.
const COLOR_MASK_SLOT: usize = 17;
const SPECULAR_MASK_SLOT: usize = 29;
/// A water surface's color texture and its shape texture, whose alpha fades it at the shore.
const WATER_COLOR_SLOT: usize = 18;
const WATER_SHAPE_SLOT: usize = 19;
const WATER_FOAM_SLOT: usize = 27;
const WATER_FOAM_DETAIL_SLOT: usize = 28;

/// Whether `material` is a terrain shader, which blends up to four layers by its blend map.
fn terrain(material: &Material) -> bool {
    material.tag.as_ref().is_some_and(|tag| tag.group == "rmtr")
}

/// A terrain shader's blend weights: the blend map's channels for its active layers. Missing
/// or black blend maps share weight among active layers through the source epsilon.
fn terrain_weights(material: &Material, samples: &BTreeMap<usize, String>) -> String {
    let blend = samples
        .get(&TERRAIN_BLEND)
        .map_or("float4(0,0,0,0)", String::as_str);
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
    format!("terrainWeights(({blend}),float4({active}))")
}

/// MCC adds 1e-8 to active channels before normalization. It affects the 0.04 cutoff as well as
/// black blend maps. Albedo and normals apply that cutoff separately from lighting's full blend.
const TERRAIN_FUNCTIONS: &str = "float4 terrainWeights(float4 w,float4 active){w=(max(w,0)+1e-8)*active;float total=((w.x+w.y)+w.z)+w.w;return total>0?w/total:float4(0,0,0,0);}";

/// MCC terrain adds each layer's signed bump and detail XY, blends those slopes and reconstructs Z.
fn terrain_normal(samples: &BTreeMap<usize, String>, weights: &str) -> String {
    let terms = (0..4)
        .map(|layer| {
            let xy = |slot: usize| {
                samples.get(&slot).map_or_else(
                    || "float2(0,0)".into(),
                    |sample| format!("(({sample}).xy*2-1)"),
                )
            };
            format!(
                "({}+{})*({weights})[{}]",
                xy(TERRAIN_BUMP[layer]),
                xy(TERRAIN_DETAIL_BUMP[layer]),
                layer
            )
        })
        .collect::<Vec<_>>()
        .join("+");
    format!("terrainNormal({terms})")
}

const TERRAIN_NORMAL: &str =
    "float3 terrainNormal(float2 xy){return float3(xy,sqrt(max(1-dot(xy,xy),0)));}";

/// The maps `material` binds and their samples. With `dense`, as for a lightmapped level material,
/// they take consecutive registers from zero and the material's diffuse table binds too.
#[allow(clippy::cognitive_complexity)]
fn textures(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    material: &Material,
    paint: Option<&crate::halo_reach::paint::Surface>,
    dense: bool,
) -> Result<TextureBindings> {
    if mux::is(material) {
        ensure!(paint.is_none(), "Mux surfaces cannot also take gear paint");
        return mux::textures(c, pages, native, g, material);
    }
    let mut bindings = Vec::new();
    let mut samples = BTreeMap::new();
    let mut notes = Vec::new();
    let mut registers = BTreeMap::new();
    let mut samplers = vec![[WRAP, WRAP, ANISOTROPIC]];
    let layers: &[(usize, &[&str], bool, bool)] = &[
        (TERRAIN_BLEND, &["blend_map"][..], false, false),
        (TERRAIN_LAYERS[1], &["base_map_m_1"][..], false, true),
        (TERRAIN_LAYERS[2], &["base_map_m_2"][..], false, true),
        (TERRAIN_LAYERS[3], &["base_map_m_3"][..], false, true),
        (TERRAIN_DETAIL[0], &["detail_map_m_0"][..], false, true),
        (TERRAIN_DETAIL[1], &["detail_map_m_1"][..], false, true),
        (TERRAIN_DETAIL[2], &["detail_map_m_2"][..], false, true),
        (TERRAIN_DETAIL[3], &["detail_map_m_3"][..], false, true),
        (TERRAIN_BUMP[1], &["bump_map_m_1"][..], true, false),
        (TERRAIN_BUMP[2], &["bump_map_m_2"][..], true, false),
        (TERRAIN_BUMP[3], &["bump_map_m_3"][..], true, false),
        (
            TERRAIN_DETAIL_BUMP[0],
            &["detail_bump_m_0"][..],
            true,
            false,
        ),
        (
            TERRAIN_DETAIL_BUMP[1],
            &["detail_bump_m_1"][..],
            true,
            false,
        ),
        (
            TERRAIN_DETAIL_BUMP[2],
            &["detail_bump_m_2"][..],
            true,
            false,
        ),
        (
            TERRAIN_DETAIL_BUMP[3],
            &["detail_bump_m_3"][..],
            true,
            false,
        ),
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
        let mut table = standard.to_vec();
        if dense {
            let option = |name: &str| material.options.get(name).map(String::as_str);
            if material.decal()
                && matches!(
                    option("albedo"),
                    Some("vector_alpha" | "vector_alpha_drop_shadow")
                )
            {
                table.push((decal::VECTOR, &["vector_map"][..], false, false));
                if option("albedo") == Some("vector_alpha_drop_shadow") {
                    table.push((decal::SHADOW, &["shadow_vector_map"][..], false, false));
                }
            }
            if matches!(
                option("specular_mask"),
                Some("specular_mask_from_texture" | "specular_mask_mult_diffuse")
            ) {
                table.push((
                    SPECULAR_MASK_SLOT,
                    &["specular_mask_texture"][..],
                    false,
                    false,
                ));
            }
            if matches!(
                option("bump_mapping"),
                Some("detail" | "three_detail_blend")
            ) {
                table.push((BUMP_DETAIL_SLOT, &["bump_detail_map"][..], true, false));
            }
            if option("bump_mapping") == Some("three_detail_blend") {
                table.push((BUMP_DETAIL2_SLOT, &["bump_detail_map2"][..], true, false));
                table.push((BUMP_DETAIL3_SLOT, &["bump_detail_map3"][..], true, false));
            }
            if matches!(
                option("albedo"),
                Some("two_detail" | "detail_blend" | "three_detail_blend")
            ) {
                table.push((DETAIL2_SLOT, &["detail_map2"][..], false, true));
            }
            if option("albedo") == Some("three_detail_blend") {
                table.push((DETAIL3_SLOT, &["detail_map3"][..], false, true));
            }
            if option("albedo") == Some("color_mask") {
                table.push((COLOR_MASK_SLOT, &["color_mask_map"][..], false, false));
            }
            if material.water() {
                table.push((WATER_COLOR_SLOT, &["watercolor_texture"][..], false, true));
                table.push((
                    WATER_SHAPE_SLOT,
                    &["global_shape_texture"][..],
                    false,
                    false,
                ));
                table.push((BUMP_DETAIL_SLOT, &["bump_detail_map"][..], true, false));
                if water::painted_foam(material) {
                    table.push((WATER_FOAM_SLOT, &["foam_texture"][..], false, true));
                    table.push((
                        WATER_FOAM_DETAIL_SLOT,
                        &["foam_texture_detail"][..],
                        false,
                        true,
                    ));
                }
            }
        }
        table
    };
    for (slot, roles, normal, srgb) in table {
        // The four-layer MCC permutation omits detail bumps to fit its source sampler budget.
        if terrain(material)
            && TERRAIN_DETAIL_BUMP.contains(&slot)
            && (0..4)
                .filter(|&layer| terrain::active(material, layer))
                .count()
                == 4
        {
            continue;
        }
        if terrain(material)
            && let Some(layer) = (0..4).find(|&layer| {
                [
                    TERRAIN_LAYERS[layer],
                    TERRAIN_DETAIL[layer],
                    TERRAIN_BUMP[layer],
                    TERRAIN_DETAIL_BUMP[layer],
                ]
                .contains(&slot)
            })
            && material
                .options
                .get(&format!("material_{layer}"))
                .is_some_and(|option| option == "off")
        {
            continue;
        }
        if slot == 9
            && !terrain(material)
            && paint.is_none_or(|paint| {
                !matches!(paint.mask, crate::halo_reach::paint::PaintMask::ChangeColor)
            })
        {
            continue;
        }
        if let Some(mapping) = material.mapping(roles) {
            let (symbol, bc5) = bound_texture(c, pages, native, g, mapping, normal, srgb)?;
            let target = if dense {
                bindings
                    .iter()
                    .find(|(_, existing)| existing == &symbol)
                    .map_or(bindings.len(), |(register, _)| *register)
            } else if paint.is_some() && slot >= 3 {
                slot + 6
            } else {
                slot
            };
            if !dense || !bindings.iter().any(|(register, _)| *register == target) {
                bindings.push((target, symbol));
            }
            registers.insert(slot, target);
            let index = sampler_index(&mut samplers, sampler_modes(mapping)?);
            let mut sampled = sample(
                c,
                material,
                slot,
                mapping,
                &sampler_name(index),
                dense,
                &mut notes,
            )?;
            if let Some(signed) = bc5 {
                let xy = if signed {
                    format!("({sampled}).rg")
                } else {
                    format!("({sampled}).rg*2-1")
                };
                sampled = format!("reachNormal({xy})");
            }
            samples.insert(slot, sampled);
        }
    }
    let mut table = None;
    if dense && let Some(mapping) = material.mapping(&["g_sample_vmf_diffuse"]) {
        let target = bindings.len();
        bindings.push((target, texture(c, pages, native, g, mapping, false, false)?));
        registers.insert(VMF_TABLE_SLOT, target);
        let index = sampler_index(&mut samplers, sampler_modes(mapping)?);
        table = Some(sampler_name(index));
    }
    Ok(TextureBindings {
        notes,
        bindings,
        samples,
        samplers,
        registers,
        table,
        functions: String::new(),
    })
}

/// The index of the sampler with address `modes` in `samplers`, which gains it when absent.
fn sampler_index(samplers: &mut Vec<[u32; 3]>, modes: [u32; 3]) -> usize {
    samplers
        .iter()
        .position(|existing| *existing == modes)
        .unwrap_or_else(|| {
            samplers.push(modes);
            samplers.len() - 1
        })
}

/// Filter zero is source trilinear on both base and detail roles. Bind a real trilinear
/// descriptor so the hardware derives the footprint in texels, including nonsquare images.
/// Other modes retain the anisotropic approximation until their MCC descriptors are recovered.
fn sampler_modes(mapping: &material::Mapping) -> Result<[u32; 3]> {
    let [u, v] = address_modes(mapping.address)?;
    Ok([
        u,
        v,
        if mapping.filter == 0 {
            TRILINEAR
        } else {
            ANISOTROPIC
        },
    ])
}

fn dye_tint(
    paint: Option<&crate::halo_reach::paint::Surface>,
    samples: &BTreeMap<usize, String>,
) -> Result<String> {
    let tinting = if let Some(paint) = paint {
        let mask = match paint.mask {
            crate::halo_reach::paint::PaintMask::Surface => "1.0".to_owned(),
            crate::halo_reach::paint::PaintMask::ChangeColor => format!(
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

/// Source area and environment specular use the VMF power table separately from diffuse.
/// The lookup and coefficient routes are retained. Analytical highlights, animated light
/// intensity and the source environment's analytical contribution remain separate limitations.
fn reflection(
    material: &Material,
    surface: &surface::Surface<'_>,
    environment: Option<&str>,
    samples: &BTreeMap<usize, String>,
    table: bool,
) -> Option<(String, [f32; 3])> {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    let (normal_tint, blend, glancing) = match option("material_model") {
        Some("two_lobe_phong") => (
            "normal_specular_tint",
            "albedo_specular_tint_blend",
            "glancing_specular_tint",
        ),
        Some("cook_torrance") => ("specular_tint", "albedo_blend", "fresnel_color"),
        _ => return None,
    };
    let texture_mask = samples
        .get(&SPECULAR_MASK_SLOT)
        .map(|sample| format!("({sample}).a"));
    let mask = match option("specular_mask") {
        Some("specular_mask_from_diffuse") => "unlit.a".to_owned(),
        Some("specular_mask_mult_diffuse") => format!("unlit.a*{}", texture_mask?),
        Some("specular_mask_from_texture") => texture_mask?,
        Some("no_specular_mask") => "1".to_owned(),
        _ => return None,
    };
    let mut coefficient = surface.scalar("specular_coefficient", 1.);
    if option("material_model") == Some("cook_torrance")
        && surface.boolean("use_material_texture") == Some(true)
    {
        let sample = samples.get(&2)?;
        let black = surface.scalar("material_texture_black_specular_multiplier", 1.);
        coefficient = format!("({coefficient})*lerp({black},1,({sample}).a)");
    }
    let normal = surface.color(normal_tint, [1.; 4]);
    let edge = surface.color(glancing, surface.constant(normal_tint).unwrap_or([1.; 4]));
    let blend = surface.scalar(blend, 0.);
    let steepness = surface.scalar("fresnel_curve_steepness", 5.);
    let tint = format!(
        "lerp(lerp({normal},unlit.rgb,{blend}),{edge},pow(1-saturate(dot(n,normalize(v5.xyz))),max({steepness},0)))"
    );
    let area = surface.scalar("area_specular_contribution", 0.);
    let mut roughness = surface.scalar("roughness", 0.5);
    if option("material_model") == Some("cook_torrance")
        && surface.boolean("use_material_texture") == Some(true)
    {
        let sample = samples.get(&2)?;
        let black = surface.scalar("material_texture_black_roughness", 1.);
        roughness = format!("lerp({black},{roughness},({sample}).a)");
    }
    let response = if table {
        format!(
            "3*reachSpecular(dot(reflect(-normalize(v5.xyz),n),lightDirection),lightLobe.a,{roughness})"
        )
    } else {
        // A missing source table does not justify replacing its angular response by a guessed
        // Phong exponent. Preserve only the known ambient part and report the missing input.
        "0".to_owned()
    };
    let basis =
        format!("3.141592654*max(lightLobeIrradiance*({response})+lightAmbientIrradiance,0)");
    let environment = if option("environment_mapping") == Some("per_pixel") {
        environment
            .map(|sample| {
                format!(
                    "+({sample})*{}*{}",
                    surface.color("env_tint_color", [1.; 4]),
                    surface.scalar("environment_map_specular_contribution", 0.)
                )
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    Some((
        format!("({tint})*{mask}*max({coefficient},0)*({basis})*({area}{environment})"),
        surface.constant("env_tint_color").unwrap_or([1.; 4])[..3]
            .try_into()
            .unwrap(),
    ))
}

/// Water retains source color, Fresnel and wave-normal reflection. Coverage still approximates
/// refraction depth because the converted forward pass has no Reach scene-depth input.
fn water_albedo(
    material: &Material,
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> String {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    let body_name = if option("watercolor") == Some("pure") {
        "water_color_pure"
    } else {
        "water_diffuse"
    };
    let body = match samples.get(&WATER_COLOR_SLOT) {
        Some(color) if option("watercolor") == Some("texture") => format!(
            "({color}).rgb*max({},0)",
            surface.scalar("watercolor_coefficient", 1.)
        ),
        _ => surface.color(body_name, [0.03, 0.08, 0.1, 1.]),
    };
    let bank = match samples.get(&WATER_SHAPE_SLOT) {
        Some(shape)
            if matches!(
                option("bankalpha"),
                Some("paint" | "from_shape_texture_alpha")
            ) =>
        {
            format!("saturate(({shape}).a)")
        }
        _ => "1".into(),
    };
    let fresnel = water::fresnel(surface);
    let murkiness = surface.scalar("water_murkiness", 8.);
    format!("float4({body},lerp(1-exp(-0.15*max({murkiness},0)),1,{fresnel})*{bank})")
}

/// Bump water uses its authored mapping transforms, including their animation functions.
fn water_normal(samples: &BTreeMap<usize, String>) -> String {
    match (samples.get(&1), samples.get(&BUMP_DETAIL_SLOT)) {
        (Some(bump), Some(detail)) => {
            format!("float4(normalize(float3((({bump}).xy+({detail}).xy)*2-2,1))*0.5+0.5,1)")
        }
        (Some(bump), None) => bump.clone(),
        _ => "float4(0.5,0.5,1,1)".into(),
    }
}

/// Evaluate baked VMF and analytical diffuse as separate source terms. The original MCC
/// static-lightmap shader adds both and divides diffuse irradiance by pi. The atlas already
/// includes that divisor and its average-level normalization. Analytical intensity does not.
fn level_lighting(
    lightmap: &Lightmap,
    selected: Option<Analytical>,
    hybrid: bool,
    bindings: &mut Vec<(usize, String)>,
    table: Option<&str>,
    reflection: Option<&str>,
    water: Option<&str>,
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
    let specular = reflection.unwrap_or("float3(0,0,0)");
    let sample = "float2 lightCoordinates=abs(v3.zw);float4 lightLobe=LightLobe.Sample(ClampSampler,lightCoordinates);float4 lightAmbient=LightAmbient.Sample(ClampSampler,lightCoordinates);float3 lightDirection=LightDirection.Sample(ClampSampler,lightCoordinates).xyz*2-1;if(v8.w>0.5){lightLobe=v6;lightAmbient=v7;lightDirection=v8.xyz;}lightDirection=normalize(lightDirection+float3(0,0,0.00001));";
    let direct = analytical_lighting(lightmap, selected, hybrid && water.is_none());
    let finish = if let Some(water) = water {
        water.to_owned()
    } else if hybrid {
        // Only the matching directional source can use the one native sun. Point lights and
        // other sky directions retain their static diffuse term in emitted radiance.
        "float3 baseColor=albedo.rgb;float3 directDiffuse=baseColor*lightDirectIrradiance;float3 bakedRadiance=baseColor*(lightResponse*lightLobeIrradiance+lightAmbientIrradiance)+lightSpecular;float3 sunlit=lightNativeSun?directDiffuse:float3(0,0,0);albedo.rgb=bakedRadiance+(lightNativeSun?float3(0,0,0):directDiffuse*lightDirectCosine);".to_owned()
    } else {
        "albedo.rgb=albedo.rgb*(lightResponse*lightLobeIrradiance+lightAmbientIrradiance+lightDirectIrradiance*lightDirectCosine)+lightSpecular;".to_owned()
    };
    let lighting = format!(
        "{sample}float lightResponse={response};float3 lightLobeIrradiance=(v8.w>0.5?lightLobe.rgb:lightLobe.rgb*lightLobe.rgb)*{scale:.9};float3 lightAmbientIrradiance=(v8.w>0.5?lightAmbient.rgb:lightAmbient.rgb*lightAmbient.rgb)*{scale:.9};float3 lightSpecular={specular};{direct}{finish}",
        scale = lightmap.scale
    );
    let declarations = format!(
        "Texture2D<float4> LightLobe:register(t{first});Texture2D<float4> LightAmbient:register(t{});Texture2D<float4> LightDirection:register(t{});",
        first + 1,
        first + 2
    );
    Ok((lighting, declarations))
}

/// MCC's analytical diffuse uses the selected direction or normalized point-to-surface vector,
/// intensity and squared baked visibility. Vertex lighting already squares visibility before
/// interpolation, while pixel lightmaps carry its unsquared value. Gel textures and animated
/// intensities are separate runtime inputs. Opaque surfaces selecting the global sun use native
/// shadow visibility instead of multiplying it by the baked shadow again. Transparent surfaces,
/// water and other analytical lights retain source visibility. Model placements use sky zero
/// and retain their sampled indirect light.
fn analytical_lighting(
    lightmap: &Lightmap,
    light: Option<Analytical>,
    native_shadows: bool,
) -> String {
    let Some(light) = light.filter(|light| light.present()) else {
        return "bool lightPlaced=v3.z<0;bool lightNativeSun=false;float lightDirectCosine=0;float3 lightDirectIrradiance=0;".into();
    };
    let [x, y, z] = light.vector;
    let direction = if light.directional {
        format!("normalize(float3({x:.9},{y:.9},{z:.9}))")
    } else {
        let point: [f32; 3] = std::array::from_fn(|k| light.vector[k] * 3.048 + lightmap.origin[k]);
        format!("float3({:.9},{:.9},{:.9})-v4", point[0], point[1], point[2])
    };
    let native = light.directional
        && lightmap
            .sun
            .is_some_and(|sun| sun.vector == light.vector && sun.intensity == light.intensity);
    let [r, g, b] = light
        .intensity
        .map(|v| v / (std::f32::consts::PI * lightmap.level));
    let visibility = if native && native_shadows {
        "1"
    } else {
        "lightVisibilitySquared"
    };
    format!(
        "bool lightPlaced=v3.z<0;bool lightNativeSun={native};float3 lightDirectDirection={direction};lightDirectDirection*=rsqrt(max(dot(lightDirectDirection,lightDirectDirection),0.00000001));float lightDirectCosine=saturate(dot(n,lightDirectDirection));float lightVisibility=saturate(lightAmbient.a);float lightVisibilitySquared=v8.w>0.5?lightVisibility:lightVisibility*lightVisibility;float3 lightDirectIrradiance=float3({r:.9},{g:.9},{b:.9})*{visibility};"
    )
}

/// Pack source radiance with an emission/direct ratio and a quantized visibility scale. Keeping
/// full direct visibility when emission dominates can put the shared color below one stored
/// color step. Scale both terms before packing emission, then divide by their decoded sum.
/// Round visibility upward so its decoded direct contribution does not fall below the requested
/// share. The two terms still share one color, so independent chromaticity in shadows remains
/// approximate. No threshold switches between two shaded colors.
///
/// Native global-light programs raise visibility to twice channel 83's exponent. Both checked
/// Shadowkeep global tables supply 0.35, giving the 0.7 power used here. This is the stock default,
/// not a captured live bank value. Native exponent overrides, sun intensity, environment response
/// and exposure remain calibration limits.
///
/// Target one holds the normal at length `0.75 + 0.25 * smoothness`, which the deferred passes
/// decode as `4 * length - 3`, so a surface's Reach roughness stores as `1 - roughness`. Stored
/// as roughness, fully rough cliffs decoded as mirrors and reflected the sky at its sharpest level.
///
/// Target-zero alpha stays below 0.5 on every pixel, as on stock plain surfaces. Above 0.5 the
/// deferred shading reads it as an iridescence row: rows 0 to 11 tint the specular, and rows 12
/// to 126 are magenta placeholders whose zero alpha removes all light. A surface that switched
/// between 0.04 and 1 at its Reach shadow edges drew oily sheens and black patches wherever
/// those values blended. A pixel outside the native sun loses native ambient light through its
/// occlusion (`o2.w` 0) instead.
fn hybrid_finish(level: f32, veil: [f32; 3], radiance: f32) -> String {
    format!(
        "float exposure={scale:.9}*frameData[1].z/max(frameData[1].x*frameData[1].w,0.000001);float3 radiance=(max(albedo.rgb+emission*{inverse:.9},0)*fog.a+fog.rgb*{inverse:.9}+float3({veil_r:.9},{veil_g:.9},{veil_b:.9}))*exposure;sunlit=max(sunlit,0)*fog.a*exposure;float sunLevel=dot(sunlit,float3(0.2126,0.7152,0.0722));bool receives=lightNativeSun&&sunLevel>0.000001;float cosine=receives?lightDirectCosine:0;float3 total=radiance+sunlit*cosine;float peak=max(total.r,max(total.g,total.b));float ratio=dot(radiance,float3(0.2126,0.7152,0.0722))/max(sunLevel,0.000001);float desiredDirect=receives?saturate(peak/max(ratio+cosine,0.000001)):0;float packedVisibility=ceil(pow(desiredDirect,1.0/0.7)*255)/255;float directFactor=receives?pow(max(packedVisibility,0.0001),0.7):0;float wanted=receives?max(ratio*directFactor,max(peak-directFactor*cosine,0)):peak;float encoded=0.5*(1+2.0/255.0+saturate((log2(wanted+1.0/128.0)+7)/13));float packed=min(ceil(encoded*255)/255,1);float decoded=exp2(13*saturate(2*packed-(1+2.0/255.0))-7)-1.0/128.0;o0=float4(saturate(total/max(decoded+directFactor*cosine,0.000001)),0.04);o1=float4(saturate(n*((1-roughness)*0.125+0.375)+0.5),0);o2=float4(0,packed,0,receives?packedVisibility:0);",
        scale = radiance * REFERENCE_ILLUMINATION,
        inverse = 1. / level,
        veil_r = veil[0],
        veil_g = veil[1],
        veil_b = veil[2]
    )
}

/// A lightmapped level's transparent surface's output, as a sky surface's transparent stage writes
/// its own: its lit color plus its self illumination over the average lit `level`, times the
/// surfaces' [`LIGHT_RADIANCE`] and the frame's exposure (`cb13[1].z` times
/// [`REFERENCE_ILLUMINATION`], as native light scales), blended by its
/// blend mode. The opaque surface behind it already holds the large bloom's floor. Additive
/// surfaces add their self illumination. A multiplying surface, such as a dirt decal, darkens what
/// lies behind it by its unlit albedo's mean faded by its coverage, the nearest a premultiplied
/// blend comes to a product. As Reach's blended programs do, the level's fog (`fog`) dims a
/// blended surface and adds its inscatter, and only dims an additive one.
fn baked_transparent_finish(material: &Material, level: f32, radiance: f32) -> String {
    if let Some(finish) = decal::multiplication(material) {
        return finish;
    }
    let scale = format!("{:.9}*frameData[1].z", radiance * REFERENCE_ILLUMINATION);
    let inverse = 1. / level;
    let emission = format!("emission*{inverse:.9}");
    match material.options.get("blend_mode").map(String::as_str) {
        Some("additive") if material.decal() => format!("o0=float4(albedo.rgb*fog.a*{scale},0);"),
        Some("additive") => format!("o0=float4({emission}*fog.a*{scale},0);"),
        Some("multiply") => "float3 product=lerp(float3(1,1,1),unlit.rgb,saturate(unlit.a));o0=float4(0,0,0,1-saturate(dot(product,float3(1,1,1))/3));".to_owned(),
        Some("pre_multiplied_alpha") if decal::premultiplied(material) => format!(
            "float coverage=saturate(albedo.a);o0=float4((albedo.rgb*fog.a+({emission}*fog.a+fog.rgb*{inverse:.9})*coverage)*{scale},coverage);"
        ),
        _ => format!(
            "float coverage=saturate(albedo.a);o0=float4(((albedo.rgb+{emission})*fog.a+fog.rgb*{inverse:.9})*{scale}*coverage,coverage);"
        ),
    }
}

#[allow(clippy::cognitive_complexity)]
pub(in crate::halo_reach) fn build(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    materials: &BTreeMap<u32, Material>,
    binding: Binding<'_>,
) -> Result<Vec<Value>> {
    build_with_progress(c, pages, native, g, materials, binding, &mut |_| {})
}

#[allow(clippy::cognitive_complexity, clippy::too_many_arguments)]
pub(in crate::halo_reach) fn build_with_progress(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    materials: &BTreeMap<u32, Material>,
    binding: Binding<'_>,
    report_progress: &mut dyn FnMut(&str),
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
    ensure!(
        !binding.statics || binding.rigid,
        "Static instances are rigid"
    );
    let motion = if binding.rigid {
        RIGID_MOTION
    } else {
        "float4 r0=0,r1=0,r2=0;[unroll] for(uint i=0;i<4;i++){uint row=8+motionPalette[bone[i]]*3;r0+=objectData[row]*weight[i];r1+=objectData[row+1]*weight[i];r2+=objectData[row+2]*weight[i];}"
    };
    let vertex = VERTEX
        .replace("$SOURCE_PALETTE$", &lookup)
        // Unused HLSL arguments can remain in the compiled input signature. Rigid maps
        // use layout 139, which has no bone inputs in either its color or shadow pass.
        .replace(
            "$SKIN_INPUTS$",
            if binding.rigid {
                ""
            } else {
                "float4 weight:BLENDWEIGHT0,uint4 bone:BLENDINDICES0,"
            },
        )
        .replace(
            "$PIXEL_POSITION$",
            if binding.lightmap.is_some() {
                "world"
            } else {
                "p.xyz"
            },
        )
        .replace("$MOTION$", motion);
    let vertex = if binding.statics {
        static_vertex(&vertex)?
    } else {
        vertex
    };
    let vertex_data_register = if binding.statics {
        STATIC_VERTEX_DATA_REGISTER
    } else {
        0
    };
    program(
        native,
        g,
        &format!("{}vertex", binding.prefix),
        shell.u32(0x48)?,
        &vertex
            .replace("$SHADOW_RESOURCE$", "")
            .replace("$SHADOW_PROJECTION$", "")
            .replace(
                "$VERTEX_LIGHT_DECL$",
                &if binding.vertex_data.is_some() {
                    vertex_data::DECLARATION.replace(
                        "register(t0)",
                        &format!("register(t{vertex_data_register})"),
                    )
                } else {
                    String::new()
                },
            )
            .replace(
                "$VERTEX_LIGHT_READ$",
                if binding.vertex_data.is_some() {
                    vertex_data::READ
                } else {
                    "o6=0;o7=0;o8=0;"
                },
            ),
        true,
    )?;
    // Surfaces read their maps through copies of the shell's sampler with source addressing and
    // trilinear filtering where requested, and a
    // lightmap atlas through the clamping original.
    let sampler_rows = shell.array(0x308, 16, None)?;
    let clamped = shell.bytes::<16>(
        *sampler_rows
            .get(CLAMPED_SAMPLER)
            .context("Native sampler table is missing")?,
    )?;
    let sampler_class = shell.u32(sampler_rows[0] - 8)?;
    let mut report = Vec::new();
    for (index, (&key, material)) in materials.iter().enumerate() {
        report_progress(&format!(
            "Converting Material {}/{} / {}",
            index + 1,
            materials.len(),
            material
                .tag
                .as_ref()
                .map_or("Unnamed Material", |tag| tag.path.as_str())
        ));
        let surface = surface::Surface::new(material, &binding, key);
        let paint = binding.paint.get(&key);
        let display = decimal_display(c, pages, material)?;
        let animated = display.is_some();
        let display = display.unwrap_or_default();
        let dense = binding.lightmap.is_some()
            || terrain(material)
            || material.water()
            || material.decal()
            || mux::is(material);
        let TextureBindings {
            mut notes,
            mut bindings,
            samples,
            samplers: modes,
            registers,
            table,
            functions: texture_functions,
        } = textures(c, pages, native, g, material, paint, dense)?;
        let samplers = samplers(native, g, clamped, &modes)?;
        let mut samples = samples;
        notes.extend(surface.parameters.notes.iter().cloned());
        let parameter_source = &surface.parameters.source;
        let curve_functions = if samples
            .values()
            .any(|sample| sample.contains("reachPeriodic("))
            || parameter_source.contains("reachPeriodic(")
        {
            animation::periodic(native, g, &mut bindings)?
        } else {
            String::new()
        };
        let mut wave_functions = String::new();
        if material.water() {
            ensure!(
                !water::painted_foam(material)
                    || [WATER_SHAPE_SLOT, WATER_FOAM_SLOT, WATER_FOAM_DETAIL_SLOT]
                        .iter()
                        .all(|slot| samples.contains_key(slot)),
                "Painted water foam is missing its shape or foam textures"
            );
            let normal = if material
                .options
                .get("waveshape")
                .is_some_and(|shape| shape == "bump")
            {
                water_normal(&samples)
            } else {
                let (functions, normal) =
                    water::waves(c, pages, native, g, material, &surface, &mut bindings)
                        .with_context(|| {
                            format!(
                                "Water waves for {}",
                                material.tag.as_ref().map_or("unnamed", |tag| &tag.path)
                            )
                        })?;
                wave_functions = functions;
                normal
            };
            samples.insert(1, normal);
            samples.remove(&BUMP_DETAIL_SLOT);
        }
        let (environment_declarations, environment) = if binding.lightmap.is_some() {
            environment::bind(
                c,
                pages,
                native,
                g,
                material,
                &surface,
                &mut bindings,
                &mut notes,
            )?
        } else {
            (String::new(), None)
        };
        let specular_table = if binding.lightmap.is_some() {
            specular::bind(c, pages, native, g, material, &mut bindings)?
        } else {
            None
        };
        if binding.lightmap.is_some()
            && matches!(
                material.options.get("material_model").map(String::as_str),
                Some("two_lobe_phong" | "cook_torrance")
            )
            && specular_table.is_none()
        {
            notes.push("The source VMF specular table is absent. Area and environment specular retain only the ambient basis.".into());
        }
        let reflected = match binding.lightmap {
            Some(lightmap) if terrain(material) => terrain::reflection(
                &surface,
                table.as_deref(),
                environment.as_deref(),
                lightmap.level,
            ),
            Some(_) if !surface.transparent() => reflection(
                material,
                &surface,
                environment.as_deref(),
                &samples,
                specular_table.is_some(),
            ),
            _ => None,
        };
        let water_lighting = binding
            .lightmap
            .filter(|_| material.water())
            .map(|lightmap| {
                water::lighting(&surface, &samples, environment.as_deref(), lightmap.level)
            });
        let (lighting, lighting_declarations) = match binding.lightmap {
            // Transparent surfaces evaluate their analytical term here, after native lighting.
            Some(lightmap) => level_lighting(
                lightmap,
                lightmap.analytical.get(&key).copied(),
                !surface.transparent(),
                &mut bindings,
                table.as_deref(),
                reflected
                    .as_ref()
                    .map(|(expression, _)| expression.as_str()),
                water_lighting.as_deref(),
            )?,
            None => (String::new(), String::new()),
        };
        let tint = surface.constant("albedo_color").unwrap_or([1.; 4]);
        let terrain_parameters = if terrain(material) {
            terrain::parameters(&surface, &terrain_weights(material, &samples))
        } else {
            String::new()
        };
        let weights = terrain(material).then_some("terrainDraw");
        let mux_parameters = mux::parameters(&surface, &samples);
        let mut color = if material.water() {
            water_albedo(material, &surface, &samples)
        } else if mux::is(material) {
            mux::albedo(&samples)?
        } else {
            albedo(material, &samples, &surface, weights)
        };
        if let Some(decal_color) = decal::albedo(material, &surface, &samples)? {
            color = decal_color;
        }
        // Glass replaces its lighting with the blend its compiled programs write.
        let lighting = if material
            .decal
            .as_ref()
            .is_some_and(|decal| decal.render_pass == 1)
        {
            ensure!(
                surface.transparent(),
                "Post-lighting opaque decals require a separate native render path"
            );
            String::new()
        } else if material.glass() {
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
        // Reach's `detail` bump mapping, read from `ca_heh_catwalk_floor1`'s compiled albedo entry
        // point: both normals unit length, the detail's XY added to the bump's, the bump's Z kept,
        // then normalized. `three_detail_blend`, read from Forge World's `big_cliffs`, adds the
        // three detail normals' XY blended by the base map's alpha as its albedo blends them.
        let unit_xy = |s: &str| format!("normalize(({s}).xyz*2-1).xy");
        let detail_xy = match (
            samples.get(&BUMP_DETAIL_SLOT),
            samples.get(&BUMP_DETAIL2_SLOT),
            samples.get(&BUMP_DETAIL3_SLOT),
        ) {
            (Some(d1), Some(d2), Some(d3)) => {
                let (first, second) = three_detail_weights(&surface, &samples);
                Some(format!(
                    "lerp(lerp({},{},{first}),{},{second})",
                    unit_xy(d1),
                    unit_xy(d2),
                    unit_xy(d3)
                ))
            }
            (Some(d1), None, None) => Some(unit_xy(d1)),
            _ => None,
        };
        let normal = if let Some(weights) = weights {
            terrain_normal(&samples, weights)
        } else {
            samples
                .get(&1)
                .map(|s| match &detail_xy {
                    Some(d) => format!(
                        "normalize(float3({}+{d},normalize(({s}).xyz*2-1).z))",
                        unit_xy(s)
                    ),
                    None => format!("({s}).xyz*2-1"),
                })
                .unwrap_or_else(|| "float3(0,0,1)".into())
        };
        // Cook-Torrance uses analytical roughness for direct light and ordinary roughness
        // for its baked VMF lookup. Only its enabled material texture blends through alpha.
        // A texture bound by another model does not define a gloss channel. Diffuse-only
        // surfaces keep the rough native response instead of gaining a reflected highlight.
        let rough = if terrain(material) {
            terrain::roughness(material)
        } else if mux::is(material) {
            "1".into()
        } else if material.options.get("material_model").map(String::as_str)
            == Some("cook_torrance")
        {
            let name = if surface.constant("analytical_roughness").is_some() {
                "analytical_roughness"
            } else {
                "roughness"
            };
            let rough = surface.scalar(name, 0.48);
            let rough = if surface.boolean("use_material_texture") == Some(true) {
                let sample = samples
                    .get(&2)
                    .context("Material texture enabled without a bound texture")?;
                let black = surface.scalar("material_texture_black_roughness", 1.);
                format!("lerp({black},{rough},({sample}).a)")
            } else {
                rough
            };
            format!("clamp({rough},0.05,1)")
        } else {
            match material.options.get("material_model").map(String::as_str) {
                Some("two_lobe_phong") => {
                    format!("clamp({},0.05,1)", surface.scalar("roughness", 0.48))
                }
                Some("diffuse_only" | "none" | "flat") => "1".into(),
                _ => "0.48".into(),
            }
        };
        let diffuse = if terrain(material) {
            "albedo.rgb*=terrainDiffuse;".to_owned()
        } else if binding.lightmap.is_some() && !material.water() && !material.glass() {
            format!(
                "albedo.rgb*=max({},0);",
                surface.scalar("diffuse_coefficient", 1.)
            )
        } else {
            String::new()
        };
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
        // A decal draws only within its own coordinates ([`material::Material::decal`]). Drawn
        // with a clamped texture past them, Boardwalk's dirt decals smeared their edge texels into
        // streaks and flat brown across its floors.
        let clip = if material.decal() {
            format!("clip(float4(v3.xy,1-v3.xy));{clip}")
        } else {
            clip
        };
        let output = surface.output();
        let (finish, frame) = match binding.lightmap {
            Some(lightmap) if !surface.transparent() => (
                hybrid_finish(lightmap.level, lightmap.veil, lightmap.radiance),
                FRAME,
            ),
            Some(lightmap) => (
                baked_transparent_finish(material, lightmap.level, lightmap.radiance),
                FRAME,
            ),
            None => (surface.finish(), ""),
        };
        // A lightmapped surface's finish reads the level's fog, or none where the level has none.
        let finish = match binding.lightmap.map(|lightmap| lightmap.fog.is_some()) {
            Some(true) => format!("float4 fog=reachFog(v5.xyz,v5.w);{finish}"),
            Some(false) => format!("float4 fog=float4(0,0,0,1);{finish}"),
            None => finish,
        };
        // An animated transform reads the game clock from the frame constants.
        let clock = samples
            .values()
            .any(|sample| sample.contains("reachPhase("))
            || parameter_source.contains("reachPhase(");
        let frame = if clock && frame.is_empty() {
            FRAME
        } else {
            frame
        };
        let time = if clock {
            "float time=frameData[0].x;"
        } else {
            ""
        };
        let detail_slots = weights.as_ref().map_or(&[][..], |_| &TERRAIN_DETAIL[..]);
        let declarations = if dense {
            // Roles can share one texture binding. HLSL requires one resource declaration per
            // register, even when the native bindings point at the same image.
            let mut declared = BTreeMap::new();
            let mut declarations = String::new();
            for (slot, register) in &registers {
                if let Some(first) = declared.get(register) {
                    declarations.push_str(&format!("\n#define Map{slot} Map{first}\n"));
                } else {
                    declared.insert(register, slot);
                    declarations.push_str(&format!(
                        "Texture2D<float4> Map{slot}:register(t{register});\n"
                    ));
                }
            }
            declarations
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
        let decal_coordinates = decal::coordinates(material, &surface);
        let decal_tint = decal::tint(material, &surface);
        let functions = format!(
            "float4 reachNormal(float2 xy){{return float4(float3(xy,sqrt(saturate(1-dot(xy,xy))))*0.5+0.5,1);}}\n{TERRAIN_NORMAL}{}{}{}{}",
            animation::CLOCK,
            surface::MULTILAYER_FUNCTIONS,
            weights.as_ref().map_or("", |_| TERRAIN_FUNCTIONS),
            binding
                .lightmap
                .and_then(|lightmap| lightmap.fog)
                .map_or_else(String::new, |fog| fog.functions())
        );
        let sampler_declarations = &samplers.declarations;
        let text = format!(
            r#"
{declarations}
{lighting_declarations}
{frame}
{dye}
{display}
{functions}
{curve_functions}
{sampler_declarations}
{texture_functions}
{environment_declarations}
{specular_declarations}
{wave_functions}
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,float4 v5:TEXCOORD5,float4 v6:TEXCOORD6,float4 v7:TEXCOORD7,float4 v8:TEXCOORD8,float4 v9:TEXCOORD9,uint front:SV_isFrontFace0,
{output}) {{
{time}{parameter_source}{decal_coordinates}{terrain_parameters}{mux_parameters}float3 n={normal};n=normalize(n.x*v1.xyz+n.y*v2.xyz+n.z*v0.xyz);float4 albedo={color};{decal_tint}{tinting}{clip}float4 unlit=albedo;{diffuse}{lighting}
float3 emission={emission};float roughness={rough};{finish}
}}
"#,
            specular_declarations = specular_table.as_deref().unwrap_or("")
        );
        let shader = format!("{}pixel-{key:08X}", binding.prefix);
        notes.extend(surface.parameters.unused(&text));
        decal::notes(material, &mut notes);
        if material.water() {
            notes.push("Water retains source slope animation and uses separate body coordinates where the geometry supplies them. Map reports count base-UV fallbacks. Painted foam and source reflection require baked lighting. Refraction depth, ripple interactions and geometry displacement remain untranslated.".into());
        }
        program(native, g, &shader, shell.u32(0x2c8)?, &text, false)?;
        let (template, envelope, stage) = if surface.transparent() {
            (0x815246aa, &transparent_shell, 7)
        } else {
            (0x80ec270d, &shell, 0)
        };
        let mut bytes = fresh_surface(envelope)?;
        if binding.statics {
            for offset in [24, 28] {
                let current = Payload(bytes.clone()).u32(offset)?;
                put(
                    &mut bytes,
                    offset,
                    &((current & !OBJECT_SCOPE) | STATIC_SCOPE).to_le_bytes(),
                )?;
            }
        }
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
        let mut patches = surface_patches(
            &bytes,
            &format!("{}vertex", binding.prefix),
            &shader,
            &bindings,
            &samplers.symbols,
        )?;
        if let Some(symbol) = binding.vertex_data {
            let mut row = vertex_data_register.to_le_bytes().to_vec();
            row.extend(u32::MAX.to_le_bytes());
            append(&mut bytes, 0x50, 0x80807211, &row, 8)?;
            let at = Payload(bytes.clone()).array(0x50, 8, None)?[0];
            patches.push(json!({"offset":at+4,"symbol":symbol}));
        }
        g.add(
            &format!("{}material-{key:08X}", binding.prefix),
            template,
            &bytes,
            None,
            patches,
        )?;
        report.push(json!({"source":material.tag,"source_properties":material.properties,"cached_decal":material.decal,"effective_constants":surface.constants,"animation_notes":notes,"animated_parameters":surface.parameters.bindings(),"options":material.options,"bound_textures":bindings,"albedo_color":tint,"emission_color":emission_tint,"emission_scale":emission_scale,"alpha":material.alpha_mode(),"native_stage":stage,"baked_emission":binding.lightmap.is_some()&&!surface.transparent(),"environment_reflection":reflected.as_ref().map(|(_, scale)| scale),"paint":paint.map(|paint| json!({"channel":paint.channel+10,"mask":paint.mask,"constant_buffer":paint.slot,"scope_mask":paint.scopes,"method":"Native equipped dye color multiplied through the authored surface mask. A missing or entirely empty dye record preserves source color."})),"limits":["Opaque surfaces use the native deferred approximation. Transparent surfaces use premultiplied output. Decimal ammunition frame functions are bound to the live magazine input. Time-driven texture scale and translation use the Reach mapping evaluator. Unbound material inputs are reported in animation_notes. Scalar and color overlays use the source constant routes. Activation events, unbound object inputs and plasma depth fading remain limited. Explicit overrides select a static appearance state."]}));
    }
    shadow(native, g, &vertex, binding.prefix)?;
    Ok(report)
}

/// The two blend weights of a `three_detail_blend` surface: with `a` its base map's alpha times
/// `blend_alpha`, saturated, the first detail gives way to the second by `min(2a, 1)` and that
/// blend to the third by `max(2a - 1, 0)`.
fn three_detail_weights(
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> (String, String) {
    let blend = surface.scalar("blend_alpha", 1.);
    let alpha = samples.get(&0).map_or_else(
        || "1".into(),
        |base| format!("saturate(({base}).a*{blend})"),
    );
    (format!("min(2*{alpha},1)"), format!("max(2*{alpha}-1,0)"))
}

/// A surface's albedo as Reach's `albedo.fx` forms it for its albedo option. Each detail map's
/// color is scaled by [`DETAIL_MULTIPLIER`], and alpha, the specular mask and coverage, is the
/// product or blend of the maps' alphas. Only `default`, `simple`, `constant_color` and
/// `color_mask` read `albedo_color`. `two_detail` multiplies both details in, `detail_blend` blends
/// the second detail over the first by the base map's alpha times `blend_alpha`, and
/// `three_detail_blend` walks from the first detail to the second over that factor's lower half and
/// to the third over its upper. `color_mask` tints by each of its mask's three channels toward
/// `albedo_color`, `albedo_color2` and `albedo_color3` over `neutral_gray`. Options without a
/// translation keep the base map times `albedo_color`, with the detail map for those named for
/// details. A terrain shader instead mixes its layers by their blend `weights`, each at its own
/// tiling and each multiplied by its own detail map.
fn albedo(
    material: &Material,
    samples: &BTreeMap<usize, String>,
    surface: &surface::Surface<'_>,
    weights: Option<&str>,
) -> String {
    let base = samples
        .get(&0)
        .cloned()
        .unwrap_or_else(|| "float4(1,1,1,1)".into());
    let tint = surface.value("albedo_color", [1.; 4]);
    let option = material.options.get("albedo").map(String::as_str);
    let details = (
        samples.get(&5),
        samples.get(&DETAIL2_SLOT),
        samples.get(&DETAIL3_SLOT),
    );
    let dm = DETAIL_MULTIPLIER;
    let detail = |d: &str| format!("float4({dm:.6}*({d}).rgb,({d}).a)");
    let mut color = match (option, details) {
        (Some("constant_color"), _) => tint.clone(),
        (Some("two_detail" | "two_detail_black_point"), (Some(d1), Some(d2), _)) => {
            format!("({base})*{}*{}", detail(d1), detail(d2))
        }
        (Some("detail_blend"), (Some(d1), Some(d2), _)) => {
            let blend = format!("saturate(({base}).a*{})", surface.scalar("blend_alpha", 1.));
            format!(
                "float4(({base}).rgb,1)*{}",
                detail(&format!("lerp({d1},{d2},{blend})"))
            )
        }
        (Some("three_detail_blend"), (Some(d1), Some(d2), Some(d3))) => {
            let (first, second) = three_detail_weights(surface, samples);
            format!(
                "float4(({base}).rgb,1)*{}",
                detail(&format!("lerp(lerp({d1},{d2},{first}),{d3},{second})"))
            )
        }
        (Some("color_mask"), (Some(d), _, _)) => {
            let gray = surface.color("neutral_gray", [1.; 4]);
            let mask = samples
                .get(&COLOR_MASK_SLOT)
                .map_or_else(|| "(float4(0,0,0,0))".into(), |m| format!("({m})"));
            let channel = |k: &str, name: &str| {
                let c = surface.value(name, [1.; 4]);
                format!("(1-{mask}.{k}+{mask}.{k}*float4(({c}).rgb/max({gray},0.000001),({c}).a))")
            };
            format!(
                "({base})*{}*{}*{}*{}",
                detail(d),
                channel("x", "albedo_color"),
                channel("y", "albedo_color2"),
                channel("z", "albedo_color3")
            )
        }
        (Some("simple"), _) | (None, _) => format!("({base})*{tint}"),
        (Some("default"), (Some(d), _, _)) => format!("({base})*{}*{tint}", detail(d)),
        (Some(m), (Some(d), _, _)) if m.contains("detail") => format!("({base})*{}", detail(d)),
        _ => format!("({base})*{tint}"),
    };
    // A decal's `diffuse_plus_alpha_mask` albedo takes its coverage from its alpha map.
    if material
        .options
        .get("albedo")
        .is_some_and(|m| matches!(m.as_str(), "diffuse_plus_alpha_mask" | "diffuse_plus_alpha"))
        && let Some(mask) = samples.get(&3)
    {
        color = format!("float4(({color}).rgb,({mask}).a*({tint}).a)");
    }
    if let Some(weights) = weights {
        let [a, b, c, d] = std::array::from_fn(|layer| {
            let base = samples
                .get(&TERRAIN_LAYERS[layer])
                .cloned()
                .unwrap_or_else(|| "float4(0,0,0,1)".into());
            let base = terrain::base(surface, layer, base);
            match samples.get(&TERRAIN_DETAIL[layer]) {
                Some(detail) => {
                    format!(
                        "float4(({base}).rgb*{DETAIL_MULTIPLIER:.6}*({detail}).rgb,({base}).a*({detail}).a)"
                    )
                }
                None => base,
            }
        });
        let tint = surface.scalar("global_albedo_tint", 1.);
        color = format!("mul({weights},float4x4({a},{b},{c},{d}))*float4({tint},{tint},{tint},1)");
    }
    color
}

/// The patches of surface record `bytes`, whose pixel program is `shader`: its two programs, then
/// each bound texture's row in its texture table and each map sampler's row in its sampler table.
fn surface_patches(
    bytes: &[u8],
    vertex: &str,
    shader: &str,
    bindings: &[(usize, String)],
    sampler_symbols: &[String],
) -> Result<Vec<Value>> {
    let record = Payload(bytes.to_vec());
    let mut patches = vec![
        json!({"offset":0x48,"symbol":vertex}),
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

fn shadow(native: &mut Reader, g: &mut Graph, vertex: &str, prefix: &str) -> Result<()> {
    let template = native.tag(0x80ec271d, Some(0x808071e8))?;
    // The native shadow pass supplies the light's atlas rectangle at t2.
    // Its projection is distinct from the full-view surface projection even
    // though both use the same object pose and view constants.
    let vertex = vertex
        .replace("$VERTEX_LIGHT_DECL$", "")
        .replace("$VERTEX_LIGHT_READ$", "o6=0;o7=0;o8=0;")
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
        &format!("{prefix}shadow-vertex"),
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
        &format!("{prefix}shadow-pixel"),
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
        &format!("{prefix}material-shadow"),
        0x80ec271d,
        &bytes,
        None,
        vec![
            json!({"offset":0x48,"symbol":format!("{prefix}shadow-vertex")}),
            json!({"offset":0x2c8,"symbol":format!("{prefix}shadow-pixel")}),
        ],
    )
}
