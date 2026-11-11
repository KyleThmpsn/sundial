//! Eight-row native CPU billboards with explicit source atlas and alpha handling.
//!
//! The plasma and modulated albedos follow the billboard entry point of MCC U13's particle
//! templates (`Jun 21 2023 15:35:31`, `shaders\particle_templates\_9_8_*`, `_8_8_*` and `_6_8_*`,
//! static disassembly of their stored D3D11 bytecode):
//!
//! - `palettized_2d_plasma` samples the red channels of `base_map` at the first frame coordinate
//!   and `base_map2` at the second, each through its own transform, and reads the palette at their
//!   absolute difference and the depth fade, 1 without one. The alpha map's alpha at the billboard
//!   coordinate is the alpha.
//! - `palettized_plasma` reads the palette at `saturate((1 - alpha * particle alpha * depth fade)
//!   * alpha_modulation_factor + difference)` and the particle's palette coordinate.
//! - `diffuse_modulated` multiplies the base map color by `lerp(tint_color, 1, dot(color, color) *
//!   modulation_factor * 0.57735)`.
//!
//! Animated `base_map` and `base_map2` transforms evaluate the material's clock curves as surface
//! materials do.
use super::*;
use crate::halo_reach::native::material::{animation, sky::SceneDepth};
use crate::presentation::append;

/// The pixel texture register of the scene depth, past the map, palette and alpha map.
const DEPTH_REGISTER: usize = 3;
/// The pixel texture register of a plasma material's second base map, past the scene depth.
const SECOND_MAP_REGISTER: u32 = 4;
/// The transforms whose material animation the billboard material evaluates.
const ANIMATED: [&str; 2] = ["base_map", "base_map2"];

/// Whether the albedo blends two base maps through the palette.
fn plasma(albedo: &str) -> bool {
    matches!(albedo, "palettized_plasma" | "palettized_2d_plasma")
}

/// The clock-driven transform animation of `base_map` and `base_map2`, when every material
/// function the compiled routes name animates one of them by a scale or offset the material
/// clock drives.
fn animated(material: &Material) -> Option<animation::Parameters> {
    let property = material.properties.first()?;
    let mut routed = vec![false; property.functions.len()];
    for (name, overlays) in &property.overlays {
        for &index in overlays {
            let function = property.functions.get(index)?;
            if !ANIMATED.contains(&name.as_str())
                || !(2..=6).contains(&function.kind)
                || !function.input.is_empty()
                || !function.range.is_empty()
            {
                return None;
            }
            routed[index] = true;
        }
    }
    // A function no route names, such as a frame selector, has no translated consumer.
    if routed.contains(&false) {
        return None;
    }
    let parameters = animation::Parameters::new(material, &property.constants, &|name| {
        !ANIMATED.contains(&name)
    });
    parameters.notes.is_empty().then_some(parameters)
}

pub(super) fn pending(material: &Material) -> Option<&'static str> {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    let Some(albedo) = option("albedo").filter(|albedo| {
        matches!(
            *albedo,
            "diffuse_only"
                | "diffuse_plus_billboard_alpha"
                | "diffuse_plus_sprite_alpha"
                | "palettized"
                | "palettized_plus_billboard_alpha"
                | "palettized_plus_sprite_alpha"
                | "diffuse_modulated"
                | "palettized_plasma"
                | "palettized_2d_plasma"
        )
    }) else {
        return Some("Another particle albedo");
    };
    // The surveyed plasma materials never blend frames, so the second coordinate their templates
    // read is taken as the converter's second frame coordinate without a blended pair.
    if plasma(albedo) && option("frame_blend") == Some("on") {
        return Some("Frame-blended plasma particle albedo");
    }
    if !matches!(
        option("blend_mode"),
        Some("alpha_blend" | "pre_multiplied_alpha" | "additive" | "add_src_times_srcalpha")
    ) {
        return Some("Another particle composition path");
    }
    if option("specialized_rendering") != Some("none")
        || !matches!(option("warp"), Some("none" | "sphere"))
    {
        return Some("Particle warp or specialized rendering");
    }
    if material.properties.len() != 1
        || (!material.properties[0].functions.is_empty() && animated(material).is_none())
    {
        return Some("Animated particle material parameters");
    }
    if !matches!(option("self_illumination"), Some("none" | "constant_color")) {
        return Some("Another particle self-illumination path");
    }
    None
}

