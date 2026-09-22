use crate::d2_mot::{
    bundle::add,
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};
fn tag(v: &Value, field: &str) -> Result<u32> {
    Ok(u32::from_str_radix(v[field].as_str().context("tag")?, 16)?)
}
fn patch(p: &mut [u8], o: usize, s: &str) -> Value {
    p[o..o + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    json!({"offset":o,"symbol":s})
}
// Shared property names: Bungie's 2019 documentation and modern Charm DyeInfo.
const VECTORS: [(usize, usize); 21] = [
    (0, 0),
    (1, 1),
    (2, 2),
    (3, 9),
    (4, 3),
    (5, 10),
    (6, 11),
    (7, 12),
    (8, 17),
    (9, 18),
    (10, 19),
    (11, 20),
    (12, 13),
    (13, 4),
    (14, 14),
    (15, 15),
    (16, 16),
    (17, 21),
    (18, 22),
    (19, 23),
    (20, 24),
];
fn convert_constants(mut constants: Vec<u8>, modern: &Value) -> Result<Vec<u8>> {
    ensure!(
        constants.len() == 432 && modern.as_array().context("constants")?.len() == 21,
        "unsupported dye constants"
    );
    for (from, to) in VECTORS {
        for c in 0..4 {
            let f = modern[from][c].as_f64().context("float")? as f32;
            ensure!(f.is_finite(), "nonfinite dye value");
            constants[to * 16 + c * 4..to * 16 + c * 4 + 4].copy_from_slice(&f.to_le_bytes());
        }
    }
    Ok(constants)
}
pub fn build(r: &mut Reader, source: &Path, native: &Path, graph: &Path) -> Result<Value> {
    let mut g: Value = serde_json::from_slice(&fs::read(graph.join("asset-graph.json"))?)?;
    let key_base = g["dye_key_base"].as_u64().unwrap_or(0xE2510000);
    let out = r.output.clone();
    let nodes = g["nodes"].as_array_mut().context("nodes")?;
    for n in nodes.iter() {
        let f = n["file"].as_str().context("file")?;
        fs::copy(graph.join(f), out.join(f))?;
    }
    let modern: Value = serde_json::from_slice(&fs::read(source.join("dyes.json"))?)?;
    let old: Value = serde_json::from_slice(&fs::read(native.join("dyes.json"))?)?;
    let mut dyes = vec![];
    for m in modern.as_array().context("modern dyes")? {
        let channel = m["channel"].as_u64().context("channel")?;
        let n = old
            .as_array()
            .context("native dyes")?
            .iter()
            .find(|n| n["channel"] == m["channel"])
            .context("native channel")?;
        ensure!(
            m["found"].as_array().context("modern result")?.len() == 1
                && n["found"].as_array().context("native result")?.len() == 1,
            "ambiguous dye"
        );
        let m = &m["found"][0];
        let n = &n["found"][0];
        let prefix = format!("dye-{channel}");
        let symbol = |s: &str| format!("{prefix}-{s}");
        let nh = tag(n, "buffer_header")?;
        let raw = r.reference(nh)?;
        let constants = convert_constants(r.tag(raw, None)?.0.clone(), &m["constants"])?;
        add(
            &out,
            nodes,
            &symbol("constants"),
            raw,
            &constants,
            Some(&symbol("buffer")),
            vec![],
        )?;
        add(
            &out,
            nodes,
            &symbol("buffer"),
            nh,
            &r.tag(nh, None)?.0,
            Some(&symbol("constants")),
            vec![],
        )?;
        let scope_tag = tag(n, "scope")?;
        let mut scope = r.tag(scope_tag, None)?.0.clone();
        let mut patches = vec![patch(&mut scope, 0xBC, &symbol("buffer"))];
        // Keep fallback constants consistent with the referenced buffer.
        let rows = Payload(scope.clone()).array(0x88, 16, None)?;
        ensure!(rows.len() == 27, "native fallback constants");
        for (i, &o) in rows.iter().enumerate() {
            scope[o..o + 16].copy_from_slice(&constants[i * 16..i * 16 + 16]);
        }
        let mt = m["textures"].as_array().context("textures")?;
        let nt = n["textures"].as_array().context("textures")?;
        ensure!(
            mt.len() == 2 && nt.len() == 2,
            "dye requires diffuse and normal detail textures"
        );
        for i in 0..2 {
            let mh = Payload(hex::decode(
                mt[i]["header"].as_str().context("texture header")?,
            )?);
            let mut data = vec![];
            let large = mh.u32(60)?;
            if ![0, u32::MAX, 0x811C9DC5].contains(&large) {
                data.extend(fs::read(
                    source.join("raw").join(format!("{large:08X}.bin")),
                )?);
            }
            data.extend(fs::read(source.join("raw").join(format!(
                "{}.bin",
                mt[i]["buffer"].as_str().context("texture buffer")?
            )))?);
            ensure!(data.len() == mh.u32(0)? as usize, "texture mip chain size");
            let th = tag(&nt[i], "tag")?;
            let mut header = r.tag(th, None)?.0.clone();
            validate_detail_mips(&mh, data.len())?;
            header[..8].copy_from_slice(&mh.0[..8]);
            header[14..22].copy_from_slice(&mh.0[34..42]);
            header[22..24].copy_from_slice(&mh.0[44..46]);
            header[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
            crate::d2_mot::texture::resident(&mut header, data.len())?;
            let ds = symbol(&format!("texture-{i}-data"));
            let hs = symbol(&format!("texture-{i}"));
            add(
                &out,
                nodes,
                &ds,
                tag(&nt[i], "buffer")?,
                &data,
                Some(&hs),
                vec![],
            )?;
            add(&out, nodes, &hs, th, &header, Some(&ds), vec![])?;
            patches.push(patch(
                &mut scope,
                nt[i]["offset"].as_u64().context("slot")? as usize + 4,
                &hs,
            ));
        }
        add(
            &out,
            nodes,
            &symbol("scope"),
            scope_tag,
            &scope,
            None,
            patches,
        )?;
        let dt = tag(n, "dye")?;
        let mut dye = r.tag(dt, None)?.0.clone();
        let fix = patch(&mut dye, 12, &symbol("scope"));
        add(
            &out,
            nodes,
            &symbol("definition"),
            dt,
            &dye,
            None,
            vec![fix],
        )?;
        let pt = tag(n, "relation")?;
        let mut parent = r.tag(pt, None)?.0.clone();
        let fix = patch(&mut parent, 16, &symbol("definition"));
        add(&out, nodes, &symbol("parent"), pt, &parent, None, vec![fix])?;
        add(
            &out,
            nodes,
            &symbol("companion"),
            0x81A662DE,
            &r.tag(0x81A662DE, None)?.0,
            None,
            vec![],
        )?;
        let c = nodes.last_mut().unwrap();
        c["shared_owner"] = json!(symbol("parent"));
        c["source_parent"] = json!(pt);
        let key = u32::try_from(key_base.checked_add(channel).context("dye key overflow")?)?;
        dyes.push(json!({"channel":channel,"manifest":key,"parent":symbol("parent")}));
    }
    g["dyes"] = json!(dyes);
    write_json(&out.join("asset-graph.json"), &g)?;
    Ok(g)
}

fn validate_detail_mips(header: &Payload, bytes: usize) -> Result<()> {
    // Both formats are already supported by the native plated texture path.
    // The source format travels with its payload; a BC1 normal detail need
    // not use the donor's BC7 encoding to sample as Texture2D<float4>.
    let block = match header.u32(4)? {
        71 | 72 => 8,
        98 | 99 => 16,
        other => anyhow::bail!("unsupported detail texture format {other}"),
    };
    let width = usize::from(header.u16(34)?);
    let height = usize::from(header.u16(36)?);
    let mips = header.u8(45)?;
    ensure!(
        width > 0 && height > 0 && (1..=16).contains(&mips),
        "invalid detail dimensions or mip count"
    );
    let expected: usize = (0..mips)
        .map(|mip| (width >> mip).max(1).div_ceil(4) * (height >> mip).max(1).div_ceil(4) * block)
        .sum();
    ensure!(
        expected == bytes && header.u32(0)? as usize == bytes,
        "detail mip chain extent mismatch"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detail_textures_accept_bc1_and_bc7_only_with_complete_mips() {
        let mut header = Payload(vec![0; 64]);
        header.0[34..36].copy_from_slice(&4u16.to_le_bytes());
        header.0[36..38].copy_from_slice(&4u16.to_le_bytes());
        header.0[45] = 2;
        for (format, bytes) in [(71u32, 16u32), (99, 32)] {
            header.0[4..8].copy_from_slice(&format.to_le_bytes());
            header.0[..4].copy_from_slice(&bytes.to_le_bytes());
            validate_detail_mips(&header, bytes as usize).unwrap();
            assert!(validate_detail_mips(&header, bytes as usize - 1).is_err());
        }
        header.0[4..8].copy_from_slice(&77u32.to_le_bytes());
        assert!(validate_detail_mips(&header, 32).is_err());
    }
    #[test]
    fn converts_named_colors_and_preserves_legacy_only_vectors() {
        let mut values = vec![[0.0; 4]; 21];
        values[3] = [0.54, 0.54, 0.54, 1.0];
        values[12] = [0.6, 0.6, 0.6, 1.0];
        let native = vec![42; 432];
        let result = convert_constants(native.clone(), &json!(values)).unwrap();
        assert_eq!(
            f32::from_le_bytes(result[144..148].try_into().unwrap()),
            0.54
        );
        assert_eq!(
            f32::from_le_bytes(result[208..212].try_into().unwrap()),
            0.6
        );
        assert_eq!(&result[80..144], &native[80..144]);
        assert_eq!(&result[400..], &native[400..]);
        assert!(convert_constants(vec![0; 336], &json!(values)).is_err());
        assert!(convert_constants(native, &json!([])).is_err());
    }
}
