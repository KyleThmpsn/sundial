//! MCC mux surfaces select discrete texture-array layers through a material map's alpha.
use super::*;

const MATERIAL_SLOT: usize = 10;

pub(super) fn is(material: &Material) -> bool {
    material.tag.as_ref().is_some_and(|tag| tag.group == "rmmx")
}

fn validate(material: &Material) -> Result<()> {
    for (category, expected) in [
        ("blending", "standard"),
        ("albedo", "base_and_detail"),
        ("bump", "base_only"),
        ("materials", "diffuse_only"),
        ("environment_mapping", "none"),
        ("parallax", "off"),
        ("wetness", "default"),
    ] {
        ensure!(
            material.options.get(category).map(String::as_str) == Some(expected),
            "Unsupported mux {category} option {:?} in {}",
            material.options.get(category),
            material.tag.as_ref().map_or("unnamed", |tag| &tag.path)
        );
    }
    Ok(())
}

pub(super) fn textures(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    material: &Material,
) -> Result<TextureBindings> {
    validate(material)?;
    let mut result = TextureBindings {
        notes: vec!["Mux surfaces retain dry albedo and normals. Source weather wetness inputs remain unbound.".into()],
        bindings: Vec::new(),
        samples: BTreeMap::new(),
        samplers: vec![[WRAP, WRAP, ANISOTROPIC]],
        registers: BTreeMap::new(),
        table: None,
        functions: String::new(),
    };
    for (slot, role, normal, color) in [
        (MATERIAL_SLOT, "material_map", false, false),
        (0, "base_map", false, true),
        (1, "bump_map", true, false),
        (5, "detail_map", false, true),
    ] {
        let mapping = material
            .mapping(&[role])
            .with_context(|| format!("Mux has no {role}"))?;
        bind(
            c,
            pages,
            native,
            graph,
            material,
            mapping,
            slot,
            normal,
            color,
            &mut result,
        )
        .with_context(|| {
            format!(
                "Mux {role} in {}",
                material.tag.as_ref().map_or("unnamed", |tag| &tag.path)
            )
        })?;
    }
    if let Some(mapping) = material.mapping(&["g_sample_vmf_diffuse"]) {
        let register = result.bindings.len();
        result.bindings.push((
            register,
            texture(c, pages, native, graph, mapping, false, false)?,
        ));
        result.registers.insert(VMF_TABLE_SLOT, register);
        let index = sampler_index(&mut result.samplers, sampler_modes(mapping)?);
        result.table = Some(sampler_name(index));
    }
    ensure!(
        result.bindings.len() <= MAX_TEXTURE_REGISTERS,
        "Mux exceeds texture binding capacity"
    );
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn bind(
    c: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    material: &Material,
    mapping: &material::Mapping,
    slot: usize,
    normal: bool,
    color: bool,
    result: &mut TextureBindings,
) -> Result<()> {
    let tag = mapping.bitmap.as_ref().context("Mux bitmap is absent")?;
    ensure!(
        !mapping.functions.iter().any(|function| function.kind == 7),
        "Mux animated image selection is not supported"
    );
    let frame = mapping.static_frame(c)?;
    let row = *c
        .block(tag.address()? + 124, 56)?
        .get(frame)
        .context("Mux image outside bitmap table")?;
    let kind = c.u8(row + 6)?;
    let index = sampler_index(&mut result.samplers, sampler_modes(mapping)?);
    let sampler = sampler_name(index);
    let sample = sample(
        c,
        material,
        slot,
        mapping,
        &sampler,
        true,
        &mut result.notes,
    )?;
    let (sample, bc5) = if kind == 0 {
        let (symbol, bc5) = bound_texture(c, pages, native, graph, mapping, normal, color)?;
        let register = result.bindings.len();
        result.bindings.push((register, symbol));
        result.registers.insert(slot, register);
        (sample, bc5)
    } else {
        ensure!(slot != MATERIAL_SLOT, "Mux material map must be 2D");
        let array = material::array::read(c, pages, tag, frame)
            .with_context(|| format!("Bitmap {} image {frame}", tag.path))?;
        ensure!(
            normal == matches!(array.format, 83 | 84),
            "Mux array format {} does not match its color or normal role",
            array.format
        );
        let count = array.layers.len();
        ensure!(
            result.bindings.len() + count <= MAX_TEXTURE_REGISTERS,
            "Mux array exceeds texture binding capacity"
        );
        let format = array.format + u32::from(color && array.curve != LINEAR_CURVE);
        let mut cases = String::new();
        for (layer, bytes) in array.layers.iter().enumerate() {
            let symbol = format!("reach-array-{:08x}-{frame}-{layer}-{format}", tag.datum);
            if !graph.contains(&symbol) {
                texture_data(
                    native,
                    graph,
                    &symbol,
                    [array.width, array.height],
                    format,
                    array.levels,
                    bytes,
                )?;
            }
            let register = result.bindings.len();
            result.bindings.push((register, symbol));
            result.functions.push_str(&format!(
                "Texture2D<float4> Mux{slot}Layer{layer}:register(t{register});\n"
            ));
            cases.push_str(&format!(
                "case {layer}:return Mux{slot}Layer{layer}.SampleGrad(s,uv,dx,dy);\n"
            ));
        }
        // D3D11 Texture2DArray rounds the array coordinate to nearest even, then clamps it.
        // Gradients depend only on UV. Compute them before the potentially divergent switch.
        result.functions.push_str(&format!(
            "float4 mux{slot}(SamplerState s,float2 uv,float cursor){{float2 dx=ddx(uv),dy=ddy(uv);uint layer=min((uint)round(cursor*{count}.0),{last});switch(layer){{{cases}}}return 0;}}\n",
            last=count-1
        ));
        let (_, _, uv) = surface::sample_parts(&sample).context("Mux sample coordinates")?;
        (
            format!("mux{slot}({sampler},{uv},muxCursor)"),
            normal.then_some(array.format == 84),
        )
    };
    let sample = match bc5 {
        Some(true) => format!("reachNormal(({sample}).rg)"),
        Some(false) => format!("reachNormal(({sample}).rg*2-1)"),
        None => sample,
    };
    result.samples.insert(slot, sample);
    Ok(())
}

pub(super) fn parameters(
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> String {
    if !is(surface.material) {
        return String::new();
    }
    let map = &samples[&MATERIAL_SLOT];
    let scale = surface.scalar("blend_material_scale", 1.);
    let offset = surface.scalar("blend_material_offset", 0.);
    format!("float muxCursor=frac(({map}).a*{scale}+{offset});")
}

pub(super) fn albedo(samples: &BTreeMap<usize, String>) -> Result<String> {
    let base = samples.get(&0).context("Mux base sample")?;
    let detail = samples.get(&5).context("Mux detail sample")?;
    Ok(format!(
        "float4(({base}).rgb*({detail}).rgb*{DETAIL_MULTIPLIER:.6},0)"
    ))
}
