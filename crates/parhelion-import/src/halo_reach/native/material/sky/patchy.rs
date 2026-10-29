//! Reach's patchy fog, translated as soft horizontal sheets drawn in the sky's transparent stage.
//!
//! Reach draws patchy fog as a screen pass (`explicit\patchy_fog.hlsl`): an `albedo` pass writes
//! eight noise layers on screen-aligned sheets, each faded by world height as
//! `saturate(scale * z + offset)^2` and by a per-sheet fade, to two private targets, and the apply
//! pass reads them with the scene depth, fades each layer by view depth, and writes the fog's two
//! tints `T * tint + (1 - T) * inner tint` with alpha `T`, where `T = exp2(optical depth * scale)` is
//! the share of the scene left. `c_patchy_fog::evaluate_shader_constants` (`825E6F00` in the Xbox
//! `halo3_tag_play` build) derives the layers' coordinates and fades on the CPU in VMX128 code,
//! which was not decoded, and moves the sheets with the camera from frame to frame. A converted
//! surface cannot add a full-screen pass with private targets. The sheets here are horizontal
//! planes on the world heights a multiple of Reach's sheet spacing apart, the [`SHEETS`] levels
//! around the camera's height, each faded by its height above the camera so a level fades out
//! before the stack moves past it. They fade against the scene depth (`t10`), as the level's
//! refracting water reads it, and within the distance Reach's eight sheets reach. A sheet's blend
//! leaves `T` of the scene and adds `(1 - T)` of the tints' mix, the reading of Reach's color and
//! alpha this takes, since Reach's blend state was not read.
//!
//! The fog tag's per-cluster parameters (`fogg` `+0x74`, named by the tag build's
//! `atmosphere_definition` fields) hold the sheet density, the tint and inner tint, an intensity,
//! the full and half intensity heights, the wind direction and the reference plane height. The
//! atmosphere globals (`atgf`) hold the noise bitmap, its texture repeat rate, the distance
//! between sheets and a depth fade factor.
use super::*;

/// A level's patchy fog. Heights are in Reach world units above the reference plane, the wind in
/// world units per second, and `origin` the map position of Reach's world origin, in meters.
#[derive(Clone, Debug)]
pub(in crate::halo_reach) struct Patchy {
    pub noise: material::Mapping,
    pub density: f32,
    pub tint: [f32; 3],
    pub inner: [f32; 3],
    pub intensity: f32,
    pub full_height: f32,
    pub half_height: f32,
    pub wind: [f32; 3],
    pub reference: f32,
    pub repeat: f32,
    pub spacing: f32,
    pub depth_fade: f32,
    pub origin: [f32; 3],
}

/// The sheets Reach draws, and so the sheet levels drawn around the camera and how far from it,
/// in sheet spacings, they reach.
pub(in crate::halo_reach) const SHEETS: usize = 8;
/// The world units one repeat of the noise covers on a sheet, in sheet spacings, at a repeat rate
/// of 1. Reach's screen-aligned sheets map the noise across the screen, which at the middle sheet's
/// distance spans a few spacings, an approximation.
const TILE_SPACINGS: f32 = 8.;
/// The steepest a view ray's path through one sheet's slab grows, as one over the sine of its
/// angle to the sheet.
const GRAZING: f32 = 0.08;

impl Patchy {
    /// The height fade's scale and offset, per world unit above the reference plane: full at the
    /// full intensity height and a half at the half intensity height, squared after the clamp as
    /// Reach's albedo pass squares it. Reach's own heights may be absolute, an untested reading.
    pub fn fade(&self) -> (f32, f32) {
        let span = self.half_height - self.full_height;
        let scale = if span.abs() > 1e-4 {
            (std::f32::consts::FRAC_1_SQRT_2 - 1.) / span
        } else {
            0.
        };
        (scale, 1. - scale * self.full_height)
    }

    /// The heights above the reference plane, in world units, between which the fade is above
    /// zero, within `floor` and `ceiling`.
    pub fn band(&self, floor: f32, ceiling: f32) -> Option<(f32, f32)> {
        let (scale, offset) = self.fade();
        let (low, high) = if scale < 0. {
            (floor, ceiling.min(-offset / scale))
        } else if scale > 0. {
            (floor.max(-offset / scale), ceiling)
        } else if offset > 0. {
            (floor, ceiling)
        } else {
            return None;
        };
        (high > low).then_some((low, high))
    }
}

/// The sheets' vertex program: each sheet's corners keep their place across the level, and its
/// height is the level of its order in the stack (`normal.w` times [`SHEETS`]) around the
/// camera's level, `spacing` meters apart. `TEXCOORD0.x` carries the level's number.
fn vertex(spacing: f32) -> String {
    format!(
        r#"
cbuffer Object : register(b11) {{float4 objectData[8];}};
cbuffer View : register(b12) {{float4 viewData[14];}};
void main(float4 position:POSITION0,float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,float2 detail:TEXCOORD2,
out float4 o0:TEXCOORD0,out float4 o1:TEXCOORD1,out float4 o2:TEXCOORD2,out float4 o3:TEXCOORD3,out float3 o4:TEXCOORD4,out float4 projected:SV_POSITION0) {{
float3 local=position.xyz*objectData[4].xyz+objectData[5].xyz;
float level=floor(viewData[7].z/{spacing:.6})-{below}+round(normal.w*{SHEETS});
float3 relative=float3(local.xy+objectData[3].xy-viewData[7].xy,level*{spacing:.6}-viewData[7].z);
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
o0=float4(level,0,0,0);o1=float4(0,0,0,0);o2=float4(relative,relative.z+viewData[7].z);o3=float4(0,0,0,0);o4=float3(0,0,0);
}}
"#,
        below = SHEETS / 2 - 1,
    )
}

