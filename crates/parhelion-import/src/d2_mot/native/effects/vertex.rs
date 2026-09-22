//! Run the source vertex equations using native geometry and animation bindings.
use super::*;
use crate::d2_mot::native::shader::replace_once;

fn table(
    c: &mut Effect,
    name: &str,
    format: u32,
    stride: usize,
    data: &[u8],
) -> Result<(String, usize)> {
    ensure!(
        !data.is_empty() && data.len().is_multiple_of(stride),
        "invalid vertex lookup table"
    );
    let count = data.len() / stride;
    let width = count.min(1024);
    if c.graph.node(name).is_ok() {
        return Ok((name.to_owned(), width));
    }
    let height = count.div_ceil(width);
    let mut data = data.to_vec();
    data.resize(width * height * stride, 0);
    // These tables are fixed shader resources, so inherit resident texture
    // storage metadata along with the header instead of using a gear plate.
    let mut header = c.graph.read("dye-4-texture-1")?.0;
    put(&mut header, 0, &u32::try_from(data.len())?.to_le_bytes())?;
    put(&mut header, 4, &format.to_le_bytes())?;
    put(&mut header, 14, &u16::try_from(width)?.to_le_bytes())?;
    put(&mut header, 16, &u16::try_from(height)?.to_le_bytes())?;
    put(&mut header, 18, &1u16.to_le_bytes())?;
    put(&mut header, 20, &1u16.to_le_bytes())?;
    header[23] = 1;
    header[22] = u8::try_from(stride * 8)?;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::d2_mot::texture::resident(&mut header, data.len())?;
    let ht = c.graph.node("dye-4-texture-1")?["template"]
        .as_u64()
        .context("texture template")?;
    let dt = c.graph.node("dye-4-texture-1-data")?["template"]
        .as_u64()
        .context("texture data template")?;
    let body = format!("{name}-data");
    c.graph.add(name, ht, &header, Some(&body), vec![])?;
    c.graph.add(&body, dt, &data, Some(name), vec![])?;
    Ok((name.to_owned(), width))
}

fn model_reads(text: &str) -> Result<String> {
    let mut result = String::new();
    let mut remaining = text;
    while let Some((before, after)) = remaining.split_once("cb1[") {
        result.push_str(before);
        let (index, rest) = after.split_once(']').context("model constant index")?;
        ensure!(!index.contains('['), "nested source model constant access");
        result.push_str(&format!("source_model({index})"));
        remaining = rest;
    }
    result.push_str(remaining);
    Ok(result)
}

pub(super) struct Vertex {
    pub text: String,
    pub textures: Vec<(u32, String)>,
    pub source: String,
}

fn metadata_slot(source: &str, code: &[u8]) -> Result<u32> {
    let mut occupied = inputs::slots(source, 't')?;
    for instruction in program::parse(code)? {
        if instruction.op == 0x56 && instruction.args[0] >> 5 == 2 {
            occupied.insert(u32::from(instruction.args[0] & 31));
        }
    }
    (3..32)
        .find(|slot| !occupied.contains(slot))
        .context("source vertex has no free metadata texture slot")
}

