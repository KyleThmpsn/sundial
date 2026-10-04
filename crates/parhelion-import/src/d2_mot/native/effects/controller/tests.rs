//! Configured emitted-bank Native execution oracle, specified before extension.
use super::*;
use serde_json::json;
use std::{env, fs, path::PathBuf, process::Command};

#[test]
#[ignore = "Requires explicitly configured bank assets, Native image, registry, Python oracle and output"]
fn multiword_bank_native_execution() -> Result<()> {
    let configured = |name| -> Result<PathBuf> {
        Ok(PathBuf::from(
            env::var_os(name).with_context(|| format!("missing {name}"))?,
        ))
    };
    let source_path = configured("PARHELION_BANK_SOURCE")?;
    let native_path = configured("PARHELION_BANK_NATIVE")?;
    let allocation_path = configured("PARHELION_BANK_ALLOCATION")?;
    let image = configured("PARHELION_BANK_IMAGE")?;
    let registry = configured("PARHELION_BANK_REGISTRY")?;
    let oracle = configured("PARHELION_BANK_ORACLE")?;
    let output = configured("PARHELION_BANK_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh bank artifact directory");
    let image_sha =
        env::var("PARHELION_BANK_IMAGE_SHA256").context("missing PARHELION_BANK_IMAGE_SHA256")?;
    let read = |path: &PathBuf| -> Result<Payload> { Ok(Payload(fs::read(path)?)) };
    let source = read(&source_path)?;
    let native = read(&native_path)?;
    let allocation = read(&allocation_path)?;
    let converted = bank(&source, &native, &allocation, 0x81FD1FFD, 0x81FD1FFE)?;
    ensure!(
        converted.channels > 32
            && converted.channels
                == source
                    .array(source.pointer(24)? + 0x148, 112, Some(0x808095A9))?
                    .len(),
        "configured bank must preserve every channel and exercise multiple mask words"
    );
    ensure!(
        !converted.gates.is_empty(),
        "bank producer/construction gates lost"
    );
    fs::create_dir_all(&output)?;
    let candidate = output.join("bank.bin");
    let candidate_allocation = output.join("allocation.bin");
    fs::write(&candidate, &converted.owner.0)?;
    fs::write(&candidate_allocation, &converted.allocation.0)?;
    for (label, bank_path, allocation_path) in [
        ("native-control", &native_path, &allocation_path),
        ("converted", &candidate, &candidate_allocation),
    ] {
        let mut command =
            Command::new(env::var_os("PARHELION_BANK_PYTHON").unwrap_or_else(|| "python".into()));
        command
            .arg(&oracle)
            .arg("--image")
            .arg(&image)
            .arg("--image-sha256")
            .arg(&image_sha)
            .arg("--registry")
            .arg(&registry)
            .arg("--bank")
            .arg(bank_path)
            .arg("--allocation")
            .arg(allocation_path)
            .arg("--output")
            .arg(output.join(format!("{label}.json")));
        if let Some(modules) = env::var_os("PARHELION_BANK_PYTHON_MODULES") {
            command.arg("--python-modules").arg(modules);
        }
        if label == "converted" {
            command.arg("--source").arg(&source_path);
        }
        ensure!(
            command.status()?.success(),
            "Native bank oracle failed for {label}"
        );
    }
    let opcode_results = |name: &str| -> Result<Vec<serde_json::Value>> {
        let report: serde_json::Value = serde_json::from_slice(&fs::read(output.join(name))?)?;
        Ok(report["computed_copy_cases"]
            .as_array()
            .context("computed bank receipt")?
            .iter()
            .filter(|row| row["equation"] == "3c002622003e00")
            .map(|row| {
                json!({"name": row["name"], "input_name": row["input_name"],
                "value": row["value"], "result": row["result"]})
            })
            .collect())
    };
    let control = opcode_results("native-control.json")?;
    ensure!(
        !control.is_empty() && control == opcode_results("converted.json")?,
        "translated opcode 2D differs from actual Native 26 counterpart"
    );
    fs::write(
        output.join("conversion.json"),
        serde_json::to_vec_pretty(&json!({
            "channels": converted.channels, "inputs": converted.inputs,
            "gates": converted.gates, "source_path": source_path,
            "native_control_path": native_path, "objects": converted.objects,
            "gameplay_verified": false
        }))?,
    )?;
    Ok(())
}
