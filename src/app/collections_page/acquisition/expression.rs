use crate::catalog::CollectionConditionTokenDef;

pub(in crate::app::collections_page) const FLAG_INSTRUCTION: u32 = 1;
pub(super) const NOT_INSTRUCTION: u32 = 2;
pub(super) const OR_INSTRUCTION: u32 = 3;
pub(super) const AND_INSTRUCTION: u32 = 4;
pub(super) const NOR_INSTRUCTION: u32 = 5;
pub(super) const NOT_EQUAL_ALTERNATE_INSTRUCTION: u32 = 6;
pub(super) const NAND_INSTRUCTION: u32 = 7;
pub(super) const EQUAL_INSTRUCTION: u32 = 8;
pub(super) const NOT_EQUAL_INSTRUCTION: u32 = 9;
pub(in crate::app::collections_page) const VALUE_INSTRUCTION: u32 = 10;
pub(super) const LITERAL_INSTRUCTION: u32 = 11;
pub(in crate::app::collections_page) const POOL_INSTRUCTION: u32 = 12;
pub(super) const GREATER_THAN_INSTRUCTION: u32 = 13;
pub(super) const GREATER_OR_EQUAL_INSTRUCTION: u32 = 14;
pub(super) const LESS_THAN_INSTRUCTION: u32 = 15;
pub(super) const LESS_OR_EQUAL_INSTRUCTION: u32 = 16;
pub(super) const ADD_INSTRUCTION: u32 = 17;
pub(super) const SUBTRACT_INSTRUCTION: u32 = 18;
pub(super) const MULTIPLY_INSTRUCTION: u32 = 19;
pub(super) const DIVIDE_INSTRUCTION: u32 = 20;
pub(super) const MODULO_INSTRUCTION: u32 = 21;
/// Empirical unary numeric negation retained for legacy package rows; not in the decoded table.
pub(super) const NEGATE_NUMBER_INSTRUCTION: u32 = 22;
pub(super) const FNV1_COMBINE_INSTRUCTION: u32 = 24;
pub(super) const BITWISE_AND_INSTRUCTION: u32 = 25;
pub(super) const BITWISE_OR_INSTRUCTION: u32 = 26;
pub(super) const BITWISE_XOR_INSTRUCTION: u32 = 27;

const FNV1_PRIME: u32 = 0x0100_0193;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum ExpressionValue {
    Unknown,
    Boolean(bool),
    Number(i32),
}

impl ExpressionValue {
    fn truthy(self) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::Boolean(value) => Some(value),
            Self::Number(value) => Some(value != 0),
        }
    }

    fn number(self) -> Option<i32> {
        match self {
            Self::Unknown => None,
            Self::Boolean(value) => Some(i32::from(value)),
            Self::Number(value) => Some(value),
        }
    }
}

pub(in crate::app) const fn is_supported_instruction(kind: u32) -> bool {
    matches!(
        kind,
        FLAG_INSTRUCTION
            | NOT_INSTRUCTION
            | OR_INSTRUCTION
            | AND_INSTRUCTION
            | NOR_INSTRUCTION
            | NOT_EQUAL_ALTERNATE_INSTRUCTION
            | NAND_INSTRUCTION
            | EQUAL_INSTRUCTION
            | NOT_EQUAL_INSTRUCTION
            | VALUE_INSTRUCTION
            | LITERAL_INSTRUCTION
            | POOL_INSTRUCTION
            | GREATER_THAN_INSTRUCTION
            | GREATER_OR_EQUAL_INSTRUCTION
            | LESS_THAN_INSTRUCTION
            | LESS_OR_EQUAL_INSTRUCTION
            | ADD_INSTRUCTION
            | SUBTRACT_INSTRUCTION
            | MULTIPLY_INSTRUCTION
            | DIVIDE_INSTRUCTION
            | MODULO_INSTRUCTION
            | NEGATE_NUMBER_INSTRUCTION
            | FNV1_COMBINE_INSTRUCTION
            | BITWISE_AND_INSTRUCTION
            | BITWISE_OR_INSTRUCTION
            | BITWISE_XOR_INSTRUCTION
    )
}

