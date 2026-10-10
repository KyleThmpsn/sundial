//! Source solver files through native serialization and the archived game consumer.
//!
//! Failure model recorded before the converter: missing type metadata can hide
//! conflicting pointer types, invalid array extents or unrecognized fields.
//! Lowering can change particle topology, state order, constraint indexes or
//! previous-position buffers. Serialization can break signatures or fixups even
//! when a matching reader accepts it. The configured native consumer runs all
//! eleven states with moving transforms and retains its complete frame output.
use anyhow::{Context, Result, ensure};
use parhelion_import::d2_mot::{cloth, reader};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};

#[test]
#[ignore = "Requires retained cloth sources, an archived native image and an isolated consumer"]
fn translated_cloth_executes_native_states_and_moving_attachments() -> Result<()> {
    let config = env::var_os("PARHELION_CLOTH_SOLVER_CASES")
        .map(PathBuf::from)
        .context("Set PARHELION_CLOTH_SOLVER_CASES")?;
    let settings: Value = serde_json::from_slice(&fs::read(config)?)?;
    let output = PathBuf::from(settings["output"].as_str().context("Artifact directory")?);
    ensure!(!output.exists(), "Use a fresh cloth artifact directory");
    let cases = settings["cases"].as_array().context("Cloth source cases")?;
    ensure!(
        cases.len() >= 2,
        "Configure both retained body alternatives"
    );
    let checker = settings["checker"]
        .as_str()
        .context("Isolated native checker")?;
    let python = settings["python"].as_str().unwrap_or("python");
    let mut receipts = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let input = fs::read(case["source"].as_str().context("Source solver")?)?;
        let converted = cloth::translate(&input, None)?;
        let dir = output.join(format!("case-{index}"));
        fs::create_dir_all(&dir)?;
        let solver = dir.join("solver.bin");
        fs::write(&solver, &converted.bytes)?;
        reader::write_json(&dir.join("conversion.json"), &converted.report)?;
        let mut command = Command::new(python);
        command
            .arg(checker)
            .arg(
                settings["image"]
                    .as_str()
                    .context("Archived native image")?,
            )
            .arg(
                settings["inventory"]
                    .as_str()
                    .context("Native type inventory")?,
            )
            .arg(
                settings["reflection"]
                    .as_str()
                    .context("Native reflection")?,
            )
            .arg(&solver)
            .arg(case["wrapper"].as_str().context("Source state bindings")?)
            .arg(
                settings["dependencies"]
                    .as_str()
                    .context("Emulator dependencies")?,
            )
            .arg(dir.join("execution"))
            .arg("--modern-wrapper");
        if let Some(witness) = case["particle_transition_witness"].as_str() {
            command.arg("--transition-witness").arg(witness);
        }
        let status = command
            .status()
            .context("Run isolated native cloth consumer")?;
        ensure!(
            status.success(),
            "Native cloth execution failed for case {index}"
        );
        let execution: Value =
            serde_json::from_slice(&fs::read(dir.join("execution/receipt.json"))?)?;
        ensure!(
            execution["status"] == "passed",
            "Native consumer did not finish"
        );
        ensure!(
            execution["game_started"] == false,
            "Checker opened a game process"
        );
        if case["particle_transition_witness"].is_string() {
            let transition = &execution["particle_transitions"];
            ensure!(
                transition["status"] == "passed",
                "Per-particle transition execution failed"
            );
            ensure!(
                transition["native_calls"].as_u64().is_some_and(|n| n >= 12),
                "Transition witness lacks repeated native execution"
            );
            ensure!(
                transition["maximum_error"]
                    .as_f64()
                    .is_some_and(|n| n < 0.0001),
                "Transition delays or distances changed"
            );
            let invalid = fs::read(
                case["invalid_transition_source"]
                    .as_str()
                    .context("Invalid transition source")?,
            )?;
            ensure!(
                cloth::translate(&invalid, None).is_err(),
                "Invalid transition parameters emitted a solver"
            );
        }
        // Truncation and an unrecognized SDK must fail before an output exists.
        ensure!(
            cloth::translate(&input[..input.len() - 1], None).is_err(),
            "Truncated source accepted"
        );
        let mut unknown = input.clone();
        let sdk = unknown
            .windows(8)
            .position(|b| b == b"20180100")
            .context("Source SDK identity")?;
        unknown[sdk] = b'9';
        ensure!(
            cloth::translate(&unknown, None).is_err(),
            "Unknown SDK accepted"
        );
        receipts.push(
            json!({"case":index,"source_sha256":hex::encode(Sha256::digest(&input)),
            "native_sha256":hex::encode(Sha256::digest(&converted.bytes)),
            "native_execution":"passed","malformed_inputs":"rejected",
            "frames":execution["frames"].as_array().context("Native frames")?.len(),
            "particle_transitions":execution.get("particle_transitions")}),
        );
    }
    reader::write_json(
        &output.join("receipt.json"),
        &json!({"cases":receipts,
        "scope":"Source graphs through archived native state selection and simulation",
        "gameplay_verified":false}),
    )
}