/// Adds the patchy fog's sheet surface keyed `key`, copying the stock sky envelope `envelope`, and
/// returns its report.
#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn surface(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    patchy: &Patchy,
    key: u32,
    envelope: u32,
    scale: SkyScale,
) -> Result<Value> {
    let envelope = (envelope, rigid_envelope(native, envelope)?);
    let (clamped, sampler_class) = sampler_shell(native)?;
    let symbol = texture(c, pages, native, g, &patchy.noise, false, false)?;
    let modes = vec![sampler_modes(&patchy.noise)?];
    let samplers = samplers(native, g, clamped, &modes)?;
    let (fade_scale, fade_offset) = patchy.fade();
    let tile = TILE_SPACINGS * patchy.spacing.max(0.01) / patchy.repeat.max(0.01);
    let wind = [patchy.wind[0] / tile, patchy.wind[1] / tile];
    let spacing = patchy.spacing * 3.048;
    let reach = SHEETS as f32 * spacing;
    let rgb = |c: [f32; 3]| format!("float3({:.6},{:.6},{:.6})", c[0], c[1], c[2]);
    let text = format!(
        r#"
Texture2D<float4> Map0:register(t0);
{samplers}
{scene}
cbuffer Frame : register(b13) {{float4 frameData[2];}};
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
out float4 o0:SV_TARGET0) {{
float time=frameData[0].x;
float distance=max(length(v2.xyz),0.001);
float3 world=(v2.xyz+viewData[7].xyz-float3({ox:.6},{oy:.6},{oz:.6}))/3.048;
float2 drift=frac(float2(0.37,0.61)*v0.x);
float2 uv=world.xy*{inverse:.9}+drift+float2({wx:.9},{wy:.9})*time;
float noise=0.5*(Map0.Sample({sampler},uv).r+Map0.Sample({sampler},uv*1.618+drift.yx-float2({wx:.9},{wy:.9})*0.5*time).r);
float fade=saturate({fade_scale:.9}*(world.z-({reference:.6}))+{fade_offset:.9});fade*=fade;
float stack=saturate(1-abs(v2.z)/{half_reach:.4});
float sceneDepth=reachSceneDepthAt(position.xy*viewData[12].zw);
float soft=saturate((sceneDepth-reachSceneDepth(position.z))*{depth_fade:.9});
float nearby=saturate(({reach:.4}-distance)/{half_reach:.4});
float slant=1/max(abs(v2.z)/distance,{GRAZING});
float density={density:.9}*noise*fade;
float transmit=exp2(-density*density*slant*soft*nearby*stack*{intensity:.9});
float3 fog=transmit*{tint}+(1-transmit)*{inner};
float3 color=clamp(fog*(1-transmit),0,{white:.9})*{radiance:.9};
o0=float4(color*{REFERENCE_ILLUMINATION:.9}*frameData[1].z,1-transmit);
}}
"#,
        samplers = samplers.declarations,
        scene = super::super::water::SCENE_DECLARATIONS,
        ox = patchy.origin[0],
        oy = patchy.origin[1],
        oz = patchy.origin[2],
        inverse = 1. / tile,
        wx = wind[0],
        wy = wind[1],
        sampler = sampler_name(0),
        reference = patchy.reference,
        depth_fade = patchy.depth_fade / 3.048,
        half_reach = reach * 0.5,
        density = patchy.density,
        intensity = patchy.intensity,
        tint = rgb(patchy.tint),
        inner = rgb(patchy.inner),
        white = scale.white,
        radiance = scale.radiance,
    );
    let vertex_symbol = format!("sky-patchy-vertex-{key:08X}");
    program(
        native,
        g,
        &vertex_symbol,
        envelope.1.u32(0x48)?,
        &vertex(spacing),
        true,
    )?;
    let pixel = format!("sky-patchy-pixel-{key:08X}");
    program(native, g, &pixel, envelope.1.u32(0x2c8)?, &text, false)?;
    write_material(
        g,
        key,
        (envelope.0, &envelope.1),
        &vertex_symbol,
        &pixel,
        &[(0, symbol)],
        &samplers,
        sampler_class,
        true,
    )?;
    Ok(json!({
        "key": format!("{key:08X}"),
        "noise": patchy.noise.bitmap,
        "density": patchy.density,
        "tint": patchy.tint,
        "inner_tint": patchy.inner,
        "intensity": patchy.intensity,
        "full_intensity_height": patchy.full_height,
        "half_intensity_height": patchy.half_height,
        "reference_plane": patchy.reference,
        "wind": patchy.wind,
        "texture_repeat": patchy.repeat,
        "sheet_spacing": patchy.spacing,
        "depth_fade": patchy.depth_fade,
        "fade_scale": fade_scale,
        "fade_offset": fade_offset,
        "tile_world_units": tile,
        "reach_metres": reach,
        "sheets": SHEETS,
        "stage": 7,
    }))
}
