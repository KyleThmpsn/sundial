//! Source VMF specular lookup sampling. Texture-array layer rounding is preserved explicitly.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn bind(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    material: &Material,
    bindings: &mut Vec<(usize, String)>,
) -> Result<Option<String>> {
    if !matches!(
        material.options.get("material_model").map(String::as_str),
        Some("two_lobe_phong" | "cook_torrance")
    ) {
        return Ok(None);
    }
    let Some(mapping) = material.mapping(&["g_diffuse_power_specular"]) else {
        return Ok(None);
    };
    ensure!(
        mapping.address == 0x11 && mapping.filter == 0,
        "Unsupported specular lookup sampler"
    );
    let tag = mapping.bitmap.as_ref().context("Specular lookup bitmap")?;
    let frame = mapping.static_frame(cache)?;
    let lookup = material::lookup::read(cache, pages, tag, frame)?;
    let symbol = format!("reach-specular-{:08x}-{frame}", tag.datum);
    if !graph.nodes.iter().any(|node| node["symbol"] == symbol) {
        rgba_texture(
            native,
            graph,
            &symbol,
            lookup.width,
            lookup.height * lookup.layers,
            lookup.rgba,
            false,
            Mips::None,
        )?;
    }
    let register = bindings.len();
    ensure!(
        register < MAX_TEXTURE_REGISTERS,
        "Specular lookup exceeds texture registers"
    );
    bindings.push((register, symbol));
    let layers = lookup.layers;
    let last = layers - 1;
    let u = 0.5 / lookup.width as f32;
    let v = 0.5 / lookup.height as f32;
    // HLSL array sampling rounds the slice to nearest even and clamps it. Source DXBC
    // interpolates two such samples at frac(roughness)*count, wrapping the next coordinate.
    Ok(Some(format!(
        "Texture2D<float4> ReachSpecularTable:register(t{register});float reachSpecularSlice(float2 uv,float layer){{uv=clamp(uv,float2({u:.9},{v:.9}),float2(1-{u:.9},1-{v:.9}));uv.y=(uv.y+clamp(round(layer),0,{last}))/{layers}.0;return ReachSpecularTable.SampleLevel(ClampSampler,uv,0).r;}}float reachSpecular(float cosine,float bandwidth,float roughness){{float layer=frac(roughness)*{layers}.0;float next=layer>={last}.0?0:layer+1;float2 uv=float2(cosine*0.5+0.5,bandwidth);return lerp(reachSpecularSlice(uv,layer),reachSpecularSlice(uv,next),frac(layer));}}"
    )))
}