pub(super) fn validate(material: &Material) -> Result<()> {
    ensure!(
        pending(material).is_none(),
        "Source particle material requires another adapter"
    );
    ensure!(
        material.mapping(&["base_map"]).is_some(),
        "Source particle has no base map"
    );
    let albedo = material.options["albedo"].as_str();
    if plasma(albedo) {
        ensure!(
            ["base_map2", "palette", "alpha_map"]
                .iter()
                .all(|role| material.mapping(&[role]).is_some()),
            "Source plasma particle lacks a second base map, palette or alpha map"
        );
    }
    let constants = match albedo {
        "palettized_plasma" => &["alpha_modulation_factor"][..],
        "diffuse_modulated" => &["tint_color", "modulation_factor"][..],
        _ => &[][..],
    };
    for name in constants {
        let value = material
            .constant(name)
            .with_context(|| format!("Source particle {name}"))?;
        ensure!(
            value.iter().all(|v| v.is_finite()),
            "Source particle {name} is invalid"
        );
    }
    sphere_warp(material)?;
    Ok(())
}

/// The sphere warp scale, zero without a warp. MCC particle templates read it from their
/// `sphere_warp_scale` parameter row in the pixel program.
fn sphere_warp(material: &Material) -> Result<f32> {
    if material.options.get("warp").map(String::as_str) != Some("sphere") {
        return Ok(0.);
    }
    let scale = material
        .constant("sphere_warp_scale")
        .context("Particle sphere warp scale")?[0];
    ensure!(scale.is_finite(), "Particle sphere warp scale is invalid");
    Ok(scale)
}

/// The depth fade range in world units, zero without a depth fade. MCC particle templates
/// multiply the map's alpha by `saturate((scene depth - particle depth) / range)` before the
/// black point, with the range from their `depth_fade_range` parameter row.
pub(super) fn depth_fade(material: &Material) -> Result<f32> {
    if material.options.get("depth_fade").map(String::as_str) != Some("on") {
        return Ok(0.);
    }
    let range = material
        .constant("depth_fade_range")
        .context("Particle depth fade range")?[0];
    ensure!(
        range.is_finite() && range > 0.,
        "Particle depth fade range is invalid"
    );
    Ok(range)
}

const VERTEX: &str = r#"
cbuffer Object : register(b11) { float4 objectData[4]; };
cbuffer View : register(b12) { float4 viewData[8]; };
static const float2 corners[4]={float2(-1,1),float2(-1,-1),float2(1,1),float2(1,-1)};
static const float2 coordinates[4]={float2(0,0),float2(0,1),float2(1,0),float2(1,1)};
void main(float4 a0:TEXCOORD0,float4 a1:TEXCOORD1,float4 a2:TEXCOORD2,float4 a3:TEXCOORD3,
float4 a4:TEXCOORD4,float4 a5:TEXCOORD5,float4 a6:TEXCOORD6,float4 a7:TEXCOORD7,uint id:SV_VertexID,
out float4 uv:TEXCOORD0,out float4 color:TEXCOORD1,out float4 property:TEXCOORD2,
out float4 animation:TEXCOORD3,out float4 random:TEXCOORD4,out float4 position:SV_POSITION0) {
float3 center=objectData[0].xyz*a2.x+objectData[1].xyz*a2.y+objectData[2].xyz*a2.z+objectData[3].xyz;
float3 eye=viewData[7].xyz-center;
float3 up=normalize(viewData[4].xyz);
float3 right=cross(eye,up);
float rightLength=dot(right,right);
right=rightLength>0.000001?right*rsqrt(rightLength):float3(1,0,0);
$BASIS$
float rotation=a2.w+$RANDOM_ROTATION$;
float sine,cosine;sincos(rotation,sine,cosine);
float2 corner=(corners[id%4]*0.5-float2($CENTER_X$,$CENTER_Y$))*a3.xy;
float2 rotated=float2(corner.x*cosine-corner.y*sine,corner.x*sine+corner.y*cosine);
float3 world=center+right*rotated.x+up*rotated.y;
position=viewData[0]*world.x+viewData[1]*world.y+viewData[2]*world.z+viewData[3];
float2 sourceUv=coordinates[id%4];
$MIRROR$
uv=float4(sourceUv,length(eye),a1.w);
color=a0;property=a3;animation=a4;random=a7;
if(a1.w>=1.0 || max(abs(a3.x),abs(a3.y))<0.000001) position=float4(10000,10000,-10000,1);
}
"#;

