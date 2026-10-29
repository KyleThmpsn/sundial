//! A Reach sky's lens flare, drawn as one sky object at the flare's marker. Reach draws a flare's
//! reflections as screen-facing quads after its transparents (`lens_flares_render`, `82560C48` in
//! the Xbox `halo3_tag_play` build), each under an occlusion query of the flare point. The
//! translation draws each reflection in the sky's transparent stage with programs of its own: the
//! vertex program projects the flare point, moves the reflection along the screen axis and sizes
//! it, and the pixel program tests the scene depth around the flare point in place of the
//! occlusion query.
//!
//! In that build, `lens_flare_evaluate_parameters` (`8255FBF0`) takes a reflection's radius as its
//! radius function times the flare's scale (the attaching object's node scale, 1 for a sky), in
//! world units at the flare's depth, and multiplies it by that depth when reflection flag bit 1
//! (`radius not scaled by distance`) is set, so the reflection keeps one screen size. It moves the
//! reflection's center by `axis * 2 * (axis point - flare)`, the axis point lying on the view axis
//! at the flare's depth, so in screen space by `-2 * axis` times the flare's screen position. Its
//! rotation is the rotation offset in degrees, plus the flare's screen angle with flag bit 0.
//! `lens_flare_project_billboard` (`8255ED10`) scales the radius by the projection over the depth.
//! `lens_flare_render_internal` (`825603A8`) draws a reflection only when its bitmap index is below
//! the bitmap's image count. The published `effects\lens_flare.hlsl` shades
//! `((1 - modulation) * pow(texture.g, tint_power) + texture * color) * brightness * exposure`, with
//! `rasterizer_draw_screen_facing_lens_flare` (`82584328`) supplying one less the modulation factor,
//! the tint power and the brightness, a register assignment inferred from the values, and blending
//! additively.
use super::*;

/// One reflection of a flare: its flags, its sprite (a frame of the flare's bitmap), rotation in
/// radians, axis offset, radius (world units at the flare's depth, or a screen-relative size with
/// flag bit 1), brightness, modulation factor, color and tint power.
#[derive(Clone, Debug)]
pub(in crate::halo_reach) struct Reflection {
    pub flags: u8,
    pub sprite: material::Mapping,
    pub rotation: f32,
    pub axis: f32,
    pub radius: f32,
    pub brightness: f32,
    pub modulation: f32,
    pub color: [f32; 3],
    pub tint_power: f32,
}

/// A flare: its reflections, the share of a corona's radius the occlusion test samples within, and
/// whether it is tested at all (lens flag bit 1, `no occlusion test`, clears it).
#[derive(Clone, Debug)]
pub(in crate::halo_reach) struct Lens {
    pub reflections: Vec<Reflection>,
    pub occlusion_scale: f32,
    pub occluded: bool,
}

/// Reflection flags: rotation aligned with the screen center, a screen-constant radius.
const ALIGN_TO_CENTER: u8 = 1;
const SCREEN_RADIUS: u8 = 2;
/// How far ahead of the camera, in meters, the reflections are drawn, in front of everything but
/// the first-person view, which Reach also draws after its flares.
const NEAR: f32 = 0.5;
/// The depth tests a flare's occlusion samples: its center and four points around it.
const OCCLUSION_TAPS: usize = 5;

/// The vertex program of reflection `reflection`, with the sky's `parallax` applied to the flare
/// point, which is the sky object's position.
fn vertex(reflection: &Reflection, parallax: Option<SkyParallax>) -> String {
    let screen = reflection.flags & SCREEN_RADIUS != 0;
    // A world radius is in Reach world units at the flare's depth, a screen radius unitless.
    let (radius, per_depth) = if screen {
        (reflection.radius, "")
    } else {
        (reflection.radius * 3.048, "/depth")
    };
    let align = if reflection.flags & ALIGN_TO_CENTER != 0 {
        "+atan2(center.y,center.x)"
    } else {
        ""
    };
    let parallax = parallax.map_or_else(String::new, |SkyParallax { origin, follow }| {
        format!(
            "relative+=(viewData[7].xyz-float3({:.6},{:.6},{:.6}))*{follow:.9};",
            origin[0], origin[1], origin[2]
        )
    });
    format!(
        r#"
cbuffer Object : register(b11) {{float4 objectData[8];}};
cbuffer View : register(b12) {{float4 viewData[14];}};
void main(float4 position:POSITION0,float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,float2 detail:TEXCOORD2,
out float4 o0:TEXCOORD0,out float4 o1:TEXCOORD1,out float4 o2:TEXCOORD2,out float4 o3:TEXCOORD3,out float3 o4:TEXCOORD4,out float4 projected:SV_POSITION0) {{
float3 relative=objectData[3].xyz-viewData[7].xyz;
{parallax}
float4 clip=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
float depth=max(clip.w,0.000001);
float2 center=clip.xy/depth;
float2 scale=float2(length(float3(viewData[0].x,viewData[1].x,viewData[2].x)),length(float3(viewData[0].y,viewData[1].y,viewData[2].y)));
float2 size={radius:.9}*scale{per_depth};
float angle={rotation:.9}{align};
float s=sin(angle);float c=cos(angle);
float2 texcoord=uv*objectData[6].xy+objectData[6].zw;
float2 corner=texcoord*2-1;
float2 turned=float2(corner.x*s+corner.y*c,corner.x*c-corner.y*s);
float3 forward=float3(viewData[0].w,viewData[1].w,viewData[2].w);
float3 ahead=forward*({NEAR:.3}/max(dot(forward,forward),0.000001));
float4 aheadClip=viewData[0]*ahead.x+viewData[1]*ahead.y+viewData[2]*ahead.z+viewData[13];
float aheadDepth=aheadClip.z/max(aheadClip.w,0.000001);
projected=clip.w>0?float4(center*(1-2*{axis:.9})+turned*size,aheadDepth,1):float4(0,0,-1,1);
o0=float4(center.x*0.5+0.5,0.5-center.y*0.5,depth,size.x*0.5*viewData[12].x);
o1=float4(0,0,0,0);o2=float4(0,0,0,0);o3=float4(texcoord,0,0);o4=float3(0,0,0);
}}
"#,
        rotation = reflection.rotation,
        axis = reflection.axis,
    )
}

