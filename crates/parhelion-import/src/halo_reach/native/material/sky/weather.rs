//! A Reach fog tag's weather effect (`fogg` `+0xAC`), such as Breakneck's falling ash and Sword
//! Base's light rain, drawn as particle fields around the camera in the sky's transparent stage.
//!
//! Reach plays the effect at its `gravity` location, a world-aligned frame at the camera, and keeps
//! its particle systems with the camera (`c_particle_system::set_attached_to_camera` in the Xbox
//! `halo3_tag_play` build). Shadowkeep's environment volumes create a type 10 row's entity at the
//! volume's box center when the camera enters it (`sub_E708F0` in the labelled later client), not
//! at the camera, and no stock vertex program reads the frame constants (`cb13`) a particle's
//! motion would need. So each emitter here is one surface on a shell around the camera whose pixel
//! program finds its particles procedurally: layers of vertical cylinders at radii spread over the
//! emitter's radius, each holding a randomized grid of sprites that falls at the emitter's speed,
//! with the grid sized so a layer holds its share of the emitter's steady particle count. Layers
//! behind the scene depth (`t10`) are hidden, as Reach's particles are by depth.
use super::*;

/// One emitter of a weather effect. Lengths are in meters and speeds in meters per second.
#[derive(Clone, Debug)]
pub(in crate::halo_reach) struct Particles {
    pub sprite: material::Mapping,
    pub additive: bool,
    pub color: [f32; 3],
    pub alpha: f32,
    pub width: f32,
    pub aspect: f32,
    pub count: f32,
    pub radii: [f32; 2],
    pub fall: f32,
}

/// The cylinder layers each field draws.
const LAYERS: usize = 8;
/// The share of a field's grid cells that hold a particle, which breaks the grid's regularity.
const PRESENCE: f32 = 0.7;
/// How far from the camera, in meters, the shell the fields draw on lies.
const SHELL: f32 = 1.5;

/// The vertex program of every weather field: the shell's corners, taken as directions from the
/// object's center, placed [`SHELL`] meters around the camera.
const VERTEX: &str = r#"
cbuffer Object : register(b11) {float4 objectData[8];};
cbuffer View : register(b12) {float4 viewData[14];};
void main(float4 position:POSITION0,float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,float2 detail:TEXCOORD2,
out float4 o0:TEXCOORD0,out float4 o1:TEXCOORD1,out float4 o2:TEXCOORD2,out float4 o3:TEXCOORD3,out float3 o4:TEXCOORD4,out float4 projected:SV_POSITION0) {
float3 local=position.xyz*objectData[4].xyz+objectData[5].xyz;
float3 relative=local*($SHELL$/max(length(local),0.000001));
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
o0=float4(0,0,0,0);o1=float4(0,0,0,0);o2=float4(relative,relative.z+viewData[7].z);o3=float4(0,0,0,0);o4=float3(0,0,0);
}
"#;

