//! Configured Reach caches through the public exporter and an independent glTF reader.
//! The witness is produced by the separate Python cache decoder, not by the importer.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

fn accessor(root: &Path, gltf: &Value, index: usize) -> Result<(Vec<u8>, usize)> {
    let a = &gltf["accessors"][index];
    ensure!(a.get("sparse").is_none(), "Unexpected sparse accessor");
    let v = &gltf["bufferViews"][a["bufferView"].as_u64().context("buffer view")? as usize];
    let b = &gltf["buffers"][v["buffer"].as_u64().context("buffer")? as usize];
    let uri = b["uri"].as_str().context("buffer URI")?;
    ensure!(
        Path::new(uri).components().count() == 1,
        "Nonlocal buffer URI"
    );
    let bytes = fs::read(root.join(uri))?;
    ensure!(
        bytes.len() as u64 == b["byteLength"].as_u64().context("buffer length")?,
        "Buffer length differs"
    );
    let width = match a["componentType"].as_u64() {
        Some(5121) => 1,
        Some(5123) => 2,
        Some(5125 | 5126) => 4,
        n => anyhow::bail!("Unhandled component {n:?}"),
    };
    let lanes = match a["type"].as_str() {
        Some("SCALAR") => 1,
        Some("VEC2") => 2,
        Some("VEC3") => 3,
        Some("VEC4") => 4,
        Some("MAT4") => 16,
        n => anyhow::bail!("Unhandled accessor {n:?}"),
    };
    let element = width * lanes;
    let stride = v["byteStride"].as_u64().unwrap_or(element as u64) as usize;
    ensure!(stride >= element, "Short accessor stride");
    let start = v["byteOffset"].as_u64().unwrap_or(0) as usize
        + a["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = a["count"].as_u64().context("accessor count")? as usize;
    let mut result = Vec::new();
    for i in 0..count {
        result.extend(
            bytes
                .get(start + i * stride..start + i * stride + element)
                .context("Accessor outside buffer")?,
        );
    }
    Ok((result, count))
}

fn verify_audio(root: &Path, sounds: &[Value], missing: &[Value]) -> Result<()> {
    let receipt: Value = serde_json::from_slice(&fs::read(root.join("audio/audio.json"))?)?;
    let mut unavailable = receipt["unavailable"]
        .as_array()
        .context("Missing-audio receipt")?
        .iter()
        .map(|row| row["id"].as_u64().context("Missing sample identity"))
        .collect::<Result<Vec<_>>>()?;
    let mut expected = missing
        .iter()
        .map(|id| id.as_u64().context("Independent missing sample identity"))
        .collect::<Result<Vec<_>>>()?;
    unavailable.sort_unstable();
    expected.sort_unstable();
    ensure!(
        unavailable == expected,
        "Source missing-audio exclusions differ from independent bank lookup"
    );
    for sound in sounds {
        let id = sound["id"].as_u64().context("Audio witness identity")?;
        let bytes = fs::read(root.join(format!("audio/{id:08X}.wav")))?;
        ensure!(
            bytes.get(..4) == Some(b"RIFF"),
            "Source audio is not a RIFF stream"
        );
        let mut at = 12;
        let mut data = None;
        let mut format = None;
        while at + 8 <= bytes.len() {
            let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into()?) as usize;
            let payload = bytes
                .get(at + 8..at + 8 + size)
                .context("Audio chunk exceeds file")?;
            match &bytes[at..at + 4] {
                b"data" => data = Some(payload),
                b"fmt " => format = Some(payload),
                _ => {}
            }
            at += 8 + size + (size & 1);
        }
        let format = format.context("Audio format")?;
        let samples = data.context("Audio samples")?;
        ensure!(
            u16::from_le_bytes(format[2..4].try_into()?) as u64
                == sound["channels"].as_u64().context("Witness channels")?,
            "Audio channels changed"
        );
        ensure!(
            u32::from_le_bytes(format[4..8].try_into()?) as u64
                == sound["rate"].as_u64().context("Witness rate")?,
            "Audio rate changed"
        );
        ensure!(
            hex::encode(Sha256::digest(samples)) == sound["sha256"],
            "Source firing PCM changed"
        );
    }
    Ok(())
}