fn pop_pair(stack: &mut Vec<ExpressionValue>) -> Option<(ExpressionValue, ExpressionValue)> {
    let right = stack.pop()?;
    let left = stack.pop()?;
    Some((left, right))
}

fn push_numeric_comparison(
    stack: &mut Vec<ExpressionValue>,
    compare: impl FnOnce(i32, i32) -> bool,
) -> Option<()> {
    let (left, right) = pop_pair(stack)?;
    let result = match (left.number(), right.number()) {
        (Some(left), Some(right)) => ExpressionValue::Boolean(compare(left, right)),
        _ => ExpressionValue::Unknown,
    };
    stack.push(result);
    Some(())
}

fn push_numeric_operation(
    stack: &mut Vec<ExpressionValue>,
    operation: impl FnOnce(i32, i32) -> Option<i32>,
) -> Option<()> {
    let (left, right) = pop_pair(stack)?;
    let result = match (left.number(), right.number()) {
        (Some(left), Some(right)) => ExpressionValue::Number(operation(left, right)?),
        _ => ExpressionValue::Unknown,
    };
    stack.push(result);
    Some(())
}

fn boolean_or(left: ExpressionValue, right: ExpressionValue) -> ExpressionValue {
    match (left.truthy(), right.truthy()) {
        (Some(true), _) | (_, Some(true)) => ExpressionValue::Boolean(true),
        (Some(false), Some(false)) => ExpressionValue::Boolean(false),
        _ => ExpressionValue::Unknown,
    }
}

fn boolean_and(left: ExpressionValue, right: ExpressionValue) -> ExpressionValue {
    match (left.truthy(), right.truthy()) {
        (Some(false), _) | (_, Some(false)) => ExpressionValue::Boolean(false),
        (Some(true), Some(true)) => ExpressionValue::Boolean(true),
        _ => ExpressionValue::Unknown,
    }
}

fn negate(value: ExpressionValue) -> ExpressionValue {
    value.truthy().map_or(ExpressionValue::Unknown, |value| {
        ExpressionValue::Boolean(!value)
    })
}

fn apply_stack_instruction(kind: u32, stack: &mut Vec<ExpressionValue>) -> Option<bool> {
    let result = match kind {
        NOT_INSTRUCTION => negate(stack.pop()?),
        OR_INSTRUCTION => {
            let (left, right) = pop_pair(stack)?;
            boolean_or(left, right)
        }
        AND_INSTRUCTION => {
            let (left, right) = pop_pair(stack)?;
            boolean_and(left, right)
        }
        NOR_INSTRUCTION => {
            let (left, right) = pop_pair(stack)?;
            negate(boolean_or(left, right))
        }
        NAND_INSTRUCTION => {
            let (left, right) = pop_pair(stack)?;
            negate(boolean_and(left, right))
        }
        EQUAL_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left == right).map(|()| true);
        }
        NOT_EQUAL_ALTERNATE_INSTRUCTION | NOT_EQUAL_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left != right).map(|()| true);
        }
        GREATER_THAN_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left > right).map(|()| true);
        }
        GREATER_OR_EQUAL_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left >= right).map(|()| true);
        }
        LESS_THAN_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left < right).map(|()| true);
        }
        LESS_OR_EQUAL_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left <= right).map(|()| true);
        }
        ADD_INSTRUCTION => {
            return push_numeric_operation(stack, |left, right| Some(left.wrapping_add(right)))
                .map(|()| true);
        }
        SUBTRACT_INSTRUCTION => {
            return push_numeric_operation(stack, |left, right| Some(left.wrapping_sub(right)))
                .map(|()| true);
        }
        MULTIPLY_INSTRUCTION => {
            return push_numeric_operation(stack, |left, right| Some(left.wrapping_mul(right)))
                .map(|()| true);
        }
        DIVIDE_INSTRUCTION => {
            return push_numeric_operation(stack, i32::checked_div).map(|()| true);
        }
        MODULO_INSTRUCTION => {
            return push_numeric_operation(stack, i32::checked_rem).map(|()| true);
        }
        NEGATE_NUMBER_INSTRUCTION => {
            let value = stack.pop()?;
            stack.push(value.number().map_or(ExpressionValue::Unknown, |value| {
                ExpressionValue::Number(value.wrapping_neg())
            }));
            return Some(true);
        }
        FNV1_COMBINE_INSTRUCTION => {
            return push_numeric_operation(stack, |left, right| {
                Some(((left as u32).wrapping_mul(FNV1_PRIME) ^ right as u32) as i32)
            })
            .map(|()| true);
        }
        BITWISE_AND_INSTRUCTION => {
            return push_numeric_operation(stack, |left, right| Some(left & right)).map(|()| true);
        }
        BITWISE_OR_INSTRUCTION => {
            return push_numeric_operation(stack, |left, right| Some(left | right)).map(|()| true);
        }
        BITWISE_XOR_INSTRUCTION => {
            return push_numeric_operation(stack, |left, right| Some(left ^ right)).map(|()| true);
        }
        _ => return Some(false),
    };
    stack.push(result);
    Some(true)
}

