//! Configured GPU comparison against original source executables, before adapter.
use super::*;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

#[test]
#[ignore = "requires configured source and native packages, rigid cases and the WARP stream-output probe"]
fn rigid_gpu_oracle() -> Result<()> {
    let cases = PathBuf::from(std::env::var("PARHELION_RIGID_CASES")?).canonicalize()?;
    let config: Value = serde_json::from_slice(&fs::read(&cases)?)?;
    let output = PathBuf::from(std::env::var("PARHELION_RIGID_OUTPUT")?);
    ensure!(!output.exists(), "rigid oracle output already exists");
    let scratch = tempfile::tempdir()?;
    let mut source = Reader::discovery(
        &PathBuf::from(std::env::var("PARHELION_IMPORT_MODERN_PACKAGES")?),
        &scratch.path().join("source"),
        true,
    )?;
    let mut native = Reader::discovery(
        &PathBuf::from(std::env::var("PARHELION_IMPORT_NATIVE_PACKAGES")?),
        &scratch.path().join("native"),
        false,
    )?;
    let tag = |value: &Value| -> Result<u32> {
        Ok(u32::from_str_radix(
            value.as_str().context("configured tag")?,
            16,
        )?)
    };
    let scopes = Scopes::read(
        &mut source,
        &mut native,
        tag(&config["source_rigid"])?,
        tag(&config["native_rigid"])?,
        tag(&config["source_view"])?,
        tag(&config["native_view"])?,
    )?;
    let streams = config["vertices"]
        .as_array()
        .context("rigid vertex streams")?;
    ensure!(!streams.is_empty(), "rigid oracle has no geometry");
    fs::create_dir_all(&output)?;
    let mut vertices = Vec::new();
    let mut checked_streams = Vec::new();
    for (index, value) in streams.iter().enumerate() {
        let stream = Stream::read(&mut source, tag(value)?)?;
        let path = output.join(format!("vertices-{index}.bin"));
        fs::write(&path, stream.data())?;
        vertices.push(path);
        checked_streams.push(stream);
    }
    let rows = config["materials"].as_array().context("rigid materials")?;
    ensure!(!rows.is_empty(), "rigid oracle has no material cases");
    let python = std::env::var_os("PARHELION_PYTHON").unwrap_or_else(|| "python".into());
    let probe = PathBuf::from(std::env::var("PARHELION_RIGID_PROBE")?).canonicalize()?;
    let mut reports = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let path = PathBuf::from(row["hlsl"].as_str().context("source HLSL")?);
        let path = if path.is_absolute() {
            path
        } else {
            cases.parent().context("case directory")?.join(path)
        };
        let text = fs::read_to_string(path)?;
        let result = Rigid::read(
            &mut source,
            tag(&row["tag"])?,
            &text,
            &scopes,
            &checked_streams[0],
        )?;
        let header = format!("candidate-{index}.header.bin");
        let bytecode = format!("candidate-{index}.dxbc");
        let original = output.join(format!("source-{index}.dxbc"));
        fs::write(output.join(&header), &result.program().header().0)?;
        fs::write(output.join(&bytecode), result.program().bytecode())?;
        fs::write(&original, result.source_bytecode())?;
        let candidate = output.join(format!("candidate-{index}.json"));
        fs::write(
            &candidate,
            serde_json::to_vec_pretty(&json!({
                "header":header,"bytecode":bytecode,"receipt":result.program().receipt(),
                "source_header_sha256":result.receipt()["source_header_sha256"],
                "source_bytecode_sha256":result.receipt()["source_bytecode_sha256"],
                "source_identity":result.receipt()["source_identity"],
                "source_identity_resolved":false,"rigid":result.receipt()
            }))?,
        )?;
        let report = output.join(format!("gpu-{index}.json"));
        let mut command = Command::new(&python);
        command
            .arg(&probe)
            .arg("--source")
            .arg(&original)
            .arg("--candidate")
            .arg(&candidate)
            .arg("--output")
            .arg(&report);
        for path in &vertices {
            command.arg("--vertices").arg(path);
        }
        let run = command.output()?;
        ensure!(
            run.status.success(),
            "rigid GPU probe failed: {} {}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let receipt: Value = serde_json::from_slice(&fs::read(&report)?)?;
        ensure!(
            receipt["passed"] == true && receipt["cases"].as_array().is_some_and(|v| !v.is_empty()),
            "rigid GPU verification is incomplete"
        );
        reports.push(receipt);
    }
    fs::write(
        output.join("rigid-oracle.json"),
        serde_json::to_vec_pretty(&json!({
            "reports":reports,"package_enrolled":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