const PIXEL: &str = r#"
Texture2D<float4> Map:register(t0);
Texture2D<float4> Palette:register(t1);
Texture2D<float4> Alpha:register(t2);
$SAMPLERS$
cbuffer Effect:register(b13) { float4 effectData[2]; };
cbuffer Frame:register(b0) { float4 frameData[$FRAME_ROWS$]; };
$SCENE$
$FUNCTIONS$
static const float4 frames[$FRAME_COUNT$]={$FRAMES$};
float4 sampleFrame(float2 sourceUv,float2 billboardUv,float paletteValue,float4 baseTransform) {
float2 uv=sourceUv*baseTransform.xy+baseTransform.zw;
$SAMPLE$
return texel;
}
float4 main(float4 uv:TEXCOORD0,float4 color:TEXCOORD1,float4 property:TEXCOORD2,
float4 animation:TEXCOORD3,float4 random:TEXCOORD4,float4 position:SV_POSITION0):SV_TARGET0 {
$PARAMETERS$
float4 baseTransform=$BASE_TRANSFORM$;
$DEPTH_FADE$
float frame=animation.x+frac(animation.w*animation.z);
frame=$FRAME_POLICY$;
float coordinate=frame*$FRAME_SPAN$;
$BACKWARDS$
coordinate-=floor(coordinate/$FRAME_COUNT$)*$FRAME_COUNT$;
uint first=min((uint)floor(coordinate),$LAST_FRAME$);
uint second=$NEXT_FRAME$;
float weight=$FRAME_BLEND$;
float4 a=frames[first],b=frames[second];
float2 scroll=float2($SCROLL_U$,$SCROLL_V$)*frameData[0].x;
float scaleA=$SCALE_A$,scaleB=$SCALE_B$;
float2 uvA=(uv.xy-0.5)*a.zw*scaleA+0.5*a.zw+a.xy+scroll*abs(a.zw);
float2 uvB=(uv.xy-0.5)*b.zw*scaleB+0.5*b.zw+b.xy+scroll*abs(b.zw);
$WARP$
$TEXEL$
texel.a*=depthFade;
$BLACK_POINT$
float opacity=saturate(texel.a*color.a);
float intensity=max(property.w,0);
$INTENSITY_ALPHA$
float visibility=saturate((uv.z-$NEAR_CUTOFF$)*$NEAR_SCALE$);
opacity*=visibility;
float3 rgb=texel.rgb*color.rgb*intensity+$SELF_ILLUM$;
$OUTPUT$
}
"#;

