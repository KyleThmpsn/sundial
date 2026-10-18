//! Source contrail profiles on the checked native CPU beam presentation.
use super::property::Property;
use crate::{
    halo_reach::{
        cache::{Cache, Tag},
        material::{self, Material},
        native_material,
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
    color: Property,
    alpha: Property,
    intensity: Property,
    black_point: Property,
    palette: Property,
    tiling: [f32; 2],
    scroll: [f32; 2],
    appearance_flags: u16,
    origin_fade: [f32; 2],
    material: Material,
}

pub(in crate::halo_reach) fn read(cache: &Cache, tag: &Tag) -> Result<Vec<Profile>> {
    ensure!(tag.group == "cntl", "Expected a source contrail system");
    cache
        .block(tag.address()?, 0x280)?
        .into_iter()
        .map(|at| {
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
            ensure!(
                Property::read(cache, at + 0x1e8)?.constant()? == 1.,
                "Source contrail requires secondary alpha"
            );
            let result = Profile {
                source: tag.clone(),
                name: cache.string_id(cache.u32(at)?)?,
                emission,
                life,
                width: Property::read(cache, at + 0xa0)?,
                color: Property::read(cache, at + 0x1a8)?,
                alpha: Property::read(cache, at + 0x1c8)?,
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
            result.intensity.expression()?;
            result.black_point.expression()?;
            result.palette.expression()?;
            Ok(result)
        })
        .collect()
}

const PIXEL: &str = r#"
Texture2D<float4> Map : register(t0);
Texture2D<float4> Palette : register(t1);
Texture2D<float4> Alpha : register(t2);
SamplerState SurfaceSampler : register(s1);
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
    profiles: f32,
) -> Result<String> {
    let map = profile
        .material
        .mapping(&["base_map"])
        .context("Contrail base map")?;
    ensure!(
        map.functions.is_empty(),
        "Contrail bitmap requires animation"
    );
    let albedo = profile.material.options["albedo"].as_str();
    let mut textures = vec![(
        0u32,
        native_material::texture(
            cache,
            pages,
            native,
            graph,
            map,
            false,
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
            native_material::texture(cache, pages, native, graph, palette, false, true)?,
        ));
        let [u, v, x, y] = palette.transform;
        let mut sample = format!(
            "float index=Map.Sample(SurfaceSampler,uv).r;\nfloat4 texel=Palette.Sample(SurfaceSampler,float2(index,{})*float2({u:.9},{v:.9})+float2({x:.9},{y:.9}));",
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
                native_material::texture(cache, pages, native, graph, alpha, false, false)?,
            ));
            // Tiling and scrolling precede each texture's independent transform.
            let [u, v, x, y] = alpha.transform;
            sample.push_str(&format!("\ntexel.a=Alpha.Sample(SurfaceSampler,sourceUv*float2({u:.9},{v:.9})+float2({x:.9},{y:.9})).a;"));
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
    for (key, value) in [
        ("$WIDTH$", profile.width.expression()?),
        ("$COLOR$", profile.color.expression()?),
        ("$ALPHA$", profile.alpha.expression()?),
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
    let shell = native.tag(template, Some(0x808071e8))?;
    ensure!(
        shell.u8(0x20)? == 0x88,
        "Contrail material composition contract differs"
    );
    let shader = format!("{prefix}-pixel");
    native_material::program(native, graph, &shader, shell.u32(0x2c8)?, &pixel, false)?;
    ensure!(
        shell.u32(0x48)? == 0x80fe372b,
        "Contrail vertex input contract differs"
    );
    let vertex = format!("{prefix}-vertex");
    let vertex_source = include_str!("trail/vertex.hlsl")
        .replace("$LENGTH$", &format!("{length:.9}"))
        .replace("$HALF_STEP$", &format!("{:.9}", 0.5 / (profiles - 1.)));
    native_material::program(
        native,
        graph,
        &vertex,
        shell.u32(0x48)?,
        &vertex_source,
        true,
    )?;
    let mut bytes = shell.0.clone();
    // Preserve the envelope's vertex constant allocation with neutral values.
    // Its donor expressions require effect-scale, flicker and distance inputs
    // that source contrails do not supply. The ribbon shader handles native
    // particle expiry, while the pixel shader evaluates source alpha.
    let visibility = [
        0x34, 0, 0x43, 1, 0x34, 0, 0x43, 76, 0x34, 1, 0x43, 105, 0x34, 0, 0x43, 111, 0x34, 2, 0x43,
        115,
    ];
    let constants = [[1f32; 4], [0.; 4], [0., 1., 0., 0.]]
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    append(&mut bytes, 0x68, 0x80800009, &visibility, 1)?;
    append(&mut bytes, 0x78, 0x80800090, &constants, 16)?;
    // Frame extern one, scalar one is the native advancing game clock. The
    // pixel stage has its own output buffer and register selection. Retain the
    // carrier's sampler bindings before publishing the clock into row zero.
    let mut expression = shell
        .array(0x2e8, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| shell.u8(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        expression == [0x4c, 0, 0x49, 0x21, 0x4c, 1, 0x49, 0x22]
            && shell.array(0x318, 16, Some(0x80800090))?.is_empty(),
        "Contrail pixel output contract differs"
    );
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
    system_tag: u32,
    prefix: &str,
    metres: f32,
    speed: f32,
) -> Result<(String, Value)> {
    let system = native.tag(system_tag, Some(0x80806e28))?;
    let particle_tag = system.u32(0)?;
    let original = native.tag(particle_tag, Some(0x80806e2c))?;
    ensure!(
        original.u8(0x14b)? == 5
            && (0..8)
                .all(|i| original.u16(0x70 + i * 2).ok() == Some([0, 25, 8, 51, 24, 225, 10, 0][i]))
            && original.array(0x60, 16, Some(0x80800090))?.len() == 41,
        "Source contrail needs the checked native CPU beam layout"
    );
    let length = speed * profile.life;
    let emission = profile.emission.emission(speed / metres)?;
    let density = emission / speed;
    // Match the native count consumer's float32 operation order. The vertex
    // segments must meet at the actual expanded sample spacing.
    let profiles = (density * length).ceil();
    ensure!(
        speed.is_finite() && speed > 0. && (2.0..=96.).contains(&profiles),
        "Source contrail exceeds native beam profile capacity"
    );
    let [a, b] = profile.width.bounds()?;
    ensure!(a > 0. && b > 0., "Source contrail width is not positive");
    let radius = (a.max(b) * metres * 0.5).max(length / (profiles - 1.) * 0.5);
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
        profiles,
    )?;
    let mut payload = (*original).clone();
    let defaults = payload.array(8, 16, Some(0x80800090))?;
    for (route, value) in [(5, profile.life), (10, 1. / profile.life), (34, density)] {
        ensure!(
            payload.u8(0x80 + route * 2)? == 6,
            "Native beam route is not a default"
        );
        let scalar = usize::from(payload.u8(0x81 + route * 2)?);
        let at = *defaults.get(scalar / 4).context("Native beam default")? + (scalar % 4) * 4;
        put(&mut payload.0, at, &value.to_le_bytes())?;
    }
    put(&mut payload.0, 0x11c, &length.to_le_bytes())?;
    put(&mut payload.0, 0x120, &(profile.life + 0.05).to_le_bytes())?;
    let rows = payload.array(0x60, 16, Some(0x80800090))?;
    let mut constants = rows
        .iter()
        .map(|&at| payload.0[at..at + 16].to_vec())
        .collect::<Vec<_>>();
    for row in [16, 17] {
        constants[row][..4].copy_from_slice(&length.to_le_bytes());
    }
    let one = u8::try_from(constants.len())?;
    constants.push([1f32.to_le_bytes(); 4].concat());
    let size = u8::try_from(constants.len())?;
    constants.push([radius.to_le_bytes(); 4].concat());
    let fields = payload.array(0x50, 1, Some(0x80800009))?;
    let mut code = fields.iter().map(|&at| payload.0[at]).collect::<Vec<_>>();
    let mut at = 0;
    let mut replaced = 0;
    while at < code.len() {
        let width = match code[at] {
            0x22 | 0x34..=0x3b | 0x40..=0x47 => 2,
            0x3e | 0x3f => 4,
            1..=0x21 | 0x23..=0x33 | 0x3d | 0x49..=0x52 => 1,
            other => anyhow::bail!("Unsupported beam instruction {other:02X}"),
        };
        ensure!(at + width <= code.len(), "Truncated beam instruction");
        if code[at] == 0x47 {
            ensure!(code[at + 1] < 4, "Beam runtime input exceeds its table");
            code[at] = 0x34;
            code[at + 1] = one;
            replaced += 1;
        }
        at += width;
    }
    ensure!(replaced == 4, "Native beam input count differs");
    // The ribbon vertex shader retains the native input layout. Appearance
    // 8 Y is its width envelope, and 6 X is the normalized sample distance.
    // Vertex interpolation makes that distance continuous for the pixel curves.
    let extra = [0x34, size, 0x3f, 1, 8, 5, 0x3e, 1, 2, 7, 0x3f, 1, 6, 4];
    code.splice(333..333, extra);
    put(
        &mut payload.0,
        0x7a,
        &(225u16 + extra.len() as u16).to_le_bytes(),
    )?;
    append(&mut payload.0, 0x50, 0x80800009, &code, 1)?;
    append(&mut payload.0, 0x60, 0x80800090, &constants.concat(), 16)?;
    put(&mut payload.0, 0x100, &[0; 16])?;
    let definition = format!("{prefix}-particle");
    graph.add(&definition, particle_tag, &payload.0, None, Vec::new())?;
    let mut bytes = system.0.clone();
    for field in [0, 0x14] {
        put(&mut bytes, field, &u32::MAX.to_le_bytes())?;
    }
    let symbol = format!("{prefix}-system");
    graph.add(
        &symbol,
        system_tag,
        &bytes,
        None,
        vec![
            json!({"offset":0,"symbol":definition}),
            json!({"offset":0x14,"symbol":material}),
        ],
    )?;
    Ok((
        symbol,
        json!({"source":profile,"length_metres":length,"profile_count":profiles,
        "source_profiles_per_second":emission,"native_emissions_per_second":1./profile.life,"radius_metres":radius,
        "vertex_visibility":"Joined camera-facing ribbon segments with native particle expiry. Source alpha is evaluated continuously along the trail.",
        "geometry":"Each native sample covers the interval between neighboring midpoints. Endpoint intervals stop at the source trail bounds.",
        "appearance":"Source tiling, game-time texture scrolling and flagged origin fading precede independent texture transforms.",
        "composition":profile.material.options["blend_mode"],
        "limits":["Source bitmaps, palette, color, width, alpha, black point and intensity use a straight ribbon. Profile age and speed-dependent emission use the declared reference speed. Historical bending, partial trail growth, source lightmap tint and atmospheric fog, randomized UV origin, edge fading and opacity from effect scale are not translated. Spline2 uses the GPU cubic basis without CPU input-bias behavior."]}),
    ))
}
