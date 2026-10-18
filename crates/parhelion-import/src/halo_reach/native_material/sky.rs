//! Sky surfaces. A Shadowkeep sky object draws its model in two stages: the transparent stage,
//! where its radiance is scaled by the frame's shading exposure and relative illumination
//! (`cb13[1].z * cb13[1].w`), and the light-shaft stage, where the same radiance unscaled becomes
//! the source the sun's shafts sample. Neither stage is lit. Each sky surface copies a stock sky
//! material's envelope for each stage, whose scope masks (`00002007` and `00006007` in Distant
//! Shore's) bind the rigid model, view and frame constants and whose blend state adds
//! premultiplied color. The programs are written here: one rigid vertex program over layout 139
//! and a pixel program per surface and stage.
//!
//! A Reach sky mesh is lit by its per-vertex color stream (lighting policy 0), and its surfaces
//! use the shader's per-vertex color entry point. The pixel programs below follow that entry
//! point's compiled programs in MCC Reach: the albedo times the vertex color, plus the self
//! illumination, with Reach's detail multiplier `2^2.2`, before exposure. Reach's dynamic simple
//! lights and its atmospheric fog have no counterpart here. Reach's final composite
//! (`rasterizer\shaders\final_composite`) scales the scene by its exposure, raises it to a gamma
//! and saturates each channel after a color matrix, so it shows nothing brighter than white, and
//! each channel of a sky color stops where Reach shows white.
use super::*;

/// The vertex program of every sky surface. It reads the rigid model's axes and position at rows
/// 0 to 3, its position scale and offset at rows 4 and 5 and its texture coordinate transform at
/// row 6, as a stock sky object's program does, and moves the object by its position less the
/// camera's before adding its geometry, which keeps precision kilometers out. The tangent stream
/// carries the square root of the vertex color over the sky's color scale, passed on unchanged.
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
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
float3 n=normalize(float3(dot(r0.xyz,normal.xyz),dot(r1.xyz,normal.xyz),dot(r2.xyz,normal.xyz)));
o0=float4(n,1);
o1=float4(tangent.xyz,p.z);o2=float4(0,0,0,0);
o3=float4(uv*objectData[6].xy+objectData[6].zw,detail);o4=p.xyz;
}
"#;

/// The scope bit of the skinning constants, which a sky object never prepares, and of the rigid
/// model constants, which it does.
const SKINNING_SCOPE: u32 = 1 << 7;
const RIGID_MODEL_SCOPE: u32 = 1 << 2;
/// Reach's detail multiplier, `2^2.2`, and its square, as its compiled programs write them.
const REACH_DETAIL: f32 = 4.594_79;
const REACH_DETAIL_SQUARED: f32 = 21.112_095;
/// The texture-transform animation kinds Reach's sky surfaces use: a uniform scale, and U and V
/// translations. Each runs on the game clock over its period.
const SCALE_UNIFORM: u32 = 2;
const SCALE_X: u32 = 3;
const SCALE_Y: u32 = 4;
const TRANSLATION_X: u32 = 5;
const TRANSLATION_Y: u32 = 6;
/// A Reach function blob's identity type, which ramps from its minimum to its maximum, and the
/// offsets of those two values.
const IDENTITY_FUNCTION: u8 = 0;
const FUNCTION_MINIMUM: usize = 4;
const FUNCTION_MAXIMUM: usize = 8;

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