/// The billboard vertex program. MCC's particle vertex program places each corner at
/// `uv * m_corner.zw + m_corner.xy`, from per-vertex UV corners `custom_vertex_0/1` and the
/// sprite's `m_corner`, then scales X by the aspect and both by the size. Source center offsets
/// stay within plus and minus a half, edge-anchored sprays and drips store exactly a half, so the
/// corners span plus and minus a half around the center and the size is the quad's full width.
fn vertex(emitter: &Emitter) -> String {
    let mut vertex = VERTEX.to_owned();
    let mut mirror = String::new();
    if emitter.appearance & 1 != 0 {
        mirror.push_str("if(a7.y<0.5)sourceUv.x=1-sourceUv.x;\n");
    }
    if emitter.appearance & 2 != 0 {
        mirror.push_str("if(a7.z<0.5)sourceUv.y=1-sourceUv.y;\n");
    }
    // MCC's particle vertex program lays billboard type 2 along the world velocity, with its
    // second axis across the velocity and the view: `normalize(velocity x (position - camera))`.
    // A particle at rest keeps the screen-facing basis.
    let basis = if emitter.orientation == 2 {
        "float3 direction=objectData[0].xyz*a1.x+objectData[1].xyz*a1.y+objectData[2].xyz*a1.z;\
float directionLength=dot(direction,direction);\
if(directionLength>0.000001){right=direction*rsqrt(directionLength);float3 side=cross(eye,right);\
float sideLength=dot(side,side);if(sideLength>0.000001)up=side*rsqrt(sideLength);}"
    } else {
        ""
    };
    for (key, value) in [
        ("$MIRROR$", mirror),
        ("$BASIS$", basis.into()),
        (
            "$RANDOM_ROTATION$",
            format!(
                "{}+{}",
                if emitter.appearance & 4 != 0 {
                    "a7.w*6.28318530718"
                } else {
                    "0.0"
                },
                if emitter.appearance & 0x20 != 0 {
                    "1.57079632679"
                } else {
                    "0.0"
                }
            ),
        ),
        ("$CENTER_X$", format!("{:.9}", emitter.center[0])),
        ("$CENTER_Y$", format!("{:.9}", emitter.center[1])),
    ] {
        vertex = vertex.replace(key, &value);
    }
    vertex
}

/// An albedo's texture sampling, as [`emit`] builds it for [`pixel`].
struct Sampling {
    /// The body of `sampleFrame`, which sets `texel` from the transformed `uv`.
    sample: String,
    /// Sampler declarations and any texture past the map, palette and alpha map.
    declarations: String,
    /// The statement in `main` that sets `texel` from the frame coordinates `uvA` and `uvB`.
    texel: String,
    /// Functions declared before `sampleFrame`.
    functions: String,
    /// Statements at the start of `main`: the clock and the animated transforms.
    parameters: String,
    /// The base map transform, scale then offset.
    base: String,
}

/// The statement that blends the frames' samples, as every albedo but plasma sets `texel`.
const FRAME_TEXEL: &str = "float4 texel=lerp(sampleFrame(uvA,uv.xy,animation.y,baseTransform),sampleFrame(uvB,uv.xy,animation.y,baseTransform),weight);";

