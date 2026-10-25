//! Source-file conversion to independently evaluated native arithmetic and a retained receipt.
//! Failure modes are recorded in docs/render-review-20261006/failure-model.md before changes.
use super::*;

fn evaluate(code: &[u8], constants: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut stack: Vec<[f32; 4]> = Vec::new();
    let mut output = vec![[0.0; 4]; 3];
    let mut bytes = code.iter().copied();
    while let Some(op) = bytes.next() {
        match op {
            0x34 => stack.push(constants[usize::from(bytes.next().unwrap())]),
            0x43 => output[usize::from(bytes.next().unwrap())] = stack.pop().unwrap(),
            0x05 | 0x06 => {
                let b = stack.pop().expect("native right operand");
                let a = stack.pop().expect("native left operand");
                stack.push(std::array::from_fn(|i| {
                    if op == 5 { a[i] * b[i] } else { a[i] + b[i] }
                }));
            }
            0x07 => {
                let a = stack.pop().unwrap();
                stack.push(a.map(|v| if v == 0.0 { 1.0 } else { 0.0 }));
            }
            _ => panic!("Unexpected native operation {op:02X}"),
        }
    }
    assert!(
        stack.is_empty(),
        "Native outputs must consume the expression"
    );
    output
}

#[test]
fn file_lowering_preserves_alias_operands_captures_and_missing_inputs() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_TEST_ARTIFACTS").map(|root| {
        std::path::PathBuf::from(root)
            .join("tfx-arithmetic")
            .into_os_string()
    });
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("tfx-arithmetic");
    fs::create_dir_all(&output).unwrap();
    let source = json!({
        "mode":"lower",
        "data":"4202420042010506550054005200540042030652014203075202",
        "constant_count":4,"output_count":3,"sampler_count":0,
        "constants":[[2,-3,0.5,4],[5,2,4,-0.5],[7,11,13,17],[0,2,0,-4]]
    });
    let input = output.join("source.json");
    write_json(&input, &source).unwrap();
    // Use a fresh destination, just as the file converter requires for a real request.
    let destination = tempfile::tempdir_in(&output).unwrap();
    let converted = destination.path().join("converted");
    run(&input, &converted).unwrap();
    let result = read(&converted.join("result.json")).unwrap();
    let code = hex::decode(result["code"].as_str().unwrap()).unwrap();
    let constants: Vec<[f32; 4]> = serde_json::from_value(source["constants"].clone()).unwrap();
    let actual = evaluate(&code, &constants);
    // Independent arithmetic: c + a*b, that captured result + d, and is_zero(d).
    let expected = [
        [17.0, 5.0, 15.0, 15.0],
        [17.0, 7.0, 15.0, 11.0],
        [1.0, 0.0, 1.0, 0.0],
    ];
    assert_eq!(actual, expected);
    let mut dependencies = Vec::new();
    for op in [0x05, 0x06] {
        let missing = lower(
            &[0x5C, 0x12, 0x34, 0x56, 0x78, 0x42, 0, op, 0x52, 0],
            &bindings(),
        )
        .unwrap();
        assert!(missing.require_runtime_inputs().is_err());
        assert!(
            missing.code.is_empty(),
            "An unresolved left operand cannot be emitted"
        );
        assert_eq!(
            missing.evidence[0]["unresolved"],
            json!(["object channel 12345678"])
        );
        assert!(lower(&[0x42, 0, op, 0x52, 0], &bindings()).is_err());
        dependencies.push(json!({"source_opcode":op,"evidence":missing.evidence}));
    }
    write_json(&output.join("converted.json"), &result).unwrap();
    write_json(
        &output.join("readback.json"),
        &json!({
            "actual":actual,"expected":expected,"missing_inputs":dependencies,
            "scope":"Modern TFX file lowering and Shadowkeep arithmetic readback",
            "gameplay_verified":false
        }),
    )
    .unwrap();
}
