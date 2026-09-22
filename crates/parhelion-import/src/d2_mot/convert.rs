use crate::d2_mot::{
    payload::Payload,
    reader::{outside, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};
pub fn split(positions: &[u8], uvs: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    split_mapped(positions, uvs, &[0])
}
/// Fully weighted vertices store their bone selector in position W in both eras.
/// A complete explicit map is mandatory; weighted/flagged selectors are rejected.
pub(crate) fn split_mapped(
    positions: &[u8],
    uvs: &[u8],
    bones: &[u16],
) -> Result<(Vec<u8>, Vec<u8>)> {
    ensure!(
        positions.len() % 24 == 0 && uvs.len() == positions.len() / 24 * 4,
        "packed vertex counts differ"
    );
    let mut p = Vec::new();
    let mut a = Vec::new();
    for (v, uv) in positions.chunks_exact(24).zip(uvs.chunks_exact(4)) {
        let source = u16::from_le_bytes(v[6..8].try_into()?);
        ensure!(
            source < 0x800,
            "weighted/flagged bone selector requires a skinning converter"
        );
        let target = *bones
            .get(source as usize)
            .context("bone remapping required; source selector is not mapped")?;
        ensure!(
            target < 0x800,
            "target bone selector outside rigid encoding"
        );
        p.extend_from_slice(&v[..8]);
        let offset = p.len() - 2;
        p[offset..].copy_from_slice(&target.to_le_bytes());
        a.extend_from_slice(uv);
        a.extend_from_slice(&v[8..]);
    }
    Ok((p, a))
}
fn header(data: &[u8], stride: u16) -> Result<Vec<u8>> {
    ensure!(data.len() % stride as usize == 0, "invalid stride");
    let mut h = vec![];
    h.extend_from_slice(&u32::try_from(data.len())?.to_le_bytes());
    h.extend_from_slice(&stride.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    h.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
    Ok(h)
}
fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub fn convert_mapped(
    source: &Path,
    native: &Path,
    output: &Path,
    index: usize,
    plated: bool,
    bones: Option<&[u16]>,
) -> Result<Value> {
    let output = outside(&outside(output, source)?, native)?;
    let report = read_json(&source.join("report.json"))?;
    let provenance = read_json(&source.join("source-manifest.json"))?;
    let template = read_json(&native.join("template-report.json"))?;
    let modern = report["models"]
        .as_array()
        .context("models missing")?
        .get(index)
        .context("model index outside report")?;
    let tag = modern["model"].as_str().context("model tag")?;
    let model = Payload(fs::read(source.join("raw").join(format!("{tag}.bin")))?);
    let meshes = model.array(16, 128, Some(0x80806EC5))?;
    ensure!(meshes.len() == 1, "expected single mesh");
    let mesh = meshes[0];
    let buffer = |offset| -> Result<Vec<u8>> {
        let h = model.u32(mesh + offset)?;
        let reference = provenance["tags"][format!("{h:08X}")]["reference"]
            .as_u64()
            .context("buffer provenance missing")?;
        Ok(fs::read(
            source.join("raw").join(format!("{reference:08X}.bin")),
        )?)
    };
    let source_positions = buffer(0)?;
    let auxiliary = if source_positions
        .chunks_exact(24)
        .any(|v| crate::d2_mot::skinning::selector(v).is_ok_and(crate::d2_mot::skinning::weighted))
    {
        buffer(24)?
    } else {
        Vec::new()
    };
    let (positions, attributes) = match bones {
        Some(bones) => split_mapped(
            &crate::d2_mot::skinning::carrier_positions(&source_positions, &auxiliary)?,
            &buffer(4)?,
            bones,
        )?,
        None => split(&buffer(0)?, &buffer(4)?)?,
    };
    let native_stride = if bones.is_some() && plated { 24 } else { 20 };
    let mut compatible = None;
    for owner in &crate::d2_mot::mapping::carrier_candidates(&template) {
        for m in owner["meshes"].as_array().context("native meshes")? {
            let mut p = false;
            let mut a = false;
            for b in m["buffers"].as_array().context("native buffers")? {
                let h = Payload(hex::decode(b["bytes"].as_str().context("header bytes")?)?);
                if h.i16(6)? == 0 {
                    p |= b["offset"] == 0 && h.i16(4)? == 8;
                    a |= b["offset"] == 4 && h.i16(4)? == native_stride;
                }
            }
            if p && a && compatible.is_none() {
                compatible = Some(m.clone());
            }
        }
    }
    let compatible = compatible.context("no compatible native position/attribute template")?;
    let ih_tag = model.u32(mesh + 16)?;
    let ih = Payload(fs::read(
        source.join("raw").join(format!("{ih_tag:08X}.bin")),
    )?);
    let indices = buffer(16)?;
    ensure!(
        ih.u8(1)? == 0 && ih.u32(8)? as usize == indices.len() && indices.len() % 2 == 0,
        "expected 16-bit index payload"
    );
    let count = positions.len() / 8;
    // Validate every LOD and render pass, not just the OBJ's LOD0 selection.
    let mut part_plan = vec![];
    for part in model.array(mesh + 32, 36, Some(0x80806ECB))? {
        let start = model.u32(part + 8)? as usize;
        let len = model.u32(part + 12)? as usize;
        let primitive = model.u16(part + 6)?;
        ensure!(primitive == 5, "unsupported primitive");
        let slice = indices
            .get(start * 2..(start + len) * 2)
            .context("part exceeds indices")?;
        let decoded = slice
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]) as u32)
            .collect::<Vec<_>>();
        let faces = crate::d2_mot::geometry::triangles(&decoded, count, 65535)?;
        part_plan.push(json!({"source_offset":part,"material":format!("{:08X}",model.u32(part)?),"index_offset":start,"index_count":len,"primitive":primitive,"lod":model.u8(part+29)?,"triangles":faces.len(),"mapping":"see mapping.parts; compute-only records are removed"}));
    }
    fs::create_dir_all(&output)?;
    for (name, data) in [
        ("positions.header.bin", header(&positions, 8)?),
        ("attributes.header.bin", header(&attributes, 20)?),
        ("positions.bin", positions),
        ("attributes.bin", attributes),
        ("indices.bin", indices),
        ("indices.header.bin", ih.0),
    ] {
        fs::write(output.join(name), data)?;
    }
    let mut mapping = if plated {
        crate::d2_mot::mapping::map_plated_stride(
            source,
            native,
            &output,
            &model,
            mesh,
            &template,
            native_stride,
        )?
    } else {
        crate::d2_mot::mapping::map(source, native, &output, &model, mesh, &template)?
    };
    if plated {
        crate::d2_mot::plated::convert(source, native, &output, &model, mesh)?;
        mapping = read_json(&output.join("mapping.json"))?;
    }
    let result = json!({"source_model":tag,"native_template_mesh":compatible,"vertices":count,"native_strides":[8,if plated{24}else{20}],"indices_preserved":!plated,"plated":plated,"model_transform_bytes":hex::encode(model.0.get(0x50..0x80).context("model transforms missing")?),"parts":part_plan,"mapping":mapping,"installable":false,"remaining":["in-game material and equipped-model verification","modern transparent effect mapping"]});
    write_json(&output.join("conversion.json"), &result)?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapped_bones_preserve_geometry_and_reject_missing_or_weighted_selectors() {
        let mut vertices = vec![0; 48];
        vertices[6..8].copy_from_slice(&1u16.to_le_bytes());
        vertices[30..32].copy_from_slice(&4u16.to_le_bytes());
        vertices[0..6].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        let uv = [7; 8];
        let (positions, attributes) = split_mapped(&vertices, &uv, &[0, 3, 2, 1, 4]).unwrap();
        assert_eq!(&positions[..6], &vertices[..6]);
        assert_eq!(&positions[6..8], &3u16.to_le_bytes());
        assert_eq!(&positions[14..16], &4u16.to_le_bytes());
        assert_eq!(&attributes[..4], &uv[..4]);
        assert!(split_mapped(&vertices, &uv, &[0, 1]).is_err());
        vertices[6..8].copy_from_slice(&0x800u16.to_le_bytes());
        assert!(split_mapped(&vertices, &uv, &[0, 1, 2, 3, 4]).is_err());
    }
    #[test]
    fn stream_roundtrip() {
        let mut v = (0..48u8).collect::<Vec<_>>();
        v[6..8].fill(0);
        v[30..32].fill(0);
        let uv = [91, 92, 93, 94, 95, 96, 97, 98];
        let (p, a) = split(&v, &uv).unwrap();
        let mut restored = vec![];
        let mut restored_uv = vec![];
        for (p, a) in p.chunks_exact(8).zip(a.chunks_exact(20)) {
            restored.extend_from_slice(p);
            restored.extend_from_slice(&a[4..]);
            restored_uv.extend_from_slice(&a[..4]);
        }
        assert_eq!(restored, v);
        assert_eq!(restored_uv, uv);
        v[6] = 1;
        assert!(split(&v, &uv).is_err());
        assert!(split(&[0; 23], &[]).is_err());
    }
}
