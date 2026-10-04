//! Package and captured native loader oracle, written before program construction.
use super::*;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

#[test]
#[ignore = "requires complete effect shaders, captured native image and the configured native loader probe"]
fn native_shader_program_oracle() -> Result<()> {
    let root = PathBuf::from(std::env::var("PARHELION_EFFECT_SHADER_CORPUS")?).canonicalize()?;
    let output = crate::d2_mot::reader::outside(
        &PathBuf::from(std::env::var("PARHELION_SHADER_PROGRAM_OUTPUT")?),
        &root,
    )?;
    ensure!(!output.exists(), "shader program output already exists");
    let probe = PathBuf::from(std::env::var("PARHELION_SHADER_LOADER_PROBE")?).canonicalize()?;
    let image = PathBuf::from(std::env::var("PARHELION_NATIVE_IMAGE")?).canonicalize()?;
    let load =
        |name: &str| -> Result<Value> { Ok(serde_json::from_slice(&fs::read(root.join(name))?)?) };
    let read = |era: &str, tag: &str, extension: &str| -> Result<Vec<u8>> {
        Ok(fs::read(
            root.join(format!("{era}-shaders/{tag}.{extension}")),
        )?)
    };
    let stage = |row: &Value| -> Result<Stage> {
        ensure!(row["file_type"] == 33, "shader package header type differs");
        Stage::from_subtype(u8::try_from(
            row["subtype"].as_u64().context("shader subtype")?,
        )?)
    };
    let native = load("native-shaders.json")?;
    let source = load("source-shaders.json")?;
    let source_rows = source.as_array().context("source shader census")?;
    let source_tags = source_rows
        .iter()
        .map(|row| Ok(row["tag"].as_str().context("source shader tag")?.to_owned()))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    ensure!(
        !source_tags.is_empty() && source_tags.len() == source_rows.len(),
        "empty or duplicate source shader census"
    );
    let scratch = tempfile::tempdir()?;
    let mut reader = crate::d2_mot::reader::Reader::discovery(
        &PathBuf::from(std::env::var("PARHELION_IMPORT_NATIVE_PACKAGES")?),
        scratch.path(),
        false,
    )?;
    for expected in [Stage::Pixel, Stage::Vertex, Stage::Compute] {
        let row = native
            .as_array()
            .context("native shader census")?
            .iter()
            .find(|row| stage(row).ok() == Some(expected))
            .context("native stage control")?;
        let tag = u32::from_str_radix(row["tag"].as_str().context("native shader tag")?, 16)?;
        let template = Template::read(&mut reader, tag)?;
        ensure!(
            template.stage() == expected
                && template.header_tag() == tag
                && format!("{:08X}", template.bytecode_tag()) == row["data"].as_str().unwrap(),
            "native shader template metadata differs"
        );
        ensure!(
            Template::read(&mut reader, template.bytecode_tag()).is_err(),
            "GPU bytecode entry accepted as an allocation header"
        );
        let name = row["tag"].as_str().unwrap();
        let program = Program::new(expected, read("native", name, "dxbc")?)?;
        let pair = program.assets("checked-shader", &template)?;
        ensure!(
            pair.header_template == tag && pair.bytecode_template == template.bytecode_tag(),
            "native shader package templates changed"
        );
        let other = native
            .as_array()
            .unwrap()
            .iter()
            .find(|row| stage(row).ok().is_some_and(|stage| stage != expected))
            .unwrap();
        let other = Template::read(
            &mut reader,
            u32::from_str_radix(other["tag"].as_str().unwrap(), 16)?,
        )?;
        ensure!(
            program.assets("wrong-stage", &other).is_err(),
            "GPU program accepted another stage's package templates"
        );
    }
    for row in native.as_array().context("native shader census")? {
        let name = row["tag"].as_str().context("native shader tag")?;
        let code = read("native", name, "dxbc")?;
        let program = Program::new(stage(row)?, code)?;
        ensure!(
            program.header().0 == read("native", name, "bin")?,
            "generated header differs from independent native resource {name}"
        );
    }
    fs::create_dir_all(&output)?;
    let mut programs = Vec::new();
    for row in source.as_array().context("source shader census")? {
        let name = row["tag"].as_str().context("source shader tag")?;
        let code = read("modern", name, "dxbc")?;
        let program = Program::new(stage(row)?, code)?;
        let header_file = format!("{name}.header.bin");
        let code_file = format!("{name}.dxbc");
        fs::write(output.join(&header_file), &program.header().0)?;
        fs::write(output.join(&code_file), program.bytecode())?;
        programs.push(json!({"source":name,"subtype":program.stage().subtype(),
            "header":header_file,"bytecode":code_file,"receipt":program.receipt(),
            "source_header_sha256":row["header_sha256"],
            "source_identity":row["compiled_identity"],"source_identity_resolved":false}));
    }
    let code = read("modern", "80D89BEA", "dxbc")?;
    ensure!(
        Program::new(Stage::Pixel, code.clone()).is_err()
            && Program::new(Stage::Vertex, code.clone()).is_err(),
        "compute executable accepted as graphics"
    );
    ensure!(
        Program::new(Stage::Compute, code[..code.len() - 4].to_vec()).is_err(),
        "truncated executable accepted"
    );
    ensure!(
        Program::new(Stage::Compute, vec![]).is_err(),
        "empty GPU program accepted"
    );
    fs::write(
        output.join("programs.json"),
        serde_json::to_vec_pretty(&json!({
            "schema":1,"programs":programs,"material_inputs_bound":false,"package_enrolled":false
        }))?,
    )?;
    let python = std::env::var_os("PARHELION_PYTHON").unwrap_or_else(|| "python".into());
    let result = Command::new(python)
        .arg(probe)
        .arg("--root")
        .arg(&root)
        .arg("--image")
        .arg(image)
        .arg("--programs")
        .arg(output.join("programs.json"))
        .arg("--output")
        .arg(output.join("native-loader"))
        .output()
        .context("execute captured native GPU loader probe")?;
    ensure!(
        result.status.success(),
        "native GPU loader probe failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(output.join("native-loader/shader-loader.json"))?)?;
    ensure!(
        receipt["source_programs"].as_u64() == Some(source_tags.len() as u64)
            && receipt["native_controls"] == 3,
        "native GPU loader coverage differs"
    );
    let rows = receipt["results"]
        .as_array()
        .context("native GPU loader results")?;
    let loaded_tags = rows
        .iter()
        .filter(|row| row["era"] == "source_program")
        .map(|row| {
            Ok(row["source"]
                .as_str()
                .context("loaded shader source")?
                .to_owned())
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        loaded_tags.len() == source_tags.len()
            && loaded_tags
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                == source_tags,
        "native GPU loader source coverage differs"
    );
    for program in &programs {
        let row = rows
            .iter()
            .find(|row| row["era"] == "source_program" && row["source"] == program["source"])
            .context("GPU program was not loaded")?;
        ensure!(
            row["status"] == 0
                && row["guards_intact"] == true
                && row["header_sha256"] == program["receipt"]["header_sha256"]
                && row["creation"]["sha256"] == program["receipt"]["bytecode_sha256"],
            "native GPU loader did not consume the emitted program"
        );
    }
    Ok(())
}