/// The texture coordinates of `mapping`: its stored transform, with each component a translation
/// or scale animation drives replaced by that animation's value on the game clock. Animations
/// other than linear ramps hold their first value, which `notes` records.
fn coordinates(mapping: &material::Mapping, notes: &mut Vec<String>) -> String {
    let [sx, sy, ox, oy] = mapping.transform;
    let mut parts = [
        format!("{sx:.9}"),
        format!("{sy:.9}"),
        format!("{ox:.9}"),
        format!("{oy:.9}"),
    ];
    for function in &mapping.functions {
        let slots: &[usize] = match function.kind {
            SCALE_UNIFORM => &[0, 1],
            SCALE_X => &[0],
            SCALE_Y => &[1],
            TRANSLATION_X => &[2],
            TRANSLATION_Y => &[3],
            other => {
                notes.push(format!(
                    "{} animation kind {other} is not translated",
                    mapping.role
                ));
                continue;
            }
        };
        let read = |at: usize| {
            function
                .data
                .get(at..at + 4)
                .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap_or([0; 4])))
        };
        let (Some(&kind), Some(minimum), Some(maximum)) = (
            function.data.first(),
            read(FUNCTION_MINIMUM),
            read(FUNCTION_MAXIMUM),
        ) else {
            notes.push(format!("{} animation has no range", mapping.role));
            continue;
        };
        let value =
            if kind == IDENTITY_FUNCTION && function.period > 0. && function.input.is_empty() {
                format!(
                    "lerp({minimum:.9},{maximum:.9},frac(time/{:.9}))",
                    function.period
                )
            } else {
                notes.push(format!(
                    "{} animation type {kind} over input {:?} holds its first value",
                    mapping.role, function.input
                ));
                format!("{minimum:.9}")
            };
        for &slot in slots {
            parts[slot] = value.clone();
        }
    }
    format!(
        "v3.xy*float2({},{})+float2({},{})",
        parts[0], parts[1], parts[2], parts[3]
    )
}