/// The pixel program of field `particles`.
fn pixel(particles: &Particles, scale: SkyScale, declarations: &str, sampler: &str) -> String {
    let [r0, r1] = particles.radii;
    let step = (r1 - r0).max(0.01) / LAYERS as f32;
    // A layer at radius r holds count / LAYERS particles over its height 2 * r1 and its
    // circumference, so a cell, with PRESENCE particles, spans
    // sqrt(PRESENCE * 2 pi r * 2 r1 * LAYERS / count).
    let area = PRESENCE * std::f32::consts::TAU * 2. * r1 * LAYERS as f32 / particles.count.max(1.);
    let [cr, cg, cb] = particles.color;
    let (accumulate, alpha) = if particles.additive {
        ("color+=sprite.rgb*tint*a;", "0")
    } else {
        (
            "color+=(1-cover)*sprite.rgb*tint*a;cover+=(1-cover)*a;",
            "cover",
        )
    };
    format!(
        r#"
{declarations}
{scene}
cbuffer Frame : register(b13) {{float4 frameData[2];}};
float weatherHash(float3 p){{p=frac(p*float3(0.1031,0.1030,0.0973));p+=dot(p,p.yxz+33.33);return frac((p.x+p.y)*p.z);}}
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
out float4 o0:SV_TARGET0) {{
float time=frameData[0].x;
float3 ray=normalize(v2.xyz);
float3 forward=normalize(float3(viewData[0].w,viewData[1].w,viewData[2].w));
float sceneDistance=reachSceneDepthAt(position.xy*viewData[12].zw)/max(dot(ray,forward),0.0001);
float horizontal=max(length(ray.xy),0.0001);
float turn=atan2(ray.y,ray.x)/6.2831853;
float3 tint=float3({cr:.6},{cg:.6},{cb:.6});
float3 color=float3(0,0,0);float cover=0;
[loop]for(int k=0;k<{LAYERS};k++){{
float radius={r0:.6}+{step:.6}*(k+0.5);
float t=radius/horizontal;
if(t>sceneDistance){{break;}}
float height=ray.z*t;
float cell=sqrt({area:.6}*radius);
float columns=max(round(6.2831853*radius/cell),1);
float rows=(height+viewData[7].z+{fall:.6}*time)/cell;
float2 place=float2(turn*columns,rows);
float2 index=floor(place);
index.x=fmod(index.x+columns,columns);
float3 seed=float3(index,k);
float present=weatherHash(seed+float3(0,0,17))<{PRESENCE}?1:0;
float2 offset=float2(weatherHash(seed),weatherHash(seed+float3(0,0,7)))*0.6+0.2;
float2 local=(frac(place)-offset)*cell/float2({width:.6},{height_scale:.6});
if(present>0&&all(abs(local)<0.5)){{
float4 sprite=Map0.SampleLevel({sampler},float2(local.x,-local.y)+0.5,0);
float a=sprite.a*{alpha_scale:.6}*saturate((sceneDistance-t)*2)*saturate(1-abs(height)/{r1:.6});
{accumulate}
}}
}}
color=clamp(color,0,{white:.9})*{radiance:.9};
o0=float4(color*{REFERENCE_ILLUMINATION:.9}*frameData[1].z,{alpha});
}}
"#,
        scene = super::super::water::SCENE_DECLARATIONS,
        width = particles.width,
        height_scale = particles.width * particles.aspect,
        alpha_scale = particles.alpha,
        fall = particles.fall,
        white = scale.white,
        radiance = scale.radiance,
    )
}

/// Adds each field of `particles` as a transparent-stage surface keyed by `keys`, copying the
/// stock sky envelope `envelope`, and returns a report of each.
#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn surfaces(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    particles: &[Particles],
    keys: &[u32],
    envelope: u32,
    scale: SkyScale,
) -> Result<Vec<Value>> {
    ensure!(
        keys.len() == particles.len(),
        "Weather keys differ from its fields"
    );
    let envelope = (envelope, rigid_envelope(native, envelope)?);
    let (clamped, sampler_class) = sampler_shell(native)?;
    let vertex = "sky-weather-vertex";
    program(
        native,
        g,
        vertex,
        envelope.1.u32(0x48)?,
        &VERTEX.replace("$SHELL$", &format!("{SHELL:.3}")),
        true,
    )?;
    let mut report = Vec::new();
    for (field, &key) in particles.iter().zip(keys) {
        let symbol = texture(c, pages, native, g, &field.sprite, false, true)?;
        let modes = vec![sampler_modes(&field.sprite)?];
        let samplers = samplers(native, g, clamped, &modes)?;
        let declarations = format!(
            "Texture2D<float4> Map0:register(t0);{}",
            samplers.declarations
        );
        let text = pixel(field, scale, &declarations, &sampler_name(0));
        let shader = format!("sky-weather-pixel-{key:08X}");
        program(native, g, &shader, envelope.1.u32(0x2c8)?, &text, false)?;
        write_material(
            g,
            key,
            (envelope.0, &envelope.1),
            vertex,
            &shader,
            &[(0, symbol)],
            &samplers,
            sampler_class,
            true,
        )?;
        report.push(json!({
            "key": format!("{key:08X}"),
            "sprite": field.sprite.bitmap,
            "additive": field.additive,
            "color": field.color,
            "alpha": field.alpha,
            "width_metres": field.width,
            "aspect": field.aspect,
            "steady_count": field.count,
            "radii_metres": field.radii,
            "fall_metres_per_second": field.fall,
            "layers": LAYERS,
            "stage": 7,
        }));
    }
    Ok(report)
}