fn pixel(emitter: &Emitter, sampling: Sampling, depth: Option<SceneDepth>) -> Result<String> {
    let material = &emitter.material;
    let count = emitter.frames.len();
    let frames = emitter
        .frames
        .iter()
        .map(|v| format!("float4({:.9},{:.9},{:.9},{:.9})", v[0], v[1], v[2], v[3]))
        .collect::<Vec<_>>()
        .join(",");
    let blend = material.options["blend_mode"].as_str();
    let output = match blend {
        "additive" => {
            "return float4(rgb*color.a*visibility*depthFade*effectData[1].z*effectData[1].w,0);"
        }
        "add_src_times_srcalpha" => "return float4(rgb*opacity*effectData[1].z*effectData[1].w,0);",
        "pre_multiplied_alpha" => "return float4(rgb*color.a*visibility*depthFade,opacity);",
        "alpha_blend" => "return float4(rgb*opacity,opacity);",
        _ => anyhow::bail!("Unsupported particle blend mode"),
    };
    let self_illum = material.constant("self_illum_color").unwrap_or([0.; 4]);
    let frame_blend = material
        .options
        .get("frame_blend")
        .is_some_and(|v| v == "on");
    let start_scale = material.constant("starting_uv_scale").map_or(1., |v| v[0]);
    let end_scale = material.constant("ending_uv_scale").map_or(1., |v| v[0]);
    ensure!(
        start_scale.is_finite() && end_scale.is_finite() && start_scale > 0. && end_scale > 0.,
        "Particle animation UV scale is invalid"
    );
    ensure!(
        emitter
            .center
            .iter()
            .chain(&emitter.scroll)
            .all(|v| v.is_finite()),
        "Nonfinite particle coordinates"
    );
    // MCC's warped templates offset both frame coordinates by d * dot(d, d)^2 * scale, with d the
    // billboard coordinate mapped to -1..1, before the base map transform. The alpha map keeps
    // the unwarped billboard coordinate.
    let warp = sphere_warp(material)?;
    // The scene depth and its projection vector, frame row 1, convert both depths to meters. The
    // fade is skipped where the bound depth's size differs from the view's, as converted water
    // does, so a draw without this view's depth keeps the unfaded particle.
    let (scene, fade) = match depth {
        Some(depth) => {
            ensure!(
                emitter.depth_fade.is_finite() && emitter.depth_fade > 0.,
                "Particle depth fade range is invalid"
            );
            (
                format!(
                    "{}cbuffer View:register(b12){{float4 viewData[14];}};{}",
                    depth.texture(),
                    SceneDepth::functions("frameData[1]")
                ),
                format!(
                    "uint depthWidth,depthHeight;SceneDepth.GetDimensions(depthWidth,depthHeight);float depthFade=all(abs(float2(depthWidth,depthHeight)-viewData[12].xy)<1.5)?saturate((reachSceneDepthAt(position.xy*viewData[12].zw)-reachSceneDepth(position.z))*{:.9}):1.0;",
                    1. / emitter.depth_fade
                ),
            )
        }
        None => (String::new(), "float depthFade=1.0;".into()),
    };
    let mut pixel = PIXEL.to_owned();
    for (key,value) in [
        ("$FRAME_ROWS$",if depth.is_some() {"2"}else{"1"}.into()),("$SCENE$",scene),("$DEPTH_FADE$",fade),
        ("$WARP$",if warp != 0. {
            format!("float2 centered=uv.xy*2.0-1.0;float radius=dot(centered,centered);float2 warp=centered*(radius*radius*{warp:.9});uvA+=warp;uvB+=warp;")
        }else{String::new()}),
        ("$FRAME_COUNT$",count.to_string()),("$LAST_FRAME$",(count-1).to_string()),("$FRAMES$",frames),
        ("$FRAME_SPAN$",if emitter.animation&1!=0 {count-1}else{count}.to_string()),
        ("$FRAME_POLICY$","frame".into()),
        ("$NEXT_FRAME$",if emitter.animation&1!=0 {format!("min(first+1,{})",count-1)}else{format!("(first+1)%{count}")}),
        ("$FRAME_BLEND$",if frame_blend {"frac(coordinate)"}else{"0.0"}.into()),
        ("$BACKWARDS$",if emitter.animation&2!=0 {
            format!("if(((uint)(random.z*256.0)&1)!=0)coordinate={}-coordinate;",if emitter.animation&1!=0 {count-1}else{count})
        }else{String::new()}),
        ("$SCROLL_U$",format!("{:.9}",emitter.scroll[0])),("$SCROLL_V$",format!("{:.9}",emitter.scroll[1])),
        ("$SCALE_A$",if frame_blend {format!("rcp(lerp({start_scale:.9},{end_scale:.9},(1.0+weight)*0.5))")}else{"1.0".into()}),
        ("$SCALE_B$",if frame_blend {format!("rcp(lerp({start_scale:.9},{end_scale:.9},weight*0.5))")}else{"1.0".into()}),
        ("$FUNCTIONS$",sampling.functions),("$PARAMETERS$",sampling.parameters),
        ("$BASE_TRANSFORM$",sampling.base),("$TEXEL$",sampling.texel),
        ("$NEAR_SCALE$",format!("{:.9}",emitter.near_fade[0])),("$NEAR_CUTOFF$",format!("{:.9}",emitter.near_fade[1])),
        ("$INTENSITY_ALPHA$",if emitter.appearance&0x40!=0 {"opacity*=intensity;"}else{""}.into()),
        ("$BLACK_POINT$",if material.options.get("black_point").is_some_and(|v|v=="on") {
            "float black=saturate(property.z);float middle=(black+1)*0.5;texel.a=middle*saturate((texel.a-black)/max(middle-black,0.000001))+saturate(texel.a-middle);"
        }else{""}.into()),
        ("$SELF_ILLUM$",format!("float3({:.9},{:.9},{:.9})",self_illum[0],self_illum[1],self_illum[2])),
        ("$SAMPLE$",sampling.sample),("$SAMPLERS$",sampling.declarations),("$OUTPUT$",output.into()),
    ] {pixel=pixel.replace(key,&value);}
    Ok(pixel)
}

