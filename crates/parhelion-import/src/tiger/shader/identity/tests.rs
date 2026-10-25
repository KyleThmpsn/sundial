//! Configured package oracle, written before the exact shader catalog.
use super::*;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

mod effects;

#[test]
#[ignore = "requires exported source and native shaders and a fresh artifact directory"]
fn shader_package_oracle() -> Result<()> {
    let root = PathBuf::from(std::env::var("PARHELION_SHADER_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_SHADER_OUTPUT")?);
    ensure!(!output.exists(), "shader oracle output already exists");
    let read =
        |name: &str| -> Result<Value> { Ok(serde_json::from_slice(&fs::read(root.join(name))?)?) };
    let stage = |value: &Value| -> Result<Stage> {
        Stage::from_subtype(u8::try_from(
            value["subtype"].as_u64().context("shader subtype")?,
        )?)
    };
    let tag = |value: &Value| -> Result<u32> {
        Ok(u32::from_str_radix(
            value.as_str().context("shader tag")?,
            16,
        )?)
    };
    let bytes = |era: &str, name: &str, extension: &str| -> Result<Vec<u8>> {
        Ok(fs::read(
            root.join(format!("{era}-shaders/{name}.{extension}")),
        )?)
    };
    let native = read("native-shaders.json")?;
    let mut catalog = Catalog::default();
    for entry in native.as_array().context("native shader census")? {
        ensure!(
            entry["file_type"] == 33,
            "native shader package type differs"
        );
        let name = entry["tag"].as_str().context("native shader name")?;
        catalog.insert(
            tag(&entry["tag"])?,
            stage(entry)?,
            &Payload(bytes("native", name, "bin")?),
            &bytes("native", name, "dxbc")?,
        )?;
    }
    let mut reports = Vec::new();
    let source = read("source-shaders.json")?;
    let expected = read("shader-equivalence.json")?;
    let mut reused = 0;
    for entry in source.as_array().context("source shader census")? {
        ensure!(
            entry["file_type"] == 33,
            "source shader package type differs"
        );
        let name = entry["tag"].as_str().context("source shader name")?;
        let header = Payload(bytes("modern", name, "bin")?);
        let code = bytes("modern", name, "dxbc")?;
        let (binding, unresolved) = match catalog.find(stage(entry)?, &header, &code) {
            Ok(binding) => (binding, None),
            Err(error) if header.u32(12)? != u32::MAX => (None, Some(error.to_string())),
            Err(error) => return Err(error),
        };
        let expected = expected["shaders"]
            .as_array()
            .context("shader correspondence")?
            .iter()
            .find(|row| row["tag"] == name)
            .context("independent shader correspondence")?;
        let matches = expected["native_exact_matches"]
            .as_array()
            .context("native matches")?;
        ensure!(
            binding.is_some() != matches.is_empty(),
            "shader reuse result differs"
        );
        if let Some(binding) = &binding {
            ensure!(
                matches
                    .iter()
                    .any(|value| tag(value).ok() == Some(binding.tag)),
                "shader binding lacks an independent native match"
            );
            let native_name = format!("{:08X}", binding.tag);
            ensure!(
                header.0 == bytes("native", &native_name, "bin")?
                    && code == bytes("native", &native_name, "dxbc")?,
                "shader binding changes complete package content"
            );
            reused += 1;
        }
        reports.push(
            json!({"source":name,"stage":stage(entry)?,"binding":binding,
            "unresolved":unresolved,"source_header_binding":header.u32(12)?,
            "header_sha256":digest(&header.0),"bytecode_sha256":digest(&code)}),
        );
    }
    ensure!(
        reused > 0 && !reports.is_empty(),
        "renderer shader corpus coverage differs"
    );
    let header = Payload(bytes("modern", "80D89D32", "bin")?);
    let code = bytes("modern", "80D89D32", "dxbc")?;
    let wrong_stage = catalog.find(Stage::Pixel, &header, &code);
    ensure!(
        wrong_stage.is_err(),
        "vertex executable accepted as a pixel shader"
    );
    let mut rejected = 1;
    for field in [0usize, 8, 12, 16, 24, 32] {
        let mut invalid = header.clone();
        invalid.0[field] ^= 1;
        ensure!(
            catalog.find(Stage::Vertex, &invalid, &code).is_err(),
            "malformed shader header accepted at {field:X}"
        );
        rejected += 1;
    }
    for field in [0usize, 20, 24, 28, 32] {
        let mut invalid = code.clone();
        invalid[field] = 0xFF;
        ensure!(
            catalog.find(Stage::Vertex, &header, &invalid).is_err(),
            "malformed DXBC accepted at {field:X}"
        );
        rejected += 1;
    }
    let mut changed = code.clone();
    let tail = changed.last_mut().context("shader executable tail")?;
    *tail ^= 1;
    ensure!(
        catalog.find(Stage::Vertex, &header, &changed)?.is_none(),
        "changed shader reused an approximate native executable"
    );
    let mut duplicates = Catalog::default();
    duplicates.insert(0x81FE1001, Stage::Vertex, &header, &code)?;
    duplicates.insert(0x81FE1000, Stage::Vertex, &header, &code)?;
    ensure!(
        duplicates
            .find(Stage::Vertex, &header, &code)?
            .context("duplicate shader")?
            .tag
            == 0x81FE1000,
        "duplicate shader selection depends on insertion order"
    );
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("shader-bindings.json"),
        serde_json::to_vec_pretty(&json!({
            "schema":1,"native_candidates":native.as_array().unwrap().len(),
            "source_shaders":reports.len(),"reused":reused,"malformed_refusals":rejected,
            "bindings":reports,"materials_bound":false,"installed":false
        }))?,
    )?;
    Ok(())
}
