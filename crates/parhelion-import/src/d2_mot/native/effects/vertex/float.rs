//! Adapt the source's float cloth fallback to the native four-influence stream.
use super::*;

/// Physical cloth reads the source float stream or the native solver's output
/// directly. Its positions never pass through the packed geometry adapter.
pub(in crate::d2_mot::native::effects) fn physical(
    model: &Payload,
    source: &str,
    bones: usize,
    simulated: bool,
) -> Result<String> {
    let count = inputs::cb_count(source, 1)?.context("Cloth model constants")?;
    ensure!(
        (simulated && matches!(count, 7 | 8))
            || (!simulated && (24..=256).contains(&count) && count.is_multiple_of(3)),
        "Physical cloth model scope differs"
    );
    let signature = if simulated {
        "void main(\n  float4 v0 : POSITION0,\n  float4 v1 : NORMAL0,\n  float4 v2 : TANGENT0,\n  float2 v3 : TEXCOORD0,\n  uint v4 : SV_VERTEXID0,\n"
    } else {
        "void main(\n  float4 v0 : POSITION0,\n  float3 v1 : NORMAL0,\n  float4 v2 : TANGENT0,\n  float2 v3 : TEXCOORD0,\n  float4 v4 : BLENDWEIGHT0,\n  uint4 v5 : BLENDINDICES0,\n  uint v6 : SV_VERTEXID0,\n"
    };
    ensure!(
        source.contains(signature) && (1..=256).contains(&bones),
        "Physical cloth vertex declaration differs"
    );
    let native_count = if simulated {
        8
    } else {
        count.max(8 + bones * 3)
    };
    let mut text = replace_once(
        source,
        &inputs::cb_decl(1, count),
        &inputs::cb_decl(11, native_count),
    )?;
    text = model_reads(&text)?;
    let view = inputs::cb_count(&text, 12)?.context("Cloth view constants")?;
    if simulated {
        ensure!(view == 4, "Simulated cloth view scope differs");
    } else {
        ensure!(view == 16, "Skinned cloth view scope differs");
        text = replace_once(&text, &inputs::cb_decl(12, 16), &inputs::cb_decl(12, 14))?;
        text = text
            .replace("cb12[15].xyz", "(-cb12[7].xyz)")
            .replace("cb12[14].xyzw", "cb12[13].xyzw")
            .replace("cb12[10].xyz", "cb12[7].xyz");
        ensure!(
            inputs::reads(&text, 12).is_some_and(|r| r.iter().all(|i| *i < 14)),
            "Unmapped physical cloth view input"
        );
    }
    let values = (0..12)
        .map(|i| model.f32(0x50 + i * 4))
        .collect::<Result<Vec<_>>>()?;
    let helpers = format!(
        "float4 source_model(uint index) {{ if (index == 4) return float4({:.9},{:.9},{:.9},cb11[4].w); if (index == 5) return float4({:.9},{:.9},{:.9},1); if (index == 6) return float4({:.9},{:.9},{:.9},{:.9}); return cb11[index]; }}\n",
        values[0],
        values[1],
        values[2],
        values[4],
        values[5],
        values[6],
        values[8],
        values[9],
        values[10],
        values[11]
    );
    replace_once(&text, "void main(\n", &format!("{helpers}\nvoid main(\n"))
}

