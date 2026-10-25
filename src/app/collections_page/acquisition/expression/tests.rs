use super::*;

fn token(kind: u32, operand: u32) -> CollectionConditionTokenDef {
    CollectionConditionTokenDef { kind, operand }
}

fn evaluate(tokens: &[CollectionConditionTokenDef]) -> Option<bool> {
    evaluate_expression_with(tokens, &[], |_| None, |_| None)
}

#[test]
fn supports_package_boolean_programs() {
    assert_eq!(
        evaluate_expression_with(
            &[token(1, 4), token(1, 9), token(3, u32::MAX)],
            &[],
            |index| Some(index == 9),
            |_| None,
        ),
        Some(true)
    );
    assert_eq!(
        evaluate_expression_with(&[token(1, 4), token(2, 0)], &[], |_| Some(false), |_| None,),
        Some(true)
    );
    assert_eq!(
        evaluate(&[token(11, 0), token(11, 0), token(5, 0)]),
        Some(true)
    );
    assert_eq!(
        evaluate(&[token(11, 1), token(11, 1), token(7, 0)]),
        Some(false)
    );
}

#[test]
fn supports_every_documented_comparison_and_arithmetic_opcode() {
    for (opcode, expected) in [
        (6, true),
        (8, false),
        (9, true),
        (13, false),
        (14, false),
        (15, true),
        (16, true),
    ] {
        assert_eq!(
            evaluate(&[token(11, 6), token(11, 7), token(opcode, 0)]),
            Some(expected),
            "opcode {opcode}"
        );
    }

    for (opcode, expected) in [
        (17, 13),
        (18, -1),
        (19, 42),
        (20, 0),
        (21, 6),
        (25, 6),
        (26, 7),
        (27, 1),
    ] {
        let program = [
            token(11, 6),
            token(11, 7),
            token(opcode, 0),
            token(11, expected as u32),
            token(8, 0),
        ];
        assert_eq!(evaluate(&program), Some(true), "opcode {opcode}");
    }

    let combined = 0xE409_7F1F_u32;
    assert_eq!(
        evaluate(&[
            token(11, 6),
            token(11, 7),
            token(24, 0),
            token(11, combined),
            token(8, 0),
        ]),
        Some(true)
    );

    assert_eq!(
        evaluate(&[
            token(11, 1),
            token(NEGATE_NUMBER_INSTRUCTION, 0),
            token(11, 0),
            token(15, 0),
        ]),
        Some(true)
    );
}

#[test]
fn evaluates_the_negative_one_sentinel_used_by_legacy_collectibles() {
    let tokens = [
        token(VALUE_INSTRUCTION, 8029),
        token(LITERAL_INSTRUCTION, 1),
        token(NEGATE_NUMBER_INSTRUCTION, 0),
        token(EQUAL_INSTRUCTION, u32::MAX),
        token(FLAG_INSTRUCTION, 5168),
        token(OR_INSTRUCTION, u32::MAX),
    ];
    assert_eq!(
        evaluate_expression_with(
            &tokens,
            &[],
            |_| Some(false),
            |index| (index == 8029).then_some(-1),
        ),
        Some(true)
    );
    assert_eq!(
        evaluate_expression_with(&tokens, &[], |_| Some(false), |_| Some(0)),
        Some(false)
    );
    assert_eq!(
        evaluate_expression_with(&tokens, &[], |_| Some(true), |_| Some(0)),
        Some(true)
    );
}

#[test]
fn pooled_expressions_keep_their_numeric_result() {
    let pool = vec![vec![token(11, 2), token(11, 3), token(17, 0)]];
    assert_eq!(
        evaluate_expression_with(
            &[token(12, 0), token(11, 5), token(8, 0)],
            &pool,
            |_| None,
            |_| None,
        ),
        Some(true)
    );
}

#[test]
fn pooled_expressions_resolve_state_and_reject_cycles() {
    let pool = vec![
        vec![token(10, 4), token(11, 2), token(19, 0)],
        vec![token(12, 1)],
    ];
    assert_eq!(
        evaluate_expression_with(
            &[token(12, 0), token(11, 6), token(14, 0)],
            &pool,
            |_| None,
            |index| (index == 4).then_some(3),
        ),
        Some(true)
    );
    assert_eq!(
        evaluate_expression_with(&[token(12, 1)], &pool, |_| None, |_| None),
        None
    );
}

#[test]
fn refuses_undocumented_or_underflowing_programs() {
    assert_eq!(evaluate(&[token(99, 1)]), None);
    assert_eq!(evaluate(&[token(3, u32::MAX)]), None);
}

#[test]
fn native_unary_hash_and_complement_use_the_full_integer() {
    for (input, expected) in [
        (0, 0x4B95_F515),
        (0x1234_5678, 0x5B14_54E5),
        (u32::MAX, 0xE316_0FB1),
    ] {
        assert_eq!(
            evaluate(&[
                token(11, input),
                token(23, 0),
                token(11, expected),
                token(8, 0),
            ]),
            Some(true)
        );
    }
    assert_eq!(
        evaluate(&[
            token(11, 0x1234_5678),
            token(28, 0),
            token(11, 0xEDCB_A987),
            token(8, 0),
        ]),
        Some(true)
    );
}

#[test]
fn native_signed_division_and_modulo_return_zero_for_zero_divisors() {
    for (opcode, expected) in [(20, -2_i32), (21, -1)] {
        assert_eq!(
            evaluate(&[
                token(11, (-7_i32) as u32),
                token(11, 3),
                token(opcode, 0),
                token(11, expected as u32),
                token(8, 0),
            ]),
            Some(true)
        );
        assert_eq!(
            evaluate(&[
                token(11, 7),
                token(11, 0),
                token(opcode, 0),
                token(11, 0),
                token(8, 0),
            ]),
            Some(true)
        );
        // Native IDIV traps on this input. Refuse it without crashing Sundial.
        assert_eq!(
            evaluate(&[
                token(11, i32::MIN as u32),
                token(11, u32::MAX),
                token(opcode, 0),
            ]),
            None
        );
    }
}

#[test]
fn absent_conditions_pass_but_empty_numeric_pool_rows_return_zero() {
    assert_eq!(evaluate(&[]), Some(true));
    assert_eq!(
        evaluate_expression_value_with(&[], &[], |_| None, |_| None),
        Some(ExpressionValue::Number(0))
    );
    assert_eq!(
        evaluate_expression_with(
            &[token(12, 0), token(11, 0), token(8, 0)],
            &[vec![]],
            |_| None,
            |_| None,
        ),
        Some(true)
    );
}

#[test]
fn rejects_programs_that_overflow_the_native_stack() {
    let mut program = vec![token(11, 1); 257];
    program.extend(vec![token(17, 0); 256]);
    assert_eq!(evaluate(&program), None);
}

#[test]
fn native_non_unit_final_stack_is_false() {
    assert_eq!(evaluate(&[token(11, 1), token(11, 1)]), Some(false));
}
