//! Source contrail profiles on ordinary CPU samples with camera-facing presentation.
mod simulation;
use super::property::Property;
use crate::{
    halo_reach::{
        cache::{Cache, Tag},
        material::{self, Material},
        native,
        resource::Pages,
    },
    presentation::{Graph, append, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize)]
pub(in crate::halo_reach) struct Profile {
    source: Tag,
    name: String,
    emission: Property,
    life: f32,
    width: Property,
    pub(in crate::halo_reach) color: Property,
    alpha: Property,
    /// Multiplies `alpha`. Its contrail age input, which the native samples do not carry, takes
    /// its resting value.
    secondary_alpha: Property,
    intensity: Property,
    black_point: Property,
    palette: Property,
    tiling: [f32; 2],
    scroll: [f32; 2],
    appearance_flags: u16,
    origin_fade: [f32; 2],
    material: Material,
}

impl Profile {
    /// Whether the profile adds its light to the scene, as energy contrails do, rather than
    /// blending over it as smoke does.
    pub(in crate::halo_reach) fn additive(&self) -> bool {
        self.material.options.get("blend_mode").map(String::as_str)
            == Some("add_src_times_srcalpha")
    }

    /// The mean linear color a palettized profile's palette multiplies its color by, or white for a
    /// diffuse-only profile. The palette row sits at the profile's palette coordinate and the base
    /// map's red channel indexes it, so the mean runs over the base map's texels, each weighted by
    /// the alpha it draws with: the alpha map's on `palettized_plus_alpha`, the palette's own
    /// otherwise. The black point and the alpha curve are left out.
    pub(in crate::halo_reach) fn palette_tint(
        &self,
        cache: &Cache,
        pages: &mut Pages,
    ) -> Result<[f32; 3]> {
        let albedo = self.material.options.get("albedo").map(String::as_str);
        if albedo == Some("diffuse_only") {
            return Ok([1.; 3]);
        }
        let mapping = |role: &str| {
            self.material
                .mapping(&[role])
                .with_context(|| format!("Contrail {role}"))
        };
        let image = |pages: &mut Pages, mapping: &material::Mapping| {
            let tag = mapping.bitmap.as_ref().context("Contrail bitmap")?;
            material::image(cache, pages, tag, mapping.frame, false)
        };
        let base = image(pages, mapping("base_map")?)?;
        let palette_map = mapping("palette")?;
        let palette = image(pages, palette_map)?;
        let alpha = if albedo == Some("palettized_plus_alpha") {
            let alpha = image(pages, mapping("alpha_map")?)?;
            ensure!(
                (alpha.width, alpha.height) == (base.width, base.height),
                "Contrail alpha map differs from its base map in size"
            );
            Some(alpha)
        } else {
            None
        };
        let row = self
            .palette
            .constant()
            .context("Contrail palette coordinate")?;
        let [u_scale, v_scale, u_offset, v_offset] = palette_map.transform;
        let gamma = palette.curve != material::LINEAR_CURVE;
        let y = ((row * v_scale + v_offset).clamp(0., 1.) * palette.height as f32) as usize;
        let y = y.min(palette.height - 1);
        let texel = |index: f32| -> [f64; 4] {
            let u = (index * u_scale + u_offset).clamp(0., 1.);
            let x = ((u * palette.width as f32) as usize).min(palette.width - 1);
            let at = (y * palette.width + x) * 4;
            std::array::from_fn(|k| {
                let value = f64::from(palette.rgba[at + k]) / 255.;
                if k < 3 && gamma {
                    value.powf(2.2)
                } else {
                    value
                }
            })
        };
        let mut sum = [0f64; 3];
        let mut weight = 0f64;
        for (k, texel_at) in base.rgba.chunks_exact(4).enumerate() {
            let color = texel(f32::from(texel_at[0]) / 255.);
            let w = alpha
                .as_ref()
                .map_or(color[3], |alpha| f64::from(alpha.rgba[k * 4 + 3]) / 255.);
            for (total, value) in sum.iter_mut().zip(color) {
                *total += value * w;
            }
            weight += w;
        }
        ensure!(weight > 0., "Contrail palette draws nothing");
        Ok(sum.map(|total| (total / weight) as f32))
    }
}

