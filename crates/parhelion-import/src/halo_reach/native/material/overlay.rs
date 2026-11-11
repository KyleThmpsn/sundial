use super::*;

pub(super) const WARP: usize = 37;
pub(super) const MAP: usize = 38;
pub(super) const DETAIL: usize = 39;
pub(super) const MULTIPLY: usize = 40;

pub(super) fn coordinates(
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> Result<String> {
    match surface.material.options.get("warp").map(String::as_str) {
        None | Some("none" | "parallax_simple") => Ok(String::new()),
        Some("from_texture") => {
            let map = samples.get(&WARP).context("Warp texture is absent")?;
            Ok(format!(
                "v3.xy+=({map}).xy*float2({},{});",
                surface.scalar("warp_amount_x", 0.),
                surface.scalar("warp_amount_y", 0.)
            ))
        }
        other => anyhow::bail!("Unsupported surface warp mode {other:?}"),
    }
}

/// Source overlays operate on the combined lit color, after self-illumination exposure.
/// Keep the additive term separate so the emission exposure cannot scale it a second time.
pub(super) fn parameters(
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
    notes: &mut Vec<String>,
) -> Result<String> {
    let option = surface.material.options.get("overlay").map(String::as_str);
    let mut multiply = "float3(1,1,1)".to_owned();
    let mut add = "float3(0,0,0)".to_owned();
    if !matches!(option, None | Some("none")) {
        let map = samples.get(&MAP).context("Overlay texture is absent")?;
        let mut color = format!(
            "({map}).rgb*{}*{}",
            surface.color("overlay_tint", [1.; 4]),
            surface.scalar("overlay_intensity", 1.)
        );
        if matches!(
            option,
            Some("additive_detail" | "multiply_and_additive_detail")
        ) {
            let detail = samples
                .get(&DETAIL)
                .context("Overlay detail texture is absent")?;
            color = format!("({color})*({detail}).rgb*{DETAIL_MULTIPLIER:.6}");
        }
        match option {
            Some("multiply") => multiply = color,
            Some("additive" | "additive_detail") => add = color,
            Some("multiply_and_additive_detail") => {
                // Holograms use material_model_none for static lighting. With no self
                // illumination, an additive surface has zero color before its overlays.
                // MCC removes this multiply sample, while retaining both additive samples.
                let zero_input = surface
                    .material
                    .tag
                    .as_ref()
                    .is_some_and(|tag| tag.group == "rmhg")
                    && surface
                        .material
                        .options
                        .get("blend_mode")
                        .map(String::as_str)
                        == Some("additive")
                    && matches!(
                        surface
                            .material
                            .options
                            .get("self_illumination")
                            .map(String::as_str),
                        Some("off" | "none")
                    );
                if zero_input {
                    notes.push("The additive hologram has no self illumination or static material lighting. Its multiplicative overlay input is zero, so the source program omits that texture sample.".into());
                } else {
                    let map = samples
                        .get(&MULTIPLY)
                        .context("Multiplicative overlay texture is absent")?;
                    multiply = format!("({map}).rgb");
                }
                add = color;
            }
            other => anyhow::bail!("Unsupported surface overlay mode {other:?}"),
        }
    }
    match surface
        .material
        .options
        .get("edge_fade")
        .map(String::as_str)
    {
        None | Some("none") => (),
        Some("simple") => {
            let fade = format!(
                "lerp({},{},pow(abs(dot(normalize(v5.xyz),n)),{}))",
                surface.color("edge_fade_edge_tint", [1.; 4]),
                surface.color("edge_fade_center_tint", [1.; 4]),
                surface.scalar("edge_fade_power", 1.)
            );
            multiply = format!("({multiply})*({fade})");
            add = format!("({add})*({fade})");
        }
        other => anyhow::bail!("Unsupported surface edge fade {other:?}"),
    }
    Ok(format!(
        "float3 surfaceMultiplier={multiply};float3 surfaceOverlay={add};albedo.rgb*=surfaceMultiplier;emission*=surfaceMultiplier;"
    ))
}