/// The pixel program of reflection `reflection`, sampling its sprite with `sample` and declaring
/// its map and samplers with `declarations`.
fn pixel(
    lens: &Lens,
    reflection: &Reflection,
    scale: SkyScale,
    declarations: &str,
    sample: &str,
) -> String {
    let occlusion = if lens.occluded {
        format!(
            "float2 size=viewData[12].xy;float r=max(v0.w*{:.6},2);float seen=0;[unroll]for(int i=0;i<{OCCLUSION_TAPS};i++){{float2 offset=i==0?float2(0,0):(i==1?float2(r,0):(i==2?float2(-r,0):(i==3?float2(0,r):float2(0,-r))));float2 p=v0.xy*size+offset;if(all(p>=0)&&all(p<size)){{seen+=reachSceneDepth(SceneDepth.Load(int3(int2(p),0)).x)>=v0.z*0.98?1:0;}}}}float visibility=seen/{OCCLUSION_TAPS};",
            lens.occlusion_scale
        )
    } else {
        "float visibility=1;".to_owned()
    };
    let [r, g, b] = reflection.color;
    format!(
        r#"
{declarations}
{scene}
cbuffer Frame : register(b13) {{float4 frameData[2];}};
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
out float4 o0:SV_TARGET0) {{
{occlusion}
float4 sprite={sample};
float3 flare={tinted:.9}*pow(max(sprite.g,0.00001),{power:.9})+sprite.rgb*float3({r:.9},{g:.9},{b:.9});
float3 color=clamp(flare*{brightness:.9}*visibility,0,{white:.9})*{radiance:.9};
o0=float4(color*{REFERENCE_ILLUMINATION:.9}*frameData[1].z,0);
}}
"#,
        scene = super::super::water::SCENE_DECLARATIONS,
        tinted = 1. - reflection.modulation,
        power = reflection.tint_power,
        brightness = reflection.brightness,
        white = scale.white,
        radiance = scale.radiance,
    )
}

/// Adds each reflection of `lens` as a transparent-stage surface keyed by `keys`, copying the
/// stock sky envelope `envelope`, and returns a report of each.
#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn surfaces(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    lens: &Lens,
    keys: &[u32],
    envelope: u32,
    scale: SkyScale,
    parallax: Option<SkyParallax>,
) -> Result<Vec<Value>> {
    ensure!(
        keys.len() == lens.reflections.len(),
        "Lens flare keys differ from its reflections"
    );
    let envelope = (envelope, rigid_envelope(native, envelope)?);
    let (clamped, sampler_class) = sampler_shell(native)?;
    let mut report = Vec::new();
    for (reflection, &key) in lens.reflections.iter().zip(keys) {
        let symbol = texture(c, pages, native, g, &reflection.sprite, false, true)?;
        let modes = vec![sampler_modes(&reflection.sprite)?];
        let samplers = samplers(native, g, clamped, &modes)?;
        let declarations = format!(
            "Texture2D<float4> Map0:register(t0);{}",
            samplers.declarations
        );
        let sample = format!("Map0.Sample({},v3.xy)", sampler_name(0));
        let vertex_symbol = format!("sky-flare-vertex-{key:08X}");
        let pixel_symbol = format!("sky-flare-pixel-{key:08X}");
        program(
            native,
            g,
            &vertex_symbol,
            envelope.1.u32(0x48)?,
            &vertex(reflection, parallax),
            true,
        )?;
        let text = pixel(lens, reflection, scale, &declarations, &sample);
        program(
            native,
            g,
            &pixel_symbol,
            envelope.1.u32(0x2c8)?,
            &text,
            false,
        )?;
        let bindings = vec![(0, symbol)];
        write_material(
            g,
            key,
            (envelope.0, &envelope.1),
            &vertex_symbol,
            &pixel_symbol,
            &bindings,
            &samplers,
            sampler_class,
            true,
        )?;
        report.push(json!({
            "key": format!("{key:08X}"),
            "sprite": reflection.sprite.bitmap,
            "frame": reflection.sprite.frame,
            "flags": reflection.flags,
            "axis": reflection.axis,
            "radius": reflection.radius,
            "screen_radius": reflection.flags & SCREEN_RADIUS != 0,
            "brightness": reflection.brightness,
            "modulation": reflection.modulation,
            "color": reflection.color,
            "tint_power": reflection.tint_power,
            "rotation_radians": reflection.rotation,
            "occlusion_tested": lens.occluded,
            "stage": 7,
        }));
    }
    Ok(report)
}
