//! Reach's patchy fog, translated as camera-facing sheets in the sky's transparent stage.
//!
//! Reach draws patchy fog as a screen pass (`explicit\patchy_fog.hlsl`): an `albedo` pass writes
//! eight noise layers on screen-aligned sheets, each faded by world height as
//! `saturate(scale * z + offset)^2` and by a per-sheet fade, to two private targets, and the apply
//! pass reads them with the scene depth, fades each layer by view depth, and writes the fog's two
//! tints `T * tint + (1 - T) * inner tint` with alpha `T`, where `T = exp2(optical depth * scale)` is
//! the share of the scene left. The HREK PC constant producer at `892D60` derives the layers'
//! coordinates and fades from retained camera history. Its height and density calculations are
//! preserved by [`attenuation`]. A converted surface cannot add a full-screen pass with private
//! targets. The sheets here span the view at
//! fixed view depths, drawn far to near. World height is reconstructed per pixel, so a sheet
//! never turns edge-on when the camera crosses its height. They fade against the scene depth,
//! which their material binds itself ([`SceneDepth`]). Their fixed half-spacing phase uses the
//! source's half-weight first and last sheets. Retained phase, noise coordinates and view warp
//! remain approximations. A sheet's blend
//! leaves `T` of the scene and adds `(1 - T)` of the tints' mix, the reading of Reach's color and
//! alpha this takes, since Reach's blend state was not read.
//!
//! The fog tag's per-cluster parameters (`fogg` `+0x74`, named by the tag build's
//! `atmosphere_definition` fields) hold the sheet density, the tint and inner tint, an intensity,
//! the full and half intensity heights, the wind direction and the reference plane height. The
//! atmosphere globals (`atgf`) hold the noise bitmap, its texture repeat rate, the distance
//! between sheets and a depth fade factor.
use super::*;

mod attenuation;

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
/// The pixel texture register the sheets bind the scene depth to ([`SceneDepth`]), after the
/// noise map's.
const DEPTH_REGISTER: usize = 1;

impl Patchy {
    /// The original PC height coefficient, per world unit above the reference plane. The
    /// constant producer adds the reference to both authored heights. Bounds use heights
    /// relative to that reference, while the generated shader uses absolute Reach world height.
    pub fn fade(&self) -> (f32, f32) {
        let (scale, offset) = attenuation::height(
            self.full_height + self.reference,
            self.half_height + self.reference,
        );
        (scale, offset + scale * self.reference)
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

/// Reconstruct a view-filling quad at each sheet's view depth. Projection columns provide the
/// camera basis, including pitch and roll. Remove forward-axis jitter before normalizing the
/// horizontal and vertical axes, then compensate that jitter when placing the corners.
/// Mesh positions retain conservative world bounds but UVs place the rendered corners.
fn vertex(spacing: f32) -> String {
    format!(
        r#"
cbuffer Object : register(b11) {{float4 objectData[8];}};
cbuffer View : register(b12) {{float4 viewData[14];}};
void main(float4 position:POSITION0,float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,float2 detail:TEXCOORD2,
out float4 o0:TEXCOORD0,out float4 o1:TEXCOORD1,out float4 o2:TEXCOORD2,out float4 o3:TEXCOORD3,out float3 o4:TEXCOORD4,out float4 projected:SV_POSITION0) {{
float level={SHEETS}-round(normal.w*{SHEETS})-0.5;
float depth=level*{spacing:.6};
float2 corner=(uv*objectData[6].xy+objectData[6].zw)*2-1;
float3 forward=normalize(float3(viewData[0].w,viewData[1].w,viewData[2].w));
float3 horizontal=float3(viewData[0].x,viewData[1].x,viewData[2].x);
float3 vertical=float3(viewData[0].y,viewData[1].y,viewData[2].y);
float3 right=normalize(horizontal-forward*dot(horizontal,forward));
float3 up=normalize(vertical-forward*dot(vertical,forward));
float3 ahead=forward*depth;
float clipW=depth+viewData[13].w;
float3 relative=ahead+right*((corner.x*clipW-dot(horizontal,ahead)-viewData[13].x)/dot(horizontal,right))
    +up*((corner.y*clipW-dot(vertical,ahead)-viewData[13].y)/dot(vertical,up));
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
o0=float4(level,depth,0,0);o1=float4(0,0,0,0);o2=float4(relative,relative.z+viewData[7].z);o3=float4(0,0,0,0);o4=float3(0,0,0);
}}
"#,
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
    let depth = SceneDepth::new(native, DEPTH_REGISTER)?;
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
{attenuation}
cbuffer Frame : register(b13) {{float4 frameData[2];}};
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
out float4 o0:SV_TARGET0) {{
{control}
float time=frameData[0].x;
float3 world=(v2.xyz+viewData[7].xyz-float3({ox:.6},{oy:.6},{oz:.6}))/3.048;
float2 drift=frac(float2(0.37,0.61)*v0.x);
float2 uv=world.xy*{inverse:.9}+drift+float2({wx:.9},{wy:.9})*time;
float noise=0.5*(Map0.Sample({sampler},uv).r+Map0.Sample({sampler},uv*1.618+drift.yx-float2({wx:.9},{wy:.9})*0.5*time).r);
float sceneDepth=reachSceneDepthAt(position.xy*viewData[12].zw);
float soft=saturate((sceneDepth-reachSceneDepth(position.z))*{depth_fade:.9});
float sheetFade=(v0.x<1||v0.x>{last_sheet})?0.5:1;
float transmit=reachPatchyTransmission(noise,world.z,soft,sheetFade);
float3 fog=transmit*{tint}+(1-transmit)*{inner};
float3 color=clamp(fog*(1-transmit),0,{white})*{radiance:.9};
o0=float4(color*{finish},1-transmit);
}}
"#,
        samplers = samplers.declarations,
        scene = depth.declarations_with(overlay::DECLARATION),
        control = overlay::DISCARD,
        attenuation = attenuation::function(
            patchy.density,
            patchy.full_height,
            patchy.half_height,
            patchy.reference,
        ),
        ox = patchy.origin[0],
        oy = patchy.origin[1],
        oz = patchy.origin[2],
        inverse = 1. / tile,
        wx = wind[0],
        wy = wind[1],
        sampler = sampler_name(0),
        depth_fade = patchy.depth_fade / 3.048,
        last_sheet = SHEETS - 1,
        tint = rgb(patchy.tint),
        inner = rgb(patchy.inner),
        white = scale.white(),
        finish = scale.finish(),
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
        Some(depth),
        Some(overlay::Kind::PatchyFog),
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
        "scene_depth_register": DEPTH_REGISTER,
        "orientation": "camera_facing",
        "stage": 7,
    }))
}