/// The texture roles a surface's albedo and self illumination read, by their Reach options.
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
        Some("two_detail") => roles.extend(["base_map", "detail_map", "detail_map2"]),
        Some("default") | None => roles.extend(["base_map", "detail_map"]),
        Some(other) => {
            notes.push(format!("Albedo {other} draws as the default albedo"));
            roles.extend(["base_map", "detail_map"]);
        }
    }
    match material
        .options
        .get("self_illumination")
        .map(String::as_str)
    {
        Some("off") | None | Some("from_diffuse") => {}
        Some("simple" | "3_channel_self_illum") => roles.push("self_illum_map"),
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

fn rgb(c: [f32; 4]) -> String {
    format!("float3({:.9},{:.9},{:.9})", c[0], c[1], c[2])
}

fn rgba(c: [f32; 4]) -> String {
    format!("float4({:.9},{:.9},{:.9},{:.9})", c[0], c[1], c[2], c[3])
}

/// The albedo and self illumination of a surface, following the per-vertex color entry point of
/// its Reach template, and whether its albedo is lit: self illumination from the diffuse map
/// replaces the albedo's lighting.
fn shading(
    material: &Material,
    constant: &dyn Fn(&str) -> Option<[f32; 4]>,
    samples: &BTreeMap<&str, String>,
    notes: &mut Vec<String>,
) -> (String, String, bool) {
    let sample = |role: &str| {
        samples
            .get(role)
            .cloned()
            .unwrap_or_else(|| "float4(1,1,1,1)".to_owned())
    };
    let tint = constant("albedo_color").unwrap_or([1.; 4]);
    let albedo = match material.options.get("albedo").map(String::as_str) {
        Some("constant_color") => rgba(tint),
        Some("two_detail_overlay") => format!(
            "float4(({base}).rgb*lerp({d1},{d2},({base}).a).rgb*({ov}).rgb*{REACH_DETAIL_SQUARED:.6},({ov}).a*lerp({d1},{d2},({base}).a).a)",
            base = sample("base_map"),
            d1 = sample("detail_map"),
            d2 = sample("detail_map2"),
            ov = sample("detail_map_overlay"),
        ),
        Some("two_detail") => format!(
            "float4(({base}).rgb*({d1}).rgb*({d2}).rgb*{REACH_DETAIL_SQUARED:.6},({base}).a*({d1}).a*({d2}).a)",
            base = sample("base_map"),
            d1 = sample("detail_map"),
            d2 = sample("detail_map2"),
        ),
        _ => format!(
            "(({base})*({detail})*{tint})*float4({REACH_DETAIL:.6},{REACH_DETAIL:.6},{REACH_DETAIL:.6},1)",
            base = sample("base_map"),
            detail = sample("detail_map"),
            tint = rgba(tint),
        ),
    };
    let intensity = constant("self_illum_intensity").map_or(1., |v| v[0]);
    let color = constant("self_illum_color").unwrap_or([1.; 4]);
    let mut lit = true;
    let self_illumination = match material
        .options
        .get("self_illumination")
        .map(String::as_str)
    {
        Some("off") | None => "float3(0,0,0)".to_owned(),
        Some("from_diffuse") => {
            lit = false;
            format!("albedo.rgb*{}*{intensity:.9}", rgb(color))
        }
        Some("3_channel_self_illum") => {
            let map = sample("self_illum_map");
            let [a, b, c] = ["channel_a", "channel_b", "channel_c"]
                .map(|name| constant(name).unwrap_or([0.; 4]));
            format!(
                "(({map}).r*{}*{:.9}+({map}).g*{}*{:.9}+({map}).b*{}*{:.9})*{intensity:.9}",
                rgb(a),
                a[3],
                rgb(b),
                b[3],
                rgb(c),
                c[3]
            )
        }
        Some("illum_detail") => format!(
            "({map}).rgb*({detail}).rgb*{}*{intensity:.9}*{REACH_DETAIL:.6}",
            rgb(color),
            map = sample("self_illum_map"),
            detail = sample("self_illum_detail_map"),
        ),
        Some("plasma") => {
            let [medium, sharp, wide] = ["color_medium", "color_sharp", "color_wide"]
                .map(|name| constant(name).unwrap_or([0.; 4]));
            let [thin_medium, thin_sharp, thin_wide] =
                ["thinness_medium", "thinness_sharp", "thinness_wide"]
                    .map(|name| constant(name).map_or(1., |v| v[0]));
            format!(
                "plasma(({mask}).a,({a}).r,({b}).r,{},{:.9},{},{:.9},{},{:.9},{thin_medium:.9},{thin_sharp:.9},{thin_wide:.9})*{intensity:.9}",
                rgb(medium),
                medium[3],
                rgb(sharp),
                sharp[3],
                rgb(wide),
                wide[3],
                mask = sample("alpha_mask_map"),
                a = sample("noise_map_a"),
                b = sample("noise_map_b"),
            )
        }
        Some(_) => {
            // Recorded by `roles`.
            format!(
                "({}).rgb*{}*{intensity:.9}",
                sample("self_illum_map"),
                rgb(color)
            )
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

/// How a sky surface's color reaches the frame: opaque surfaces replace what is behind them,
/// blended ones cover it by their alpha, additive ones add to it, and multiplying ones darken it
/// by their mean color, the nearest a premultiplied blend comes to a product. The transparent
/// stage scales by the frame's exposure as stock sky surfaces do, and the light-shaft stage does
/// not.
fn output(material: &Material, stage: Stage) -> String {
    let exposure = match stage {
        Stage::Transparent => "*frameData[1].z*frameData[1].w",
        Stage::Shafts => "",
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

/// Adds every sky surface's transparent and light-shaft materials, each copying the envelope of
/// stock material `envelopes[0]` or `envelopes[1]`, and returns a report of each.
pub(in crate::halo_reach) fn surfaces(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    g: &mut Graph,
    surfaces: &[SkySurface<'_>],
    envelopes: [u32; 2],
    scale: SkyScale,
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
        SKY_VERTEX,
        true,
    )?;
    // Surfaces read their maps through wrapping copies of the opaque shell's anisotropic sampler,
    // as level surfaces do.
    let shell = native.tag(0x80ec270d, Some(0x808071e8))?;
    let sampler_rows = shell.array(0x308, 16, None)?;
    let clamped = shell.bytes::<16>(
        *sampler_rows
            .get(CLAMPED_SAMPLER)
            .context("Native sampler table is missing")?,
    )?;
    let sampler_class = shell.u32(sampler_rows[0] - 8)?;
    let mut report = Vec::new();
    for sky in surfaces {
        let material = sky.material;
        let mut notes = Vec::new();
        let constants = material
            .properties
            .first()
            .map(|p| p.constants.clone())
            .unwrap_or_default();
        let constant = |name: &str| constants.get(name).copied();
        // Every role the surface reads takes the next register, and each distinct address mode
        // pair a sampler.
        let mut bindings = Vec::new();
        let mut modes = vec![[WRAP, WRAP]];
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
            let index = sampler_index(&mut modes, address_modes(mapping.address)?);
            samples.insert(
                role,
                format!(
                    "Map{register}.Sample({},{})",
                    sampler_name(index),
                    coordinates(mapping, &mut notes)
                ),
            );
        }
        ensure!(
            bindings.len() <= MAX_TEXTURE_REGISTERS,
            "A sky surface reads {} maps",
            bindings.len()
        );
        let samplers = samplers(native, g, clamped, &modes)?;
        let (albedo, self_illumination, lit) = shading(material, &constant, &samples, &mut notes);
        let lighting = if lit {
            "vertexColor*albedo.rgb"
        } else {
            "float3(0,0,0)"
        };
        let clip = if material.alpha_mode() == "MASK" {
            "clip(albedo.a-0.5);"
        } else {
            ""
        };
        let declarations = (0..bindings.len())
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
void main(float4 v0:TEXCOORD0,float4 v1:TEXCOORD1,float4 v2:TEXCOORD2,float4 v3:TEXCOORD3,float3 v4:TEXCOORD4,float4 position:SV_POSITION0,uint front:SV_isFrontFace0,
out float4 o0:SV_TARGET0) {{
float time=frameData[0].x;
float3 vertexColor=v1.xyz*v1.xyz*{vertex_color:.9};
float4 albedo={albedo};{clip}
float3 selfIllumination={self_illumination};
float3 color=clamp({lighting}+selfIllumination,0,{white:.9})*{radiance:.9};
{finish}
}}
"#,
                vertex_color = scale.vertex_color,
                radiance = scale.radiance,
                white = scale.white,
            );
            let shader = format!("sky-pixel-{key:08X}");
            program(native, g, &shader, envelope.u32(0x2c8)?, &text, false)?;
            let mut bytes = fresh_surface(envelope)?;
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
            let expression = if bindings.is_empty() {
                vec![]
            } else {
                samplers.expression.clone()
            };
            append(&mut bytes, 0x318, 0x80800090, &[], 16)?;
            put(&mut bytes, 0x34c, &u32::MAX.to_le_bytes())?;
            append(&mut bytes, 0x2e8, 0x80800009, &expression, 1)?;
            let mut patches = vec![
                json!({"offset":0x48,"symbol":"sky-vertex"}),
                json!({"offset":0x2c8,"symbol":shader}),
            ];
            for (row, (_, symbol)) in Payload(bytes.clone())
                .array(0x2d0, 8, None)?
                .into_iter()
                .zip(&bindings)
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
                *envelope_tag,
                &bytes,
                None,
                patches,
            )?;
            report.push(json!({
                "key": format!("{key:08X}"),
                "source": material.tag,
                "options": material.options,
                "constants": constants,
                "bound_textures": bindings,
                "stage": if stage == Stage::Transparent { 7 } else { 9 },
                "envelope": format!("{envelope_tag:08X}"),
                "albedo": albedo,
                "self_illumination": self_illumination,
                "lit": lit,
                "output": finish,
                "notes": notes,
            }));
        }
    }
    Ok(report)
}
