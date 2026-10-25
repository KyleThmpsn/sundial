//! Directional source environment sampling for authored material reflections.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn bind(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    material: &Material,
    surface: &surface::Surface<'_>,
    bindings: &mut Vec<(usize, String)>,
    notes: &mut Vec<String>,
) -> Result<(String, Option<String>)> {
    let Some(mapping) = material.mapping(&["environment_map"]) else {
        return Ok((String::new(), None));
    };
    let bitmap = mapping.bitmap.as_ref().context("Environment bitmap")?;
    let image = mapping.static_frame(c)?;
    let row = *c
        .block(bitmap.address()? + 124, 56)?
        .get(image)
        .context("Environment image")?;
    if c.u8(row + 6)? != 2 {
        let mean = material::cube_mean(c, pages, bitmap)?;
        notes.push(format!(
            "{} is not a cubemap and retains its mean environment color",
            bitmap.path
        ));
        return Ok((
            String::new(),
            if material.water() {
                notes.push("Water's source HDR reflection requires a cube image. A non-cube environment remains unbound.".into());
                None
            } else {
                Some(format!("float3({:e},{:e},{:e})", mean[0], mean[1], mean[2]))
            },
        ));
    }
    let cube = material::cube(c, pages, bitmap, image)?;
    let symbol = format!("reach-environment-{:08x}-{image}", bitmap.datum);
    if !graph.nodes.iter().any(|node| node["symbol"] == symbol) {
        let gamma = c.u8(row + 17)? != LINEAR_CURVE;
        let radiance = cube
            .mips
            .iter()
            .map(|faces| {
                faces.each_ref().map(|face| {
                    face.chunks_exact(4)
                        .map(|texel| {
                            std::array::from_fn(|lane| {
                                let value = f32::from(texel[lane]) / 255.;
                                if gamma && lane < 3 {
                                    value.powf(2.2)
                                } else {
                                    value
                                }
                            })
                        })
                        .collect::<Vec<[f32; 4]>>()
                })
            })
            .collect::<Vec<_>>();
        cube_mips(native, graph, &symbol, cube.size, &radiance)?;
    }
    let register = bindings.iter().map(|(slot, _)| slot + 1).max().unwrap_or(0);
    ensure!(
        register < MAX_TEXTURE_REGISTERS,
        "Environment map exceeds texture binding capacity"
    );
    bindings.push((register, symbol));
    let lod = format!(
        "max(({}-0.5)*8,0)",
        surface.scalar("env_roughness_offset", 0.5)
    );
    // Reach's compiled pixel programs negate the reflected vector's Y before cube sampling.
    let sample = if material.water() {
        let variation = surface.scalar("normal_variation_tweak", 1.);
        format!(
            "ReachEnvironment.Sample(ClampSampler,reflect(-normalize(v5.xyz),lerp(float3(0,0,1),n,{variation}))*float3(1,-1,1))"
        )
    } else {
        format!("reachEnvironment(reflect(-normalize(v5.xyz),n)*float3(1,-1,1),{lod})")
    };
    notes.push(if material.water() {
        "Water reflection retains all source mips and HDR alpha for the source sun-spot calculation. Refraction and underwater state remain untranslated."
    } else {
        "Environment reflection retains all declared source mip levels and multiplies sampled RGB by sampled alpha. Cube orientation and the native BRDF still require gameplay comparison."
    }.into());
    Ok((
        format!(
            "TextureCube<float4> ReachEnvironment:register(t{register});float3 reachEnvironment(float3 direction,float lod){{float4 value=ReachEnvironment.SampleLevel(ClampSampler,direction,lod);return value.rgb*value.a;}}"
        ),
        Some(sample),
    ))
}