pub(super) fn build(c: &mut Effect, draw: &SourceDraw, material: &Payload) -> Result<Vertex> {
    let vs = material.u32(0x70)?;
    let source = fs::read_to_string(
        c.refs
            .join(format!("library-surfaces-01/source-shaders/{vs:08X}.hlsl")),
    )?
    .replace("\r\n", "\n");
    let mut text = source.clone();
    let count = inputs::cb_count(&text, 1)?.context("source skinning buffer")?;
    ensure!(
        (24..=256).contains(&count) && count.is_multiple_of(3),
        "source bone buffer size differs"
    );
    let native_bones = c.graph.manifest["rig_mapping"]["native_bone_count"]
        .as_u64()
        .unwrap_or(0) as usize;
    let native_count = count.max(8 + native_bones * 3);
    text = replace_once(
        &text,
        &inputs::cb_decl(1, count),
        &inputs::cb_decl(11, native_count),
    )?;
    text = model_reads(&text)?;
    ensure!(
        inputs::cb_count(&text, 12)? == Some(16),
        "source vertex view layout differs"
    );
    text = replace_once(&text, &inputs::cb_decl(12, 16), &inputs::cb_decl(12, 14))?;
    text = text.replace("cb12[15].xyz", "(-cb12[7].xyz)");
    text = text.replace("cb12[14].xyzw", "cb12[13].xyzw");
    text = text.replace("cb12[10].xyz", "cb12[7].xyz");
    ensure!(
        inputs::reads(&text, 12).is_some_and(|reads| reads.iter().all(|i| *i < 14)),
        "unmapped source vertex view input"
    );

    let model = c.source.raw(&draw.model_tag)?;
    let meshes = model.array(16, 128, None)?;
    ensure!(meshes.len() == 1, "source vertex model requires one mesh");
    let mesh = meshes[0];
    let positions = c.source.buffer(model.u32(mesh)?)?;
    let vertices = positions.0.len() / 24;
    let normal_meta = (0..vertices)
        .flat_map(|v| {
            positions.0[v * 24 + 14..v * 24 + 16]
                .iter()
                .copied()
                .chain(positions.0[v * 24 + 6..v * 24 + 8].iter().copied())
        })
        .collect::<Vec<_>>();
    let (metadata, width) = table(
        c,
        &format!("source-vertex-{}-metadata", draw.model),
        42,
        4,
        &normal_meta,
    )?;
    let metadata_slot = metadata_slot(&source, &array_bytes(material, 0x90, 1)?)?;
    let mut textures = vec![(metadata_slot, metadata)];
    let mut helpers = format!("Texture2D<uint> sourceMetadata : register(t{metadata_slot});\n");
    let weighted = positions
        .0
        .chunks_exact(24)
        .any(|v| crate::d2_mot::skinning::selector(v).is_ok_and(crate::d2_mot::skinning::weighted));
    ensure!(
        !weighted
            || (text.contains("Buffer<uint4> t1 : register(t1);") && text.contains("0xffffc000")),
        "weighted source vertex shader uses an unknown auxiliary encoding"
    );
    if text.contains("Buffer<uint4> t1 : register(t1);") {
        let aux = c.source.buffer(model.u32(mesh + 24)?)?;
        let data = if weighted {
            let bones = c.graph.manifest["rig_mapping"]["bone_map"]
                .as_array()
                .context("weighted rig mapping")?
                .iter()
                .map(|v| Ok(u16::try_from(v.as_u64().context("weighted bone mapping")?)?))
                .collect::<Result<Vec<_>>>()?;
            crate::d2_mot::skinning::remap(&positions.0, &aux.0, &bones)?
        } else {
            aux.0
        };
        let (symbol, w) = table(
            c,
            &format!("source-vertex-{}-auxiliary", draw.model),
            30,
            4,
            &data,
        )?;
        textures.push((1, symbol));
        text = replace_once(&text, "Buffer<uint4> t1 : register(t1);", "")?;
        helpers.push_str(&format!("Texture2D<uint4> sourceAuxiliary : register(t1);\nuint4 source_auxiliary(uint index) {{ return sourceAuxiliary.Load(int3(index % {w}u, index / {w}u, 0)); }}\n"));
        text = text.replace("t1.Load(", "source_auxiliary(");
    }
    let mut color_count = 0;
    if text.contains("Buffer<float4> t0 : register(t0);") {
        let root = c.refs.join(format!(
            "library-surfaces-01/vertex-colors/{}",
            draw.model_tag
        ));
        let report = load(&root.join("colors.json"))?;
        let data = if report["source_has_color_buffer"] == true {
            color_count = number(&report["count"])?;
            ensure!(
                color_count > 0 && color_count <= vertices,
                "source vertex color count differs"
            );
            let data = fs::read(root.join(format!(
                "raw/{}.bin",
                report["buffer"].as_str().context("source color buffer")?
            )))?;
            ensure!(
                data.len() == color_count * 4,
                "source vertex color bytes differ"
            );
            data
        } else {
            // The source renderer binds color0_fallback when the buffer is absent.
            vec![0, 0, 255, 255]
        };
        let (symbol, w) = table(
            c,
            &format!("source-vertex-{}-colors", draw.model),
            28,
            4,
            &data,
        )?;
        textures.push((0, symbol));
        text = replace_once(&text, "Buffer<float4> t0 : register(t0);", "")?;
        helpers.push_str(&format!("Texture2D<float4> sourceColors : register(t0);\nfloat4 source_colors(uint index) {{ return sourceColors.Load(int3(index % {w}u, index / {w}u, 0)); }}\n"));
        text = text.replace("t0.Load(", "source_colors(");
    }
    let scale = model.f32(0x6C)?;
    let offset = [model.f32(0x60)?, model.f32(0x64)?, model.f32(0x68)?];
    ensure!(
        scale.is_finite()
            && scale > 0.
            && (0..3).all(|i| model
                .f32(0x50 + i * 4)
                .is_ok_and(|v| (v - scale).abs() <= scale * 1e-6)),
        "source vertex scale is not uniform"
    );
    let color_max = color_count.saturating_sub(1);
    helpers.push_str(&format!("float4 source_model(uint index) {{ if (index == 5) return float4({:.9},{:.9},{:.9},{scale:.9}); if (index == 6) return float4(1,1,0,0); float4 v = cb11[index]; if (index == 4) v.w = asfloat({color_max}u); return v; }}\n",offset[0],offset[1],offset[2]));
    let signature_start = text
        .find("void main(\n")
        .context("source vertex entry point")?;
    let outputs = text[signature_start..]
        .find("  out ")
        .context("source vertex output signature")?
        + signature_start;
    text.replace_range(signature_start..outputs,"void main(\n  float4 nativePosition : POSITION0,\n  float2 nativeUv : TEXCOORD0,\n  float3 nativeNormal : NORMAL0,\n  float4 nativeTangent : TANGENT0,\n  uint nativeVertex : SV_VERTEXID0,\n");
    let ([x, y, w, h], [aw, ah]) = c.atlas(draw.model)?;
    let locals = format!(
        "uint4 bitmask, uiDest;\n  uint vertex_index = nativeVertex - {}u;\n  uint normal_word = sourceMetadata.Load(int3(vertex_index % {width}u, vertex_index / {width}u, 0));\n  float4 v0 = float4((nativePosition.xyz * cb11[5].www + cb11[5].xyz - float3({:.9},{:.9},{:.9})) / {scale:.9}, abs((int)normal_word >> 16) >= 2048 ? ((float)((int)normal_word >> 16) / 32767.0) : nativePosition.w);\n  float4 v1 = float4(nativeNormal, max(-1.0, (float)((int)(normal_word << 16) >> 16) / 32767.0));\n  float4 v2 = nativeTangent;\n  float2 atlas_uv = nativeUv * cb11[6].xy + cb11[6].zw;\n  float2 v3 = (atlas_uv * float2({aw},{ah}) - float2({x},{y})) / float2({w},{h});\n  uint v4 = vertex_index;",
        draw.base, offset[0], offset[1], offset[2]
    );
    // Source surface shaders can run without the gear scope that supplies
    // cb11[6]. Use the transform written with this converted model instead.
    let native_model = c.graph.read("model")?;
    let uv = (0..4)
        .map(|i| native_model.f32(0x70 + i * 4))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        uv.iter().all(|v| v.is_finite()),
        "nonfinite native UV transform"
    );
    let locals = replace_once(
        &locals,
        "nativeUv * cb11[6].xy + cb11[6].zw",
        &format!(
            "nativeUv * float2({:.9},{:.9}) + float2({:.9},{:.9})",
            uv[0], uv[1], uv[2], uv[3]
        ),
    )?;
    text = replace_once(&text, "uint4 bitmask, uiDest;", &locals)?;
    text = replace_once(&text, "void main(\n", &format!("{helpers}\nvoid main(\n"))?;
    Ok(Vertex {
        text,
        textures,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_avoids_shader_and_runtime_texture_slots() {
        let source = "Texture2D<float4> t3 : register(t3);";
        assert_eq!(metadata_slot(source, &[]).unwrap(), 4);
        assert_eq!(metadata_slot(source, &[0x5B, 0, 0x56, 0x44]).unwrap(), 5);
        let full = (3..32)
            .map(|slot| format!("register(t{slot})\n"))
            .collect::<String>();
        assert!(metadata_slot(&full, &[]).is_err());
    }
}
