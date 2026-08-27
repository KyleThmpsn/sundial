use crate::catalog::CollectionConditionTokenDef;

pub(in crate::app::collections_page) const FLAG_INSTRUCTION: u32 = 1;
pub(super) const NOT_INSTRUCTION: u32 = 2;
pub(super) const OR_INSTRUCTION: u32 = 3;
pub(super) const AND_INSTRUCTION: u32 = 4;
pub(super) const EQUAL_INSTRUCTION: u32 = 8;
pub(super) const NOT_EQUAL_INSTRUCTION: u32 = 9;
pub(in crate::app::collections_page) const VALUE_INSTRUCTION: u32 = 10;
pub(super) const LITERAL_INSTRUCTION: u32 = 11;
pub(in crate::app::collections_page) const OBJECTIVE_INSTRUCTION: u32 = 12;
pub(super) const GREATER_THAN_INSTRUCTION: u32 = 13;
pub(super) const GREATER_OR_EQUAL_INSTRUCTION: u32 = 14;
pub(super) const LEGACY_LITERAL_ENCODING_INSTRUCTION: u32 = 22;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExpressionValue {
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

fn apply_stack_instruction(
    kind: u32,
    operand: u32,
    stack: &mut Vec<ExpressionValue>,
) -> Option<bool> {
    let result = match kind {
        NOT_INSTRUCTION => {
            let value = stack.pop()?;
            value.truthy().map_or(ExpressionValue::Unknown, |value| {
                ExpressionValue::Boolean(!value)
            })
        }
        OR_INSTRUCTION => {
            let (left, right) = pop_pair(stack)?;
            match (left.truthy(), right.truthy()) {
                (Some(true), _) | (_, Some(true)) => ExpressionValue::Boolean(true),
                (Some(false), Some(false)) => ExpressionValue::Boolean(false),
                _ => ExpressionValue::Unknown,
            }
        }
        AND_INSTRUCTION => {
            let (left, right) = pop_pair(stack)?;
            match (left.truthy(), right.truthy()) {
                (Some(false), _) | (_, Some(false)) => ExpressionValue::Boolean(false),
                (Some(true), Some(true)) => ExpressionValue::Boolean(true),
                _ => ExpressionValue::Unknown,
            }
        }
        EQUAL_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left == right).map(|()| true);
        }
        NOT_EQUAL_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left != right).map(|()| true);
        }
        GREATER_THAN_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left > right).map(|()| true);
        }
        GREATER_OR_EQUAL_INSTRUCTION => {
            return push_numeric_comparison(stack, |left, right| left >= right).map(|()| true);
        }
        LEGACY_LITERAL_ENCODING_INSTRUCTION if operand == 0 => stack.pop()?,
        _ => return Some(false),
    };
    stack.push(result);
    Some(true)
}

pub(super) fn evaluate_expression_with(
    tokens: &[CollectionConditionTokenDef],
    mut flag: impl FnMut(usize) -> Option<bool>,
    mut value: impl FnMut(usize) -> Option<i32>,
    mut objective: impl FnMut(usize) -> Option<bool>,
) -> Option<bool> {
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
            OBJECTIVE_INSTRUCTION => {
                let index = token.operand as usize;
                stack.push(
                    objective(index).map_or(ExpressionValue::Unknown, ExpressionValue::Boolean),
                );
            }
            _ if apply_stack_instruction(token.kind, token.operand, &mut stack)? => {}
            _ => return None,
        }
    }
    (stack.len() == 1).then(|| stack[0].truthy()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(kind: u32, operand: u32) -> CollectionConditionTokenDef {
        CollectionConditionTokenDef { kind, operand }
    }

    #[test]
    fn supports_package_boolean_programs() {
        assert_eq!(
            evaluate_expression_with(
                &[token(1, 4), token(1, 9), token(3, u32::MAX)],
                |index| Some(index == 9),
                |_| None,
                |_| None,
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(
                &[token(1, 4), token(2, 0)],
                |_| Some(false),
                |_| None,
                |_| None,
            ),
            Some(true)
        );
    }

    #[test]
    fn supports_package_value_comparisons() {
        assert_eq!(
            evaluate_expression_with(
                &[token(10, 465), token(11, 20), token(14, u32::MAX)],
                |_| None,
                |_| Some(20),
                |_| None,
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(
                &[token(10, 842), token(11, 10), token(8, u32::MAX)],
                |_| None,
                |_| Some(9),
                |_| None,
            ),
            Some(false)
        );
        assert_eq!(
            evaluate_expression_with(
                &[
                    token(VALUE_INSTRUCTION, 1_613),
                    token(LITERAL_INSTRUCTION, 0),
                    token(GREATER_THAN_INSTRUCTION, u32::MAX),
                ],
                |_| None,
                |_| Some(1),
                |_| None,
            ),
            Some(true)
        );
    }

    #[test]
    fn resolves_legacy_quest_literal_encoding() {
        let quest_completed = [
            token(VALUE_INSTRUCTION, 8_029),
            token(LITERAL_INSTRUCTION, 1),
            token(LEGACY_LITERAL_ENCODING_INSTRUCTION, 0),
            token(EQUAL_INSTRUCTION, u32::MAX),
        ];
        assert_eq!(
            evaluate_expression_with(
                &quest_completed,
                |_| None,
                |index| (index == 8_029).then_some(1),
                |_| None,
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(
                &quest_completed,
                |_| None,
                |index| (index == 8_029).then_some(0),
                |_| None,
            ),
            Some(false)
        );
    }

    #[test]
    fn refuses_unknown_or_malformed_programs() {
        assert_eq!(
            evaluate_expression_with(&[token(99, 1)], |_| None, |_| None, |_| None),
            None
        );
        assert_eq!(
            evaluate_expression_with(&[token(3, u32::MAX)], |_| None, |_| None, |_| None),
            None
        );
    }

    #[test]
    fn resolves_divinity_style_objective_or_flag_programs() {
        let program = [
            token(OBJECTIVE_INSTRUCTION, 6_383),
            token(FLAG_INSTRUCTION, 10_514),
            token(OR_INSTRUCTION, u32::MAX),
        ];
        assert_eq!(
            evaluate_expression_with(
                &program,
                |index| (index == 10_514).then_some(true),
                |_| None,
                |_| None,
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(&program, |_| Some(false), |_| None, |_| None),
            None
        );
        assert_eq!(
            evaluate_expression_with(&program, |_| Some(false), |_| None, |_| Some(true)),
            Some(true)
        );
    }
}