pub(super) fn emit(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    emitter: &Emitter,
    prefix: &str,
) -> Result<String> {
    let material = &emitter.material;
    let albedo = material.options["albedo"].as_str();
    let map = material
        .mapping(&["base_map"])
        .context("Particle base map")?;
    // MCC's particle templates sample each map through its own sampler, `s0` to `s2`, with the
    // material's address mode, so a palette lookup clamps at its ends instead of wrapping between
    // the darkest and brightest entries.
    let template = sampler_template(native)?;
    let mut samplers = vec![(
        "SurfaceSampler",
        native::material::texture_sampler(native, graph, template, map)?,
    )];
    let mut textures = vec![(
        0u32,
        native::material::effect_texture(
            cache,
            pages,
            native,
            graph,
            map,
            !albedo.starts_with("palettized"),
        )?,
    )];
    let parameters = if material.properties[0].functions.is_empty() {
        animation::Parameters::default()
    } else {
        animated(material).context("Particle material animation")?
    };
    let transform = |mapping: &crate::halo_reach::material::Mapping| {
        parameters.value(&mapping.role, mapping.transform)
    };
    let plasma = plasma(albedo);
    let mut sample: String = if plasma {
        // Plasma samples its maps in `main`, at both frame coordinates.
        "float4 texel=0;".into()
    } else if albedo.starts_with("palettized") {
        let palette = material.mapping(&["palette"]).context("Particle palette")?;
        textures.push((
            1,
            native::material::texture(cache, pages, native, graph, palette, false, true)?,
        ));
        samplers.push((
            "PaletteSampler",
            native::material::texture_sampler(native, graph, template, palette)?,
        ));
        "float index=Map.Sample(SurfaceSampler,uv).r;\nfloat4 texel=Palette.Sample(PaletteSampler,float2(index,paletteValue));".into()
    } else {
        "float4 texel=Map.Sample(SurfaceSampler,uv);".into()
    };
    if albedo == "diffuse_modulated" {
        let tint = material.constant("tint_color").context("Particle tint")?;
        let factor = material
            .constant("modulation_factor")
            .context("Particle modulation factor")?[0];
        sample.push_str(&format!(
            "\ntexel.rgb*=lerp(float3({:e},{:e},{:e}),1.0,dot(texel.rgb,texel.rgb)*({factor:e}*0.577350));",
            tint[0], tint[1], tint[2]
        ));
    }
    let mut alpha_coordinates = None;
    if albedo.contains("alpha") || plasma {
        let alpha = material
            .mapping(&["alpha_map"])
            .context("Particle alpha map")?;
        textures.push((
            2,
            native::material::effect_texture(cache, pages, native, graph, alpha, false)?,
        ));
        samplers.push((
            "AlphaSampler",
            native::material::texture_sampler(native, graph, template, alpha)?,
        ));
        let [u, v, x, y] = alpha.transform;
        let coordinates = if albedo.contains("billboard") || plasma {
            "billboardUv"
        } else {
            "sourceUv"
        };
        let coordinates = format!("{coordinates}*float2({u:.9},{v:.9})+float2({x:.9},{y:.9})");
        if !plasma {
            sample.push_str(&format!(
                "\ntexel.a=Alpha.Sample(AlphaSampler,{coordinates}).a;"
            ));
        }
        alpha_coordinates = Some(coordinates);
    }
    let mut extra = String::new();
    let texel = if plasma {
        let palette = material.mapping(&["palette"]).context("Particle palette")?;
        let second = material
            .mapping(&["base_map2"])
            .context("Particle second base map")?;
        textures.push((
            1,
            native::material::texture(cache, pages, native, graph, palette, false, true)?,
        ));
        samplers.push((
            "PaletteSampler",
            native::material::texture_sampler(native, graph, template, palette)?,
        ));
        textures.push((
            SECOND_MAP_REGISTER,
            native::material::effect_texture(cache, pages, native, graph, second, false)?,
        ));
        samplers.push((
            "SecondSampler",
            native::material::texture_sampler(native, graph, template, second)?,
        ));
        extra.push_str(&format!(
            "Texture2D<float4> SecondMap:register(t{SECOND_MAP_REGISTER});\n"
        ));
        // `billboardUv` names the billboard coordinate in `sampleFrame`, `uv.xy` in `main`.
        let alpha = alpha_coordinates
            .context("Particle alpha map")?
            .replace("billboardUv", "uv.xy");
        let index = if albedo == "palettized_2d_plasma" {
            "float2(difference,depthFade)".to_owned()
        } else {
            let factor = material
                .constant("alpha_modulation_factor")
                .context("Particle alpha modulation factor")?[0];
            format!(
                "float2(saturate((1.0-plasmaAlpha*color.a*depthFade)*{factor:e}+difference),animation.y)"
            )
        };
        format!(
            "float4 secondTransform={};\n\
float difference=abs(Map.Sample(SurfaceSampler,uvA*baseTransform.xy+baseTransform.zw).r-SecondMap.Sample(SecondSampler,uvB*secondTransform.xy+secondTransform.zw).r);\n\
float plasmaAlpha=Alpha.Sample(AlphaSampler,{alpha}).a;\n\
float4 texel=float4(Palette.Sample(PaletteSampler,{index}).rgb,plasmaAlpha);",
            transform(second)
        )
    } else {
        FRAME_TEXEL.into()
    };
    // A curve that reads a periodic table samples it past every other texture and the depth.
    let mut functions = String::new();
    if parameters.source.contains("reachPeriodic(") {
        let mut bindings = textures
            .iter()
            .map(|(slot, symbol)| (*slot as usize, symbol.clone()))
            .chain([(DEPTH_REGISTER, String::new())])
            .collect::<Vec<_>>();
        functions.push_str(&animation::periodic(native, graph, &mut bindings)?);
        let (slot, symbol) = bindings.pop().context("Particle curve table binding")?;
        textures.push((u32::try_from(slot)?, symbol));
    }
    let clock = parameters.source.contains("reachPhase(");
    if clock {
        functions.push_str(animation::CLOCK);
    }
    let declarations = samplers
        .iter()
        .enumerate()
        .map(|(index, (name, _))| format!("SamplerState {name}:register(s{});\n", index + 1))
        .collect::<String>()
        + &extra;
    let vertex = vertex(emitter);
    let depth = (emitter.depth_fade > 0.)
        .then(|| SceneDepth::new(native, DEPTH_REGISTER))
        .transpose()?;
    let sampling = Sampling {
        sample,
        declarations,
        texel,
        functions,
        parameters: format!(
            "{}{}",
            if clock {
                "float time=frameData[0].x;\n"
            } else {
                ""
            },
            parameters.source
        ),
        base: transform(map),
    };
    let pixel = pixel(emitter, sampling, depth)?;
    let samplers = samplers
        .into_iter()
        .map(|(_, symbol)| symbol)
        .collect::<Vec<_>>();
    shell(
        native,
        graph,
        prefix,
        (&vertex, &pixel),
        (textures, &samplers),
        depth,
    )
}

