//! Sky surfaces. A Shadowkeep sky object draws its model in two stages: the transparent stage,
//! where stock sky programs scale its radiance by the frame's shading exposure and relative
//! illumination (`cb13[1].z * cb13[1].w`), and the light-shaft stage, where the same radiance
//! unscaled becomes the source the sun's shafts sample. Neither stage is lit. Each sky surface
//! copies a stock sky material's envelope for each stage, whose scope masks (`00002007` and
//! `00006007` in Distant Shore's) bind the rigid model, view and frame constants and whose blend
//! state adds premultiplied color. The programs are written here: one rigid vertex program over
//! layout 139 and a pixel program per surface and stage.
//!
//! The transparent stage takes `REFERENCE_ILLUMINATION * cb13[1].z`, the level's own finish, not
//! the stock `z * w`. With `w = x^(-2/3)` the stock factor drew the sky `4w` times as bright as the
//! level beside it, 5.6 times on Breakneck and 2.7 times on Paradiso, and a large bright sky also
//! weighs on automatic exposure.
//!
//! A Reach sky mesh is lit by its per-vertex color stream (lighting policy 0), and its surfaces
//! use the shader's per-vertex color entry point. The pixel programs below follow that entry
//! point's compiled programs in MCC Reach: the albedo times the vertex color, plus the self
//! illumination, with Reach's detail multiplier `2^2.2`, before exposure. Reach's dynamic simple
//! lights have no counterpart here. Reach fogs its sky with the level's atmospheric fog and fog
//! light: the opaque sky through the screen fog pass, which reads a table that stops at 10,000
//! world units, and the blended layers per vertex (`static_per_vertex_color_vs`). Opaque, blended
//! and premultiplied layers take `color * extinction + inscatter`, additive layers only the
//! extinction, and multiplying layers neither, with no exposure. The programs evaluate the same
//! fog per pixel. Reach's final composite (`rasterizer\shaders\final_composite`) scales the scene
//! by its exposure, raises it to a gamma and saturates each channel after a color matrix, so it
//! shows nothing brighter than white, and each channel of a fogged sky color stops where Reach
//! shows white.
use super::*;

pub(in crate::halo_reach) mod lens;
pub(in crate::halo_reach) mod patchy;
pub(in crate::halo_reach) mod weather;

/// The vertex program of every sky surface. It reads the rigid model's axes and position at rows
/// 0 to 3, its position scale and offset at rows 4 and 5 and its texture coordinate transform at
/// row 6, as a stock sky object's program does, and moves the object by its position less the
/// camera's before adding its geometry, which keeps precision kilometers out. The tangent stream
/// carries the square root of the vertex color over the sky's color scale, passed on unchanged.
/// `TEXCOORD2` carries the position less the camera's, in meters, and the world height, which the
/// fog reads. `$PARALLAX$` moves a sky that follows the camera ([`SkyParallax`]).
const SKY_VERTEX: &str = r#"
cbuffer Object : register(b11) {float4 objectData[8];};
cbuffer View : register(b12) {float4 viewData[14];};
void main(float4 position:POSITION0,float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,float2 detail:TEXCOORD2,
out float4 o0:TEXCOORD0,out float4 o1:TEXCOORD1,out float4 o2:TEXCOORD2,out float4 o3:TEXCOORD3,out float3 o4:TEXCOORD4,out float4 projected:SV_POSITION0) {
float4 p=float4(position.xyz*objectData[4].xyz+objectData[5].xyz,1);
float4 r0=float4(objectData[0].x,objectData[1].x,objectData[2].x,objectData[3].x-viewData[7].x);
float4 r1=float4(objectData[0].y,objectData[1].y,objectData[2].y,objectData[3].y-viewData[7].y);
float4 r2=float4(objectData[0].z,objectData[1].z,objectData[2].z,objectData[3].z-viewData[7].z);
float3 relative=float3(dot(r0,p),dot(r1,p),dot(r2,p));
$PARALLAX$
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
float3 n=normalize(float3(dot(r0.xyz,normal.xyz),dot(r1.xyz,normal.xyz),dot(r2.xyz,normal.xyz)));
o0=float4(n,1);
o1=float4(tangent.xyz,p.z);o2=float4(relative,relative.z+viewData[7].z);
o3=float4(uv*objectData[6].xy+objectData[6].zw,detail);o4=p.xyz;
}
"#;