pub(in crate::halo_reach) fn read(cache: &Cache, tag: &Tag) -> Result<Vec<Profile>> {
    each(cache, tag)?
        .into_iter()
        .map(|(_, profile)| profile)
        .collect()
}

/// Every profile of source contrail `tag` with its name, each read on its own, so a caller can
/// draw the profiles that translate and report why the others do not.
pub(in crate::halo_reach) fn each(
    cache: &Cache,
    tag: &Tag,
) -> Result<Vec<(String, Result<Profile>)>> {
    ensure!(tag.group == "cntl", "Expected a source contrail system");
    cache
        .block(tag.address()?, 0x280)?
        .into_iter()
        .map(|at| Ok((cache.string_id(cache.u32(at)?)?, profile(cache, tag, at))))
        .collect()
}

/// Source contrail inputs a profile property may read: profile age (0), the sample's own age, and
/// contrail age (7), the time since the contrail began.
const PROFILE_AGE: u8 = 0;
const CONTRAIL_AGE: u8 = 7;

/// The secondary alpha's expression. Contrail age, which no native sample carries, takes one, the
/// end of a mapping's unit domain, as a resting value. A curve that settles before it, such as the
/// Grenade Launcher `hot_glow` fade-in that holds one from 0.0425, keeps its settled value, and an
/// earlier source fade is not drawn.
fn secondary_alpha(profile: &Profile) -> Result<String> {
    profile
        .secondary_alpha
        .expression_with(&|index| match index {
            PROFILE_AGE => Ok("age".into()),
            CONTRAIL_AGE => Ok("1.0".into()),
            other => anyhow::bail!("Source contrail secondary alpha reads source input {other}"),
        })
}

fn profile(cache: &Cache, tag: &Tag, at: usize) -> Result<Profile> {
    let emission = Property::read(cache, at + 0x28)?;
    let life = Property::read(cache, at + 0x48)?.constant()?;
    ensure!(
        emission.emission(1.)? > 0. && life > 0.,
        "Contrail has no finite emission or lifetime"
    );
    let material = material::embedded(cache, tag.clone(), at + 0x134)?;
    ensure!(
        matches!(
            material.options.get("albedo").map(String::as_str),
            Some("diffuse_only" | "palettized" | "palettized_plus_alpha")
        ) && matches!(
            material.options.get("blend_mode").map(String::as_str),
            Some("add_src_times_srcalpha" | "alpha_blend")
        ) && matches!(
            material.options.get("black_point").map(String::as_str),
            Some("off" | "on")
        ) && matches!(
            material.options.get("fog").map(String::as_str),
            Some("off" | "on")
        ),
        "Source contrail needs an unsupported material mode"
    );
    ensure!(
        cache.u8(at + 0x132)? == 0
            && material.properties.len() == 1
            && material.properties[0].functions.is_empty(),
        "Source contrail needs another profile shape or animated material"
    );
    ensure!(
        cache.floats::<6>(at + 0x88)? == [0.; 6]
            && cache.u8(at + 0x6a)? == 0
            && cache.floats::<4>(at + 0xe0)? == [0.; 4]
            && cache.u8(at + 0xc2)? == 0
            && Property::read(cache, at + 0xf0)?.constant()? == 0.
            && Property::read(cache, at + 0x110)?.constant()? == 0.,
        "Source contrail requires profile acceleration, offset or rotation"
    );
    let result = Profile {
        source: tag.clone(),
        name: cache.string_id(cache.u32(at)?)?,
        emission,
        life,
        width: Property::read(cache, at + 0xa0)?,
        color: Property::read(cache, at + 0x1a8)?,
        alpha: Property::read(cache, at + 0x1c8)?,
        secondary_alpha: Property::read(cache, at + 0x1e8)?,
        intensity: Property::read(cache, at + 0x248)?,
        black_point: Property::read(cache, at + 0x208)?,
        palette: Property::read(cache, at + 0x228)?,
        tiling: cache.floats::<2>(at + 0x198)?,
        scroll: cache.floats::<2>(at + 0x1a0)?,
        appearance_flags: cache.u16(at + 0x130)?,
        origin_fade: cache.floats::<2>(at + 4)?,
        material,
    };
    ensure!(
        result.tiling.into_iter().all(|v| v.is_finite() && v > 0.)
            && result.scroll.into_iter().all(f32::is_finite)
            && result.origin_fade.into_iter().all(f32::is_finite)
            && (result.appearance_flags & 0x20 == 0
                || result.origin_fade[0] > 0. && result.origin_fade[1] >= 0.)
            && result.appearance_flags & !0xed == 0
            && result.intensity.bounds()?.into_iter().all(|v| v >= 0.),
        "Source contrail has unsupported appearance settings"
    );
    // Validate the complete selected mapping before emitting any graph nodes.
    result.width.expression()?;
    result.color.expression()?;
    result.alpha.expression()?;
    secondary_alpha(&result)?;
    result.intensity.expression()?;
    result.black_point.expression()?;
    result.palette.expression()?;
    Ok(result)
}