/// The native CPU billboard shell, which every converted particle material copies.
const SHELL: u32 = 0x80fc7e03;

/// The native sampler the shell binds first, the template for source-addressed samplers.
pub(super) fn sampler_template(native: &mut Reader) -> Result<u32> {
    let shell = native.tag(SHELL, Some(0x808071e8))?;
    let rows = shell.array(0x308, 16, None)?;
    shell.u32(
        *rows
            .first()
            .context("Native particle sampler table is absent")?,
    )
}

/// Adds billboard material `{prefix}-material`, a copy of the native CPU billboard shell drawing
/// both faces with programs `vertex` and `pixel`, `textures` at their pixel registers, `samplers`
/// at pixel sampler registers from one, or the shell's own sampler there without any, and, with
/// `depth`, the scene depth.
pub(super) fn shell(
    native: &mut Reader,
    graph: &mut Graph,
    prefix: &str,
    (vertex, pixel): (&str, &str),
    (textures, samplers): (Vec<(u32, String)>, &[String]),
    depth: Option<SceneDepth>,
) -> Result<String> {
    let template = SHELL;
    let shell = native.tag(template, Some(0x808071e8))?;
    ensure!(
        shell.u8(0x20)? == 0x88,
        "Particle material composition contract differs"
    );
    let vertex_name = format!("{prefix}-vertex");
    let pixel_name = format!("{prefix}-pixel");
    native::material::program(native, graph, &vertex_name, shell.u32(0x48)?, vertex, true)?;
    native::material::program(native, graph, &pixel_name, shell.u32(0x2c8)?, pixel, false)?;
    let mut bytes = shell.0.clone();
    bytes[0x22] = crate::halo_reach::effect::BOTH_FACES;
    for stage in [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368] {
        bytes[stage..stage + 0xa0].fill(0);
        for at in [stage, stage + 0x80, stage + 0x84] {
            put(&mut bytes, at, &u32::MAX.to_le_bytes())?;
        }
    }
    let rows = shell.array(0x308, 16, None)?;
    ensure!(!rows.is_empty(), "Native particle sampler table is absent");
    let class = u64::from(shell.u32(rows[0] - 8)?);
    // The pixel stage binds sampler table row `i` to register `s(i + 1)` before publishing time.
    let mut expression = Vec::new();
    if samplers.is_empty() {
        append(&mut bytes, 0x308, class, &shell.bytes::<16>(rows[0])?, 16)?;
        expression.extend([0x4c, 0, 0x49, 0x21]);
    } else {
        let mut table = Vec::new();
        for index in 0..samplers.len() {
            let index = u8::try_from(index)?;
            expression.extend([0x4c, index, 0x49, 0x21 + index]);
            table.extend(u32::MAX.to_le_bytes());
            table.extend(1u32.to_le_bytes());
            table.extend(0u64.to_le_bytes());
        }
        append(&mut bytes, 0x308, class, &table, 16)?;
    }
    expression.extend([0x3c, 1, 1, 0x43, 0]);
    if let Some(depth) = depth {
        expression.extend(depth.binding(1));
    }
    append(&mut bytes, 0x2e8, 0x80800009, &expression, 1)?;
    let rows = if depth.is_some() { 2 } else { 1 };
    append(&mut bytes, 0x318, 0x80800090, &vec![0; 16 * rows], 16)?;
    bytes[0x33c] = 0x10;
    put(&mut bytes, 0x348, &0u32.to_le_bytes())?;
    let bindings = textures
        .iter()
        .flat_map(|(slot, _)| [slot.to_le_bytes(), u32::MAX.to_le_bytes()].concat())
        .collect::<Vec<_>>();
    append(&mut bytes, 0x2d0, 0x80807211, &bindings, 8)?;
    let mut patches = vec![
        json!({"offset":0x48,"symbol":vertex_name}),
        json!({"offset":0x2c8,"symbol":pixel_name}),
    ];
    for (row, (_, texture)) in Payload(bytes.clone())
        .array(0x2d0, 8, Some(0x80807211))?
        .into_iter()
        .zip(textures)
    {
        patches.push(json!({"offset":row+4,"symbol":texture}));
    }
    for (row, sampler) in Payload(bytes.clone())
        .array(0x308, 16, None)?
        .into_iter()
        .zip(samplers)
    {
        patches.push(json!({"offset":row,"symbol":sampler}));
    }
    let symbol = format!("{prefix}-material");
    graph.add(&symbol, template, &bytes, None, patches)?;
    Ok(symbol)
}
