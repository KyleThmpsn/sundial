//! Reach water wave sampling, retaining the source signed slopes and authored clocks.
use super::*;

pub(super) fn painted_foam(material: &Material) -> bool {
    matches!(
        material.options.get("foam").map(String::as_str),
        Some("paint" | "both")
    )
}

pub(super) fn fresnel(surface: &surface::Surface<'_>) -> String {
    let coefficient = surface.scalar("fresnel_coefficient", 1.);
    let dark = surface.scalar("fresnel_dark_spot", 1.);
    format!("saturate({coefficient}*pow(saturate({dark}-saturate(dot(n,normalize(v5.xyz)))),2))")
}

/// The MCC above-water program decodes its reflection's HDR alpha separately from surface
/// lighting. Scene-depth refraction, ripple buffers and underwater camera state are not bound.
pub(super) fn lighting(
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
    environment: Option<&str>,
    level: f32,
) -> String {
    let environment = environment.unwrap_or("float4(0,0,0,0)");
    let cut = surface.scalar("sunspot_cut", 1.);
    let shadow = surface.scalar("shadow_intensity_mark", 0.);
    let reflection = surface.scalar("reflection_coefficient", 1.);
    let diffuse = surface.color("water_diffuse", [0.; 4]);
    let fresnel = fresnel(surface);
    let foam = foam(surface, samples, level);
    format!(
        "float3 waterLight=lightLobeIrradiance*{level:e}+lightAmbient.a;float4 waterEnvironment={environment};float3 waterShadow=saturate(waterLight-{shadow});float waterSunAlpha=min(waterEnvironment.a,{cut})+saturate(waterEnvironment.a-{cut})*dot(waterShadow,waterShadow);float3 waterReflection=waterEnvironment.rgb*(256*waterSunAlpha*{reflection});albedo.rgb=(lerp(albedo.rgb*waterLight,waterReflection,{fresnel})+{diffuse}*saturate(n.z)*waterLight)/{level:e};{foam}"
    )
}

/// Painted foam takes the shape map's blue channel and samples its two maps on wave UVs.
/// Its source distance fade interpolates reciprocal distance from the vertices. Convert the
/// final scene-color blend to premultiplied coverage while depth refraction remains unavailable.
fn foam(surface: &surface::Surface<'_>, samples: &BTreeMap<usize, String>, level: f32) -> String {
    // The combined source permutation takes the maximum of painted and automatic foam.
    // Retain its painted contribution while displacement and interaction foam are unavailable.
    if !painted_foam(surface.material) {
        return String::new();
    }
    let (Some(shape), Some(base), Some(detail)) = (
        samples.get(&WATER_SHAPE_SLOT),
        samples.get(&WATER_FOAM_SLOT),
        samples.get(&WATER_FOAM_DETAIL_SLOT),
    ) else {
        return String::new();
    };
    let coefficient = surface.scalar("foam_coefficient", 0.);
    format!(
        "float waterFoamAmount=({shape}).b*{coefficient}*saturate(v9.z*20);float4 waterFoam=0;if(waterFoamAmount>0.002){{waterFoam=({base})*({detail});waterFoamAmount*=waterFoam.a;}}float waterCoverage=lerp(albedo.a,1,waterFoamAmount);albedo.rgb=lerp(albedo.rgb*albedo.a,waterFoam.rgb*waterLight/{level:e},waterFoamAmount)/max(waterCoverage,0.000001);albedo.a=waterCoverage;"
    )
}