const PIXEL: &str = r#"
Texture2D<float4> Map : register(t0);
Texture2D<float4> Palette : register(t1);
Texture2D<float4> Alpha : register(t2);
$SAMPLERS$
cbuffer TrailFrame : register(b0) { float4 trailFrame; };
cbuffer Effect : register(b13) { float4 effectData[2]; };
float4 main(float4 v0:TEXCOORD0,float3 v1:TEXCOORD1,float3 v2:TEXCOORD2,
float4 v3:TEXCOORD3,float4 v4:TEXCOORD4,float4 v5:TEXCOORD5,float4 v6:TEXCOORD6,
float4 v7:TEXCOORD7,float4 v8:TEXCOORD8,float2 v9:TEXCOORD9,float4 position:SV_POSITION0):SV_TARGET0 {
float age=saturate(v7.x);
float width=max($WIDTH$*$METRES$,0.000001);
float across=(v0.y-0.5)*2.0*$RADIUS$/width+0.5;
clip(across);clip(1.0-across);
float2 sourceUv=float2(age*$LENGTH$*$TILING_U$,(across-0.5)*$TILING_V$+0.5);
sourceUv+=trailFrame.x*float2($SCROLL_U$,$SCROLL_V$);
float2 uv=sourceUv*float2($MAP_U$,$MAP_V$)+float2($MAP_X$,$MAP_Y$);
$SAMPLE$
$BLACK_POINT$
float3 color=$COLOR$;
float intensity=max($INTENSITY$,0.0);
float opacity=max($ALPHA$,0.0)*texel.a*max(v0.z,0.0)*intensity;
$ORIGIN_FADE$
$OUTPUT$
}
"#;