fn verify_animation(root: &Path, graphs: &[Value]) -> Result<()> {
    let actual: Value = serde_json::from_slice(&fs::read(root.join("animations.json"))?)?;
    for graph in graphs {
        let tag = u32::from_str_radix(
            graph["tag"]["datum"]
                .as_str()
                .context("Witness animation identity")?,
            16,
        )?;
        let found = actual
            .as_array()
            .context("Animation graphs")?
            .iter()
            .find(|g| g["tag"]["datum"] == tag)
            .context("Source animation graph was lost")?;
        for group in graph["groups"]
            .as_array()
            .context("Witness animation groups")?
        {
            let found = found["groups"]
                .as_array()
                .context("Resource groups")?
                .iter()
                .find(|g| g["index"] == group["index"])
                .context("Source animation resource was lost")?;
            for member in group["members"]
                .as_array()
                .context("Witness animation members")?
            {
                let found = found["members"]
                    .as_array()
                    .context("Animation members")?
                    .iter()
                    .find(|m| m["index"] == member["index"])
                    .context("Source animation member was lost")?;
                let bytes = fs::read(root.join(found["file"].as_str().context("Animation file")?))?;
                ensure!(
                    hex::encode(Sha256::digest(&bytes)) == member["sha256"],
                    "Source animation data changed"
                );
                ensure!(
                    found["frames"] == member["frames"] && found["nodes"] == member["nodes"],
                    "Source animation shape changed"
                );
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_REACH_MAPS, independent PARHELION_REACH_WITNESS and fresh PARHELION_REACH_OUTPUT"]
fn weapons_and_vehicles_preserve_geometry_skinning_and_material_inputs() -> Result<()> {
    let maps = configured("PARHELION_REACH_MAPS")?;
    let output = configured("PARHELION_REACH_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh artifact directory");
    let witness_path = configured("PARHELION_REACH_WITNESS")?;
    let witness: Value = serde_json::from_slice(&fs::read(&witness_path)?)?;
    let cases = witness["cases"].as_array().context("witness cases")?;
    ensure!(!cases.is_empty(), "Required source corpus is empty");
    let mut receipt = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        let request = serde_json::from_value(case["request"].clone())?;
        let root = output.join(format!("case-{i:03}"));
        let report = parhelion_import::halo_reach::export(&maps, &request, &root)?;
        if let Some(sounds) = case["audio"].as_array() {
            verify_audio(
                &root,
                sounds,
                case["missing_audio"]
                    .as_array()
                    .context("Missing-audio witness")?,
            )?;
        }
        if let Some(graphs) = case["animation"].as_array() {
            verify_animation(&root, graphs)?;
        }
        if let Some(wanted) = case.get("behavior") {
            let behavior: Value = serde_json::from_slice(&fs::read(root.join("behavior.json"))?)?;
            for sound in wanted["sounds"].as_array().context("Independent sounds")? {
                let id = sound["datum"].as_str().context("Sound datum")?;
                let actual = &behavior["nodes"][id];
                ensure!(
                    actual["tag"]["path"] == sound["path"],
                    "Source sound identity changed"
                );
                let ids = actual["data"]["pitch_ranges"]
                    .as_array()
                    .context("Pitch ranges")?
                    .iter()
                    .flat_map(|r| r["variants"].as_array().into_iter().flatten())
                    .map(|v| v["fsb_info"].clone())
                    .collect::<Vec<_>>();
                ensure!(
                    json!(ids) == sound["samples"],
                    "Source sound variations changed"
                );
            }
            for projectile in wanted["projectiles"]
                .as_array()
                .context("Independent projectiles")?
            {
                let id = projectile["datum"].as_str().context("Projectile datum")?;
                for field in [
                    "initial_velocity",
                    "final_velocity",
                    "air_gravity_scale",
                    "maximum_range",
                ] {
                    let actual = behavior["nodes"][id]["data"][field]
                        .as_f64()
                        .context("Projectile value")?;
                    let expected = projectile[field]
                        .as_f64()
                        .context("Independent projectile value")?;
                    ensure!(
                        (actual - expected).abs() < 0.0001,
                        "Source projectile value changed"
                    );
                }
            }
        }
        let gltf: Value = serde_json::from_slice(&fs::read(root.join("scene.gltf"))?)?;
        let expected = case["primitives"]
            .as_array()
            .context("witness primitives")?;
        let primitives = gltf["meshes"]
            .as_array()
            .context("glTF meshes")?
            .iter()
            .flat_map(|m| m["primitives"].as_array().into_iter().flatten())
            .collect::<Vec<_>>();
        ensure!(
            primitives.len() == expected.len() && !primitives.is_empty(),
            "Missing or duplicated source draws"
        );
        for (primitive, expected) in primitives.into_iter().zip(expected) {
            ensure!(
                primitive["mode"].as_u64().unwrap_or(4) == 4,
                "Expected triangles"
            );
            for semantic in ["POSITION", "NORMAL", "TEXCOORD_0", "JOINTS_0", "WEIGHTS_0"] {
                let (bytes, count) = accessor(
                    &root,
                    &gltf,
                    primitive["attributes"][semantic]
                        .as_u64()
                        .with_context(|| format!("Missing {semantic}"))?
                        as usize,
                )?;
                ensure!(
                    count as u64 == expected["vertices"].as_u64().context("witness vertices")?,
                    "Vertex count differs"
                );
                let values = &expected[semantic];
                if semantic == "JOINTS_0" {
                    let actual = bytes
                        .chunks_exact(2)
                        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                        .collect::<Vec<_>>();
                    ensure!(
                        serde_json::to_value(actual)? == *values,
                        "Bone palette or rigid assignment differs"
                    );
                } else {
                    let expected = values.as_array().context("witness vector")?;
                    ensure!(
                        bytes.len() / 4 == expected.len(),
                        "Witness vector length differs"
                    );
                    for (b, v) in bytes.chunks_exact(4).zip(expected) {
                        let actual = f32::from_le_bytes(b.try_into().unwrap());
                        let wanted = v.as_f64().context("witness number")? as f32;
                        ensure!(
                            actual.is_finite() && (actual - wanted).abs() < 0.00002,
                            "{semantic} differs: {actual} vs {wanted}"
                        );
                    }
                }
            }
            let (bytes, _) = accessor(
                &root,
                &gltf,
                primitive["indices"].as_u64().context("indices")? as usize,
            )?;
            let indices = bytes
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>();
            ensure!(
                serde_json::to_value(indices)? == expected["indices"],
                "Triangle winding, restart or selection differs"
            );
        }
        let source: Value = serde_json::from_slice(&fs::read(root.join("source.json"))?)?;
        ensure!(
            source["object"]["tag"]["path"] == case["request"]["path"],
            "Wrong source identity"
        );
        for material in case["textures"].as_array().context("texture witness")? {
            let file = material["file"].as_str().context("texture path")?;
            let bytes = fs::read(root.join(file))?;
            let expected = fs::read(
                witness_path
                    .parent()
                    .context("witness folder")?
                    .join(material["witness"].as_str().context("pixel witness")?),
            )?;
            ensure!(
                hex::encode(Sha256::digest(&expected))
                    == material["sha256"].as_str().context("texture digest")?,
                "Pixel witness changed"
            );
            ensure!(
                bytes.len() == expected.len()
                    && bytes.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                "Texture pixels differ by more than one decode rounding step"
            );
        }
        ensure!(
            parhelion_import::halo_reach::export(&maps, &request, &root).is_err(),
            "Existing output was accepted"
        );
        receipt.push(json!({"request":case["request"],"report":report,"independent_readback":"matched source witness geometry, skinning and texture pixels"}));
    }
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(
            &json!({"source_witness":witness["provenance"],"cases":receipt,"limits":["no native package or gameplay claim from glTF readback"]}),
        )?,
    )?;
    Ok(())
}