pub(super) fn build(
    c: &Effect,
    draw: &SourceDraw,
    model: &Payload,
    source: String,
) -> Result<Vertex> {
    let count = inputs::cb_count(&source, 1)?.context("float cloth model constants")?;
    ensure!(
        (24..=256).contains(&count) && count.is_multiple_of(3),
        "float cloth bone buffer differs"
    );
    ensure!(
        inputs::cb_count(&source, 12)? == Some(16),
        "float cloth view constants differ"
    );
    let signature = "void main(\n  float4 v0 : POSITION0,\n  float3 v1 : NORMAL0,\n  float4 v2 : TANGENT0,\n  float2 v3 : TEXCOORD0,\n  float4 v4 : BLENDWEIGHT0,\n  uint4 v5 : BLENDINDICES0,\n  uint v6 : SV_VERTEXID0,\n";
    ensure!(
        source.contains(signature),
        "float cloth vertex input contract differs"
    );
    let bones = number(&c.graph.manifest["rig_mapping"]["native_bone_count"])?;
    ensure!(
        bones > 0 && bones <= 256,
        "float cloth native palette differs"
    );
    let mut text = replace_once(
        &source,
        &inputs::cb_decl(1, count),
        &inputs::cb_decl(11, count.max(8 + bones * 3)),
    )?;
    text = model_reads(&text)?;
    text = replace_once(&text, &inputs::cb_decl(12, 16), &inputs::cb_decl(12, 14))?;
    text = text.replace("cb12[15].xyz", "(-cb12[7].xyz)");
    text = text.replace("cb12[14].xyzw", "cb12[13].xyzw");
    text = text.replace("cb12[10].xyz", "cb12[7].xyz");
    ensure!(
        inputs::reads(&text, 12).is_some_and(|reads| reads.iter().all(|i| *i < 14)),
        "unmapped float cloth view input"
    );
    let scale = (0..3)
        .map(|i| model.f32(0x50 + i * 4))
        .collect::<Result<Vec<_>>>()?;
    let offset = (0..3)
        .map(|i| model.f32(0x60 + i * 4))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        scale.iter().all(|v| v.is_finite() && *v > 0.) && offset.iter().all(|v| v.is_finite()),
        "float cloth model transform differs"
    );
    let scale = format!("float3({:.9},{:.9},{:.9})", scale[0], scale[1], scale[2]);
    let offset = format!("float3({:.9},{:.9},{:.9})", offset[0], offset[1], offset[2]);
    let helpers = format!(
        "float4 source_model(uint index) {{ if (index == 4) return float4({scale},cb11[4].w); if (index == 5) return float4({offset},1); if (index == 6) return float4(1,1,0,0); return cb11[index]; }}\n"
    );
    text = replace_once(
        &text,
        signature,
        "void main(\n  float4 nativePosition : POSITION0,\n  float2 nativeUv : TEXCOORD0,\n  float3 nativeNormal : NORMAL0,\n  float4 nativeTangent : TANGENT0,\n  float4 nativeWeights : BLENDWEIGHT0,\n  uint4 nativeIndices : BLENDINDICES0,\n  uint nativeVertex : SV_VERTEXID0,\n",
    )?;
    let ([x, y, w, h], [aw, ah]) = c.atlas(draw.model)?;
    let native = c.graph.read("model")?;
    let uv = (0..4)
        .map(|i| native.f32(0x70 + i * 4))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        uv.iter().all(|v| v.is_finite()),
        "nonfinite native cloth UV transform"
    );
    let locals = format!(
        "uint4 bitmask, uiDest;\n  float4 v0 = float4((nativePosition.xyz * cb11[5].www + cb11[5].xyz - {offset}) / {scale},1);\n  float3 v1 = nativeNormal;\n  float4 v2 = nativeTangent;\n  float2 atlas_uv = nativeUv * float2({:.9},{:.9}) + float2({:.9},{:.9});\n  float2 v3 = (atlas_uv * float2({aw},{ah}) - float2({x},{y})) / float2({w},{h});\n  float4 v4 = nativeWeights;\n  uint4 v5 = nativeIndices;\n  uint v6 = nativeVertex - {}u;",
        uv[0], uv[1], uv[2], uv[3], draw.base
    );
    text = replace_once(&text, "uint4 bitmask, uiDest;", &locals)?;
    text = replace_once(&text, "void main(\n", &format!("{helpers}\nvoid main(\n"))?;
    Ok(Vertex {
        text,
        textures: vec![],
        source,
    })
}
