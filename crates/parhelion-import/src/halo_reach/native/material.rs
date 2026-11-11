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
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};
pub(in crate::halo_reach) mod animation;
mod decal;
mod emission;
mod environment;
mod fog;
mod lighting_control;
mod mux;
mod normal;
mod opacity;
mod overlay;
mod parallax;
mod pixel;
mod resources;
pub(in crate::halo_reach) mod sky;
mod specular;
mod support;
mod surface;
mod technique;
mod terrain;
pub(in crate::halo_reach) mod vertex_data;
mod water;
pub(in crate::halo_reach) use fog::{Band, Fog, Light, METRES_PER_UNIT};

pub(in crate::halo_reach) struct Binding<'a> {
    pub palette: &'a [u8],
    pub rigid: bool,
    /// The model inputs the surfaces may read. A weapon that connects aim visibility shows its
    /// source HUD scope at full aim instead of itself, so every surface discards there.
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
    /// Static cutout materials with a reverse triangle for every face. Their authored copies
    /// supply both sides, so back-face culling preserves each side's own normal and baked light.
    pub paired_faces: &'a BTreeSet<u32>,
    /// Prefixes the symbols of the programs and surfaces this build adds, so a second build with
    /// another binding can share the graph. Textures and samplers are shared across builds.
    pub prefix: &'a str,
    /// Whether the build adds [`prepass`]'s depth technique, for a held item whose opaque
    /// surfaces draw in the depth prepass as stock weapon parts do.
    pub prepass: bool,
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
/// level's occlusion. Its detail lanes, or a vertex data record that stands in for them, hold
/// fixed-point lightmap coordinates ([`vertex_data::coordinate_bits`]), which it decodes into
/// `o3.zw` after the record read.
fn static_vertex(text: &str) -> Result<String> {
    let mut text = text.to_owned();
    let coordinates = format!(
        "$VERTEX_LIGHT_READ$o3.zw={};",
        vertex_data::coordinates("o3.zw")
    );
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
        ("$VERTEX_LIGHT_READ$", coordinates.as_str()),
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
/// The pixel texture registers Shadowkeep's transparent renderer scope binds itself: scene depth
/// at `t10` (`3F 03 07 47 2A`) and the completed opaque scene color at `t18` (`3F 27 07 47 32`).
/// A surface that reads them binds no fixed texture there.
const SCENE_REGISTERS: [usize; 2] = [10, 18];

/// The first texture register past every one `bindings` holds from which `count` consecutive
/// registers avoid `reserved`.
fn free_registers(bindings: &[(usize, String)], count: usize, reserved: &[usize]) -> usize {
    let mut first = bindings
        .iter()
        .map(|(register, _)| register + 1)
        .max()
        .unwrap_or(0);
    while (first..first + count).any(|register| reserved.contains(&register)) {
        first += 1;
    }
    first
}

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
/// Nightfall's value keeps its look. The sky's transparent stage takes the same factor.
pub(in crate::halo_reach) const REFERENCE_ILLUMINATION: f32 = 0.25;

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
    reserve_program(native, g, symbol, template)?;
    write_program(native, g, symbol, template, text, &bytes, &warnings)
}

/// Places an edited copy of native program `template`, whose `bytes` replace its data. The
/// `note` records the edit beside the program, where a compiled program keeps its source.
pub(in crate::halo_reach) fn bytecode(
    native: &mut Reader,
    g: &mut Graph,
    symbol: &str,
    template: u32,
    bytes: &[u8],
    note: &str,
) -> Result<()> {
    reserve_program(native, g, symbol, template)?;
    write_program(native, g, symbol, template, note, bytes, "")
}

/// Places program `symbol`'s header and data nodes, whose payloads [`write_program`] writes.
fn reserve_program(native: &mut Reader, g: &mut Graph, symbol: &str, template: u32) -> Result<()> {
    let data = format!("{symbol}-data");
    g.reserve(symbol, template, Some(&data), vec![])?;
    g.reserve(&data, native.reference(template)?, Some(symbol), vec![])
}

/// Writes program `symbol`'s source, compiler messages, header and compiled `bytes`.
fn write_program(
    native: &mut Reader,
    g: &Graph,
    symbol: &str,
    template: u32,
    text: &str,
    bytes: &[u8],
    warnings: &str,
) -> Result<()> {
    fs::write(g.root.join(format!("{symbol}.hlsl")), text)?;
    fs::write(g.root.join(format!("{symbol}.log")), warnings)?;
    let mut header = native.tag(template, None)?.0.clone();
    put(&mut header, 8, &u32::try_from(bytes.len())?.to_le_bytes())?;
    g.write(symbol, &header)?;
    g.write(&format!("{symbol}-data"), bytes)
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
    Ok(bound_texture(c, pages, native, g, mapping, normal, srgb, srgb)?.0)
}

/// A texture an effect draws. Its base, alpha and second maps are the effect's art whatever their
/// colour space, so they compress as colour does ([`bound_texture`]). Its palette stays an exact
/// lookup.
pub(in crate::halo_reach) fn effect_texture(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    mapping: &material::Mapping,
    srgb: bool,
) -> Result<String> {
    Ok(bound_texture(c, pages, native, g, mapping, false, srgb, true)?.0)
}

/// The optional flag identifies signed or unsigned BC5 normal samples. `art` marks a binding whose
/// texels are seen as an image rather than read as numbers.
#[allow(clippy::too_many_arguments)]
fn bound_texture(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    mapping: &material::Mapping,
    normal: bool,
    srgb: bool,
    art: bool,
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
    // tables are lookup tables read at exact coordinates. A scope's extended border reads only the
    // top level, and without smaller levels a client that drops a texture's top level has no other
    // level to show. Stock ships its odd-sized RGBA8 textures with one level as well. The sniper
    // rifle's mask is 394 by 250.
    let mips = if animated
        || matches!(
            mapping.role.as_str(),
            "palette" | "g_sample_vmf_diffuse" | "scope_border"
        ) {
        Mips::None
    } else if normal {
        Mips::Normal
    } else if srgb {
        Mips::Color
    } else {
        Mips::Data
    };
    // Art that Reach stores in a format with no native twin decodes to RGBA8, and then takes BC7
    // as stock's effect and colour art does, a quarter of the memory. Normals, masks, noise and
    // other values read as numbers keep RGBA8, as do lookups and strips without smaller levels.
    // A block-compressed top level must be whole 4 by 4 blocks.
    let compress = art
        && bitmap.block_format().is_none()
        && matches!(mips, Mips::Color | Mips::Data)
        && bitmap.width.is_multiple_of(4)
        && bitmap.height.is_multiple_of(4);
    let symbol = format!(
        "{}-{}{}{}{}",
        bitmap.name(),
        if srgb { "srgb" } else { "linear" },
        if animated { "-decimal" } else { "" },
        if mips == Mips::None && !animated {
            "-lookup"
        } else {
            ""
        },
        if compress { "-bc7" } else { "" }
    );
    let bc5 = (mips != Mips::None && normal && matches!(bitmap.format, 38 | 44))
        .then_some(bitmap.format == 38);
    // Repeated bindings need only the checked identity. Compressed passthrough reads the authored
    // mip chain directly, so neither path needs an intermediate RGBA decode.
    if g.contains(&symbol) {
        return Ok((symbol, bc5));
    }
    let extent = [bitmap.width, bitmap.height];
    // Reads go through the shared page cache here. Decoding, mips and the texture's files follow
    // on the deferral's workers ([`defer`]), its nodes already in place.
    if mips != Mips::None
        && let Some(blocks) = bitmap.blocks(c, pages)?
    {
        let format = blocks.format + u32::from(srgb && matches!(blocks.format, 71 | 74 | 77));
        let texture = reserve_texture(native, g, &symbol, format)?;
        defer(move || texture.write(extent, format, blocks.levels, &blocks.data))?;
        return Ok((symbol, bc5));
    }
    if !animated {
        let stored = bitmap
            .stored(c, pages)
            .with_context(|| format!("Surface bitmap role {}", mapping.role))?;
        let format = match (compress, srgb) {
            (true, true) => 99,
            (true, false) => 98,
            (false, true) => 29,
            (false, false) => 28,
        };
        let texture = reserve_texture(native, g, &symbol, format)?;
        let role = mapping.role.clone();
        defer(move || {
            // Decoding, the mip chain and BC7 encoding repeat for every weapon whose material
            // binds this bitmap, so what they make is remembered by its whole input, the
            // converter revision included, and a later import takes it from the store.
            let mut input = stored.input();
            input.extend_from_slice(&crate::halo_reach::REVISION.to_le_bytes());
            input.push(match mips {
                Mips::None => 0,
                Mips::Color => 1,
                Mips::Data => 2,
                Mips::Normal => 3,
            });
            input.push(u8::from(compress));
            if let Some(made) = crate::store::recall(SURFACE, &input)
                && let Some((&levels, pixels)) = made.split_first()
            {
                return texture.write(extent, format, levels, pixels);
            }
            let mut rgba = stored
                .decode()
                .with_context(|| format!("Surface bitmap role {role}"))?;
            ensure!(
                rgba.len() == extent[0] * extent[1] * 4,
                "Texture size differs"
            );
            let levels = mip_chain(extent[0], extent[1], &mut rgba, mips);
            let pixels = if compress {
                crate::tiger::texture::bc7_chain(extent[0], extent[1], levels, &rgba)
                    .with_context(|| format!("Surface bitmap role {role}"))?
            } else {
                rgba
            };
            if pixels.len() as u64 >= crate::store::SMALLEST {
                let mut made = Vec::with_capacity(pixels.len() + 1);
                made.push(levels);
                made.extend_from_slice(&pixels);
                crate::store::remember(SURFACE, &input, &made);
            }
            texture.write(extent, format, levels, &pixels)
        })?;
        return Ok((symbol, None));
    }
    let mut image = bitmap
        .decode(c, pages)
        .with_context(|| format!("Surface bitmap role {}", mapping.role))?;
    expand_decimal_frames(c, pages, mapping, normal, &mut image)?;
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

/// The store recipe decoded surface bitmaps are remembered under: a level count, then the data
/// entry's mip chain, RGBA8 or BC7. Its input carries the converter revision, so a revision that
/// changes decoding or encoding misses the older results.
const SURFACE: &str = "reach-surface-1";

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
    extent: [usize; 2],
    format: u32,
    levels: u8,
    pixels: &[u8],
) -> Result<()> {
    reserve_texture(native, g, symbol, format)?.write(extent, format, levels, pixels)
}

