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

/// Whether `material` refracts the scene behind it, Reach's `refraction` option `dynamic`.
pub(super) fn refracts(material: &Material) -> bool {
    material.water()
        && material
            .options
            .get("refraction")
            .is_some_and(|option| option == "dynamic")
}

/// The transparent scope's scene inputs refracting water reads ([`SCENE_REGISTERS`]): the scene
/// depth with its linearization `1 / (depth * CB2[0].y + CB2[0].x)`, as native forward program
/// `80B38506` and the native particle programs take it, the completed opaque scene color, and the
/// view row `CB12[12]`, the full-size target's width and height and their reciprocals, which
/// native transparent programs scale screen positions by. The depth texel follows the screen
/// coordinate over the depth texture's own size, which is the same texel wherever the texture
/// matches the target. `80B38506` takes the target's size. This does not establish that a converted
/// water draw receives the same resources. Refraction keeps a separate dimension guard.
pub(super) const SCENE_DECLARATIONS: &str = r#"
Texture2D<float4> SceneDepth:register(t10);
Texture2D<float4> SceneColor:register(t18);
cbuffer SceneDepthConstants:register(b2){float4 depthData[1];};
cbuffer View:register(b12){float4 viewData[14];};
float reachSceneDepth(float depth){return 1/max(depth*depthData[0].y+depthData[0].x,0.000001);}
float reachSceneDepthValue(float2 uv){
uint width,height;SceneDepth.GetDimensions(width,height);int2 size=int2(width,height);
return SceneDepth.Load(int3(clamp(int2(saturate(uv)*float2(size)),int2(0,0),size-1),0)).x;
}
bool reachScenePoint(float2 uv,float depth,out float3 relative){
float3 ndc=float3(uv*float2(2,-2)+float2(-1,1),depth);
float3 w=float3(viewData[0].w,viewData[1].w,viewData[2].w);
float3 x=float3(viewData[0].x,viewData[1].x,viewData[2].x)-ndc.x*w;
float3 y=float3(viewData[0].y,viewData[1].y,viewData[2].y)-ndc.y*w;
float3 z=float3(viewData[0].z,viewData[1].z,viewData[2].z)-ndc.z*w;
float3 b=ndc*viewData[13].w-viewData[13].xyz;
float3 yz=cross(y,z);float determinant=dot(x,yz);
relative=0;
if(abs(determinant)<=1e-20||!isfinite(determinant))return false;
relative=(b.x*yz+b.y*cross(z,x)+b.z*cross(x,y))/determinant;
return all(isfinite(relative));
}
"#;

/// The end of a refracting water surface's output. Its premultiplied color and coverage cover the
/// scene behind it, and the refracted scene replaces the unrefracted one by the share left
/// uncovered, so the blend leaves `water + refracted * (1 - coverage)`.
pub(super) const REFRACTED_FINISH: &str = "o0.rgb+=(waterRefracted-waterBehind)*(1-o0.a);";

/// The MCC above-water program decodes its reflection's HDR alpha separately from surface
/// lighting. Ripple buffers and underwater camera state are not bound.
///
/// Refracting water follows `water_shading.fx` with `refraction = dynamic`. It offsets its screen
/// position by its tangent slope times `refraction_texcoord_shift`, `saturate(2 / distance)` and
/// its bank alpha, and keeps the offset only where the scene there lies behind the water. Its
/// transparency is `saturate(exp2(murkiness * h)) * saturate(refraction_extinct_distance /
/// distance)`, with `h` the refracted point's height less the water's and the distance in Reach
/// units. The body color lit by the baked light blends toward the refracted scene by the
/// transparency, the reflection blends over that by Fresnel, the diffuse term adds, and the bank
/// alpha blends the result back toward the scene. The refracted point is reconstructed at the
/// offset coordinate through the same camera-relative projection the vertex program uses, from
/// view rows 0, 1, 2 and 13. A singular projection or infinite ray keeps the ground-depth fallback.
/// Water without refraction keeps its coverage approximation ([`super::water_albedo`]).
///
/// Refraction runs only where the scene depth and scene color textures both match the view's
/// target size in `CB12[12]`, the size stock transparent statics such as `80B9F0A1` take their
/// depth texel at. Elsewhere the textures are not this view's scene, and `h` is instead the depth
/// of the ground below the water's vertices, interpolated from the vertex data (`v9.w`, written by
/// [`super::vertex_data`] from the level's ground), with no refraction offset. A vertex without that
/// depth draws as deep water, where `exp2(murkiness * h)` has fallen to zero: its body color under
/// the Fresnel reflection, covering the scene but for the bank alpha. At the default murkiness of
/// 8, Reach's water lets less than 3 percent through below two metres. Matching dimensions alone
/// do not establish the resource's identity. Native scene-resource capture remains necessary.
/// How many times murkier water draws when it fades by the ground depth instead of refracting.
/// Reach's refraction offsets the scene by the waves, so even clear shallows read as water.
/// The threefold fallback factor is a visual approximation for the missing distortion. It keeps
/// the water body legible while fading to zero at the shore, rather than matching source optics.
const FALLBACK_MURKINESS: f32 = 3.;

