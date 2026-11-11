//! Reach's screen fog, translated as one camera-facing sheet in the sky's transparent stage.
//!
//! Reach fogs its opaque scene in one screen pass after lighting and before transparents.
//! `c_player_view::render` (`82590908` in the Xbox `halo3_tag_play` build) calls
//! `render_screen_space_fogs` (`8258D2A0`) after the static lighting, the lights and the water,
//! and `c_atmosphere_fog_interface::render_screen_atm_fog` (`825C7290`) draws the fog with the
//! depth buffer bound and only RGB written. The pixel programs of `explicit\screen_atm_fog.hlsl`
//! rebuild each pixel's world position from depth and write `color * extinction + inscatter`,
//! through a table that saturates at 10,000 world units (`calculate_screen_fog_LUT_constants`,
//! `82605080`). Every opaque pixel takes it, the level, its objects and the player's own weapon
//! alike. Destiny lights players, weapons and vehicles itself, and their programs carry no Reach
//! fog, so the translation draws the same pass: a sheet just ahead of the camera whose pixel
//! program reads the scene depth, which its material binds itself ([`SceneDepth`]), rebuilds the
//! pixel's position along its view ray and evaluates the level surfaces' `reachFog` there.
//! Its premultiplied blend leaves the extinction of the scene and adds the in-scatter. Opaque
//! level surfaces therefore leave their own fog to this sheet ([`super::super::hybrid_finish`]).
//! Blended level surfaces keep fogging themselves, as Reach's transparents fog per vertex, and
//! the sky fogs its own surfaces. A pixel whose depth is the cleared far depth holds only sky and
//! is left alone.
use super::*;

/// The view depth of the sheet, in meters, ahead of every surface it fogs. Surfaces nearer than
/// this, such as parts of the held weapon, stay clear, where Reach's distance bias leaves the
/// first 15 to 50 world units clear on every sampled map. The lens flare draws at the same depth.
const SHEET_DEPTH: f32 = 0.5;

/// The share of the far depth beyond which a pixel counts as cleared, holding only sky.
const FAR_SHARE: f32 = 0.999;

/// The pixel texture register the sheet binds the scene depth to ([`SceneDepth`]).
const DEPTH_REGISTER: usize = 0;

/// The sheet's vertex program: a quad filling the view at [`SHEET_DEPTH`], placed as the patchy
/// fog's sheets are ([`super::patchy`]). Projection columns give the camera's axes, forward-axis
/// jitter is removed before normalizing the horizontal and vertical axes, and decoded UVs select
/// the corners. `TEXCOORD0.y` carries the sheet's view depth and `TEXCOORD2` its position less the
/// camera's.
fn vertex() -> String {
    format!(
        r#"
cbuffer Object : register(b11) {{float4 objectData[8];}};
cbuffer View : register(b12) {{float4 viewData[14];}};
void main(float4 position:POSITION0,float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,float2 detail:TEXCOORD2,
out float4 o0:TEXCOORD0,out float4 o1:TEXCOORD1,out float4 o2:TEXCOORD2,out float4 o3:TEXCOORD3,out float3 o4:TEXCOORD4,out float4 projected:SV_POSITION0) {{
float depth={SHEET_DEPTH:.6};
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
o0=float4(0,depth,0,0);o1=float4(0,0,0,0);o2=float4(relative,relative.z+viewData[7].z);o3=float4(0,0,0,0);o4=float3(0,0,0);
}}
"#,
    )
}

/// The sheet's pixel program. The scene depth is a view depth, so the scene point on the pixel's
/// ray is the sheet's point scaled by the scene depth over the sheet's. Rays stop at the fog
/// table's 10,000 world units with their height moved along them, as the opaque sky's do. The
/// in-scatter shows at the sky's Reach exposure ([`SkyScale::exposure`]), so fogged distance meets
/// the sky's horizon at one brightness, as in Reach, while the scene behind it keeps the frame's.
fn pixel(fog: &Fog, scale: SkyScale, depth: SceneDepth) -> String {
    format!(
        r#"
{scene}
cbuffer Frame : register(b13) {{float4 frameData[2];}};
{functions}
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
out float4 o0:SV_TARGET0) {{
{control}
float sceneDepth=reachSceneDepthAt(position.xy*viewData[12].zw);
float farDepth=max(reachSceneDepth(0),reachSceneDepth(1));
if(!(sceneDepth>0)||sceneDepth>=farDepth*{FAR_SHARE:.6}){{o0=float4(0,0,0,0);return;}}
float3 toPoint=v2.xyz*(sceneDepth/max(v0.y,0.000001));
float kept=min(1,{table:.3}/max(length(toPoint),0.001));
float4 fog=reachFog(-toPoint*kept,viewData[7].z+toPoint.z*kept);
o0=float4(max(fog.rgb,0)*{exposure},1-saturate(fog.a));
}}
"#,
        scene = depth.declarations_with(overlay::DECLARATION),
        control = overlay::DISCARD,
        functions = fog.functions(),
        table = FOG_TABLE_DISTANCE,
        exposure = scale.exposure(),
    )
}

/// Adds the screen fog's sheet surface keyed `key` for the level's `fog`, copying the stock sky
/// envelope `envelope`, and returns its report.
pub(in crate::halo_reach) fn surface(
    native: &mut Reader,
    g: &mut Graph,
    fog: &Fog,
    key: u32,
    envelope: u32,
    scale: SkyScale,
) -> Result<Value> {
    let envelope = (envelope, rigid_envelope(native, envelope)?);
    let (clamped, sampler_class) = sampler_shell(native)?;
    let samplers = samplers(native, g, clamped, &[])?;
    let depth = SceneDepth::new(native, DEPTH_REGISTER)?;
    let vertex_symbol = format!("sky-screen-fog-vertex-{key:08X}");
    program(
        native,
        g,
        &vertex_symbol,
        envelope.1.u32(0x48)?,
        &vertex(),
        true,
    )?;
    let pixel_symbol = format!("sky-screen-fog-pixel-{key:08X}");
    program(
        native,
        g,
        &pixel_symbol,
        envelope.1.u32(0x2c8)?,
        &pixel(fog, scale, depth),
        false,
    )?;
    write_material(
        g,
        key,
        (envelope.0, &envelope.1),
        &vertex_symbol,
        &pixel_symbol,
        &[],
        &samplers,
        sampler_class,
        true,
        Some(depth),
        Some(overlay::Kind::ScreenFog),
    )?;
    Ok(json!({
        "key": format!("{key:08X}"),
        "sheet_depth_metres": SHEET_DEPTH,
        "far_share": FAR_SHARE,
        "scene_depth_register": DEPTH_REGISTER,
        "table_metres": FOG_TABLE_DISTANCE,
        "radiance_scale": scale.radiance,
        "sky_band": {"base": fog.sky.base, "height": fog.sky.height, "rate": fog.sky.rate, "reach": fog.sky.reach, "color": fog.sky.color},
        "ground_band": {"base": fog.ground.base, "height": fog.ground.height, "rate": fog.ground.rate, "reach": fog.ground.reach, "color": fog.ground.color},
        "bias": fog.bias,
        "lift_metres": fog.lift,
        "fog_light": fog.light.is_some(),
        "orientation": "camera_facing",
        "stage": 7,
    }))
}
