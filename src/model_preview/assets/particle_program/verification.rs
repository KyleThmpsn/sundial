//! Configured integration checks from serialized native definitions through the production VM.
//! The independent expected results come from captured native instructions, not a second VM.
use super::*;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PARTICLE_CORPUS and SUNDIAL_PREVIEW_VERIFY_OUTPUT"]
fn exported_native_corpus_reports_instruction_coverage() {
    let directory =
        PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PARTICLE_CORPUS").expect("corpus"));
    let output = PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_VERIFY_OUTPUT").expect("output"));
    fs::create_dir_all(&output).unwrap();
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "bin"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut programs = 0;
    let mut instructions = 0;
    let mut available = 0;
    let mut failures = Vec::new();
    for path in paths {
        let result =
            Program::read(&fs::read(&path).unwrap()).and_then(|program| program.coverage());
        match result {
            Ok(coverage) => {
                programs += 1;
                instructions += coverage.instructions;
                available += coverage.available;
                if !coverage.unsupported.is_empty() {
                    failures.push(json!({"file":path.file_name().unwrap().to_string_lossy(),
                        "unsupported":coverage.unsupported}));
                }
            }
            Err(error) => failures
                .push(json!({"file":path.file_name().unwrap().to_string_lossy(),"error":error})),
        }
    }
    fs::write(output.join("particle-coverage.json"), serde_json::to_vec_pretty(&json!({
        "programs":programs,"instructions":instructions,"available":available,
        "scope":"Instruction availability, not engine binding or playback verification", "failures":failures,
    })).unwrap()).unwrap();
    assert!(programs > 0, "empty corpus");
    assert!(
        failures.is_empty(),
        "{} corpus failures, first: {:?}",
        failures.len(),
        failures.first()
    );
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_VM_FIXTURES and SUNDIAL_PREVIEW_VERIFY_OUTPUT"]
fn native_definitions_match_captured_execution() {
    let paths = std::env::var_os("SUNDIAL_PREVIEW_VM_FIXTURES").expect("fixture paths");
    let output = PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_VERIFY_OUTPUT").expect("output"));
    fs::create_dir_all(&output).unwrap();
    let mut results = Vec::new();
    for path in std::env::split_paths(&paths) {
        let source: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for row in source["cases"].as_array().expect("native cases") {
            results.push(compare(row));
        }
    }
    let failed: Vec<_> = results.iter().filter(|row| row["passed"] != true).collect();
    let receipt = json!({"cases": results.len(), "passed": results.len() - failed.len(),
        "gameplay_verified": false, "results": results});
    fs::write(
        output.join("native-execution.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    assert!(!results.is_empty(), "no native cases");
    assert!(
        failed.is_empty(),
        "{} native disagreements, first: {:?}",
        failed.len(),
        failed.first()
    );
}

fn vectors(value: &Value) -> Vec<[f32; 4]> {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

fn transform(value: &Value) -> vm::Transform {
    vm::Transform {
        translation: serde_json::from_value(value[0].clone()).unwrap(),
        rotation: serde_json::from_value(value[1].clone()).unwrap(),
    }
}

fn compare(row: &Value) -> Value {
    let code: Vec<u8> = serde_json::from_value(row["code"].clone()).unwrap();
    let defaults = vectors(&row["defaults"]);
    let constants = vectors(&row["constants"]);
    let program =
        Program::read(&definition(&code, &constants, &defaults)).expect("native definition");
    let mut registers = Registers::new(&program);
    for value in row["registers"].as_array().unwrap() {
        registers
            .set(
                value[0].as_u64().unwrap() as u8,
                value[1].as_u64().unwrap() as u8,
                serde_json::from_value(value[2].clone()).unwrap(),
            )
            .unwrap();
    }
    let mut runtime = vm::Runtime {
        seed: Some(row["seed"].as_u64().unwrap() as u32),
        transforms: row["transforms"]
            .as_array()
            .unwrap()
            .iter()
            .map(transform)
            .collect(),
    };
    let scoped = vectors(&row["inputs"]);
    let inputs = vectors(&row["named_inputs"]);
    let external = vectors(&row["external_inputs"]);
    let sources = vm::Sources {
        scoped: &scoped,
        inputs: &inputs,
        external: &external,
        pushes: &[],
    };
    let initial_registers = registers.clone();
    let initial_runtime = runtime.clone();
    let status = program.evaluate_section_with_state(0, &mut registers, sources, &mut runtime);
    let mut max_error = 0.0_f32;
    let mut finite = true;
    let mut compare = |actual: [f32; 4], expected: [f32; 4]| {
        for (a, b) in actual.into_iter().zip(expected) {
            finite &= a.is_finite() && b.is_finite();
            max_error = max_error.max((a - b).abs() / (1.0 + b.abs()));
        }
    };
    for value in row["outputs"].as_array().unwrap() {
        compare(
            registers
                .get(
                    value[0].as_u64().unwrap() as u8,
                    value[1].as_u64().unwrap() as u8,
                )
                .unwrap(),
            serde_json::from_value(value[2].clone()).unwrap(),
        );
    }
    for (actual, expected) in runtime
        .transforms
        .iter()
        .zip(row["expected_transforms"].as_array().unwrap())
    {
        let expected = transform(expected);
        compare(actual.translation, expected.translation);
        compare(actual.rotation, expected.rotation);
    }
    let rejected = row["expected_rejection"] == true;
    let passed = if rejected {
        status.is_err()
    } else {
        status.is_ok()
            && finite
            && max_error <= 0.00002
            && runtime.seed == Some(row["expected_seed"].as_u64().unwrap() as u32)
    };
    // A later failure must roll back earlier stores, transforms and random advances.
    let mut invalid = code;
    invalid.push(0xFF);
    let invalid = Program::read(&definition(&invalid, &constants, &defaults)).unwrap();
    let mut rollback_registers = initial_registers.clone();
    let mut rollback_runtime = initial_runtime.clone();
    let failed = invalid
        .evaluate_section_with_state(0, &mut rollback_registers, sources, &mut rollback_runtime)
        .is_err();
    let rollback = failed
        && (0..7).all(|bank| {
            (0..64)
                .all(|slot| rollback_registers.get(bank, slot) == initial_registers.get(bank, slot))
        })
        && rollback_runtime.seed == initial_runtime.seed
        && rollback_runtime
            .transforms
            .iter()
            .zip(&initial_runtime.transforms)
            .all(|(a, b)| a.translation == b.translation && a.rotation == b.rotation);
    json!({"name":row["name"], "passed":passed && rollback, "relative_error":max_error,
        "expected_rejection":rejected, "error":status.err(), "rollback":rollback})
}

/// Encode a real native envelope so the check includes the production bounds and table reader.
fn definition(code: &[u8], constants: &[[f32; 4]], defaults: &[[f32; 4]]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 0x150];
    bytes[0x80..0xF0].fill(0xFF);
    bytes[0x70..0x72].copy_from_slice(&u16::try_from(code.len()).unwrap().to_le_bytes());
    let mut array = |descriptor: usize, class: u32, count: usize, payload: &[u8]| {
        if count == 0 {
            return;
        }
        let at = bytes.len();
        bytes[descriptor..descriptor + 8].copy_from_slice(&(count as u64).to_le_bytes());
        bytes[descriptor + 8..descriptor + 16]
            .copy_from_slice(&((at - descriptor - 8) as u64).to_le_bytes());
        bytes.extend_from_slice(&(count as u64).to_le_bytes());
        bytes.extend_from_slice(&class.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(payload);
    };
    array(0x50, 0x8080_0009, code.len(), code);
    for (at, vectors) in [(0x08, defaults), (0x60, constants)] {
        let payload: Vec<_> = vectors
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        array(at, 0x8080_0090, vectors.len(), &payload);
    }
    let len = bytes.len() as u64;
    bytes[..8].copy_from_slice(&len.to_le_bytes());
    bytes
}