/// A texture whose header and data nodes are in the graph, and whose files are still to be
/// written: the native header template's payload and the two files' paths.
struct ReservedTexture {
    header: Vec<u8>,
    files: [std::path::PathBuf; 2],
}

/// Places texture `symbol`'s header and data nodes for pixels of `format`.
fn reserve_texture(
    native: &mut Reader,
    g: &mut Graph,
    symbol: &str,
    format: u32,
) -> Result<ReservedTexture> {
    // Both templates are stock resident BC7 headers, 99 with colour space byte 5 and 98 with 3, so
    // the sRGB one serves every sRGB format.
    let srgb = matches!(format, 29 | 72 | 75 | 78 | 99);
    let template = if srgb { 0x80ec31b9 } else { 0x80ec31bc };
    let header = native.tag(template, None)?.0.clone();
    let data = format!("{symbol}-data");
    g.reserve(symbol, template, Some(&data), vec![])?;
    g.reserve(&data, native.reference(template)?, Some(symbol), vec![])?;
    Ok(ReservedTexture {
        header,
        files: [g.path(symbol), g.path(&data)],
    })
}

impl ReservedTexture {
    /// Writes the header for `levels` levels of `pixels` and the pixels themselves.
    fn write(
        mut self,
        [width, height]: [usize; 2],
        format: u32,
        levels: u8,
        pixels: &[u8],
    ) -> Result<()> {
        let header = &mut self.header;
        put(header, 0, &u32::try_from(pixels.len())?.to_le_bytes())?;
        put(header, 4, &format.to_le_bytes())?;
        put(header, 14, &u16::try_from(width)?.to_le_bytes())?;
        put(header, 16, &u16::try_from(height)?.to_le_bytes())?;
        put(header, 18, &1u16.to_le_bytes())?;
        put(header, 20, &1u16.to_le_bytes())?;
        header[22] = 0;
        header[23] = levels;
        put(header, 36, &u32::MAX.to_le_bytes())?;
        crate::tiger::texture::resident(header, pixels.len())?;
        fs::write(&self.files[0], &self.header)?;
        fs::write(&self.files[1], pixels)?;
        Ok(())
    }
}

/// Texture work that [`build_with_progress`] lets run on workers while its surfaces build.
struct Deferral(crate::cancellation::Background<()>);

thread_local! {
    static DEFERRAL: std::cell::RefCell<Option<Deferral>> = const { std::cell::RefCell::new(None) };
}

/// Runs `job`, which only writes files of nodes already placed, on this thread's deferral, or at
/// once without one.
fn defer(job: impl FnOnce() -> Result<()> + Send + 'static) -> Result<()> {
    let job = DEFERRAL.with(|deferral| match deferral.borrow_mut().as_mut() {
        Some(Deferral(work)) => {
            work.submit(job);
            None
        }
        None => Some(job),
    });
    job.map_or(Ok(()), |job| job())
}

/// This thread's deferral while a build holds it. An inner build leaves it to the outer one, and
/// a failed build drops it.
struct DeferralScope {
    owner: bool,
}

impl DeferralScope {
    fn open() -> Result<Self> {
        let owner = DEFERRAL.with(|deferral| deferral.borrow().is_none());
        if owner {
            let work = crate::cancellation::Background::new()?;
            DEFERRAL.with(|deferral| *deferral.borrow_mut() = Some(Deferral(work)));
        }
        Ok(Self { owner })
    }

    /// Waits for the deferred work, when this scope opened the deferral.
    fn finish(self) -> Result<()> {
        if !self.owner {
            return Ok(());
        }
        match DEFERRAL.with(|deferral| deferral.borrow_mut().take()) {
            Some(Deferral(work)) => work.finish().map(drop),
            None => Ok(()),
        }
    }
}

impl Drop for DeferralScope {
    fn drop(&mut self) {
        if self.owner {
            DEFERRAL.with(|deferral| deferral.borrow_mut().take());
        }
    }
}

/// A level's baked lighting: its source atlas textures, probe table, lighting scale and the
/// average lit level that lighting is a share of, as [`crate::halo_reach::level::lightmap::Atlas`] describes
/// them, the light its opaque surfaces add for the level's large bloom, as a share of that level,
/// and the level's fog.
#[derive(Clone)]
pub(in crate::halo_reach) struct Lightmap {
    pub intensity: [String; 3],
    pub mask: String,
    pub probes: String,
    pub decoder: String,
    pub scale: f32,
    pub level: f32,
    pub radiance: f32,
    /// Source indirect luminance represented by the shared open native visibility.
    pub reference: f32,
    pub veil: [f32; 3],
    pub sun: Option<Analytical>,
    pub analytical: BTreeMap<u32, Analytical>,
    pub origin: [f32; 3],
    pub fog: Option<Fog>,
    /// Source camera limits shared with sky and screen-fog in-scatter, as linear exposure.
    pub fog_exposure: Option<[f32; 2]>,
    /// Whether the sky's screen fog sheet ([`sky::screen_fog`]) fogs the level's opaque surfaces,
    /// which then leave `fog` to it ([`native_finish`]).
    pub screen_fog: bool,
    /// Self illumination's scale and power over the frame's exposure scale `x`, `scale * x^power`,
    /// which hold it at Reach's own self illumination exposure ([`self_illumination_exposure`]).
    pub self_illumination: [f32; 2],
}

/// Adds the original BC3 intensity layers and BC5 exponent/visibility map as four native 2D
/// textures. The shader decodes their filtered values. Constant probes have a separate linear
/// table. Images retain their source top level, with no generated mips across packed charts.
pub(in crate::halo_reach) fn lightmap_textures(
    native: &mut Reader,
    g: &mut Graph,
    atlas: &crate::halo_reach::level::lightmap::Atlas,
    veil: [f32; 3],
    sun: Option<Analytical>,
    fog: Option<Fog>,
) -> Result<Lightmap> {
    let mut add = |name: &str, extent: [usize; 2], format: u32, bytes: &[u8]| -> Result<String> {
        let symbol = format!("lightmap-{name}");
        texture_data(native, g, &symbol, extent, format, 1, bytes)?;
        Ok(symbol)
    };
    let extent = [atlas.width, atlas.height];
    let source = &atlas.sampling;
    Ok(Lightmap {
        intensity: [
            add("intensity-0", extent, 77, &source.intensity[0])?,
            add("intensity-1", extent, 77, &source.intensity[1])?,
            add("intensity-2", extent, 77, &source.intensity[2])?,
        ],
        mask: add("mask", extent, 83, &source.mask)?,
        probes: add("probes", source.probe_extent, RGBA16_FLOAT, &source.probes)?,
        decoder: source.function(atlas.scale),
        scale: atlas.scale,
        level: atlas.level,
        radiance: LIGHT_RADIANCE,
        reference: 1.,
        veil,
        sun,
        analytical: BTreeMap::new(),
        origin: [0.; 3],
        fog,
        fog_exposure: None,
        screen_fog: false,
        self_illumination: [1., 0.],
    })
}