/// Separate native 2D resources avoid interpreting a sixteen-slice array as a native cubemap.
/// Each resource retains its original BC5_SNORM bytes and mip levels.
#[allow(clippy::too_many_arguments)]
pub(super) fn waves(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    material: &Material,
    surface: &surface::Surface<'_>,
    bindings: &mut Vec<(usize, String)>,
) -> Result<(String, String)> {
    let mapping = material
        .properties
        .iter()
        .flat_map(|p| &p.textures)
        .find(|mapping| mapping.role == "wave_slope_array" && mapping.bitmap.is_some())
        .context("Water has no source wave slope array")?;
    let tag = mapping.bitmap.as_ref().context("Wave bitmap")?;
    let frame = mapping.static_frame(c)?;
    let slopes = material::slopes(c, pages, tag, frame)?;
    let first = bindings.iter().map(|(slot, _)| slot + 1).max().unwrap_or(0);
    let count = slopes.frames.len();
    ensure!(
        first + count <= MAX_TEXTURE_REGISTERS,
        "Water wave array exceeds texture binding capacity"
    );
    let mut declarations = String::new();
    let mut cases = String::new();
    for (index, bytes) in slopes.frames.iter().enumerate() {
        let symbol = format!("reach-wave-{:08x}-{frame}-{index}", tag.datum);
        if !graph.nodes.iter().any(|node| node["symbol"] == symbol) {
            let template = 0x80ec31bc;
            let mut header = native.tag(template, None)?.0.clone();
            put(&mut header, 0, &u32::try_from(bytes.len())?.to_le_bytes())?;
            put(&mut header, 4, &84u32.to_le_bytes())?;
            put(&mut header, 14, &u16::try_from(slopes.width)?.to_le_bytes())?;
            put(
                &mut header,
                16,
                &u16::try_from(slopes.height)?.to_le_bytes(),
            )?;
            put(&mut header, 18, &1u16.to_le_bytes())?;
            put(&mut header, 20, &1u16.to_le_bytes())?;
            header[23] = slopes.levels;
            put(&mut header, 36, &u32::MAX.to_le_bytes())?;
            crate::tiger::texture::resident(&mut header, bytes.len())?;
            let data = format!("{symbol}-data");
            graph.add(&symbol, template, &header, Some(&data), vec![])?;
            graph.add(
                &data,
                native.reference(template)?,
                bytes,
                Some(&symbol),
                vec![],
            )?;
        }
        let register = first + index;
        bindings.push((register, symbol));
        declarations.push_str(&format!(
            "Texture2D<float2> Wave{index}:register(t{register});\n"
        ));
        cases.push_str(&format!(
            "case {index}:return Wave{index}.SampleGrad(SurfaceSampler,uv,dx,dy);\n"
        ));
    }
    declarations.push_str(&format!(r#"
float2 reachWaveFrame(uint frame,float2 uv,float2 dx,float2 dy) {{
    switch(frame) {{ {cases} }} return 0;
}}
float2 reachWave(float2 uv,float phase) {{
    float cursor=frac(phase)*{count}.0;
    uint current=(uint)floor(cursor);
    float2 dx=ddx(uv),dy=ddy(uv);
    return lerp(reachWaveFrame(current,uv,dx,dy),reachWaveFrame((current+1)%{count},uv,dx,dy),frac(cursor));
}}
"#));
    let main = surface.value("wave_displacement_array", [1., 1., 0., 0.]);
    let aux = surface.value("wave_slope_array", [1., 1., 0., 0.]);
    let phase = surface.scalar("time_warp", 0.);
    let phase_aux = surface.scalar("time_warp_aux", 0.);
    let detail_x = surface.scalar("detail_slope_scale_x", 1.);
    let detail_y = surface.scalar("detail_slope_scale_y", 1.);
    let detail_z = surface.scalar("detail_slope_scale_z", 1.);
    let detail = surface.scalar("detail_slope_steepness", 0.);
    let slope = surface.scalar("slope_scaler", 1.);
    let xy = format!(
        "(reachWave(v3.xy*({main}).xy+({main}).zw,{phase})+reachWave(v3.xy*({aux}).xy+({aux}).zw,{phase_aux})+reachWave(v3.xy*({main}).xy*float2({detail_x},{detail_y})+({main}).zw,{phase}*{detail_z})*{detail})*{slope}"
    );
    Ok((
        declarations,
        format!("float4(normalize(float3({xy},1))*0.5+0.5,1)"),
    ))
}