/// How a Reach sky follows the camera. A sky whose model (`hlmt`) sets flag bit 6 of `+0x15C`
/// (`If sky attaches to camera` in the Xbox `halo3_tag_play` build's `model_flags_definition`,
/// whose list also holds `model use sky lighting` at bit 2) is moved every frame by
/// `adjust_sky_object_position` (`825A9910`, through `submit_and_render_sky` pass 0 from
/// `c_player_view::render_single_pass_lighting`) by the camera's offset from its node position
/// times `1 - b / 255`. `get_object_values` (`82C63188`) sets `b` to the model's
/// `sky parallax percent` (`+0x1B4`) times 255, truncated, and the render flag `0x40` the move
/// tests. The tag's own note reads "% between sky pos and camera pos, 0=camera": 0 keeps the sky
/// at the camera, 1 leaves it in place. Reach moves every sky object by the first one's offset.
#[derive(Clone, Copy, Debug)]
pub(in crate::halo_reach) struct SkyParallax {
    /// The Reach sky object's origin, its node position, in the map's meters.
    pub origin: [f32; 3],
    /// The share of the camera's offset from the origin the sky moves by, `1 - b / 255`.
    pub follow: f32,
}

/// The sky vertex program, with `parallax` applied.
fn sky_vertex(parallax: Option<SkyParallax>) -> String {
    let parallax = parallax.map_or_else(String::new, |SkyParallax { origin, follow }| {
        format!(
            "relative+=(viewData[7].xyz-float3({:.6},{:.6},{:.6}))*{follow:.9};",
            origin[0], origin[1], origin[2]
        )
    });
    SKY_VERTEX.replace("$PARALLAX$", &parallax)
}

/// The scope bit of the skinning constants, which a sky object never prepares, and of the rigid
/// model constants, which it does.
const SKINNING_SCOPE: u32 = 1 << 7;
const RIGID_MODEL_SCOPE: u32 = 1 << 2;
/// Reach's detail multiplier, `2^2.2`, and its square, as its compiled programs write them.
const REACH_DETAIL: f32 = 4.594_79;
const REACH_DETAIL_SQUARED: f32 = 21.112_095;
/// The farthest the opaque sky's fog reaches, in meters: Reach's screen fog reads a table that
/// `calculate_screen_fog_LUT_constants` (`82605080` in the Xbox `halo3_tag_play` build) sizes to
/// 10,000 world units, beyond which `LUT_get_x_coord` saturates. Blended layers fog per vertex
/// with no such limit.
const FOG_TABLE_DISTANCE: f32 = 10_000. * 3.048;
/// The level surface sample slots [`super::albedo`] reads each albedo map from.
const ALBEDO_SLOTS: [(&str, usize); 5] = [
    ("base_map", 0),
    ("detail_map", 5),
    ("detail_map2", DETAIL2_SLOT),
    ("detail_map3", DETAIL3_SLOT),
    ("color_mask_map", COLOR_MASK_SLOT),
];
/// One sky surface and the keys of its transparent and light-shaft materials, which take the
/// symbols `material-{key:08X}` a sky model's draws name.
pub(in crate::halo_reach) struct SkySurface<'a> {
    pub material: &'a Material,
    pub transparent: u32,
    pub shafts: u32,
}