/// Self illumination's factor over the main exposure. Reach draws it at its own exposure `I`,
/// `self * 2^I` where lit surfaces take `2^E`: `get_illum_render_scale` (`8258CA40` in the Xbox
/// `halo3_tag_play` build) returns `2^(I - E)` beside `get_render_exposure`'s `2^E`, and
/// `c_camera_fx_values::update` (`82595FD0`) sets `I = P + C * (E - P)`, with `P` the camera
/// effects' preferred self illumination exposure (`+0xC0`) and `C` how much of the main exposure it
/// follows (`+0xD0`). A lit surface draws at the frame's `x` where Reach draws `2^E`, at
/// `x = 2^(E + offset)` with `offset` the level's exposure offset
/// ([`crate::halo_reach::level::native::NativeExposure`]), so self illumination takes
/// `2^((1 - C) * (P + offset)) * x^(C - 1)` of what the main exposure alone gives it, 1 when the
/// frame sits at the preferred exposure.
pub(in crate::halo_reach) fn self_illumination_exposure(
    preferred: f32,
    change: f32,
    offset: f32,
) -> [f32; 2] {
    [((1. - change) * (preferred + offset)).exp2(), change - 1.]
}

/// The pixel program factor of a lightmapped surface's self illumination, `scale * x^power`.
fn self_illumination_factor([scale, power]: [f32; 2]) -> String {
    format!("{scale:.9}*pow(max(frameData[1].z,0.000001),{power:.9})")
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
    if g.contains(&symbol) {
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
    // The material constants that hold `magazine` are declared with the surface's other rows.
    Ok(Some(format!(
        "float2 decimalUv(float2 uv,float place) {{ float digit=fmod(floor(max(magazine.x,0)/place),10); return float2((digit+clamp(uv.x,{:.9},{:.9}))/10,clamp(uv.y,{:.9},{:.9})); }}",
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
const DETAIL_OVERLAY_SLOT: usize = 30;
const ILLUM_DETAIL_SLOT: usize = 31;
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

/// Texture roles use consecutive native registers, skipping the caller's reserved scope inputs.
/// The same binding rules apply to baked surfaces, dynamic models and painted equipment.
#[allow(clippy::cognitive_complexity, clippy::too_many_arguments)]
fn textures(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    material: &Material,
    paint: Option<&crate::halo_reach::paint::Surface>,
    reserved: &[usize],
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
                .is_none_or(|v| !matches!(v.as_str(), "3_channel_self_illum" | "meter")),
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
        {
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
                Some("detail" | "detail_blend" | "three_detail_blend" | "detail_wrinkle")
            ) {
                table.push((BUMP_DETAIL_SLOT, &["bump_detail_map"][..], true, false));
            }
            if matches!(
                option("bump_mapping"),
                Some("detail_blend" | "three_detail_blend")
            ) {
                table.push((BUMP_DETAIL2_SLOT, &["bump_detail_map2"][..], true, false));
            }
            if option("bump_mapping") == Some("three_detail_blend") {
                table.push((BUMP_DETAIL3_SLOT, &["bump_detail_map3"][..], true, false));
            }
            if matches!(
                option("albedo"),
                Some(
                    "two_detail"
                        | "detail_blend"
                        | "three_detail_blend"
                        | "two_detail_overlay"
                        | "two_detail_black_point"
                )
            ) {
                table.push((DETAIL2_SLOT, &["detail_map2"][..], false, true));
            }
            if matches!(
                option("parallax"),
                Some("simple" | "interpolated" | "simple_detail")
            ) || option("warp") == Some("parallax_simple")
            {
                table.push((parallax::HEIGHT, &["height_map"][..], false, false));
                if option("parallax") == Some("simple_detail") {
                    table.push((parallax::SCALE, &["height_scale_map"][..], false, false));
                }
            }
            if matches!(
                option("alpha_blend_source"),
                Some(
                    "from_opacity_map_alpha"
                        | "from_opacity_map_rgb"
                        | "from_opacity_map_alpha_and_albedo_alpha"
                )
            ) {
                table.push((GLASS_OPACITY_SLOT, &["opacity_texture"][..], false, false));
            }
            if option("warp") == Some("from_texture") {
                table.push((overlay::WARP, &["warp_map"][..], false, false));
            }
            if matches!(
                option("overlay"),
                Some("additive" | "multiply" | "additive_detail" | "multiply_and_additive_detail")
            ) {
                table.push((overlay::MAP, &["overlay_map"][..], false, true));
            }
            if matches!(
                option("overlay"),
                Some("additive_detail" | "multiply_and_additive_detail")
            ) {
                table.push((overlay::DETAIL, &["overlay_detail_map"][..], false, true));
            }
            if option("overlay") == Some("multiply_and_additive_detail") {
                table.push((
                    overlay::MULTIPLY,
                    &["overlay_multiply_map"][..],
                    false,
                    true,
                ));
            }
            if option("albedo") == Some("two_detail_overlay") {
                table.push((
                    DETAIL_OVERLAY_SLOT,
                    &["detail_map_overlay"][..],
                    false,
                    true,
                ));
            }
            if matches!(
                option("self_illumination"),
                Some("illum_detail" | "change_color_detail")
            ) {
                table.push((
                    ILLUM_DETAIL_SLOT,
                    &["self_illum_detail_map"][..],
                    false,
                    true,
                ));
            }
            if matches!(
                option("bump_mapping"),
                Some("standard_wrinkle" | "detail_wrinkle")
            ) {
                table.push((normal::WRINKLE, &["wrinkle_normal"][..], true, false));
                table.push((normal::MASK_A, &["wrinkle_mask_a"][..], false, false));
                table.push((normal::MASK_B, &["wrinkle_mask_b"][..], false, false));
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
            && change_colors(material) == 0
            && paint.is_none_or(|paint| {
                !matches!(paint.mask, crate::halo_reach::paint::PaintMask::ChangeColor)
            })
        {
            continue;
        }
        if let Some(mapping) = material.mapping(roles) {
            // The table's colour roles are its art. Its other roles are read as numbers.
            let (symbol, bc5) = bound_texture(c, pages, native, g, mapping, normal, srgb, srgb)?;
            let target = bindings
                .iter()
                .find(|(_, existing)| existing == &symbol)
                .map_or_else(
                    || free_registers(&bindings, 1, reserved),
                    |(register, _)| *register,
                );
            ensure!(
                target < MAX_TEXTURE_REGISTERS,
                "Surface texture binding capacity exceeded"
            );
            if !bindings.iter().any(|(register, _)| *register == target) {
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
                true,
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
    if let Some(mapping) = material.mapping(&["g_sample_vmf_diffuse"]) {
        let target = free_registers(&bindings, 1, reserved);
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

/// The light a surface's analytical highlight comes from: the direction toward it from the
/// surface and its intensity over pi and the level's average lit level, the units of the
/// translated baked light.
struct Highlight {
    direction: String,
    intensity: [f32; 3],
}

/// The selected analytical light of a lightmapped surface as [`Highlight`] takes it, `None`
/// without a present light. A point light's direction follows the surface's world position, as
/// [`analytical_lighting`] takes it.
fn highlight(lightmap: &Lightmap, light: Option<Analytical>) -> Option<Highlight> {
    let light = light.filter(|light| light.present())?;
    let [x, y, z] = light.vector;
    let direction = if light.directional {
        format!("normalize(float3({x:.9},{y:.9},{z:.9}))")
    } else {
        let point: [f32; 3] = std::array::from_fn(|k| light.vector[k] * 3.048 + lightmap.origin[k]);
        format!(
            "normalize(float3({:.9},{:.9},{:.9})-v4+float3(0,0,0.00001))",
            point[0], point[1], point[2]
        )
    };
    Some(Highlight {
        direction,
        intensity: light
            .intensity
            .map(|v| v / (std::f32::consts::PI * lightmap.level.max(1e-6))),
    })
}

/// The baked sun visibility of a lightmapped surface's texel or vertex record. A texel carries it
/// unsquared and a vertex record squared, as [`analytical_lighting`] reads it.
const BAKED_VISIBILITY: &str =
    "(v8.w>0.5?saturate(lightAmbient.a):saturate(lightAmbient.a)*saturate(lightAmbient.a))";

/// Reach's analytical highlights per unit of light intensity over pi, from the surface normal
/// `n`, the direction `v` toward the camera and `l` toward the light. Cook-Torrance, as
/// `calc_material_analytic_specular` in `cook_torrance_core.fx` forms it, is the Beckmann
/// distribution of slope `m` times the masking term `saturate(min(2 NH NV / HV, 2 NH NL / HV))`
/// over `NV`, without its divisor of pi. Two-lobe Phong, as `analytical_Phong_specular`, is
/// `saturate(R.L)^p * (1 + p)` with `R` the view reflected about the normal, which the caller
/// multiplies by pi.
const HIGHLIGHT_FUNCTIONS: &str = "float reachBeckmannHighlight(float3 n,float3 v,float3 l,float m){float3 h=normalize(l+v+float3(0,0,0.00001));float nh=max(dot(n,h),0.0001);float nv=dot(n,v);float nl=dot(n,l);float hv=max(dot(v,h),0.0001);float m2=max(m*m,0.0000000001);float d=exp(-(1-nh*nh)/(nh*nh*m2))/(m2*nh*nh*nh*nh+0.00001);float g=saturate(min(2*nh*nv/hv,2*nh*nl/hv));return nv!=0?max(d*g/nv,0):0;}\nfloat reachPhongHighlight(float3 n,float3 v,float3 l,float power){power=max(power,0.0001);return pow(saturate(dot(reflect(-v,n),l)),power)*(1+power);}\n";

/// Source area and environment specular use the VMF power table separately from diffuse.
/// The lookup and coefficient routes are retained. Per-pixel and dynamic environment maps add
/// their reflection over the same basis. The selected analytical light adds Reach's analytical
/// highlight, masked by the baked sun visibility, since the native sun's own highlight is a
/// dielectric's at a fraction of the native light. Animated light intensity remains a limitation.
fn reflection(
    material: &Material,
    surface: &surface::Surface<'_>,
    environment: Option<&str>,
    samples: &BTreeMap<usize, String>,
    table: bool,
    light: Option<&Highlight>,
) -> Option<(String, [f32; 3])> {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    let (normal_tint, blend, glancing) = match option("material_model") {
        Some("two_lobe_phong") => (
            "normal_specular_tint",
            "albedo_specular_tint_blend",
            "glancing_specular_tint",
        ),
        Some("cook_torrance" | "organism") => ("specular_tint", "albedo_blend", "fresnel_color"),
        _ => return None,
    };
    let phong = option("material_model") == Some("two_lobe_phong");
    let mask = specular_mask(material, samples)?;
    let mut coefficient = surface.scalar("specular_coefficient", 1.);
    if !phong && surface.boolean("use_material_texture") == Some(true) {
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
    let environment = if matches!(option("environment_mapping"), Some("per_pixel" | "dynamic")) {
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
    // The highlight's Fresnel color is the tint above, as both models' `final_specular_color`.
    let direct = light
        .map(|light| {
            let lobe = if phong {
                format!(
                    "3.141592654*reachPhongHighlight(n,normalize(v5.xyz),{},{})",
                    light.direction,
                    surface.scalar("analytical_power", 16.)
                )
            } else {
                let name = if surface.constant("analytical_roughness").is_some() {
                    "analytical_roughness"
                } else {
                    "roughness"
                };
                let mut slope = format!("max({},0.00001)", surface.scalar(name, 0.48));
                if surface.boolean("use_material_texture") == Some(true)
                    && let Some(sample) = samples.get(&2)
                {
                    let black = surface.scalar("material_texture_black_roughness", 1.);
                    slope = format!("lerp({black},{slope},({sample}).a)");
                }
                format!(
                    "reachBeckmannHighlight(n,normalize(v5.xyz),{},saturate({slope}))",
                    light.direction
                )
            };
            let [r, g, b] = light.intensity;
            format!(
                "+{lobe}*float3({r:e},{g:e},{b:e})*{BAKED_VISIBILITY}*max({},0)",
                surface.scalar("analytical_specular_contribution", 0.5)
            )
        })
        .unwrap_or_default();
    Some((
        format!("({tint})*{mask}*max({coefficient},0)*(({basis})*({area}{environment}){direct})"),
        surface.constant("env_tint_color").unwrap_or([1.; 4])[..3]
            .try_into()
            .unwrap(),
    ))
}

/// Water's body color and, for refracting water ([`water::lighting`]), its bank alpha, which the
/// lighting blends toward the scene behind. Water without refraction keeps a coverage that
/// approximates the refraction depth: `1 - exp(-0.15 * murkiness)` rising to one by Fresnel, times
/// the bank alpha.
fn water_albedo(
    material: &Material,
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
    refracts: bool,
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
    if refracts {
        return format!("float4({body},{bank})");
    }
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
/// static-lightmap shader adds both and divides diffuse irradiance by pi. The sampled VMF
/// decoder and vertex records use the existing atlas normalization. Analytical intensity does
/// not. Four source images and the probe table bind clear of `reserved`.
#[allow(clippy::too_many_arguments)]
fn level_lighting(
    lightmap: &Lightmap,
    selected: Option<Analytical>,
    deferred: bool,
    bindings: &mut Vec<(usize, String)>,
    table: Option<&str>,
    reflection: Option<&str>,
    water: Option<&str>,
    reserved: &[usize],
) -> Result<(String, String)> {
    let first = free_registers(bindings, 5, reserved);
    ensure!(
        first + 5 <= MAX_TEXTURE_REGISTERS,
        "A level surface needs {} texture registers",
        first + 5
    );
    for (offset, symbol) in lightmap
        .intensity
        .iter()
        .chain([&lightmap.mask, &lightmap.probes])
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
    let sample = "float4 lightLobe;float4 lightAmbient;float3 lightDirection;if(v8.w>0.5){lightLobe=v6;lightAmbient=v7;lightDirection=normalize(v8.xyz+float3(0,0,0.00001));}else{reachLightmap(abs(v3.zw),lightLobe,lightAmbient,lightDirection);}";
    let direct = analytical_lighting(lightmap, selected, deferred && water.is_none());
    let finish = if let Some(water) = water {
        water.to_owned()
    } else if deferred {
        // Source irradiance controls native visibility without becoming material colour.
        // The native cube and light passes own specular. A separate light draw may retain an
        // additional analytical source, but never repeats this indirect illumination.
        "float3 lightIndirect=max(lightResponse*lightLobeIrradiance+lightAmbientIrradiance,0);"
            .to_owned()
    } else {
        "albedo.rgb=albedo.rgb*(lightResponse*lightLobeIrradiance+lightAmbientIrradiance+lightDirectIrradiance*lightDirectCosine)+lightSpecular;".to_owned()
    };
    // The sampler decoder and vertex records both return linear shares of the atlas peak.
    let lighting = format!(
        "{sample}float lightResponse={response};float3 lightLobeIrradiance=lightLobe.rgb*{scale:.9};float3 lightAmbientIrradiance=lightAmbient.rgb*{scale:.9};float3 lightSpecular=({specular})*sourceSpecularOpacity;{direct}{finish}",
        scale = lightmap.scale
    );
    let mut declarations = [
        "LightIntensity0",
        "LightIntensity1",
        "LightIntensity2",
        "LightMask",
        "LightProbes",
    ]
    .iter()
    .enumerate()
    .map(|(offset, name)| format!("Texture2D<float4> {name}:register(t{});", first + offset))
    .collect::<String>();
    declarations.push_str(&lightmap.decoder);
    Ok((lighting, declarations))
}

/// MCC's analytical diffuse uses the selected direction or normalized point-to-surface vector,
/// intensity and squared baked visibility. Vertex lighting already squares visibility before
/// interpolation, while pixel lightmaps carry its unsquared value. Gel textures and animated
/// intensities are separate runtime inputs. Opaque surfaces selecting the global sun use native
/// shadow visibility. Their material constants can separately restore a share of the baked mask
/// over a view-distance interval. Native visibility couples that mask to ambient light. Transparent surfaces,
/// water and other analytical lights retain source visibility. Model placements use sky zero
/// and retain their sampled indirect light.
fn analytical_lighting(
    lightmap: &Lightmap,
    light: Option<Analytical>,
    native_shadows: bool,
) -> String {
    let Some(light) = light.filter(|light| light.present()) else {
        return "bool lightPlaced=v3.z<0;bool lightNativeSun=false;float lightNativeMask=1;float lightDirectCosine=0;float3 lightDirectIrradiance=0;".into();
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
    // Native defaults disable the baked sunlight mask. Its optional strength and distance fade
    // are applied through native visibility for opaque sun receivers. Other lights retain the
    // source visibility on their analytical coefficient.
    let visibility = if native && native_shadows {
        "lerp(1,lightVisibilitySquared,saturate(bakedControl.x)*saturate((length(v5.xyz)-bakedControl.y)/max(bakedControl.z-bakedControl.y,0.001)))"
    } else {
        "lightVisibilitySquared"
    };
    format!(
        "bool lightPlaced=v3.z<0;bool lightNativeSun={native};float3 lightDirectDirection={direction};lightDirectDirection*=rsqrt(max(dot(lightDirectDirection,lightDirectDirection),0.00000001));float lightDirectCosine=saturate(dot(n,lightDirectDirection));float lightVisibility=saturate(lightAmbient.a);float lightVisibilitySquared=v8.w>0.5?lightVisibility:lightVisibility*lightVisibility;float lightNativeMask=lightNativeSun?({visibility}):1;float3 lightDirectIrradiance=float3({r:.9},{g:.9},{b:.9})*{visibility};"
    )
}

/// Ordinary material colour and bake-derived visibility, calibrated exactly as object probes.
/// The directional source atlas remains sampled at its original resolution. It contributes no
/// second diffuse-light draw. Native cubemap grids carry broad spatial colour separately.
/// Native W affects both direct and environment light, so optional baked visibility controls
/// inherit that coupling. Exposure and base albedo never participate in visibility selection.
fn native_finish(lightmap: &Lightmap) -> String {
    use super::lighting;
    let fog = if lightmap.screen_fog || lightmap.fog.is_none() {
        "1"
    } else {
        "fog.a"
    };
    format!(
        "float bakedLevel=dot(lightIndirect,float3(0.2126,0.7152,0.0722))*max(bakedControl.w,0);float nativeVisibility=clamp({open:.9}*sqrt(max(bakedLevel,0)/{reference:.9}),{low:.9},{high:.9});nativeVisibility*=pow(saturate(lightNativeMask),1.0/{power:.9})*sqrt(saturate({fog}));o0=float4(saturate(albedo.rgb),0.04);o1=float4(saturate(n*((1-roughness)*0.125+0.375)+0.5),0);o2=float4(0,0.5,0,saturate(nativeVisibility));",
        open = lighting::OPEN,
        low = lighting::RANGE[0],
        high = lighting::RANGE[1],
        power = lighting::DIRECT_POWER,
        reference = lightmap.reference.max(1e-4),
    )
}

/// Only an analytical light not represented by the principal native sun needs a light draw.
fn additional_light(lightmap: &Lightmap, key: u32) -> bool {
    lightmap.analytical.get(&key).is_some_and(|light| {
        light.present()
            && !(light.directional
                && lightmap.sun.is_some_and(|sun| {
                    sun.vector == light.vector && sun.intensity == light.intensity
                }))
    })
}

/// Retain additional source analytical diffuse in the native light buffer. Its base colour is
/// already in the G-buffer, and native environment reflection supplies the specular response.
fn baked_finish(lightmap: &Lightmap) -> String {
    let fog = if lightmap.screen_fog || lightmap.fog.is_none() {
        "1"
    } else {
        "fog.a"
    };
    let fog_declaration = lightmap.fog.map_or_else(
        || "float4 fog=float4(0,0,0,1);".to_owned(),
        |_| "float4 fog=reachFog(v5.xyz,v5.w);".to_owned(),
    );
    format!(
        "{fog_declaration}o0=float4(max(lightDirectIrradiance*lightDirectCosine,0)*({scale:.9}*frameData[1].z)*{fog},0);o1=0;",
        scale = lightmap.radiance * REFERENCE_ILLUMINATION
    )
}

/// Ordinary stage 7 follows the sky's transparent stage, so glow applies its own extinction.
/// The screen sheet already supplies in-scatter over the opaque surface beneath this draw.
/// Alpha zero with the native premultiplied blend adds color without replacing that surface.
fn glow_finish(lightmap: &Lightmap) -> String {
    let fog = if lightmap.fog.is_some() {
        "float4 fog=reachFog(v5.xyz,v5.w);"
    } else {
        "float4 fog=float4(0,0,0,1);"
    };
    let scatter = if lightmap.screen_fog {
        "float3(0,0,0)"
    } else {
        "fog.rgb"
    };
    let [r, g, b] = lightmap.veil;
    format!(
        "{fog}float3 glow=(max(emission*{illumination}+surfaceOverlay,0)*fog.a+{scatter})*{inverse:.9}+float3({r:.9},{g:.9},{b:.9});o0=float4(max(glow,0)*({scale:.9}*frameData[1].z),0);",
        illumination = self_illumination_factor(lightmap.self_illumination),
        inverse = 1. / lightmap.level,
        scale = lightmap.radiance * REFERENCE_ILLUMINATION,
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
fn baked_transparent_finish(material: &Material, lightmap: &Lightmap) -> String {
    let scale = format!(
        "{:.9}*frameData[1].z",
        lightmap.radiance * REFERENCE_ILLUMINATION
    );
    let inverse = 1. / lightmap.level;
    let exposure = lightmap.fog_exposure.map_or_else(
        || format!("({scale})*{inverse:.9}"),
        |exposure| {
            sky::SkyScale {
                vertex_color: 1.,
                radiance: lightmap.radiance / lightmap.level,
                exposure,
            }
            .exposure()
        },
    );
    let scatter = format!("max(fog.rgb,0)*({exposure})");
    // The destination already contains T * surface + scatter. Multiplying only the receiver
    // requires M * destination + (1 - M) * scatter, so full fog is independent of decal colour.
    if let Some(finish) = decal::multiplication(material, &scatter) {
        return finish;
    }
    let emission = format!(
        "(emission*{}+surfaceOverlay)*{inverse:.9}",
        self_illumination_factor(lightmap.self_illumination)
    );
    match material.options.get("blend_mode").map(String::as_str) {
        Some("additive") if material.decal() => format!("o0=float4(albedo.rgb*fog.a*{scale},0);"),
        Some("additive") => format!("o0=float4({emission}*fog.a*{scale},0);"),
        Some("multiply") => format!(
            "float3 product=lerp(float3(1,1,1),unlit.rgb,saturate(unlit.a));float coverage=1-saturate(dot(product,float3(1,1,1))/3);o0=float4(({scatter})*coverage,coverage);"
        ),
        Some("pre_multiplied_alpha") if decal::premultiplied(material) => format!(
            "float coverage=saturate(albedo.a);o0=float4((albedo.rgb+{emission}*coverage)*fog.a*{scale}+({scatter})*coverage,coverage);"
        ),
        _ => format!(
            "float coverage=saturate(albedo.a);o0=float4(((albedo.rgb+{emission})*fog.a*{scale}+{scatter})*coverage,coverage);"
        ),
    }
}

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

/// Builds every surface of `materials` in stages: [`resources`] binds what its pixel program
/// reads, [`pixel`] writes that program and its variants and submits them to compile workers, and
/// [`technique`] adds its native records to the graph. Once the last surface is placed, the
/// compiled programs and deferred textures are written, then the shared shadow and prepass
/// techniques.
#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn build_with_progress(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    materials: &BTreeMap<u32, Material>,
    binding: Binding<'_>,
    report_progress: &mut dyn FnMut(&str),
) -> Result<Vec<Value>> {
    let mut shells = Shells::read(native)?;
    let vertex = vertex_program(native, g, &binding, &shells.opaque)?;
    let mut report = Vec::new();
    let mut programs = pixel::Programs::new()?;
    // Textures decode, take their mips and write their files on workers too ([`defer`]).
    let deferral = DeferralScope::open()?;
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
        let setup = Setup::new(c, pages, &binding, key, material)?;
        let mut resources =
            resources::Resources::bind(c, pages, native, g, &binding, &setup, shells.clamped)?;
        let lighting = resources::Lighting::bind(&binding, &setup, &mut resources)?;
        let pixel = pixel::Pixel::write(&binding, &setup, &mut resources, &lighting)?;
        let shaders = programs.submit(native, g, &binding, key, shells.pixel_template, pixel)?;
        let stage = technique::add(
            native,
            g,
            &binding,
            &setup,
            &mut shells,
            &resources,
            &shaders,
        )?;
        report.push(entry(&setup, &resources, &lighting, &shaders, stage));
    }
    programs.finish(native, g)?;
    deferral.finish()?;
    shadow(native, g, &vertex, binding.prefix)?;
    if binding.prepass {
        prepass(native, g, &vertex, binding.prefix)?;
    }
    Ok(report)
}

/// The stock surface techniques a surface's record starts from, and what every surface takes
/// from them.
struct Shells {
    opaque: std::sync::Arc<Payload>,
    transparent: std::sync::Arc<Payload>,
    /// The opaque shell's clamping sampler row. Surfaces read their maps through copies of the
    /// shell's sampler with source addressing and trilinear filtering where requested, and a
    /// lightmap atlas through this original.
    clamped: [u8; 16],
    sampler_class: u32,
    /// The native program the pixel programs copy.
    pixel_template: u32,
    /// The matching static or entity alpha-tested shadow envelope, read only once a surface needs
    /// alpha coverage.
    clipped: Option<std::sync::Arc<Payload>>,
}

impl Shells {
    fn read(native: &mut Reader) -> Result<Self> {
        let opaque = native.tag(0x80ec270d, Some(0x808071e8))?;
        let transparent = native.tag(0x815246aa, Some(0x808071e8))?;
        ensure!(
            transparent.0[32] == 0x88,
            "Native premultiplied surface state differs"
        );
        let rows = opaque.array(0x308, 16, None)?;
        let clamped = opaque.bytes::<16>(
            *rows
                .get(CLAMPED_SAMPLER)
                .context("Native sampler table is missing")?,
        )?;
        let sampler_class = opaque.u32(rows[0] - 8)?;
        let pixel_template = opaque.u32(0x2c8)?;
        Ok(Self {
            opaque,
            transparent,
            clamped,
            sampler_class,
            pixel_template,
            clipped: None,
        })
    }
}

impl Binding<'_> {
    /// The vertex stage register of the surfaces' vertex data texture.
    fn vertex_data_register(&self) -> u32 {
        if self.statics {
            STATIC_VERTEX_DATA_REGISTER
        } else {
            0
        }
    }
}

/// Places the surfaces' shared vertex program, copied from the opaque shell's, and returns its
/// text before the color pass substitutions, which the shadow and prepass techniques make their
/// own way.
fn vertex_program(
    native: &mut Reader,
    g: &mut Graph,
    binding: &Binding<'_>,
    shell: &Payload,
) -> Result<String> {
    let palette = binding.palette;
    ensure!(
        !palette.is_empty() && palette.len() <= 256,
        "Invalid source motion palette"
    );
    ensure!(
        binding.lightmap.is_none() || binding.paint.is_empty(),
        "Lightmapped surfaces cannot also take gear paint"
    );
    ensure!(
        !binding.statics || binding.rigid,
        "Static instances are rigid"
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
    let (declaration, read) = if binding.vertex_data.is_some() {
        (
            vertex_data::DECLARATION.replace(
                "register(t0)",
                &format!("register(t{})", binding.vertex_data_register()),
            ),
            vertex_data::READ,
        )
    } else {
        (String::new(), "o6=0;o7=0;o8=0;")
    };
    program(
        native,
        g,
        &format!("{}vertex", binding.prefix),
        shell.u32(0x48)?,
        &vertex
            .replace("$SHADOW_RESOURCE$", "")
            .replace("$SHADOW_PROJECTION$", "")
            .replace("$VERTEX_LIGHT_DECL$", &declaration)
            .replace("$VERTEX_LIGHT_READ$", read),
        true,
    )?;
    Ok(vertex)
}

/// What every stage of one surface's build reads: its source material, resolved constants and
/// gear paint, and the model inputs and texture registers its pixel program takes.
struct Setup<'a> {
    key: u32,
    material: &'a Material,
    surface: surface::Surface<'a>,
    paint: Option<&'a crate::halo_reach::paint::Surface>,
    /// The material constants block and decimal display helper the pixel program declares.
    display: String,
    /// Whether the surface shows a decimal ammunition display, which reads the live magazine.
    animated: bool,
    /// The aim visibility input. Past 0.99 aim visibility, where Shadowkeep's reticle spline rises
    /// and the source HUD scope appears, the weapon discards its color fragments as an enclosed
    /// Marathon sight does. Its shadow technique keeps drawing.
    aimed: Option<u8>,
    /// Whether an opaque lightmapped surface reads the native lighting control rows.
    controlled_lighting: bool,
    refracts: bool,
    /// Texture registers no binding may take. Logical texture roles are independent of native
    /// register numbers. Gear paint owns t3 through t8, and scene refraction owns its scope
    /// resources.
    reserved: Vec<usize>,
}

impl<'a> Setup<'a> {
    fn new(
        c: &Cache,
        pages: &mut Pages,
        binding: &Binding<'a>,
        key: u32,
        material: &'a Material,
    ) -> Result<Self> {
        let surface = surface::Surface::new(material, binding, key);
        let paint = binding.paint.get(&key);
        let display = decimal_display(c, pages, material)?;
        let animated = display.is_some();
        let aimed = binding
            .objects
            .get(&format!(
                "{:08X}",
                crate::tiger::channel::inherited::AIM_VISIBILITY
            ))
            .copied();
        let controlled_lighting = binding.lightmap.is_some() && !surface.transparent();
        let mut rows = if controlled_lighting {
            lighting_control::DECLARATION.to_owned()
        } else {
            String::new()
        };
        rows.extend(
            [
                animated.then_some("magazine"),
                aimed.map(|_| "aimVisibility"),
            ]
            .into_iter()
            .flatten()
            .map(|row| format!("float4 {row};")),
        );
        let display = format!(
            "{}{}",
            if rows.is_empty() {
                String::new()
            } else {
                format!("cbuffer Material : register(b0) {{ {rows} }};\n")
            },
            display.unwrap_or_default()
        );
        let refracts = binding.lightmap.is_some() && water::refracts(material);
        let mut reserved = Vec::new();
        if paint.is_some() {
            reserved.extend(3..=8);
        }
        if refracts {
            reserved.extend(SCENE_REGISTERS);
        }
        if emission::depth_fade(material) {
            reserved.push(10);
        }
        Ok(Self {
            key,
            material,
            surface,
            paint,
            display,
            animated,
            aimed,
            controlled_lighting,
            refracts,
            reserved,
        })
    }

    /// The first material row the surface's model inputs fill, after any lighting control rows.
    fn input_base(&self) -> usize {
        if self.controlled_lighting {
            lighting_control::ROWS
        } else {
            0
        }
    }
}

/// The report entry of one built surface.
fn entry(
    setup: &Setup<'_>,
    resources: &resources::Resources,
    lighting: &resources::Lighting,
    shaders: &pixel::Shaders,
    stage: u8,
) -> Value {
    let material = setup.material;
    let surface = &setup.surface;
    let tint = surface.constant("albedo_color").unwrap_or([1.; 4]);
    let emission_tint = surface
        .constant("self_illum_color")
        .or_else(|| surface.constant("self_illum_tint_color"))
        .unwrap_or([1.; 4]);
    let emission_scale = surface
        .constant("self_illum_intensity")
        .map_or(1., |v| v[0])
        .max(0.);
    json!({"source":material.tag,"source_properties":material.properties,"cached_decal":material.decal,"effective_constants":surface.constants,"animation_notes":resources.notes,"animated_parameters":surface.parameters.bindings(),"options":material.options,"bound_textures":resources.bindings,"texture_registers":resources.registers,"albedo_color":tint,"emission_color":emission_tint,"emission_scale":emission_scale,"alpha":material.alpha_mode(),"native_stage":stage,"hidden_when_aimed":setup.aimed.is_some(),"baked_emission":false,"baked_transport":false,"baked_visibility":setup.controlled_lighting,"additional_analytical_light":shaders.baked.is_some(),"baked_stage":shaders.baked.as_ref().map(|_|4),"glow_stage":shaders.glow.as_ref().map(|_|7),"environment_reflection":lighting.reflected.as_ref().map(|(_, scale)| scale),"native_environment_reflection":setup.controlled_lighting,"paint":setup.paint.map(|paint| json!({"channel":paint.channel+10,"mask":paint.mask,"constant_buffer":paint.slot,"scope_mask":paint.scopes,"method":"Native equipped dye color multiplied through the authored surface mask. A missing or entirely empty dye record preserves source color."})),"limits":["Opaque surfaces use the native deferred approximation. Transparent surfaces use premultiplied output. Decimal ammunition frame functions are bound to the live magazine input. Time-driven texture scale and translation use the Reach mapping evaluator. Unbound material inputs are reported in animation_notes. Scalar and color overlays use the source constant routes. Activation events and unbound object inputs remain limited. Scene-depth fade transport needs live draw verification. Explicit overrides select a static appearance state."]})
}

/// The stock depth prepass technique of weapon parts. Every opaque surface of a stock weapon
/// model, such as `80C689B7`, also draws in stage 12 with this vertex-only technique, in every
/// detail band. Destiny 2 ports keep it through their native parts and stay drawn in first person
/// against walls, while Reach and Marathon weapons drawn without it were cut by walls and vanished
/// behind them. That the prepass is what holds a held weapon over nearer world geometry is
/// inferred from that difference: no consumer of the stage was traced.
const PREPASS: u32 = 0x80ef_b192;

/// Adds `material-prepass`, [`PREPASS`]'s header with the surfaces' own vertex program, so the
/// prepass writes the depth the surface draws at.
fn prepass(native: &mut Reader, g: &mut Graph, vertex: &str, prefix: &str) -> Result<()> {
    let template = native.tag(PREPASS, Some(0x808071e8))?;
    ensure!(
        template.u32(0x08)? == 2
            && template.u32(0x2c8)? == u32::MAX
            && template.0.get(0x24..0x28) == Some(&[0x80, 1, 1, 0][..]),
        "Native depth prepass technique differs"
    );
    let vertex = vertex
        .replace("$VERTEX_LIGHT_DECL$", "")
        .replace("$VERTEX_LIGHT_READ$", "o6=0;o7=0;o8=0;")
        .replace("$SHADOW_RESOURCE$", "")
        .replace("$SHADOW_PROJECTION$", "");
    program(
        native,
        g,
        &format!("{prefix}prepass-vertex"),
        template.u32(0x48)?,
        &vertex,
        true,
    )?;
    let mut bytes = template.0.clone();
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    g.add(
        &format!("{prefix}material-prepass"),
        PREPASS,
        &bytes,
        None,
        vec![json!({"offset":0x48,"symbol":format!("{prefix}prepass-vertex")})],
    )
}

/// The texture role under which a lightmapped surface with a `dynamic` environment map carries the
/// cube of the cluster it stands in, as Reach's renderer binds `dynamic_environment_map_0`. The
/// level's surfaces and posed props split into variants by that cube ([`environment::bind`]).
pub(in crate::halo_reach) const DYNAMIC_ROLE: &str = "dynamic_environment_map_0";

/// The constants a posed prop's surface variants carry its lighting probe in, as the three vertex
/// lighting records a lightmapped surface reads (`v6` to `v8`): the lobe's color over the atlas
/// scale with its bandwidth, the ambient color over the scale with the squared sun visibility,
/// and the lobe's unit direction with one, the marker of linear vertex lighting. No Reach
/// material names these constants.
pub(in crate::halo_reach) const PROBE_CONSTANTS: [&str; 3] = [
    "parhelion_probe_lobe",
    "parhelion_probe_ambient",
    "parhelion_probe_direction",
];

/// A surface's lighting probe ([`PROBE_CONSTANTS`]) written over its vertex lighting records, so
/// the level lighting decodes it as linear vertex light, or nothing for a surface without one.
fn probe(surface: &surface::Surface<'_>) -> String {
    match PROBE_CONSTANTS.map(|name| surface.constant(name)) {
        [Some(lobe), Some(ambient), Some(direction)] => format!(
            "v6={};v7={};v8={};",
            animation::vector(lobe),
            animation::vector(ambient),
            animation::vector(direction)
        ),
        _ => String::new(),
    }
}

/// Reach's object change colors, in the order the change-color albedos mask them by the red,
/// green, blue and alpha channels of `change_color_map`.
pub(in crate::halo_reach) const CHANGE_COLORS: [&str; 4] = [
    "primary_change_color",
    "secondary_change_color",
    "tertiary_change_color",
    "quaternary_change_color",
];

/// How many object change colors a surface's albedo option multiplies in, zero for none.
pub(in crate::halo_reach) fn change_colors(material: &Material) -> usize {
    match material.options.get("albedo").map(String::as_str) {
        Some("two_change_color") => 2,
        Some("four_change_color" | "four_change_color_applying_to_specular") => 4,
        _ => 0,
    }
}

/// Placement colors can drive emission even when the albedo has no change-color mask.
pub(in crate::halo_reach) fn change_color_inputs(material: &Material) -> usize {
    let emission = usize::from(matches!(
        material
            .options
            .get("self_illumination")
            .map(String::as_str),
        Some(
            "change_color"
                | "change_color_detail"
                | "palettized_plasma_change_color"
                | "times_diffuse"
                | "self_illum_times_diffuse"
        )
    ));
    change_colors(material).max(emission)
}

/// The specular color at normal incidence of a native dielectric (`F0 = 0.04 + 0.84 * metalness`,
/// with metalness zero on every converted surface).
const NATIVE_DIELECTRIC_F0: f32 = 0.04;

/// The value a surface stores as `roughness`, one minus the native smoothness `s`, for a Reach
/// highlight of Beckmann slope `slope` and strength `strength`. The native lobe is GGX with
/// `alpha = (1 - 0.9 s)^2`, and a Beckmann slope `m` matches a GGX `alpha` of about `m`, so
/// `s = (1 - sqrt(m)) / 0.9`. A Phong exponent `p` matches a Beckmann slope of
/// `sqrt(2 / (p + 2))`. The native specular color stays at [`NATIVE_DIELECTRIC_F0`], so a surface
/// whose Reach highlight is weaker than that scales its smoothness down by the ratio and draws a
/// broader, dimmer native highlight. Taking Reach's authored `roughness` as one minus the
/// smoothness drew Breakneck's concrete at a median smoothness of 0.95, where its Phong exponents
/// ask for a broad highlight.
fn native_roughness(slope: &str, strength: &str) -> String {
    format!(
        "(1-saturate((1-sqrt(saturate({slope})))/0.9)*saturate(({strength})/{NATIVE_DIELECTRIC_F0:.2}))"
    )
}

/// Reach's specular mask for a surface's `specular_mask` option, as `calc_specular_mask_ps` reads
/// it: the unlit albedo's alpha, the specular mask texture's alpha, their product, or one. `None`
/// for an option without a translation or a missing mask texture.
fn specular_mask(material: &Material, samples: &BTreeMap<usize, String>) -> Option<String> {
    let texture_mask = samples
        .get(&SPECULAR_MASK_SLOT)
        .map(|sample| format!("({sample}).a"));
    match material.options.get("specular_mask").map(String::as_str) {
        Some("specular_mask_from_diffuse") => Some("unlit.a".to_owned()),
        Some("specular_mask_mult_diffuse") => Some(format!("unlit.a*{}", texture_mask?)),
        Some("specular_mask_from_texture") => texture_mask,
        Some("no_specular_mask") => Some("1".to_owned()),
        _ => None,
    }
}

/// The strength of a Cook-Torrance, organism or two-lobe Phong surface's analytical highlight at
/// normal incidence: its specular mask times `specular_coefficient`, which an enabled material
/// texture scales, `analytical_specular_contribution` and the brightest channel of its normal
/// specular tint, as `calc_material_analytic_specular` and the material models scale it. An
/// untranslated mask counts as one.
fn specular_strength(
    material: &Material,
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> String {
    let phong =
        material.options.get("material_model").map(String::as_str) == Some("two_lobe_phong");
    let mut coefficient = format!("max({},0)", surface.scalar("specular_coefficient", 1.));
    if !phong
        && surface.boolean("use_material_texture") == Some(true)
        && let Some(sample) = samples.get(&2)
    {
        let black = surface.scalar("material_texture_black_specular_multiplier", 1.);
        coefficient = format!("{coefficient}*lerp({black},1,({sample}).a)");
    }
    let tint = surface.color(
        if phong {
            "normal_specular_tint"
        } else {
            "specular_tint"
        },
        [1.; 4],
    );
    format!(
        "{}*{coefficient}*max({},0)*max(({tint}).r,max(({tint}).g,({tint}).b))",
        specular_mask(material, samples).unwrap_or_else(|| "1".into()),
        surface.scalar("analytical_specular_contribution", 0.5),
    )
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
/// `albedo_color`, `albedo_color2` and `albedo_color3` over `neutral_gray`. `two_change_color` and
/// the `four_change_color` albedos multiply the base and detail maps by
/// `1 - m + m * color` for each of the object's change colors ([`CHANGE_COLORS`]) and the channel
/// `m` of `change_color_map` that masks it, as `calc_albedo_four_change_color_ps` does. The colors
/// are the placement's, which the level's objects write into each colored variant's constants. A
/// surface without them keeps white. Surface permutations are checked before generation.
/// A terrain shader instead mixes its layers by their blend `weights`, each at its own
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
        (Some("two_detail"), (Some(d1), Some(d2), _)) => {
            format!("({base})*{}*{}", detail(d1), detail(d2))
        }
        (Some("two_detail_black_point"), (Some(d1), Some(d2), _)) => {
            let midpoint = format!("((({base}).a+1)*0.5)");
            let alpha = format!("(({d1}).a*({d2}).a)");
            let mask = format!(
                "({midpoint}*saturate(({alpha}-({base}).a)/({midpoint}-({base}).a))+saturate({alpha}-{midpoint}))"
            );
            format!("float4(({base}).rgb*({d1}).rgb*({d2}).rgb*{dm:.6}*{dm:.6},{mask})")
        }
        (Some("two_detail_overlay"), (Some(d1), Some(d2), _)) => {
            let overlay = samples
                .get(&DETAIL_OVERLAY_SLOT)
                .map_or("float4(1,1,1,1)", String::as_str);
            format!(
                "float4(({base}).rgb,1)*{}*{}",
                detail(&format!("lerp({d1},{d2},({base}).a)")),
                detail(overlay)
            )
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
        (Some(_), (first, _, _)) if change_colors(material) > 0 => {
            let mask = samples
                .get(&9)
                .map_or_else(|| "(float4(0,0,0,0))".into(), |m| format!("({m})"));
            let factors = CHANGE_COLORS[..change_colors(material)]
                .iter()
                .zip(["x", "y", "z", "w"])
                .map(|(name, k)| {
                    format!(
                        "float4(1-{mask}.{k}+{mask}.{k}*{},1)",
                        surface.color(name, [1.; 4])
                    )
                })
                .collect::<Vec<_>>()
                .join("*");
            let detail = first.map_or_else(|| "float4(1,1,1,1)".into(), |d| detail(d));
            format!("({base})*{detail}*{factors}")
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

/// The stock alpha-tested static shadow technique in `w64_globals_03ab`. Distant Shore's main
/// static group draws 174 shadow groups with it, and 14 of the group's 15 static shadow techniques
/// with a pixel program, 280 of its 282 such shadow groups, share its header: mode 1, scopes
/// `0x8203` and `0x80008203`, and rasterizer byte `0x81` at `+0x22`, row 1 of the native table,
/// which culls neither face. The solid static shadow technique stores no rasterizer byte and
/// inherits the context's state.
const CLIPPED_STATIC_SHADOW: u32 = 0x80f5_6bdc;
/// Scope 15, `sdsm_bias_and_scale_textures` in the render globals (`80B9E57C`), which binds the
/// light's atlas rectangle a shadow vertex program reads at `t2`. Every stock static shadow
/// technique sets it, and `simple_geometry_depth_only_with_sdsm` sets it where
/// `simple_geometry_depth_only` does not.
const SHADOW_ATLAS_SCOPE: u32 = 0x8000;
/// A material's rasterizer byte, and the value that selects row 1 without face culling.
const RASTERIZER: usize = 0x22;
const TWO_SIDED: u8 = 0x81;
/// Native row 2 uses counterclockwise front faces and back-face culling.
const BACK_FACES_CULLED: u8 = 0x82;
/// The state byte after the rasterizer word. The entity surface shell (`80EC270D`) holds `0x80`
/// there, an explicit depth state, and stock static surfaces, opaque (`80F56C45`) and
/// alpha-tested (`80F56C21`, `80FE117F`), hold 0, which keeps the stage's own. Every opaque level
/// surface drawn on the statics takes 0. Opaque statics that kept the shell's `0x80` received no native sun
/// shadow: on Paradiso neither a cliff's shadow nor the player's fell on them inside the 30 m where
/// the cascades replace the baked shadow, and turning the cascades off changed no pixel, while the
/// cascades' views were built along the map's sun and fitted to the visible ground (live reads of
/// the client's cascade bases, sizes and planes).
const DEPTH_STATE: usize = 0x24;

/// Whether level surface `material` is an alpha-tested static's, which draws as stock
/// alpha-tested statics do: both faces, no explicit depth state and no depth prepass
/// ([`crate::halo_reach::level::native`]'s statics). In Distant Shore's 59 static meshes all 66
/// parts with an alpha-tested shadow draw a surface at layout 127 or 128 with state bytes
/// `00 00 81 00 00 7F 7F 00` at `+0x20`, and none draws in the prepass. Reach draws its foliage
/// cards from both sides, and culled, Paradiso's pine branches showed from above only. Both faces
/// light by the authored normal. A material in [`Binding::paired_faces`] already supplies opposite
/// triangles with their own normals and instead selects back-face culling.
fn two_sided_static(material: &Material) -> bool {
    material.alpha_mode() == "MASK" && !material.decal()
}
/// The material header before its first stage record. It holds the mode, scopes and state bytes
/// and no array descriptor.
const MATERIAL_HEADER: usize = 0x48;

/// Whether surface `material` also casts a sun shadow through a shadow technique of its own
/// ([`clipped_shadow`]): an alpha-tested surface that is neither a decal nor a decorator set's
/// (`dctr`). The shared solid shadow technique draws every triangle whole, so a branch or a grate
/// drawn with it would shadow its cut-out texels too. Decorators, the grass, bushes and pebbles
/// scattered over a structure, stay out of the shadow stage. When alpha-tested surfaces drew there
/// with the solid technique, Forge World's 113,424 ground cover cards shadowed the grass they stand
/// in.
pub(in crate::halo_reach) fn clipped_caster(material: &Material) -> bool {
    material.alpha_mode() == "MASK"
        && !material.decal()
        && !material.tag.as_ref().is_some_and(|tag| tag.group == "dctr")
}

/// The pixel program of an alpha-tested surface's shadow technique: surface program `text`, whose
/// outputs are `output`, up to and including its alpha `clip`, writing nothing. It keeps exactly
/// the texels the surface keeps, through the same declarations, samples and parameters.
fn clip_program(text: &str, output: &str, clip: &str) -> Result<String> {
    let signature = format!(",\n{output}) {{\n");
    ensure!(
        text.matches(&signature).count() == 1,
        "The surface program's source differs at its outputs"
    );
    let (head, body) = text
        .split_once(&signature)
        .context("The surface program has no outputs")?;
    ensure!(
        !clip.is_empty() && body.matches(clip).count() == 1,
        "The surface program's source differs at its alpha clip"
    );
    let end = body
        .find(clip)
        .context("The surface program has no alpha clip")?
        + clip.len();
    Ok(format!("{head}) {{\n{}\n}}\n", &body[..end]))
}

/// A pixel-bearing shadow envelope with the owning geometry's scopes. Static alpha casters use
/// the verified two-sided envelope. Entities retain the ordinary object shadow envelope.
fn clipped_shadow_envelope(native: &mut Reader, statics: bool) -> Result<std::sync::Arc<Payload>> {
    let envelope = native.tag(
        if statics {
            CLIPPED_STATIC_SHADOW
        } else {
            0x80ec271d
        },
        Some(0x808071e8),
    )?;
    let (required, excluded) = if statics {
        (STATIC_SCOPE, OBJECT_SCOPE)
    } else {
        (OBJECT_SCOPE, STATIC_SCOPE)
    };
    let scoped = |at: usize| {
        envelope.u32(at).is_ok_and(|scope| {
            scope & (required | SHADOW_ATLAS_SCOPE) == required | SHADOW_ATLAS_SCOPE
                && scope & excluded == 0
        })
    };
    ensure!(
        envelope.u32(0x08)? == 1
            && (!statics || envelope.0.get(RASTERIZER) == Some(&TWO_SIDED))
            && scoped(0x18)
            && scoped(0x1c),
        "Native alpha-tested shadow technique differs"
    );
    Ok(envelope)
}

/// Adds an alpha-tested shadow technique with the surface's pixel textures, samplers and
/// expression. The owning envelope supplies depth state and the shadow atlas. Static cards use
/// its two-sided state. Entities retain their surface's culling and extra scopes. The shadow
/// vertex program reads no baked light, so that vertex binding is left out. The clipping program
/// discards the same material texels as the surface.
#[allow(clippy::too_many_arguments)]
fn clipped_shadow(
    g: &mut Graph,
    envelope: &Payload,
    statics: bool,
    surface: &[u8],
    vertex: &str,
    pixel: &str,
    bindings: &[(usize, String)],
    sampler_symbols: &[String],
    symbol: &str,
) -> Result<()> {
    let mut bytes = surface.to_vec();
    lighting_control::stage(&mut bytes, 3)?;
    bytes[0x08..MATERIAL_HEADER].copy_from_slice(
        envelope
            .0
            .get(0x08..MATERIAL_HEADER)
            .context("The alpha-tested shadow technique is too short")?,
    );
    if !statics {
        // Preserve the surface's extra scopes, including gear paint, material animation and
        // frame time. The shadow envelope contributes its depth mode and atlas scope.
        for at in [0x18, 0x1c] {
            let scope = Payload(surface.to_vec()).u32(at)? | SHADOW_ATLAS_SCOPE;
            put(&mut bytes, at, &scope.to_le_bytes())?;
        }
        bytes[RASTERIZER] = surface[RASTERIZER];
    }
    put(&mut bytes, 0x50, &[0; 16])?;
    let patches = surface_patches(&bytes, vertex, pixel, bindings, sampler_symbols)?;
    g.add(
        symbol,
        if statics {
            CLIPPED_STATIC_SHADOW
        } else {
            0x80ec271d
        },
        &bytes,
        None,
        patches,
    )
}

fn expand_decimal_frames(
    c: &Cache,
    pages: &mut Pages,
    mapping: &material::Mapping,
    normal: bool,
    image: &mut material::Image,
) -> Result<()> {
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
    Ok(())
}
