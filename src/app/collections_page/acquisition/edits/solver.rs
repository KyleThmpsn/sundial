//! Solve the expression's branches before falling back to a bounded candidate search.
//! Every plan is checked by the native-compatible evaluator and again after persistence.
use super::super::expression::*;
use super::*;
mod branches;
use crate::catalog::CollectionConditionTokenDef as Token;

#[derive(Clone, Copy)]
enum Goal {
    Equal(i32),
    Different(i32),
    AtLeast(i32),
    AtMost(i32),
}
impl Goal {
    fn accepts(self, value: i32) -> bool {
        match self {
            Self::Equal(n) => value == n,
            Self::Different(n) => value != n,
            Self::AtLeast(n) => value >= n,
            Self::AtMost(n) => value <= n,
        }
    }
    fn targets(self, current: Option<i32>) -> Vec<i32> {
        let mut out = Vec::new();
        if let Some(n) = current.filter(|n| self.accepts(*n)) {
            out.push(n);
        }
        match self {
            Self::Equal(n) | Self::AtLeast(n) | Self::AtMost(n) => out.push(n),
            Self::Different(n) => {
                out.extend(n.checked_add(1));
                out.extend(n.checked_sub(1));
            }
        }
        out.extend([0, 1].into_iter().filter(|n| self.accepts(*n)));
        out.dedup();
        out
    }
}

struct Expr {
    tokens: Vec<Token>,
    children: Vec<Expr>,
    kind: u32,
    operand: usize,
}
fn parse(
    tokens: &[Token],
    catalog: &Catalog,
    active: &mut Vec<usize>,
    budget: &mut usize,
) -> Option<Expr> {
    if active.len() >= 64 {
        return None;
    }
    let mut stack: Vec<Expr> = Vec::new();
    for token in tokens {
        *budget = budget.checked_sub(1)?;
        if token.kind == POOL_INSTRUCTION {
            let index = token.operand as usize;
            if active.contains(&index) {
                return None;
            }
            active.push(index);
            let expr = parse(catalog.shared_expression(index)?, catalog, active, budget)?;
            active.pop();
            stack.push(expr);
            continue;
        }
        let count = match token.kind {
            FLAG_INSTRUCTION | VALUE_INSTRUCTION | LITERAL_INSTRUCTION => 0,
            NOT_INSTRUCTION
            | NEGATE_NUMBER_INSTRUCTION
            | BITWISE_NOT_INSTRUCTION
            | HASH_NUMBER_INSTRUCTION => 1,
            kind if is_supported_instruction(kind) => 2,
            _ => return None,
        };
        let children = stack.split_off(stack.len().checked_sub(count)?);
        let mut program = children
            .iter()
            .flat_map(|child| child.tokens.clone())
            .collect::<Vec<_>>();
        program.push(token.clone());
        stack.push(Expr {
            tokens: program,
            children,
            kind: token.kind,
            operand: token.operand as usize,
        });
        if stack.len() > 256 {
            return None;
        }
    }
    if stack.is_empty() {
        return Some(Expr {
            tokens: vec![Token {
                kind: 11,
                operand: 0,
            }],
            children: vec![],
            kind: 11,
            operand: 0,
        });
    }
    (stack.len() == 1).then(|| stack.pop().unwrap())
}

struct Solver<'a> {
    snapshot: &'a CollectionStateSnapshot,
    catalog: &'a Catalog,
    budget: usize,
}
type Plan = Vec<CollectionStateEdit>;
impl Solver<'_> {
    fn value(&self, expr: &Expr, plan: &Plan) -> Option<i32> {
        match evaluate_expression_value_with(
            &expr.tokens,
            &[],
            |index| {
                plan.iter()
                    .find_map(|edit| match edit {
                        CollectionStateEdit::Flag {
                            definition_index,
                            set,
                        } if *definition_index == index => Some(*set),
                        _ => None,
                    })
                    .or_else(|| self.snapshot.evaluated_flag(index, self.catalog))
            },
            |index| {
                plan.iter()
                    .find_map(|edit| match edit {
                        CollectionStateEdit::Value {
                            definition_index,
                            value,
                        } if *definition_index == index => Some(*value),
                        _ => None,
                    })
                    .or_else(|| self.snapshot.evaluated_value(index, self.catalog))
            },
        )? {
            ExpressionValue::Boolean(value) => Some(i32::from(value)),
            ExpressionValue::Number(value) => Some(value),
            ExpressionValue::Unknown => None,
        }
    }
    fn solve(&mut self, expr: &Expr, goal: Goal, plan: &Plan, depth: usize) -> Vec<Plan> {
        if self.budget == 0 || depth > 64 {
            return Vec::new();
        }
        self.budget -= 1;
        let current = self.value(expr, plan);
        if current.is_some_and(|value| goal.accepts(value)) {
            return vec![plan.clone()];
        }
        let targets = goal.targets(current);
        let mut out = Vec::new();
        match expr.kind {
            FLAG_INSTRUCTION | VALUE_INSTRUCTION => out = self.leaf(expr, goal, plan, depth),
            NOT_INSTRUCTION => {
                for n in [0, 1].into_iter().filter(|n| goal.accepts(*n)) {
                    out.extend(self.solve(
                        &expr.children[0],
                        if n == 0 {
                            Goal::Different(0)
                        } else {
                            Goal::Equal(0)
                        },
                        plan,
                        depth + 1,
                    ));
                }
            }
            NEGATE_NUMBER_INSTRUCTION | BITWISE_NOT_INSTRUCTION => {
                for n in targets {
                    out.extend(self.solve(
                        &expr.children[0],
                        Goal::Equal(if expr.kind == NEGATE_NUMBER_INSTRUCTION {
                            n.wrapping_neg()
                        } else {
                            !n
                        }),
                        plan,
                        depth + 1,
                    ));
                }
            }
            OR_INSTRUCTION | AND_INSTRUCTION | NOR_INSTRUCTION | NAND_INSTRUCTION => {
                out = self.boolean(expr, goal, plan, depth);
            }
            EQUAL_INSTRUCTION
            | NOT_EQUAL_INSTRUCTION
            | NOT_EQUAL_ALTERNATE_INSTRUCTION
            | GREATER_THAN_INSTRUCTION
            | GREATER_OR_EQUAL_INSTRUCTION
            | LESS_THAN_INSTRUCTION
            | LESS_OR_EQUAL_INSTRUCTION => out = self.compare(expr, goal, plan, depth),
            ADD_INSTRUCTION
            | SUBTRACT_INSTRUCTION
            | MULTIPLY_INSTRUCTION
            | DIVIDE_INSTRUCTION
            | MODULO_INSTRUCTION
            | BITWISE_AND_INSTRUCTION
            | BITWISE_OR_INSTRUCTION
            | BITWISE_XOR_INSTRUCTION => out = self.arithmetic(expr, goal, plan, depth),
            _ => {}
        }
        out.retain(|plan| {
            self.value(expr, plan)
                .is_some_and(|value| goal.accepts(value))
        });
        out.sort_by_key(Vec::len);
        out.dedup();
        out.truncate(8);
        out
    }
}