/// How the sky's colors reach Destiny's radiance: the scale its vertex colors are stored over,
/// the factor from Reach's radiance before exposure to Destiny's scene radiance, and the Reach
/// radiance Reach's final composite shows as white, where each channel stops. The large bloom's
/// floor, which the level's opaque surfaces add, is small beside the sky's own light and is left
/// out, so layered sky surfaces do not add it more than once.
#[derive(Clone, Copy)]
pub(in crate::halo_reach) struct SkyScale {
    pub vertex_color: f32,
    pub radiance: f32,
    pub white: f32,
}

/// The two stages a sky surface draws in.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Transparent,
    Shafts,
}

/// The texture roles a surface's albedo and self illumination read, by their Reach options. The
/// albedo reads the maps the level surfaces' albedo ([`super::albedo`]) reads for its option, and
/// `two_detail_overlay`, which only sky shaders use, its overlay as well.
fn roles(material: &Material, notes: &mut Vec<String>) -> Vec<&'static str> {
    let mut roles = Vec::new();
    match material.options.get("albedo").map(String::as_str) {
        Some("constant_color") => {}
        Some("two_detail_overlay") => {
            roles.extend([
                "base_map",
                "detail_map",
                "detail_map2",
                "detail_map_overlay",
            ]);
        }
        Some("two_detail" | "two_detail_black_point" | "detail_blend") => {
            roles.extend(["base_map", "detail_map", "detail_map2"]);
        }
        Some("three_detail_blend") => {
            roles.extend(["base_map", "detail_map", "detail_map2", "detail_map3"]);
        }
        Some("color_mask") => roles.extend(["base_map", "detail_map", "color_mask_map"]),
        Some("simple") | None => roles.push("base_map"),
        Some("default") => roles.extend(["base_map", "detail_map"]),
        Some(other) if other.contains("detail") => {
            notes.push(format!("Albedo {other} draws as its base and detail maps"));
            roles.extend(["base_map", "detail_map"]);
        }
        Some(other) => {
            notes.push(format!(
                "Albedo {other} draws as its base map times albedo_color"
            ));
            roles.push("base_map");
        }
    }
    match material
        .options
        .get("self_illumination")
        .map(String::as_str)
    {
        Some("off") | None | Some("from_diffuse" | "from_albedo") => {}
        Some("simple" | "3_channel_self_illum" | "multilayer_additive") => {
            roles.push("self_illum_map");
        }
        Some("illum_detail") => roles.extend(["self_illum_map", "self_illum_detail_map"]),
        Some("plasma") => roles.extend(["alpha_mask_map", "noise_map_a", "noise_map_b"]),
        Some(other) => {
            notes.push(format!(
                "Self illumination {other} draws as simple self illumination"
            ));
            roles.push("self_illum_map");
        }
    }
    roles
}

