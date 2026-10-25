//! Terrain material parameters follow the MCC albedo and static-lightmap programs.
use super::*;

pub(super) fn active(material: &Material, layer: usize) -> bool {
    material
        .options
        .get(&format!("material_{layer}"))
        .is_some_and(|option| option != "off")
}

fn specular(material: &Material, layer: usize) -> bool {
    // The fourth category names its sampler-budget restriction in the option itself.
    matches!(
        material
            .options
            .get(&format!("material_{layer}"))
            .map(String::as_str),
        Some(
            "diffuse_plus_specular"
                | "diffuse_plus_specular_(four_material_shaders_disable_detail_bump)"
        )
    )
}

/// Lighting keeps the normalized weights before the albedo pass's small-layer cutoff.
pub(super) fn parameters(surface: &surface::Surface<'_>, weights: &str) -> String {
    let mut result = format!(
        "float4 terrainBlend={weights};float4 terrainDraw=terrainBlend*float4(terrainBlend>0.04);\nfloat terrainDiffuse=0,terrainSpecWeight=0.001,terrainSpecPower=0.001,terrainSpecSteepness=0.005,terrainAlbedoBlend=0,terrainArea=0,terrainEnvironment=0;float3 terrainSpecTint=0;\n"
    );
    for layer in 0..4 {
        if !active(surface.material, layer) {
            continue;
        }
        let weight = format!("terrainBlend[{layer}]");
        let scalar = |name: &str, default| surface.scalar(&format!("{name}_m_{layer}"), default);
        if specular(surface.material, layer) {
            let diffuse = scalar("diffuse_coefficient", 1.);
            let power = scalar("specular_power", 16.);
            let steepness = scalar("fresnel_curve_steepness", 5.);
            let blend = scalar("albedo_specular_tint_blend", 0.);
            let coefficient = scalar("specular_coefficient", 1.);
            let area = scalar("area_specular_contribution", 0.);
            let environment = scalar("environment_specular_contribution", 0.);
            let tint = surface.color(&format!("specular_tint_m_{layer}"), [1.; 4]);
            result.push_str(&format!(
                "terrainDiffuse+={weight}*{diffuse};terrainSpecWeight+={weight};terrainSpecPower+={weight}*{power};terrainSpecSteepness+={weight}*{steepness};terrainSpecTint+={weight}*{tint}*(1-{blend});terrainAlbedoBlend+={weight}*{blend};terrainArea+={weight}*{coefficient}*{area};terrainEnvironment+={weight}*{coefficient}*{environment};\n"
            ));
        } else {
            result.push_str(&format!("terrainDiffuse+={weight};\n"));
        }
    }
    result
        .push_str("terrainSpecPower/=terrainSpecWeight;terrainSpecSteepness/=terrainSpecWeight;\n");
    result
}

pub(super) fn base(surface: &surface::Surface<'_>, layer: usize, sample: String) -> String {
    if surface.material.options.get("blending").map(String::as_str) != Some("distance_blend_base") {
        return sample;
    }
    // MCC's terrain vertex program supplies camera distance in source world units. Keep the
    // distance interpolated from vertices, rather than recomputing its length per pixel.
    let slope = surface.scalar("blend_slope", 0.);
    let offset = surface.scalar("blend_offset", 0.);
    let maximum = surface.scalar(&format!("blend_max_{layer}"), 1.);
    let target = surface.value(&format!("blend_target_{layer}"), [1.; 4]);
    format!("lerp({sample},{target},min(saturate(v1.w*{slope}+{offset}),{maximum}))")
}

/// Map the blended Phong exponent to the native roughness input. The two BRDFs differ.
pub(super) fn roughness(material: &Material) -> String {
    if (0..4).any(|layer| specular(material, layer)) {
        "clamp(sqrt(2/(max(terrainSpecPower,0)+2)),0.05,1)".into()
    } else {
        "1".into()
    }
}

pub(super) fn reflection(
    surface: &surface::Surface<'_>,
    table: Option<&str>,
    environment: Option<&str>,
    level: f32,
) -> Option<(String, [f32; 3])> {
    if !(0..4).any(|layer| specular(surface.material, layer)) {
        return None;
    }
    let response = match table {
        Some(sampler) => format!(
            "Map{VMF_TABLE_SLOT}.SampleLevel({sampler},float2(saturate(dot(reflect(-normalize(v5.xyz),n),lightDirection)*0.5+0.5),lightLobe.a),0).y"
        ),
        None => "lerp(0.5,saturate(dot(reflect(-normalize(v5.xyz),n),lightDirection)),lightLobe.a)"
            .into(),
    };
    let area = format!("max(lightLobeIrradiance*({response})+lightAmbientIrradiance,0)");
    let environment = if surface
        .material
        .options
        .get("environment_map")
        .map(String::as_str)
        == Some("per_pixel")
    {
        environment.map_or_else(
            || "float3(0,0,0)".into(),
            |sample| {
                let tint = surface.color("env_tint_color", [1.; 4]);
                format!(
                    "(({area})*0.6+{:e})*({sample})*{tint}*terrainEnvironment",
                    0.005 / level.max(1e-6)
                )
            },
        )
    } else {
        "float3(0,0,0)".into()
    };
    Some((
        format!(
            "unlit.a*lerp(lerp(terrainSpecTint,unlit.rgb,terrainAlbedoBlend),float3(1,1,1),pow(1-saturate(dot(n,normalize(v5.xyz))),max(terrainSpecSteepness,0)))*(({area})*terrainArea+{environment})"
        ),
        surface.constant("env_tint_color").unwrap_or([1.; 4])[..3]
            .try_into()
            .unwrap(),
    ))
}
