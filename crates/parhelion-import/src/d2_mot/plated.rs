//! Audited Shadowkeep plated-sword conversion. Modern per-vertex dye selectors
//! become native draw groups; packed detail-scale lookup becomes TEXCOORD2.
use crate::d2_mot::{geometry::triangles, payload::Payload, reader::write_json};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

fn put(b: &mut [u8], o: usize, v: &[u8]) {
    b[o..o + v.len()].copy_from_slice(v);
}
fn attributes(old: &[u8], scales: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    ensure!(
        old.len() % 20 == 0 && scales.len() % 4 == 0,
        "invalid plated streams"
    );
    let mut bytes = Vec::new();
    let mut channels = Vec::new();
    for v in old.chunks_exact(20) {
        let packed = u16::from_le_bytes(v[10..12].try_into()?);
        let channel = (packed & 7) as u8;
        ensure!(channel < 6, "unsupported dye selector {channel}");
        let index = ((packed >> 3) & 4095) as usize;
        let scale = scales
            .get(index * 4..index * 4 + 4)
            .context("detail scale outside lookup")?;
        for half in scale.chunks_exact(2) {
            let h = u16::from_le_bytes(half.try_into()?);
            ensure!(h & 0x7C00 != 0x7C00, "nonfinite detail UV scale");
        }
        bytes.extend_from_slice(v);
        let end = bytes.len();
        bytes[end - 10..end - 8].fill(0); // normal W is not a dye lookup in this native layout.
        bytes.extend_from_slice(scale);
        channels.push(channel);
    }
    Ok((bytes, channels))
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub fn convert(
    source: &Path,
    native: &Path,
    out: &Path,
    modern: &Payload,
    mesh: usize,
) -> Result<()> {
    let mut report: Value = serde_json::from_slice(&fs::read(out.join("mapping.json"))?)?;
    // The mapper validates the carrier's native input layout. The fixed
    // plated material bank below supplies the resulting 8/24 vertex contract.
    let provenance: Value =
        serde_json::from_slice(&fs::read(source.join("source-manifest.json"))?)?;
    let auxiliary = modern.u32(mesh + 24)?;
    let reference = provenance["tags"][format!("{auxiliary:08X}")]["reference"]
        .as_u64()
        .context("missing detail scale export")?;
    let scales = fs::read(source.join("raw").join(format!("{reference:08X}.bin")))?;
    let (attrs, channels) = attributes(&fs::read(out.join("attributes.bin"))?, &scales)?;
    let old_indices = fs::read(out.join("indices.bin"))?;
    let original = Payload(fs::read(out.join("model.unlinked.bin"))?);
    let original_parts = original.array(0xB0 + 24, 32, Some(0x8080737E))?;
    let mut records = Vec::new();
    let mut indices = Vec::new();
    let mut patches = report["relocations"]
        .as_array()
        .context("relocations")?
        .iter()
        .filter(|p| p.get("part_index").is_none())
        .cloned()
        .collect::<Vec<_>>();
    let mut materials = serde_json::Map::new();
    let mut counts = [0u16; 24];
    let mut groups = Vec::new();
    // The old effect carriers are not equivalent to the source weapon's extra passes.
    // Keep the plated G-buffer and its native plated shadow pass only.
    for stage in [0usize, 3] {
        for (index, entry) in report["parts"]
            .as_array()
            .context("parts")?
            .iter()
            .enumerate()
        {
            if entry["stage"]
                != if stage == 0 {
                    "GenerateGbuffer"
                } else {
                    "ShadowGenerate"
                }
            {
                continue;
            }
            let at = original_parts[index];
            let start = original.u32(at + 8)? as usize;
            let count = original.u32(at + 12)? as usize;
            let slice = old_indices
                .get(start * 2..(start + count) * 2)
                .context("index range")?;
            let input = slice
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]) as u32)
                .collect::<Vec<_>>();
            let faces = triangles(&input, channels.len(), 65535)?;
            let mut split = BTreeMap::<u8, Vec<[u32; 3]>>::new();
            for face in faces {
                let channel = channels[face[0] as usize];
                ensure!(
                    face.iter().all(|&v| channels[v as usize] == channel),
                    "triangle crosses dye selectors"
                );
                split.entry(channel).or_default().push(face);
            }
            for (channel, faces) in split {
                let donor = if stage == 3 {
                    0x80EC271Du32
                } else {
                    match channel {
                        0 | 1 => 0x80EC270D,
                        2 => 0x80EC2713,
                        3 => 0x80EC270C,
                        4 | 5 => 0x80EC2710,
                        _ => unreachable!(),
                    }
                };
                let symbol = format!("material-plated-{donor:08X}-stage-{stage}");
                if !materials.contains_key(&symbol) {
                    let bytes = fs::read(native.join("raw").join(format!("{donor:08X}.bin")))?;
                    let mat = Payload(bytes.clone());
                    ensure!(
                        mat.u32(24)? & (1 << 25) != 0,
                        "carrier does not bind plated textures"
                    );
                    ensure!(mat.u64(0x2D0)? == 0, "carrier uses fixed effect textures");
                    fs::write(out.join(format!("{symbol}.bin")), bytes)?;
                    materials.insert(symbol.clone(),json!({"native_donor":format!("{donor:08X}"),"payload":format!("{symbol}.bin"),"stage":if stage==0{"GenerateGbuffer"}else{"ShadowGenerate"},"source_texture_slots":[],"appearance":"native plated shader with converted dye draw groups"}));
                }
                let start = indices.len() / 2;
                for face in &faces {
                    for &v in face {
                        indices.extend_from_slice(&(v as u16).to_le_bytes());
                    }
                    indices.extend_from_slice(&u16::MAX.to_le_bytes());
                }
                let mut part = original.0[at..at + 32].to_vec();
                put(&mut part, 8, &(start as u32).to_le_bytes());
                put(
                    &mut part,
                    12,
                    &((indices.len() / 2 - start) as u32).to_le_bytes(),
                );
                put(&mut part, 16, &(faces.len() as u32).to_le_bytes());
                put(&mut part, 22, &(records.len() as u16).to_le_bytes());
                part[26] = channel;
                // Splitting a source draw creates independent material/dye groups.
                // Its old group length can skip subsequent converted draws.
                part[29] = 1;
                patches.push(json!({"offset":0x150+records.len()*32,"symbol":symbol}));
                groups.push(
                    json!({"stage":stage,"lod":part[27],"channel":channel,"triangles":faces.len(),"source_material":entry["source_material"]}),
                );
                records.push(part);
            }
        }
        counts[stage + 1] = u16::try_from(records.len())?;
    }
    for i in 1..24 {
        counts[i] = counts[i].max(counts[i - 1]);
    }
    let mut model = original.0[..0x150].to_vec();
    for record in &records {
        model.extend_from_slice(record);
    }
    let size = model.len() as u64;
    put(&mut model, 0, &size.to_le_bytes());
    for offset in [0xC8, 0x140] {
        put(&mut model, offset, &(records.len() as u64).to_le_bytes());
    }
    for (i, count) in counts.iter().enumerate() {
        put(&mut model, 0xB0 + 40 + i * 2, &count.to_le_bytes());
    }
    for stage in 0..23 {
        put(
            &mut model,
            0xB0 + 88 + stage * 2,
            &(if stage == 0 || stage == 3 { 139i16 } else { -1 }).to_le_bytes(),
        );
    }
    let mut ah = fs::read(out.join("attributes.header.bin"))?;
    put(&mut ah, 0, &(attrs.len() as u32).to_le_bytes());
    put(&mut ah, 4, &24u16.to_le_bytes());
    let mut ih = fs::read(out.join("indices.header.bin"))?;
    put(&mut ih, 8, &(indices.len() as u32).to_le_bytes());
    for (name, bytes) in [
        ("attributes.bin", attrs),
        ("attributes.header.bin", ah),
        ("indices.bin", indices),
        ("indices.header.bin", ih),
        ("model.unlinked.bin", model),
    ] {
        fs::write(out.join(name), bytes)?;
    }
    report["materials"] = Value::Object(materials);
    report["relocations"] = json!(patches);
    report["native_parts"] = json!(records.len());
    if report["native_carrier"] == "80EC2722" {
        // Hook's selected 8/20 effect mesh shares its owner with this 8/24
        // plated mesh. Other families were selected as 8/24 from the start.
        report["native_mesh"] = json!(328);
    }
    report["plated_groups"] = json!(groups);
    let mut stages = report["stages"].as_array().context("stages")?.clone();
    let mut omitted = Vec::new();
    for entry in &mut stages {
        let stage = entry["stage"].as_u64().context("stage index")? as usize;
        ensure!(stage < 23, "stage outside native range");
        let retained = stage == 0 || stage == 3;
        if !retained {
            omitted.push(stage);
        }
        entry["native_layout"] = json!(if retained { 139 } else { -1 });
        entry["native_range"] = json!([counts[stage], counts[stage + 1]]);
    }
    report["stages"] = json!(stages);
    report["omitted_effect_stages"] = json!(omitted);
    write_json(&out.join("mapping.json"), &report)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_draws_are_all_visited_by_the_native_group_iterator() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let native = root.path().join("native");
        let out = root.path().join("out");
        for path in [source.join("raw"), native.join("raw"), out.clone()] {
            fs::create_dir_all(path).unwrap();
        }
        write_json(
            &source.join("source-manifest.json"),
            &json!({"tags":{"00000001":{"reference":2}}}),
        )
        .unwrap();
        fs::write(source.join("raw/00000002.bin"), [0, 60, 0, 60]).unwrap();
        let mut modern = Payload(vec![0; 128]);
        put(&mut modern.0, 24, &1u32.to_le_bytes());
        let mut attrs = vec![0; 6 * 20];
        for vertex in 3..6 {
            put(&mut attrs, vertex * 20 + 10, &2u16.to_le_bytes());
        }
        fs::write(out.join("attributes.bin"), attrs).unwrap();
        fs::write(out.join("attributes.header.bin"), [0; 12]).unwrap();
        fs::write(out.join("indices.header.bin"), [0; 24]).unwrap();
        let indices = [0u16, 1, 2, u16::MAX, 3, 4, 5];
        fs::write(
            out.join("indices.bin"),
            indices
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut model = vec![0; 0x170];
        put(&mut model, 0xC8, &1u64.to_le_bytes());
        put(&mut model, 0xD0, &0x70i64.to_le_bytes());
        put(&mut model, 0x140, &1u64.to_le_bytes());
        put(&mut model, 0x148, &0x8080737Eu32.to_le_bytes());
        put(&mut model, 0x15C, &7u32.to_le_bytes());
        model[0x150 + 29] = 3;
        fs::write(out.join("model.unlinked.bin"), model).unwrap();
        for tag in [0x80EC270Du32, 0x80EC2713] {
            let mut material = vec![0; 0x300];
            put(&mut material, 24, &(1u32 << 25).to_le_bytes());
            fs::write(native.join(format!("raw/{tag:08X}.bin")), material).unwrap();
        }
        write_json(&out.join("mapping.json"), &json!({"relocations":[],"parts":[{"stage":"GenerateGbuffer","source_material":"test"}],"stages":[{"stage":0}]})).unwrap();
        convert(&source, &native, &out, &modern, 0).unwrap();
        let result = Payload(fs::read(out.join("model.unlinked.bin")).unwrap());
        let parts = result.array(0xC8, 32, Some(0x8080737E)).unwrap();
        let mut visited = Vec::new();
        let mut index = 0;
        while index < parts.len() {
            visited.push(result.u8(parts[index] + 26).unwrap());
            let step = result.u8(parts[index] + 29).unwrap() as usize;
            assert!(step > 0);
            index += step;
        }
        assert_eq!(visited, [0, 2]);
        assert_eq!(index, parts.len());
    }

    #[test]
    fn unpacks_dye_and_half_precision_detail_scale() {
        let mut old = vec![0; 20];
        old[10..12].copy_from_slice(&10u16.to_le_bytes());
        let (result, channels) = attributes(&old, &[0, 60, 0, 60, 84, 63, 84, 63]).unwrap();
        assert_eq!(channels, [2]);
        assert_eq!(&result[20..24], &[84, 63, 84, 63]);
        assert_eq!(&result[10..12], &[0, 0]);
        assert!(attributes(&old, &[0; 4]).is_err());
        old[10] = 7;
        assert!(attributes(&old, &[0; 8]).is_err());
    }
}
