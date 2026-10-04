//! Configured executable/decompiler/package oracle, written before the reader.
use super::*;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires the complete effect shader closure and independent Microsoft/decompiler contracts"]
fn shader_input_closure_oracle() -> Result<()> {
    let root = PathBuf::from(std::env::var("PARHELION_EFFECT_SHADER_CORPUS")?).canonicalize()?;
    let oracle = PathBuf::from(std::env::var("PARHELION_SHADER_INPUT_ORACLE")?).canonicalize()?;
    let output = crate::d2_mot::reader::outside(
        &PathBuf::from(std::env::var("PARHELION_SHADER_INPUT_OUTPUT")?),
        &root,
    )?;
    ensure!(!output.exists(), "shader input output already exists");
    let baseline: Value = serde_json::from_slice(&fs::read(oracle.join("shader-inputs.json"))?)?;
    let decompiled: Value =
        serde_json::from_slice(&fs::read(root.join("source-interfaces.json"))?)?;
    let source: Value = serde_json::from_slice(&fs::read(root.join("source-shaders.json"))?)?;
    let tags = |rows: &Value| -> Result<std::collections::BTreeSet<String>> {
        let rows = rows.as_array().context("shader contract rows")?;
        let tags = rows
            .iter()
            .map(|row| {
                Ok(row["tag"]
                    .as_str()
                    .context("shader contract tag")?
                    .to_owned())
            })
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        ensure!(tags.len() == rows.len(), "duplicate shader contract tag");
        Ok(tags)
    };
    let source_tags = tags(&source)?;
    ensure!(
        !source_tags.is_empty()
            && source_tags == tags(&baseline["shaders"])?
            && source_tags == tags(&decompiled["shaders"])?,
        "independent shader closures differ"
    );
    let mut artifacts = Vec::new();
    let mut files = Vec::new();
    for row in source.as_array().context("source shader census")? {
        let tag = row["tag"].as_str().context("source shader tag")?;
        let code = fs::read(root.join(format!("modern-shaders/{tag}.dxbc")))?;
        let stage = Stage::from_subtype(u8::try_from(row["subtype"].as_u64().context("stage")?)?)?;
        let inspection = Inspection::read(stage, &code)?;
        let expected = baseline["shaders"]
            .as_array()
            .context("independent executable contracts")?
            .iter()
            .find(|row| row["tag"] == tag)
            .context("missing executable contract")?;
        ensure!(
            serde_json::to_value(&inspection.inputs)? == expected["inputs"],
            "executable input contract differs for {tag}"
        );
        ensure!(
            inspection.inputs.bytecode_sha256 == row["bytecode_sha256"].as_str().unwrap(),
            "shader input contract has another executable digest"
        );
        let previous = decompiled["shaders"]
            .as_array()
            .context("independent decompiler contracts")?
            .iter()
            .find(|row| row["tag"] == tag)
            .context("missing decompiler contract")?;
        ensure!(
            serde_json::to_value(&inspection.inputs.constant_buffers)?
                == previous["constant_buffers"],
            "Microsoft and decompiler constant declarations differ for {tag}"
        );
        // Capacity checks deliberately do not establish producer semantics.
        let mut bindings = Bindings {
            constant_buffers: inspection.inputs.constant_buffers.clone(),
            resources: inspection.inputs.resources.clone(),
            samplers: inspection.inputs.samplers.clone(),
            unordered_access: inspection.inputs.unordered_access.clone(),
        };
        inspection.inputs.verify_layout(&bindings)?;
        if let Some((&slot, &vectors)) = bindings.constant_buffers.iter().next() {
            bindings.constant_buffers.insert(slot, vectors - 1);
            ensure!(
                inspection.inputs.verify_layout(&bindings).is_err(),
                "undersized constant producer accepted for {tag}"
            );
            bindings.constant_buffers.remove(&slot);
            ensure!(
                inspection.inputs.verify_layout(&bindings).is_err(),
                "missing constant producer accepted for {tag}"
            );
            bindings.constant_buffers = inspection.inputs.constant_buffers.clone();
        }
        if let Some((&slot, _)) = bindings.resources.iter().next() {
            bindings
                .resources
                .insert(slot, "incompatible-resource".into());
            ensure!(
                inspection.inputs.verify_layout(&bindings).is_err(),
                "incompatible resource accepted for {tag}"
            );
            bindings.resources.remove(&slot);
            ensure!(
                inspection.inputs.verify_layout(&bindings).is_err(),
                "missing resource accepted for {tag}"
            );
        }
        files.push((format!("{tag}.asm"), inspection.assembly));
        artifacts.push(json!({"tag":tag,"inputs":inspection.inputs,
            "material_inputs_bound":false,"layout_negative_controls_passed":true}));
    }
    for control in baseline["native_controls"]
        .as_array()
        .context("native controls")?
    {
        let tag = control["tag"].as_str().context("native control tag")?;
        let code = fs::read(root.join(format!("native-shaders/{tag}.dxbc")))?;
        let stage = Stage::from_subtype(u8::try_from(control["subtype"].as_u64().unwrap())?)?;
        ensure!(
            serde_json::to_value(Inspection::read(stage, &code)?.inputs)? == control["inputs"],
            "native executable control differs for {tag}"
        );
    }
    ensure!(
        artifacts.len() == source_tags.len(),
        "incomplete shader input closure"
    );
    fs::create_dir_all(&output)?;
    for (name, text) in files {
        fs::write(output.join(name), text)?;
    }
    fs::write(
        output.join("shader-inputs.json"),
        serde_json::to_vec_pretty(&json!({"schema":1,"shaders":artifacts,
            "source_count":artifacts.len(),"native_controls":baseline["native_controls"],
            "material_inputs_bound":false,"installed":false}))?,
    )?;
    Ok(())
}
