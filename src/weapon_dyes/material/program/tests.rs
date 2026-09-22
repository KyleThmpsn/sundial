use super::*;

#[test]
fn time_drives_native_outputs_without_accumulating_between_frames() {
    let p = Program {
        constants: vec![[0.25; 4], [0.5; 4]],
        ops: vec![
            Op::Time,
            Op::Constant(0),
            Op::Binary(3),
            Op::Unary(0x1F),
            Op::Constant(1),
            Op::Constant(1),
            Op::Ternary(0x12),
            Op::Store(3),
        ],
    };
    let base = [[0.25; 4]; 27];
    let first = p.run(base, 0.0).unwrap();
    let next = p.run(base, 2.0).unwrap();
    assert!((first[3][0] - 1.0).abs() < 1e-5);
    assert!(next[3][0].abs() < 1e-5);
    assert_eq!(first[9], base[9]);
    assert_eq!(p.run(base, 0.0).unwrap(), first);
    assert!(p.run(base, f32::NAN).is_err());
}

#[test]
fn stack_and_output_errors_are_rejected() {
    let p = |ops| Program {
        constants: vec![],
        ops,
    };
    assert!(p(vec![Op::Store(0)]).run([[0.0; 4]; 27], 0.0).is_err());
    assert!(
        p(vec![Op::Time, Op::Store(27)])
            .run([[0.0; 4]; 27], 0.0)
            .is_err()
    );
    assert!(p(vec![Op::Time]).run([[0.0; 4]; 27], 0.0).is_err());
    assert!(p(vec![Op::PushTemp(0)]).run([[0.0; 4]; 27], 0.0).is_err());
    assert!(p(vec![Op::Constant(0)]).run([[0.0; 4]; 27], 0.0).is_err());
}

#[test]
fn merges_keep_native_uv_scale_and_offset_components() {
    let a = [2.0, 3.0, 4.0, 5.0];
    let b = [6.0, 7.0, 8.0, 9.0];
    assert_eq!(binary(0x0C, a, b), [2.0, 6.0, 7.0, 8.0]);
    assert_eq!(binary(0x0D, a, b), [2.0, 3.0, 6.0, 7.0]);
    assert_eq!(binary(0x0E, a, b), [2.0, 3.0, 4.0, 6.0]);
}

#[test]
#[ignore = "requires local installed dye scope survey"]
fn installed_dye_programs_evaluate_finitely_and_report_unsupported_inputs() {
    let rows: serde_json::Value =
        serde_json::from_slice(&std::fs::read("docs/model-preview/dye-tfx.json").unwrap()).unwrap();
    let mut supported = 0;
    let mut unsupported = std::collections::BTreeMap::<String, usize>::new();
    for row in rows.as_array().unwrap() {
        let scope = std::fs::read(format!(
            "docs/model-preview/payloads/{}.bin",
            row["scope"].as_str().unwrap()
        ))
        .unwrap();
        match Program::read(&scope) {
            Ok(Some(program)) => {
                for t in [0.0, 0.1, 1.5, 10.0, 60.0, 3600.0] {
                    assert!(
                        program
                            .run([[0.0; 4]; 27], t)
                            .unwrap()
                            .iter()
                            .flatten()
                            .all(|v| v.is_finite())
                    );
                }
                supported += 1;
            }
            Err(error) => *unsupported.entry(error).or_default() += 1,
            Ok(None) => panic!("Surveyed animated scope unexpectedly empty"),
        }
    }
    println!("{supported} supported dye rows, unsupported {unsupported:?}");
    assert!(supported > 100);
}
