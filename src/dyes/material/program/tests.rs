use super::*;

#[test]
fn time_drives_native_outputs_without_accumulating_between_frames() {
    let p = Program {
        outputs: 27,
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
        outputs: 27,
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
