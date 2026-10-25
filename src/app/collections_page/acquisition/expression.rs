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
pub(super) const NEGATE_NUMBER_INSTRUCTION: u32 = 22;
pub(super) const HASH_NUMBER_INSTRUCTION: u32 = 23;
pub(super) const HASH_COMBINE_INSTRUCTION: u32 = 24;
pub(super) const BITWISE_AND_INSTRUCTION: u32 = 25;
pub(super) const BITWISE_OR_INSTRUCTION: u32 = 26;
pub(super) const BITWISE_XOR_INSTRUCTION: u32 = 27;
pub(super) const BITWISE_NOT_INSTRUCTION: u32 = 28;

const FNV_PRIME: u32 = 0x0100_0193;
const FNV_OFFSET_BASIS: u32 = 0x811C_9DC5;
const NATIVE_STACK_CAPACITY: usize = 256;

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
            | HASH_NUMBER_INSTRUCTION
            | HASH_COMBINE_INSTRUCTION
            | BITWISE_AND_INSTRUCTION
            | BITWISE_OR_INSTRUCTION
            | BITWISE_XOR_INSTRUCTION
            | BITWISE_NOT_INSTRUCTION
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

// Client build 86657, evaluator RVA 0x554310. The hash arms process the four
// big-endian bytes, XORing each byte before multiplying. This is FNV-1a.
fn hash_number(seed: u32, value: i32) -> i32 {
    value.to_be_bytes().into_iter().fold(seed, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(FNV_PRIME)
    }) as i32
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
            return push_numeric_operation(stack, |left, right| {
                if right == 0 {
                    Some(0)
                } else {
                    left.checked_div(right)
                }
            })
            .map(|()| true);
        }
        MODULO_INSTRUCTION => {
            return push_numeric_operation(stack, |left, right| {
                if right == 0 {
                    Some(0)
                } else {
                    left.checked_rem(right)
                }
            })
            .map(|()| true);
        }
        NEGATE_NUMBER_INSTRUCTION => {
            let value = stack.pop()?;
            stack.push(value.number().map_or(ExpressionValue::Unknown, |value| {
                ExpressionValue::Number(value.wrapping_neg())
            }));
            return Some(true);
        }
        HASH_NUMBER_INSTRUCTION | BITWISE_NOT_INSTRUCTION => {
            let value = stack.pop()?;
            stack.push(value.number().map_or(ExpressionValue::Unknown, |value| {
                ExpressionValue::Number(if kind == HASH_NUMBER_INSTRUCTION {
                    hash_number(FNV_OFFSET_BASIS, value)
                } else {
                    !value
                })
            }));
            return Some(true);
        }
        HASH_COMBINE_INSTRUCTION => {
            return push_numeric_operation(stack, |left, right| {
                Some(hash_number(left as u32, right))
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
        return Some(ExpressionValue::Number(0));
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
        if stack.len() > NATIVE_STACK_CAPACITY {
            return None;
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
    // An absent condition passes. Numeric programs, including pool rows, return zero.
    if tokens.is_empty() {
        return Some(true);
    }
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
mod tests;