#[expect(
    clippy::too_many_arguments,
    reason = "The material joins a source profile with native graph resources and dimensions"
)]
fn material(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    profile: &Profile,
    template: u32,
    prefix: &str,
    metres: f32,
    length: f32,
    radius: f32,
    emission: f32,
) -> Result<String> {
    let shell = native.tag(template, Some(0x808071e8))?;
    ensure!(
        shell.u8(0x20)? == 0x88,
        "Contrail material composition contract differs"
    );
    let sampler_rows = shell.array(0x308, 16, None)?;
    let sampler_row = *sampler_rows.first().context("Contrail sampler table")?;
    let sampler_template = shell.u32(sampler_row)?;
    let sampler_class = shell.u32(sampler_row - 8)?;
    let map = profile
        .material
        .mapping(&["base_map"])
        .context("Contrail base map")?;
    ensure!(
        map.functions.is_empty(),
        "Contrail bitmap requires animation"
    );
    let albedo = profile.material.options["albedo"].as_str();
    let mut samplers = vec![native::material::texture_sampler(
        native,
        graph,
        sampler_template,
        map,
    )?];
    let mut textures = vec![(
        0u32,
        native::material::effect_texture(
            cache,
            pages,
            native,
            graph,
            map,
            albedo == "diffuse_only",
        )?,
    )];
    let sample = if albedo == "diffuse_only" {
        "float4 texel=Map.Sample(SurfaceSampler,uv);".to_owned()
    } else {
        let palette = profile
            .material
            .mapping(&["palette"])
            .context("Contrail palette")?;
        ensure!(
            palette.functions.is_empty(),
            "Contrail palette requires animation"
        );
        textures.push((
            1,
            native::material::texture(cache, pages, native, graph, palette, false, true)?,
        ));
        let [u, v, x, y] = palette.transform;
        samplers.push(native::material::texture_sampler(
            native,
            graph,
            sampler_template,
            palette,
        )?);
        let mut sample = format!(
            "float index=Map.Sample(SurfaceSampler,uv).r;\nfloat4 texel=Palette.Sample(PaletteSampler,float2(index,{})*float2({u:.9},{v:.9})+float2({x:.9},{y:.9}));",
            profile.palette.expression()?
        );
        if albedo == "palettized_plus_alpha" {
            let alpha = profile
                .material
                .mapping(&["alpha_map"])
                .context("Contrail alpha map")?;
            ensure!(
                alpha.functions.is_empty(),
                "Contrail alpha map requires animation"
            );
            textures.push((
                2,
                native::material::effect_texture(cache, pages, native, graph, alpha, false)?,
            ));
            // Tiling and scrolling precede each texture's independent transform.
            let [u, v, x, y] = alpha.transform;
            samplers.push(native::material::texture_sampler(
                native,
                graph,
                sampler_template,
                alpha,
            )?);
            sample.push_str(&format!("\ntexel.a=Alpha.Sample(AlphaSampler,sourceUv*float2({u:.9},{v:.9})+float2({x:.9},{y:.9})).a;"));
        }
        sample
    };
    let black_point = if profile.material.options["black_point"] == "on" {
        format!(
            "float black=saturate({});\nfloat middle=(black+1.0)*0.5;\ntexel.a=middle*saturate((texel.a-black)/max(middle-black,0.000001))+saturate(texel.a-middle);",
            profile.black_point.expression()?
        )
    } else {
        String::new()
    };
    let origin_fade = if profile.appearance_flags & 0x20 != 0 {
        let [range, cutoff] = profile.origin_fade.map(|value| value * metres);
        format!("opacity*=saturate((age*{length:.9}-{cutoff:.9})/{range:.9});")
    } else {
        String::new()
    };
    let mut pixel = PIXEL.to_owned();
    let declarations = ["SurfaceSampler", "PaletteSampler", "AlphaSampler"]
        .iter()
        .take(samplers.len())
        .enumerate()
        .map(|(index, name)| format!("SamplerState {name}:register(s{});", index + 1))
        .collect::<String>();
    for (key, value) in [
        ("$SAMPLERS$", declarations),
        ("$WIDTH$", profile.width.expression()?),
        ("$COLOR$", profile.color.expression()?),
        (
            "$ALPHA$",
            if profile.secondary_alpha.constant().ok() == Some(1.) {
                profile.alpha.expression()?
            } else {
                format!(
                    "{}*max({},0.0)",
                    profile.alpha.expression()?,
                    secondary_alpha(profile)?
                )
            },
        ),
        ("$METRES$", format!("{metres:.9}")),
        ("$LENGTH$", format!("{length:.9}")),
        ("$RADIUS$", format!("{radius:.9}")),
        ("$INTENSITY$", profile.intensity.expression()?),
        ("$SAMPLE$", sample),
        ("$BLACK_POINT$", black_point),
        ("$TILING_U$", format!("{:.9}", profile.tiling[0] / metres)),
        ("$TILING_V$", format!("{:.9}", profile.tiling[1])),
        ("$SCROLL_U$", format!("{:.9}", profile.scroll[0])),
        ("$SCROLL_V$", format!("{:.9}", profile.scroll[1])),
        ("$ORIGIN_FADE$", origin_fade),
        ("$OUTPUT$", if profile.material.options["blend_mode"] == "alpha_blend" {
            "opacity=saturate(opacity);return float4(texel.rgb*color*intensity*opacity,opacity);"
        } else {
            "return float4(texel.rgb*color*opacity*intensity*effectData[1].z*effectData[1].w,0.0);"
        }.into()),
        ("$MAP_U$", format!("{:.9}", map.transform[0])),
        ("$MAP_V$", format!("{:.9}", map.transform[1])),
        ("$MAP_X$", format!("{:.9}", map.transform[2])),
        ("$MAP_Y$", format!("{:.9}", map.transform[3])),
    ] {
        pixel = pixel.replace(key, &value);
    }
    let shader = format!("{prefix}-pixel");
    native::material::program(native, graph, &shader, shell.u32(0x2c8)?, &pixel, false)?;
    ensure!(
        shell.u32(0x48)? == 0x80ff02af,
        "Contrail vertex input contract differs"
    );
    let vertex = format!("{prefix}-vertex");
    let half_step = 0.75 / (emission * profile.life);
    let vertex_source = include_str!("trail/billboard.hlsl")
        .replace("$HALF_LENGTH$", &format!("{:.9}", length * half_step))
        .replace("$HALF_STEP$", &format!("{half_step:.9}"));
    native::material::program(
        native,
        graph,
        &vertex,
        shell.u32(0x48)?,
        &vertex_source,
        true,
    )?;
    let mut bytes = shell.0.clone();
    bytes[0x22] = super::BOTH_FACES;
    // Custom particle stages read object and view scopes directly. Drop the
    // carrier's expressions and inactive shader stages before binding our own.
    for stage in [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368] {
        bytes[stage..stage + 0xa0].fill(0);
        for at in [stage, stage + 0x80, stage + 0x84] {
            put(&mut bytes, at, &u32::MAX.to_le_bytes())?;
        }
    }
    // Frame extern one, scalar one is the native advancing game clock. The
    // pixel stage binds each source sampler independently before publishing time.
    let mut expression = Vec::new();
    let mut sampler_table = Vec::new();
    for index in 0..samplers.len() {
        expression.extend([
            0x4c,
            u8::try_from(index)?,
            0x49,
            0x21 + u8::try_from(index)?,
        ]);
        sampler_table.extend(u32::MAX.to_le_bytes());
        sampler_table.extend(1u32.to_le_bytes());
        sampler_table.extend(0u64.to_le_bytes());
    }
    append(
        &mut bytes,
        0x308,
        u64::from(sampler_class),
        &sampler_table,
        16,
    )?;
    expression.extend([0x3c, 1, 1, 0x43, 0]);
    append(&mut bytes, 0x2e8, 0x80800009, &expression, 1)?;
    append(&mut bytes, 0x318, 0x80800090, &[0; 16], 16)?;
    bytes[0x33c] = 0x10;
    put(&mut bytes, 0x348, &0u32.to_le_bytes())?;
    let mut bindings = Vec::new();
    for (slot, _) in &textures {
        bindings.extend(slot.to_le_bytes());
        bindings.extend(u32::MAX.to_le_bytes());
    }
    append(&mut bytes, 0x2d0, 0x80807211, &bindings, 8)?;
    let rows = Payload(bytes.clone()).array(0x2d0, 8, Some(0x80807211))?;
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x2c8, &u32::MAX.to_le_bytes())?;
    let symbol = format!("{prefix}-material");
    let mut patches = vec![
        json!({"offset":0x48,"symbol":vertex}),
        json!({"offset":0x2c8,"symbol":shader}),
    ];
    patches.extend(
        rows.into_iter()
            .zip(textures)
            .map(|(row, (_, texture))| json!({"offset":row+4,"symbol":texture})),
    );
    patches.extend(
        Payload(bytes.clone())
            .array(0x308, 16, None)?
            .into_iter()
            .zip(samplers)
            .map(|(row, symbol)| json!({"offset":row,"symbol":symbol})),
    );
    graph.add(&symbol, template, &bytes, None, patches)?;
    Ok(symbol)
}

