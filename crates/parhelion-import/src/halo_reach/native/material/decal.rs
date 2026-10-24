//! Cached decal layers keep projection coordinates separate from sprite and tiled samples.
use super::*;

pub(super) const VECTOR: usize = 30;
pub(super) const SHADOW: usize = 31;

/// Source multiplicative color ignores alpha. The native premultiplied pass approximates
/// scalar attenuation, but cannot multiply channels or brighten the receiver.
pub(super) fn multiplication(material: &Material) -> Option<String> {
    if !material.decal() {
        return None;
    }
    let factor = match material.options.get("blend_mode").map(String::as_str) {
        Some("multiply") => 1.,
        Some("double_multiply") => 2.,
        _ => return None,
    };
    Some(format!(
        "o0=float4(0,0,0,1-saturate(dot(unlit.rgb,float3(1,1,1))*{factor:.1}/3));"
    ))
}

fn vector(material: &Material) -> bool {
    material.decal()
        && matches!(
            material.options.get("albedo").map(String::as_str),
            Some("vector_alpha" | "vector_alpha_drop_shadow")
        )
}

pub(super) fn premultiplied(material: &Material) -> bool {
    vector(material)
        && material
            .options
            .get("blend_mode")
            .is_some_and(|mode| mode == "pre_multiplied_alpha")
}

/// MCC's original distance-field program differentiates the sprite/tiled coordinates,
/// samples the vector map without its transform, and returns premultiplied color.
pub(super) fn albedo(
    material: &Material,
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> Result<Option<String>> {
    if !vector(material) {
        return Ok(None);
    }
    let distance = samples
        .get(&VECTOR)
        .context("Decal vector alpha has no vector map")?;
    let base = samples
        .get(&0)
        .context("Decal vector alpha has no base map")?;
    let scale = format!(
        "max({}/max(length(float4(ddx_coarse(decalUv),ddy_coarse(decalUv))),1e-20),1)",
        surface.scalar("antialias_tweak", 1.)
    );
    let alpha = format!(
        "saturate((({distance}).g-0.5)*min({scale},{})+0.5)",
        surface.scalar("vector_sharpness", 1.)
    );
    let coverage = if material
        .options
        .get("albedo")
        .is_some_and(|mode| mode == "vector_alpha_drop_shadow")
    {
        let shadow = samples
            .get(&SHADOW)
            .context("Decal drop shadow has no shadow vector map")?;
        format!(
            "({alpha})+saturate((({shadow}).g-0.5)*min({scale},{})+0.5)*{}",
            surface.scalar("shadow_sharpness", 1.),
            surface.scalar("shadow_darkness", 1.)
        )
    } else {
        alpha.clone()
    };
    Ok(Some(format!("float4(({base}).rgb*({alpha}),{coverage})")))
}

pub(super) fn coordinates(material: &Material, surface: &surface::Surface<'_>) -> String {
    if !material.decal() {
        return String::new();
    }
    let [x, y, u, v] = material
        .decal
        .as_ref()
        .map_or([0., 0., 1., 1.], |decal| decal.sprite);
    format!(
        "float2 decalUv=mad(v3.xy,float2({u:.9},{v:.9}),float2({x:.9},{y:.9}))*float2({},{});",
        surface.scalar("u_tiles", 1.),
        surface.scalar("v_tiles", 1.)
    )
}

pub(super) fn base(material: &Material, slot: usize) -> &'static str {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    if slot == 3 && option("albedo") == Some("diffuse_plus_alpha_mask")
        || slot == 1 && option("bump_mapping") == Some("standard_mask")
    {
        "v3.xy"
    } else {
        "decalUv"
    }
}

pub(super) fn tint(material: &Material, surface: &surface::Surface<'_>) -> String {
    if !material.decal() {
        return String::new();
    }
    let modulation = match material.options.get("tinting").map(String::as_str) {
        Some("unmodulated") => "0".to_owned(),
        Some("fully_modulated") => "1".to_owned(),
        Some("partially_modulated") => surface.scalar("modulation_factor", 0.),
        _ => return String::new(),
    };
    format!(
        "albedo.rgb*=lerp({},float3(1,1,1),({modulation})*length(albedo.rgb)*0.577350269)*{};",
        surface.color("tint_color", [1.; 4]),
        surface.scalar("intensity", 1.)
    )
}

pub(super) fn notes(material: &Material, notes: &mut Vec<String>) {
    if !material.decal() {
        return;
    }
    notes.push("Decal projection clipping, cached sprite selection, tiling and tint use source inputs. Native depth bias is approximated by a geometric offset. Source distance fade is not translated.".into());
    if material
        .decal
        .as_ref()
        .is_none_or(|decal| decal.render_pass == 0)
    {
        notes.push("Source pre-lighting decals modify the receiver G-buffer. This translation uses a separately lit surface, so receiver lighting, normal and specular modification remain approximate.".into());
    }
    if material
        .options
        .get("blend_mode")
        .is_some_and(|mode| matches!(mode.as_str(), "multiply" | "double_multiply"))
    {
        notes.push("Decal multiplication is approximated by scalar darkening in the native premultiplied pass. Double multiplication retains its factor of two, but channel-specific multiplication and brightening remain unsupported.".into());
    }
}