/// The albedo and self illumination of a surface, following the per-vertex color entry point of
/// its Reach template, and whether its albedo is lit: self illumination from the diffuse map
/// replaces the albedo's lighting. Every albedo but `two_detail_overlay` follows the level
/// surfaces' rules ([`super::albedo`]), which come from the same `albedo.fx` functions, so
/// Nightfall's `detail_blend` clouds blend their two details by the base map's alpha.
fn shading(
    material: &Material,
    surface: &super::surface::Surface<'_>,
    samples: &BTreeMap<&str, String>,
    notes: &mut Vec<String>,
) -> (String, String, bool) {
    let sample = |role: &str| {
        samples
            .get(role)
            .cloned()
            .unwrap_or_else(|| "float4(1,1,1,1)".to_owned())
    };
    let value = |name: &str, default: [f32; 4]| surface.value(name, default);
    let albedo = match material.options.get("albedo").map(String::as_str) {
        Some("two_detail_overlay") => format!(
            "float4(({base}).rgb*lerp({d1},{d2},({base}).a).rgb*({ov}).rgb*{REACH_DETAIL_SQUARED:.6},({ov}).a*lerp({d1},{d2},({base}).a).a)",
            base = sample("base_map"),
            d1 = sample("detail_map"),
            d2 = sample("detail_map2"),
            ov = sample("detail_map_overlay"),
        ),
        _ => {
            let slots = ALBEDO_SLOTS
                .iter()
                .filter_map(|(role, slot)| samples.get(role).map(|s| (*slot, s.clone())))
                .collect::<BTreeMap<_, _>>();
            super::albedo(material, &slots, surface, None)
        }
    };
    let intensity = format!("({}).x", value("self_illum_intensity", [1.; 4]));
    let color = format!("({}).rgb", value("self_illum_color", [1.; 4]));
    let mut lit = true;
    let self_illumination = match material
        .options
        .get("self_illumination")
        .map(String::as_str)
    {
        Some("off") | None => "float3(0,0,0)".to_owned(),
        Some("from_diffuse" | "from_albedo") => {
            lit = false;
            format!("albedo.rgb*{}*{intensity}", color)
        }
        Some("multilayer_additive") => {
            match samples
                .get("self_illum_map")
                .and_then(|sample| super::surface::sample_parts(sample))
            {
                Some((map, sampler, uv)) => {
                    let scalar =
                        |name: &str, default: f32| format!("({}).x", value(name, [default; 4]));
                    format!(
                        "multilayerIllumination({map},{sampler},{uv},({transform}).xy,skyToCamera,skyTangent,skyBitangent,{depth},{aspect},clamp({layers},1,64),{darken},{contrast})*{color}*{intensity}",
                        transform = value("self_illum_map", [1., 1., 0., 0.]),
                        depth = scalar("layer_depth", 1.),
                        aspect = scalar("texcoord_aspect_ratio", 1.),
                        layers = scalar("layers_of_4", 1.),
                        darken = scalar("depth_darken", 1.),
                        contrast = scalar("layer_contrast", 1.),
                    )
                }
                None => "float3(0,0,0)".to_owned(),
            }
        }
        Some("3_channel_self_illum") => {
            let map = sample("self_illum_map");
            let terms = ["channel_a", "channel_b", "channel_c"]
                .into_iter()
                .zip(["r", "g", "b"])
                .map(|(name, lane)| {
                    let channel = value(name, [0.; 4]);
                    format!("({map}).{lane}*({channel}).rgb*({channel}).a")
                })
                .collect::<Vec<_>>()
                .join("+");
            format!("({terms})*{intensity}")
        }
        Some("illum_detail") => format!(
            "({map}).rgb*({detail}).rgb*{}*{intensity}*{REACH_DETAIL:.6}",
            color,
            map = sample("self_illum_map"),
            detail = sample("self_illum_detail_map"),
        ),
        Some("plasma") => {
            let [medium, sharp, wide] =
                ["color_medium", "color_sharp", "color_wide"].map(|name| value(name, [0.; 4]));
            let [thin_medium, thin_sharp, thin_wide] =
                ["thinness_medium", "thinness_sharp", "thinness_wide"]
                    .map(|name| format!("({}).x", value(name, [1.; 4])));
            format!(
                "plasma(({mask}).a,({a}).r,({b}).r,({medium}).rgb,({medium}).a,({sharp}).rgb,({sharp}).a,({wide}).rgb,({wide}).a,{thin_medium},{thin_sharp},{thin_wide})*{intensity}",
                mask = sample("alpha_mask_map"),
                a = sample("noise_map_a"),
                b = sample("noise_map_b")
            )
        }
        Some(_) => {
            // Recorded by `roles`.
            format!("({}).rgb*{}*{intensity}", sample("self_illum_map"), color)
        }
    };
    if material
        .options
        .get("alpha_test")
        .is_some_and(|v| v != "none")
    {
        notes.push("Alpha test clips at half coverage".to_owned());
    }
    (albedo, self_illumination, lit)
}