#[expect(
    clippy::too_many_arguments,
    reason = "A source trail joins its cache, native carrier and graph output"
)]
pub(in crate::halo_reach) fn emit(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    profile: &Profile,
    prefix: &str,
    metres: f32,
    speed: f32,
) -> Result<(String, Value)> {
    // The caller's event carrier and the particle draw envelope are independent.
    // Source contrails need ordinary births, not selector-5 line expansion.
    let template = 0x80fc7e04;
    let system = native.tag(template, Some(0x80806e28))?;
    ensure!(
        system.u32(0x18)? == 0x80c70b97
            && [4, 8, 16]
                .into_iter()
                .all(|at| system.u32(at).ok() == Some(u32::MAX)),
        "Contrail billboard system has an unsupported computational stage"
    );
    let particle_tag = system.u32(0)?;
    let original = native.tag(particle_tag, Some(0x80806e2c))?;
    let length = speed * profile.life;
    let emission = profile.emission.emission(speed / metres)?;
    let profiles = (emission * profile.life).ceil();
    ensure!(
        speed.is_finite() && speed > 0. && (2.0..=96.).contains(&profiles),
        "Source contrail exceeds native sample capacity"
    );
    let [a, b] = profile.width.bounds()?;
    ensure!(a > 0. && b > 0., "Source contrail width is not positive");
    let radius = a.max(b) * metres * 0.5;
    let material = material(
        cache,
        pages,
        native,
        graph,
        profile,
        system.u32(0x14)?,
        prefix,
        metres,
        length,
        radius,
        emission,
    )?;
    let payload = simulation::build(&original, profile.life, emission, radius)?;
    let definition = format!("{prefix}-particle");
    graph.add(&definition, particle_tag, &payload.0, None, Vec::new())?;
    let mut bytes = system.0.clone();
    for field in [0, 0x14] {
        put(&mut bytes, field, &u32::MAX.to_le_bytes())?;
    }
    let symbol = format!("{prefix}-system");
    graph.add(
        &symbol,
        template,
        &bytes,
        None,
        vec![
            json!({"offset":0,"symbol":definition}),
            json!({"offset":0x14,"symbol":material}),
        ],
    )?;
    let mut limits = vec![
        "Billboards approximate the source connected ribbon. Emission and longitudinal UV distance use reference speed. Source lightmap tint and atmospheric fog, randomized UV origin, edge fading and opacity from effect scale are not translated. Spline2 uses the GPU cubic basis without CPU input-bias behavior.",
    ];
    if profile.secondary_alpha.constant().ok() != Some(1.) {
        limits.push("Secondary alpha multiplies alpha. Where it reads contrail age, it takes its value at contrail age one, so a source fade earlier in the contrail's life is not drawn.");
    }
    Ok((
        symbol,
        json!({
            "source":profile,"length_metres":length,"profile_count":profiles,
            "source_profiles_per_second":emission,"native_emissions_per_second":emission,
            "radius_metres":radius,
            "geometry":"Stationary CPU particles born at successive marker positions, with linear within-frame placement and overlapping camera-facing quads.",
            "appearance":"Source lifetime, width, color, alpha, tiling, scrolling and flagged origin fading. Each texture uses its source address and filter modes without mip bias.",
            "composition":profile.material.options["blend_mode"],"gameplay_verified":false,
            "limits":limits
        }),
    ))
}
