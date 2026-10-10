//! Configured native-scope execution through the production material reader and animator.
//! Expectations come from the captured native TFX interpreter, not preview formulas.
use super::*;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_MATERIAL_FIXTURES and SUNDIAL_PREVIEW_VERIFY_OUTPUT"]
fn native_material_scopes_match_captured_execution() {
    let fixture = PathBuf::from(
        std::env::var_os("SUNDIAL_PREVIEW_MATERIAL_FIXTURES").expect("native material fixtures"),
    );
    let output = PathBuf::from(
        std::env::var_os("SUNDIAL_PREVIEW_VERIFY_OUTPUT").expect("verification output"),
    );
    fs::create_dir_all(&output).unwrap();
    let fixture: Value = serde_json::from_slice(&fs::read(fixture).unwrap()).unwrap();
    let mut results = Vec::new();
    for row in fixture["cases"].as_array().unwrap() {
        let code: Vec<u8> = serde_json::from_value(row["code"].clone()).unwrap();
        let constants: Vec<[f32; 4]> = serde_json::from_value(row["constants"].clone()).unwrap();
        let channels: Vec<[f32; 4]> = serde_json::from_value(row["channels"].clone()).unwrap();
        let seconds = row["seconds"].as_f64().unwrap() as f32;
        let rejected = row["expected_rejection"] == true;
        let expected: Vec<[f32; 4]> = serde_json::from_value(row["expected"].clone()).unwrap();
        let loaded = program::Program::read(&scope(&code, &constants), &channels);
        let mut max_error = 0.0_f32;
        let (passed, error) = match loaded {
            Ok(Some(program)) => match program.run([[0.25; 4]; 27], seconds) {
                Ok(actual) if !rejected && expected.len() == 27 => {
                    let finite = actual.iter().flatten().all(|value| value.is_finite());
                    for (a, b) in actual.iter().flatten().zip(expected.iter().flatten()) {
                        max_error = max_error.max((a - b).abs() / (1.0 + b.abs()));
                    }
                    (finite && max_error <= 0.00002, None)
                }
                Ok(_) => (false, Some("Native rejected the numeric result".to_owned())),
                Err(error) => (rejected, Some(error)),
            },
            Ok(None) => (
                false,
                Some("Scope unexpectedly has no animation".to_owned()),
            ),
            Err(error) => (false, Some(error)),
        };
        // A malformed later instruction must not become a partially accepted material.
        let mut invalid = code;
        invalid.push(0xFF);
        let invalid_rejected =
            program::Program::read(&scope(&invalid, &constants), &channels).is_err();
        results.push(
            json!({"name":row["name"],"passed":passed && invalid_rejected,
            "relative_error":max_error,"expected_rejection":rejected,"error":error,
            "invalid_suffix_rejected":invalid_rejected}),
        );
    }
    let mut controls = Vec::new();
    let malformed = [
        ("missing constant", vec![0x34, 1, 0x43, 0]),
        ("truncated constant", vec![0x34]),
        ("truncated extern", vec![0x3C, 1]),
        ("uninitialized temporary", vec![0x45, 0, 0x43, 0]),
        ("temporary outside table", vec![0x34, 0, 0x46, 16]),
        ("output outside table", vec![0x34, 0, 0x43, 27]),
        ("matrix underflow", vec![0x34, 0, 0x2D, 0x43, 0]),
        ("curve outside constants", vec![0x34, 0, 0x37, 0, 0x43, 0]),
        ("write before underflow", vec![0x34, 0, 0x43, 0, 0x43, 1]),
        ("unfinished expression", vec![0x34, 0]),
        ("stack overflow", (0..65).flat_map(|_| [0x34, 0]).collect()),
    ];
    for (name, code) in malformed {
        let rejected = program::Program::read(&scope(&code, &[[1.0; 4]]), &[]).is_err();
        controls.push(json!({"name":name,"passed":rejected}));
    }
    let mut truncated = scope(&[0x34, 0, 0x43, 0], &[[1.0; 4]]);
    truncated.pop();
    controls.push(json!({"name":"truncated array payload",
        "passed":program::Program::read(&truncated,&[]).is_err()}));
    let passed = results.iter().filter(|row| row["passed"] == true).count();
    fs::write(
        output.join("material-execution.json"),
        serde_json::to_vec_pretty(&json!({
            "cases":results.len(),"passed":passed,"gameplay_verified":false,"results":results,
            "malformed_controls":controls
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(!results.is_empty(), "no captured material cases");
    assert_eq!(
        passed,
        results.len(),
        "First disagreement: {:?}",
        results.iter().find(|row| row["passed"] != true)
    );
    assert!(
        controls.iter().all(|row| row["passed"] == true),
        "Malformed scope accepted: {controls:?}"
    );
}

fn scope(code: &[u8], constants: &[[f32; 4]]) -> Vec<u8> {
    let mut bytes = vec![0; 0xA0];
    let mut array = |descriptor: usize, class: u32, count: usize, payload: &[u8]| {
        let at = bytes.len();
        bytes[descriptor..descriptor + 8].copy_from_slice(&(count as u64).to_le_bytes());
        bytes[descriptor + 8..descriptor + 16]
            .copy_from_slice(&((at - descriptor - 8) as u64).to_le_bytes());
        bytes.extend_from_slice(&(count as u64).to_le_bytes());
        bytes.extend_from_slice(&class.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(payload);
    };
    array(0x58, 0x8080_0009, code.len(), code);
    let payload: Vec<_> = constants
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    array(0x68, 0x8080_0090, constants.len(), &payload);
    let len = bytes.len() as u64;
    bytes[..8].copy_from_slice(&len.to_le_bytes());
    bytes
}

/// The same serialized cubic material used by the native capture's timeline cases. Only the
/// desktop GPU verification renders it.
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn timeline() -> (Animation, [[f32; 4]; 27]) {
    let code = [0x3C, 1, 0, 0x37, 0, 0x43, 9];
    let constants = [
        [0.0; 4],
        [0.0; 4],
        [1.0, 0.0, -1.0, 0.0],
        [0.0, 0.5, 1.5, 0.0],
        [0.0, 0.5, 1.0, 1.5],
    ];
    let mut base = [[0.0; 4]; 27];
    base[0] = [1.0, 1.0, 0.0, 0.0];
    base[1] = base[0];
    for offset in [9, 13] {
        base[offset] = [0.2; 4];
        base[offset + 1] = [1.0, 1.0, 1.0, 0.0];
        base[offset + 2] = [-1.0, 0.0, 0.0, 0.0];
        base[offset + 3] = [0.0, 1.0, 0.0, 1.0];
    }
    for offset in [17, 21] {
        base[offset] = [0.2; 4];
        base[offset + 1] = [0.0, 1.0, 0.0, 1.0];
        base[offset + 2] = [0.0, 1.0, 0.0, 1.0];
        base[offset + 3] = [1.0, 1.0, 1.0, 0.0];
    }
    let animation = source_animation(&scope(&code, &constants), &[], base)
        .unwrap()
        .unwrap();
    (animation, base)
}