/// Reach's plasma self illumination: three bands of the difference of two noise samples, each
/// a power of it, colored and weighted, under the alpha mask.
const PLASMA: &str = "float3 plasma(float mask,float a,float b,float3 medium,float mediumWeight,float3 sharp,float sharpWeight,float3 wide,float wideWeight,float thinMedium,float thinSharp,float thinWide){float d=max(1-abs(a-b),0);float m=pow(d,thinMedium);float s=pow(d,thinSharp);float w=pow(d,thinWide);return (medium*mediumWeight*(m-s)+sharp*sharpWeight*s+wide*wideWeight*(w-m))*mask;}";

/// A sky surface's color in Destiny's radiance before exposure, from its lit albedo `lit` and its
/// self illumination, fogged as Reach fogs it by the extinction and inscatter in `fog` and then
/// clamped where Reach's final composite shows white. Opaque, blended and premultiplied surfaces
/// take both, and additive ones only the extinction (`BLEND_FOG_INSCATTER_SCALE` in
/// `shared\blend.fx`). A multiplying surface keeps Reach's unfogged and unexposed product.
fn color(material: &Material, scale: SkyScale) -> String {
    let SkyScale {
        white, radiance, ..
    } = scale;
    match material.options.get("blend_mode").map(String::as_str) {
        Some("additive") => {
            format!("float3 color=clamp((lit+selfIllumination)*fog.a,0,{white:.9})*{radiance:.9};")
        }
        Some("multiply") => "float3 color=max(lit+selfIllumination,0);".to_owned(),
        _ => format!(
            "float3 color=clamp((lit+selfIllumination)*fog.a+fog.rgb,0,{white:.9})*{radiance:.9};"
        ),
    }
}

/// The fog a sky surface reads: the level's fog toward its pixel, the opaque sky's shortened to
/// the screen fog table's reach ([`FOG_TABLE_DISTANCE`]) from the camera with its world height moved
/// along, or none where the level has no fog.
fn fog_line(material: &Material, fog: Option<&Fog>) -> String {
    match fog {
        None => "float4 fog=float4(0,0,0,1);".to_owned(),
        Some(_)
            if material
                .options
                .get("blend_mode")
                .is_some_and(|mode| mode != "opaque") =>
        {
            "float4 fog=reachFog(-v2.xyz,v2.w);".to_owned()
        }
        Some(_) => format!(
            "float3 toCamera=-v2.xyz;float kept=min(1,{FOG_TABLE_DISTANCE:.3}/max(length(toCamera),0.001));float4 fog=reachFog(toCamera*kept,v2.w+toCamera.z*(1-kept));"
        ),
    }
}

/// How a sky surface's color reaches the frame: opaque surfaces replace what is behind them,
/// blended ones cover it by their alpha, additive ones add to it, and multiplying ones darken it
/// by their mean color, the nearest a premultiplied blend comes to a product. The transparent
/// stage scales by the frame's exposure as the level's transparent surfaces do,
/// [`REFERENCE_ILLUMINATION`] times `cb13[1].z`, and the light-shaft stage does not, as the stock
/// light-shaft program does not.
fn output(material: &Material, stage: Stage) -> String {
    let exposure = match stage {
        Stage::Transparent => format!("*{REFERENCE_ILLUMINATION:.9}*frameData[1].z"),
        Stage::Shafts => String::new(),
    };
    let coverage = if material.alpha_mode() == "MASK" {
        "1"
    } else {
        "saturate(albedo.a)"
    };
    match material.options.get("blend_mode").map(String::as_str) {
        Some("additive") => format!("o0=float4(color{exposure},0);"),
        Some("multiply") => "o0=float4(0,0,0,1-saturate(dot(color,float3(1,1,1))/3));".to_owned(),
        Some("alpha_blend") => format!("o0=float4(color*{coverage}{exposure},{coverage});"),
        Some("pre_multiplied_alpha") => format!("o0=float4(color{exposure},{coverage});"),
        _ => format!("o0=float4(color{exposure},1);"),
    }
}

