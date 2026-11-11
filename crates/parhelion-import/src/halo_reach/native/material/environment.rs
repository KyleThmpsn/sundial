//! Directional source environment sampling for authored material reflections.
use super::*;

/// Reach's dynamic cubemap scale. The DX11 consumer filters encoded RGBA before multiplying the
/// sample by four and computing RGB squared times alpha. Applying this nonlinear decode to each
/// texel before filtering changes both bilinear and fractional-mip samples.
const DYNAMIC_SCALE: f32 = 64.;

/// A surface's environment map, bound past its other textures and clear of `reserved`, and its
/// reflection sample. A `per_pixel` map is its own cube with alpha kept for the HDR scale. A
/// `dynamic` map is the cube Reach binds for the surface, decoded to `64 * rgb^2 * a`,
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
        let pixels = cube
            .mips
            .iter()
            .map(|faces| {
                faces.each_ref().map(|face| {
                    face.chunks_exact(4)
                        .map(|texel| {
                            let unit = |lane: usize| f32::from(texel[lane]) / 255.;
                            std::array::from_fn(|lane| {
                                if gamma && lane < 3 {
                                    unit(lane).powf(2.2)
                                } else {
                                    unit(lane)
                                }
                            })
                        })
                        .collect::<Vec<[f32; 4]>>()
                })
            })
            .collect::<Vec<_>>();
        cube_mips(native, graph, &symbol, cube.size, &pixels)?;
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
        "Dynamic environment reflection filters the encoded cube before decoding the sample as 64 * rgb^2 * a. Reach blends toward a second cube only on probe-lit objects. Cube orientation and the native BRDF still require gameplay comparison."
    } else {
        "Environment reflection retains all declared source mip levels and multiplies sampled RGB by sampled alpha. Cube orientation and the native BRDF still require gameplay comparison."
    }.into());
    let decode = if dynamic {
        format!("{DYNAMIC_SCALE:e}*value.rgb*value.rgb*value.a")
    } else {
        "value.rgb*value.a".into()
    };
    Ok((
        format!(
            "TextureCube<float4> ReachEnvironment:register(t{register});float3 reachEnvironment(float3 direction,float lod){{float4 value=ReachEnvironment.SampleLevel(ClampSampler,direction,lod);return {decode};}}"
        ),
        Some(sample),
    ))
}