fn comparison(kind: u32, desired: bool, reverse: bool, n: i32) -> Option<Goal> {
    let kind = if reverse {
        match kind {
            13 => 15,
            14 => 16,
            15 => 13,
            16 => 14,
            _ => kind,
        }
    } else {
        kind
    };
    Some(match (kind, desired) {
        (8, true) | (6 | 9, false) => Goal::Equal(n),
        (8, false) | (6 | 9, true) => Goal::Different(n),
        (13, true) | (16, false) => Goal::AtLeast(n.checked_add(1)?),
        (14, true) | (15, false) => Goal::AtLeast(n),
        (15, true) | (14, false) => Goal::AtMost(n.checked_sub(1)?),
        (16, true) | (13, false) => Goal::AtMost(n),
        _ => return None,
    })
}

fn inverse(kind: u32, right: bool, anchor: i32, target: i32) -> Vec<i32> {
    match kind {
        ADD_INSTRUCTION => vec![target.wrapping_sub(anchor)],
        SUBTRACT_INSTRUCTION => vec![if right {
            anchor.wrapping_sub(target)
        } else {
            target.wrapping_add(anchor)
        }],
        MULTIPLY_INSTRUCTION => {
            if anchor == 0 {
                return if target == 0 { vec![0] } else { vec![] };
            }
            // Solve modulo 2^32, matching wrapping native multiplication.
            let shift = (anchor as u32).trailing_zeros();
            if (target as u32) & ((1_u32 << shift) - 1) != 0 {
                return vec![];
            }
            let odd = (anchor as u32) >> shift;
            let mut reciprocal = odd;
            for _ in 0..5 {
                reciprocal =
                    reciprocal.wrapping_mul(2_u32.wrapping_sub(odd.wrapping_mul(reciprocal)));
            }
            let value = ((target as u32) >> shift).wrapping_mul(reciprocal);
            let mask = u32::MAX >> shift;
            vec![(value & mask) as i32, (value | !mask) as i32]
        }
        DIVIDE_INSTRUCTION if !right && anchor != 0 => {
            let base = i64::from(target) * i64::from(anchor);
            [
                base,
                base + i64::from(anchor.abs_diff(0)) - 1,
                base - i64::from(anchor.abs_diff(0)) + 1,
            ]
            .into_iter()
            .filter_map(|n| i32::try_from(n).ok())
            .collect()
        }
        DIVIDE_INSTRUCTION if right && target != 0 => {
            anchor.checked_div(target).into_iter().collect()
        }
        MODULO_INSTRUCTION if !right => vec![
            target,
            target.wrapping_add(anchor),
            target.wrapping_sub(anchor),
        ],
        BITWISE_AND_INSTRUCTION => vec![target],
        BITWISE_OR_INSTRUCTION => vec![target & !anchor, target],
        BITWISE_XOR_INSTRUCTION => vec![target ^ anchor],
        _ => vec![],
    }
}

pub(super) fn solve(
    tokens: &[Token],
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    desired: bool,
) -> Option<Plan> {
    let expr = parse(tokens, catalog, &mut Vec::new(), &mut 4096)?;
    let mut solver = Solver {
        snapshot,
        catalog,
        budget: 8192,
    };
    solver
        .solve(
            &expr,
            if desired {
                Goal::Different(0)
            } else {
                Goal::Equal(0)
            },
            &Vec::new(),
            0,
        )
        .into_iter()
        .map(|plan| {
            plan.into_iter()
                .filter(|edit| collection_edit_changes_state(*edit, snapshot, catalog))
                .collect::<Vec<_>>()
        })
        .filter(|plan| !plan.is_empty())
        .min_by_key(Vec::len)
}

#[cfg(test)]
mod tests;