/// Stock sky material `tag`, whose envelope a sky surface copies, checked to bind a rigid model's
/// scopes and no skinning.
fn rigid_envelope(native: &mut Reader, tag: u32) -> Result<std::sync::Arc<Payload>> {
    let envelope = native.tag(tag, Some(0x808071e8))?;
    for mask in [envelope.u32(24)?, envelope.u32(28)?] {
        ensure!(
            mask & SKINNING_SCOPE == 0 && mask & RIGID_MODEL_SCOPE != 0,
            "Sky material {tag:08X} binds scopes {mask:08X}, not a rigid model's"
        );
    }
    Ok(envelope)
}

/// The level surfaces' clamping sampler row and the class of a sampler table, from the native
/// surface shell, so sky maps take the same address and filter translation as level surfaces.
fn sampler_shell(native: &mut Reader) -> Result<([u8; 16], u32)> {
    let shell = native.tag(0x80ec270d, Some(0x808071e8))?;
    let sampler_rows = shell.array(0x308, 16, None)?;
    let clamped = shell.bytes::<16>(
        *sampler_rows
            .get(CLAMPED_SAMPLER)
            .context("Native sampler table is missing")?,
    )?;
    Ok((clamped, shell.u32(sampler_rows[0] - 8)?))
}

/// A material's rasterizer state byte. Its high bit overrides the state the draw inherits, and its
/// low bits select a row of the client's rasterizer table, row 1 drawing both faces. The stock sky
/// materials leave it zero and inherit.
const RASTERIZER_STATE: usize = 0x22;
const BOTH_FACES: u8 = 0x81;