fn evaluate_program_with(
    tokens: &[CollectionConditionTokenDef],
    expression_pool: &[Vec<CollectionConditionTokenDef>],
    active_pool_rows: &mut [bool],
    flag: &mut impl FnMut(usize) -> Option<bool>,
    value: &mut impl FnMut(usize) -> Option<i32>,
) -> Option<ExpressionValue> {
    if tokens.is_empty() {
        return Some(ExpressionValue::Boolean(true));
    }

    let mut stack = Vec::new();
    for token in tokens {
        match token.kind {
            FLAG_INSTRUCTION => {
                let index = token.operand as usize;
                stack.push(flag(index).map_or(ExpressionValue::Unknown, ExpressionValue::Boolean));
            }
            VALUE_INSTRUCTION => {
                let index = token.operand as usize;
                stack.push(value(index).map_or(ExpressionValue::Unknown, ExpressionValue::Number));
            }
            LITERAL_INSTRUCTION => stack.push(ExpressionValue::Number(token.operand as i32)),
            POOL_INSTRUCTION => {
                let index = token.operand as usize;
                let program = expression_pool.get(index)?;
                let active = active_pool_rows.get_mut(index)?;
                if *active {
                    return None;
                }
                *active = true;
                let result =
                    evaluate_program_with(program, expression_pool, active_pool_rows, flag, value);
                active_pool_rows[index] = false;
                stack.push(result?);
            }
            _ if apply_stack_instruction(token.kind, &mut stack)? => {}
            _ => return None,
        }
    }

    if stack.len() == 1 {
        stack.pop()
    } else {
        // The native evaluator returns numeric zero when a program does not leave one result.
        Some(ExpressionValue::Number(0))
    }
}

pub(super) fn evaluate_expression_with(
    tokens: &[CollectionConditionTokenDef],
    expression_pool: &[Vec<CollectionConditionTokenDef>],
    flag: impl FnMut(usize) -> Option<bool>,
    value: impl FnMut(usize) -> Option<i32>,
) -> Option<bool> {
    evaluate_expression_value_with(tokens, expression_pool, flag, value)?.truthy()
}

pub(in crate::app) fn evaluate_expression_value_with(
    tokens: &[CollectionConditionTokenDef],
    expression_pool: &[Vec<CollectionConditionTokenDef>],
    mut flag: impl FnMut(usize) -> Option<bool>,
    mut value: impl FnMut(usize) -> Option<i32>,
) -> Option<ExpressionValue> {
    let mut active_pool_rows = vec![false; expression_pool.len()];
    evaluate_program_with(
        tokens,
        expression_pool,
        &mut active_pool_rows,
        &mut flag,
        &mut value,
    )
}

#[cfg(test)]
mod tests {
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

        let combined = 6_u32.wrapping_mul(FNV1_PRIME) ^ 7;
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
        assert_eq!(evaluate(&[token(11, 6), token(11, 7), token(23, 0)]), None);
        assert_eq!(evaluate(&[token(99, 1)]), None);
        assert_eq!(evaluate(&[token(3, u32::MAX)]), None);
        assert_eq!(evaluate(&[token(11, 1), token(11, 0), token(20, 0)]), None);
    }

    #[test]
    fn native_non_unit_final_stack_is_false() {
        assert_eq!(evaluate(&[token(11, 1), token(11, 1)]), Some(false));
    }
}
