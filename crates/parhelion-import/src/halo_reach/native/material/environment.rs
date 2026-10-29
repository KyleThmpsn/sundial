//! Directional source environment sampling for authored material reflections.
use super::*;

/// Reach's dynamic cubemap scale. `calc_environment_map_dynamic_ps` scales the sample by four on
/// the PC and then squares its color and multiplies it by its alpha, `64 * rgb^2 * a`, normalized
/// to an average of one.
const DYNAMIC_SCALE: f32 = 64.;

/// A surface's environment map, bound past its other textures and clear of `reserved`, and its
/// reflection sample. A `per_pixel` map is its own cube with alpha kept for the HDR scale. A
/// `dynamic` map is the cube of the cluster the surface stands in, decoded to `64 * rgb^2 * a`,
/// and sampled at Reach's level `(r + (env_roughness_offset - 0.5) * 2) * 4`, with `r` the
/// Cook-Torrance analytical roughness or the two-lobe Phong `1.01 - normal_specular_power / 200`.
#[allow(clippy::too_many_arguments)]
pub(super) fn bind(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    material: &Material,
    surface: &surface::Surface<'_>,
    bindings: &mut Vec<(usize, String)>,
    reserved: &[usize],
    notes: &mut Vec<String>,
) -> Result<(String, Option<String>)> {
    let dynamic = !material.water()
        && material
            .options
            .get("environment_mapping")
            .is_some_and(|option| option == "dynamic");
    let mapping = if dynamic {
        material.mapping(&[DYNAMIC_ROLE])
    } else {
        material.mapping(&["environment_map"])
    };
    let Some(mapping) = mapping else {
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
    let symbol = if dynamic {
        format!("reach-dynamic-environment-{:08x}-{image}", bitmap.datum)
    } else {
        format!("reach-environment-{:08x}-{image}", bitmap.datum)
    };
    if !graph.contains(&symbol) {
        let cube = material::cube(c, pages, bitmap, image)?;
        let gamma = !dynamic && c.u8(row + 17)? != LINEAR_CURVE;
        let radiance = cube
            .mips
            .iter()
            .map(|faces| {
                faces.each_ref().map(|face| {
                    face.chunks_exact(4)
                        .map(|texel| {
                            let unit = |lane: usize| f32::from(texel[lane]) / 255.;
                            if dynamic {
                                let scale = DYNAMIC_SCALE * unit(3);
                                [
                                    unit(0).powi(2) * scale,
                                    unit(1).powi(2) * scale,
                                    unit(2).powi(2) * scale,
                                    1.,
                                ]
                            } else {
                                std::array::from_fn(|lane| {
                                    if gamma && lane < 3 {
                                        unit(lane).powf(2.2)
                                    } else {
                                        unit(lane)
                                    }
                                })
                            }
                        })
                        .collect::<Vec<[f32; 4]>>()
                })
            })
            .collect::<Vec<_>>();
        cube_mips(native, graph, &symbol, cube.size, &radiance)?;
    }
    let register = free_registers(bindings, 1, reserved);
    ensure!(
        register < MAX_TEXTURE_REGISTERS,
        "Environment map exceeds texture binding capacity"
    );
    bindings.push((register, symbol));
    let offset = surface.scalar("env_roughness_offset", 0.5);
    let lod = if dynamic {
        let roughness = match material.options.get("material_model").map(String::as_str) {
            Some("cook_torrance" | "organism") => surface.scalar(
                if surface.constant("analytical_roughness").is_some() {
                    "analytical_roughness"
                } else {
                    "roughness"
                },
                0.48,
            ),
            Some("two_lobe_phong") => format!(
                "max(0.01,1.01-{}/200)",
                surface.scalar("normal_specular_power", 10.)
            ),
            _ => "0".into(),
        };
        format!("max(({roughness}+({offset}-0.5)*2)*4,0)")
    } else {
        format!("max(({offset}-0.5)*8,0)")
    };
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
        "Water reflection retains all source mips and HDR alpha for the source sun-spot calculation. Underwater state remains untranslated."
    } else if dynamic {
        "Dynamic environment reflection samples the cube of the surface's cluster, decoded as 64 * rgb^2 * a, without Reach's blend toward the cluster's second cube. Cube orientation and the native BRDF still require gameplay comparison."
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