/// Adds sky surface `material-{key}`, a fresh copy of `envelope` naming vertex program `vertex`,
/// pixel program `shader`, the textures of `bindings` at their registers and `samplers`. A
/// surface whose programs place its triangles on screen themselves draws `both_faces`.
#[allow(clippy::too_many_arguments)]
fn write_material(
    g: &mut Graph,
    key: u32,
    (envelope_tag, envelope): (u32, &Payload),
    vertex: &str,
    shader: &str,
    bindings: &[(usize, String)],
    samplers: &Samplers,
    sampler_class: u32,
    both_faces: bool,
) -> Result<()> {
    let mut bytes = fresh_surface(envelope)?;
    if both_faces {
        put(&mut bytes, RASTERIZER_STATE, &[BOTH_FACES])?;
    }
    let mut fixed = Vec::new();
    for (slot, _) in bindings {
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
    let expression = if bindings.is_empty() {
        vec![]
    } else {
        samplers.expression.clone()
    };
    append(&mut bytes, 0x318, 0x80800090, &[], 16)?;
    put(&mut bytes, 0x34c, &u32::MAX.to_le_bytes())?;
    append(&mut bytes, 0x2e8, 0x80800009, &expression, 1)?;
    let mut patches = vec![
        json!({"offset":0x48,"symbol":vertex}),
        json!({"offset":0x2c8,"symbol":shader}),
    ];
    for (row, (_, symbol)) in Payload(bytes.clone())
        .array(0x2d0, 8, None)?
        .into_iter()
        .zip(bindings)
    {
        patches.push(json!({"offset":row+4,"symbol":symbol}));
    }
    for (row, symbol) in Payload(bytes.clone())
        .array(0x308, 16, None)?
        .into_iter()
        .zip(&samplers.symbols)
    {
        patches.push(json!({"offset":row,"symbol":symbol}));
    }
    g.add(
        &format!("material-{key:08X}"),
        envelope_tag,
        &bytes,
        None,
        patches,
    )
}

/// Adds every sky surface's transparent and light-shaft materials, each copying the envelope of
/// stock material `envelopes[0]` or `envelopes[1]` and fogged by the level's `fog`, and returns a
/// report of each. Both stages take the same fogged color, so the shafts' source matches what is
/// seen. The vertex program moves the sky by `parallax` when Reach's does.
#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn surfaces(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    surfaces: &[SkySurface<'_>],
    envelopes: [u32; 2],
    scale: SkyScale,
    fog: Option<&Fog>,
    parallax: Option<SkyParallax>,
) -> Result<Vec<Value>> {
    if surfaces.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(
        scale.vertex_color.is_finite()
            && scale.vertex_color > 0.
            && scale.radiance.is_finite()
            && scale.radiance > 0.
            && scale.white.is_finite()
            && scale.white > 0.,
        "Invalid sky color scale"
    );
    let envelopes = envelopes.map(|tag| {
        native
            .tag(tag, Some(0x808071e8))
            .map(|payload| (tag, payload))
    });
    let [transparent, shafts] = envelopes;
    let envelopes = [transparent?, shafts?];
    for (tag, envelope) in &envelopes {
        for mask in [envelope.u32(24)?, envelope.u32(28)?] {
            ensure!(
                mask & SKINNING_SCOPE == 0 && mask & RIGID_MODEL_SCOPE != 0,
                "Sky material {tag:08X} binds scopes {mask:08X}, not a rigid model's"
            );
        }
    }
    program(
        native,
        g,
        "sky-vertex",
        envelopes[0].1.u32(0x48)?,
        &sky_vertex(parallax),
        true,
    )?;
    // Sky maps use the same address and filter translation as level surfaces.
    let shell = native.tag(0x80ec270d, Some(0x808071e8))?;
    let sampler_rows = shell.array(0x308, 16, None)?;
    let clamped = shell.bytes::<16>(
        *sampler_rows
            .get(CLAMPED_SAMPLER)
            .context("Native sampler table is missing")?,
    )?;
    let sampler_class = shell.u32(sampler_rows[0] - 8)?;
    let fog_functions = fog.map_or_else(String::new, Fog::functions);
    // A sky surface takes no overrides, lens, paint or lightmap, so its constants and animated
    // parameters are its material's own.
    let (objects, overrides, lenses, paint) = (
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    let binding = Binding {
        palette: &[0],
        rigid: true,
        objects: &objects,
        overrides: &overrides,
        lenses: &lenses,
        paint: &paint,
        lightmap: None,
        vertex_data: None,
        statics: false,
        prefix: "",
    };
    let mut report = Vec::new();
    for sky in surfaces {
        let material = sky.material;
        let mut notes = Vec::new();
        let surface = super::surface::Surface::new(material, &binding, sky.transparent);
        let constants = &surface.constants;
        let parameters = &surface.parameters;
        notes.extend(parameters.notes.iter().cloned());
        let parameter_source = &parameters.source;
        // Every role takes the next register, with one sampler per address and filter combination.
        let mut bindings = Vec::new();
        let mut modes = vec![[WRAP, WRAP, ANISOTROPIC]];
        let mut samples = BTreeMap::new();
        for role in roles(material, &mut notes) {
            let Some(mapping) = material.mapping(&[role]) else {
                notes.push(format!("{role} has no bitmap and reads as white"));
                continue;
            };
            let register = bindings.len();
            bindings.push((
                register,
                texture(c, pages, native, g, mapping, false, true)?,
            ));
            let index = sampler_index(&mut modes, sampler_modes(mapping)?);
            samples.insert(
                role,
                format!(
                    "Map{register}.Sample({},{})",
                    sampler_name(index),
                    animation::coordinates(mapping, "v3.xy", &mut notes)
                ),
            );
        }
        let texture_count = bindings.len();
        let curve_functions = if samples
            .values()
            .any(|sample| sample.contains("reachPeriodic("))
            || parameter_source.contains("reachPeriodic(")
        {
            animation::periodic(native, g, &mut bindings)?
        } else {
            String::new()
        };
        ensure!(
            bindings.len() <= MAX_TEXTURE_REGISTERS,
            "A sky surface reads {} maps",
            bindings.len()
        );
        let samplers = samplers(native, g, clamped, &modes)?;
        let (albedo, self_illumination, lit) = shading(material, &surface, &samples, &mut notes);
        let lighting = if lit {
            "vertexColor*albedo.rgb"
        } else {
            "float3(0,0,0)"
        };
        // Multilayer self illumination steps its map along the view in the surface's tangent
        // frame. The sky's tangent stream carries its vertex colors, so the frame comes from the
        // screen derivatives of the position and map coordinates.
        let (multilayer_functions, tangent_frame) = if self_illumination
            .contains("multilayerIllumination(")
        {
            (
                super::surface::MULTILAYER_FUNCTIONS,
                "float3 skyToCamera=-v2.xyz;float3 skyNormal=normalize(v0.xyz);float3 skyAcross=cross(skyNormal,ddx(v2.xyz));float3 skyAlong=cross(ddy(v2.xyz),skyNormal);float2 skyU=ddx(v3.xy);float2 skyV=ddy(v3.xy);float3 skyTangent=skyAlong*skyU.x+skyAcross*skyV.x;float3 skyBitangent=skyAlong*skyU.y+skyAcross*skyV.y;skyTangent*=rsqrt(max(dot(skyTangent,skyTangent),0.000000000001));skyBitangent*=rsqrt(max(dot(skyBitangent,skyBitangent),0.000000000001));",
            )
        } else {
            ("", "")
        };
        let fog_read = fog_line(material, fog);
        let fogged = color(material, scale);
        let clip = if material.alpha_mode() == "MASK" {
            "clip(albedo.a-0.5);"
        } else {
            ""
        };
        let declarations = (0..texture_count)
            .map(|register| format!("Texture2D<float4> Map{register}:register(t{register});"))
            .collect::<String>();
        let sampler_declarations = &samplers.declarations;
        for (stage, key, (envelope_tag, envelope)) in [
            (Stage::Transparent, sky.transparent, &envelopes[0]),
            (Stage::Shafts, sky.shafts, &envelopes[1]),
        ] {
            let finish = output(material, stage);
            let text = format!(
                r#"
{declarations}
{sampler_declarations}
cbuffer Frame : register(b13) {{float4 frameData[2];}};
{PLASMA}
{multilayer_functions}
{fog_functions}
{clock_function}
{curve_functions}
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
out float4 o0:SV_TARGET0) {{
float time=frameData[0].x;
{parameter_source}
float3 vertexColor=v1.xyz*v1.xyz*{vertex_color:.9};
{tangent_frame}
float4 albedo={albedo};{clip}
float3 selfIllumination={self_illumination};
float3 lit={lighting};
{fog_read}
{fogged}
{finish}
}}
"#,
                clock_function = animation::CLOCK,
                vertex_color = scale.vertex_color,
            );
            let shader = format!("sky-pixel-{key:08X}");
            if stage == Stage::Transparent {
                notes.extend(parameters.unused(&text));
            }
            program(native, g, &shader, envelope.u32(0x2c8)?, &text, false)?;
            write_material(
                g,
                key,
                (*envelope_tag, envelope),
                "sky-vertex",
                &shader,
                &bindings,
                &samplers,
                sampler_class,
                false,
            )?;
            report.push(json!({
                "key": format!("{key:08X}"),
                "source": material.tag,
                "options": material.options,
                "constants": constants,
                "source_properties": material.properties,
                "animated_parameters": parameters.bindings(),
                "bound_textures": bindings,
                "stage": if stage == Stage::Transparent { 7 } else { 9 },
                "envelope": format!("{envelope_tag:08X}"),
                "albedo": albedo,
                "self_illumination": self_illumination,
                "lit": lit,
                "fog": fog_read,
                "color": fogged,
                "output": finish,
                "notes": notes,
            }));
        }
    }
    Ok(report)
}
