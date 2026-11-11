use super::*;

pub(super) fn parameters(
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> Result<String> {
    let option = surface
        .material
        .options
        .get("alpha_blend_source")
        .map(String::as_str);
    if !surface.transparent()
        || surface.material.glass()
        || matches!(option, None | Some("from_albedo_alpha_without_fresnel"))
    {
        return Ok("float sourceSpecularOpacity=1;".into());
    }
    let fresnel = format!(
        "float sourceNdotV=dot(v0.xyz,normalize(v5.xyz));float sourceFresnel=sourceNdotV>0?saturate({}*pow(saturate(1-sourceNdotV),{})+{}):0;float sourceSpecularOpacity=1+sourceFresnel;",
        surface.scalar("opacity_fresnel_coefficient", 0.),
        surface.scalar("opacity_fresnel_curve_steepness", 1.),
        surface.scalar("opacity_fresnel_curve_bias", 0.)
    );
    // HREK's additive blend writes zero alpha. MCC U13 removes opacity_texture from
    // these programs even when their selected alpha source names it. Fresnel still
    // scales specular and environment radiance independently of the unused coverage.
    if surface
        .material
        .options
        .get("blend_mode")
        .is_some_and(|mode| mode == "additive")
        && matches!(
            option,
            Some(
                "from_albedo_alpha"
                    | "from_opacity_map_alpha"
                    | "from_opacity_map_rgb"
                    | "from_opacity_map_alpha_and_albedo_alpha"
            )
        )
    {
        return Ok(fresnel);
    }
    let coverage = match option {
        Some("from_albedo_alpha") => "albedo.a".into(),
        Some(
            "from_opacity_map_alpha"
            | "from_opacity_map_rgb"
            | "from_opacity_map_alpha_and_albedo_alpha",
        ) => {
            let sample = samples
                .get(&GLASS_OPACITY_SLOT)
                .context("Opacity source texture is absent")?;
            match option {
                Some("from_opacity_map_rgb") => {
                    format!("dot(({sample}).rgb,float3(0.2126,0.7152,0.0722))")
                }
                Some("from_opacity_map_alpha_and_albedo_alpha") => {
                    format!("(1-(1-({sample}).a)*(1-albedo.a))")
                }
                _ => format!("({sample}).a"),
            }
        }
        other => anyhow::bail!("Unsupported alpha blend source {other:?}"),
    };
    Ok(format!(
        "{fresnel}albedo.a=1-(1-({coverage}))*(1-sourceFresnel);"
    ))
}