pub(super) fn lighting(
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
    environment: Option<&str>,
    level: f32,
    refracts: bool,
) -> String {
    let environment = environment.unwrap_or("float4(0,0,0,0)");
    let cut = surface.scalar("sunspot_cut", 1.);
    let shadow = surface.scalar("shadow_intensity_mark", 0.);
    let reflection = surface.scalar("reflection_coefficient", 1.);
    let diffuse = surface.color("water_diffuse", [0.; 4]);
    let fresnel = fresnel(surface);
    let foam = foam(surface, samples, level);
    let light = format!(
        "float3 waterLight=lightLobeIrradiance*{level:e}+lightAmbient.a;float4 waterEnvironment={environment};float3 waterShadow=saturate(waterLight-{shadow});float waterSunAlpha=min(waterEnvironment.a,{cut})+saturate(waterEnvironment.a-{cut})*dot(waterShadow,waterShadow);float3 waterReflection=waterEnvironment.rgb*(256*waterSunAlpha*{reflection});"
    );
    if !refracts {
        return format!(
            "{light}albedo.rgb=(lerp(albedo.rgb*waterLight,waterReflection,{fresnel})+{diffuse}*saturate(n.z)*waterLight)/{level:e};{foam}"
        );
    }
    let shift = surface.scalar("refraction_texcoord_shift", 0.);
    let murkiness = surface.scalar("water_murkiness", 8.);
    let extinction = surface.scalar("refraction_extinct_distance", 1.);
    format!(
        "{light}float waterFresnel={fresnel};float waterBank=saturate(albedo.a);uint waterDepthWidth,waterDepthHeight,waterColorWidth,waterColorHeight;SceneDepth.GetDimensions(waterDepthWidth,waterDepthHeight);SceneColor.GetDimensions(waterColorWidth,waterColorHeight);bool waterScene=all(abs(float4(waterDepthWidth,waterDepthHeight,waterColorWidth,waterColorHeight)-viewData[12].xyxy)<1.5);float3 waterBehind=0;float3 waterRefracted=0;float2 waterScreen=position.xy*viewData[12].zw;float2 waterUv=waterScreen;float waterHeight=v9.w>1.5?-(v9.w-2)/3.048*{FALLBACK_MURKINESS:.1}:-1000;\
if(waterScene){{float3 waterTangent=float3(dot(n,v1.xyz),dot(n,v2.xyz),dot(n,v0.xyz));waterUv=saturate(waterScreen+waterTangent.xy/max(waterTangent.z,0.01)*{shift}*saturate(2*v9.z)*waterBank);float waterSurface=reachSceneDepth(position.z);float waterDepth=reachSceneDepthValue(waterUv);if(reachSceneDepth(waterDepth)<=waterSurface){{waterUv=waterScreen;waterDepth=reachSceneDepthValue(waterScreen);}}float3 waterPoint;if(reachScenePoint(waterUv,waterDepth,waterPoint)){{waterHeight=(waterPoint.z+v5.z)/3.048;}}}}\
float waterClear=saturate(exp2({murkiness}*waterHeight))*saturate({extinction}*v9.z);float waterSeen=waterClear*(1-waterFresnel)*waterBank+1-waterBank;float3 waterColor=(albedo.rgb*waterLight*(1-waterClear)*(1-waterFresnel)+waterReflection*waterFresnel+{diffuse}*saturate(n.z)*waterLight)*waterBank/{level:e};albedo=float4(waterColor/max(1-waterSeen,0.00001),1-waterSeen);\
if(waterScene){{waterBehind=SceneColor.SampleLevel(ClampSampler,waterScreen,0).rgb;waterRefracted=SceneColor.SampleLevel(ClampSampler,waterUv,0).rgb;}}{foam}"
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
/// Each resource retains its original BC5_SNORM bytes and mip levels, and binds to the next
/// register clear of `reserved`.
#[allow(clippy::too_many_arguments)]
pub(super) fn waves(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    material: &Material,
    surface: &surface::Surface<'_>,
    bindings: &mut Vec<(usize, String)>,
    reserved: &[usize],
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
    let count = slopes.frames.len();
    let mut declarations = String::new();
    let mut cases = String::new();
    for (index, bytes) in slopes.frames.iter().enumerate() {
        let symbol = format!("reach-wave-{:08x}-{frame}-{index}", tag.datum);
        if !graph.contains(&symbol) {
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
        let register = free_registers(bindings, 1, reserved);
        ensure!(
            register < MAX_TEXTURE_REGISTERS,
            "Water wave array exceeds texture binding capacity"
        );
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
