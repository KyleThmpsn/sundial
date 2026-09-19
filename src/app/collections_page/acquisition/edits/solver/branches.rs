use super::*;

impl Solver<'_> {
    pub(super) fn leaf(
        &mut self,
        expr: &Expr,
        goal: Goal,
        plan: &Plan,
        _depth: usize,
    ) -> Vec<Plan> {
        let mut out = Vec::new();
        let targets = goal.targets(self.value(expr, plan));

        let flag = expr.kind == FLAG_INSTRUCTION;
        let Some(definition) = (if flag {
            self.catalog.unlock_flag_definition(expr.operand)
        } else {
            self.catalog.unlock_value_definition(expr.operand)
        }) else {
            return out;
        };
        if (flag && definition.bank() == 5)
            || (!flag && definition.bank() == 4)
            || (definition.compact_slot.is_some()
                && !(if flag {
                    matches!(definition.bank(), 1 | 2 | 3 | 6)
                } else {
                    matches!(definition.bank(), 1 | 2)
                }))
        {
            return out;
        }
        for value in targets {
            if expr.kind == FLAG_INSTRUCTION && !matches!(value, 0 | 1) {
                continue;
            }
            let edit = if expr.kind == FLAG_INSTRUCTION {
                CollectionStateEdit::Flag {
                    definition_index: expr.operand,
                    set: value != 0,
                }
            } else {
                CollectionStateEdit::Value {
                    definition_index: expr.operand,
                    value,
                }
            };
            let mut next = plan.clone();
            next.retain(|entry| match (entry, edit) {
                (
                    CollectionStateEdit::Flag {
                        definition_index: a,
                        ..
                    },
                    CollectionStateEdit::Flag {
                        definition_index: b,
                        ..
                    },
                )
                | (
                    CollectionStateEdit::Value {
                        definition_index: a,
                        ..
                    },
                    CollectionStateEdit::Value {
                        definition_index: b,
                        ..
                    },
                ) => *a != b,
                _ => true,
            });
            next.push(edit);
            out.push(next);
        }

        out
    }
    pub(super) fn boolean(
        &mut self,
        expr: &Expr,
        goal: Goal,
        plan: &Plan,
        depth: usize,
    ) -> Vec<Plan> {
        let mut out = Vec::new();

        let (decisive, result) = match expr.kind {
            OR_INSTRUCTION => (true, true),
            AND_INSTRUCTION => (false, false),
            NOR_INSTRUCTION => (true, false),
            _ => (false, true),
        };
        if goal.accepts(i32::from(result)) {
            for child in &expr.children {
                out.extend(self.solve(
                    child,
                    if decisive {
                        Goal::Different(0)
                    } else {
                        Goal::Equal(0)
                    },
                    plan,
                    depth + 1,
                ));
            }
        }
        for (a, b) in [(false, false), (true, false), (false, true), (true, true)] {
            let value = match expr.kind {
                OR_INSTRUCTION => a || b,
                AND_INSTRUCTION => a && b,
                NOR_INSTRUCTION => !(a || b),
                _ => !(a && b),
            };
            if !goal.accepts(i32::from(value)) {
                continue;
            }
            let left = if a {
                Goal::Different(0)
            } else {
                Goal::Equal(0)
            };
            let right = if b {
                Goal::Different(0)
            } else {
                Goal::Equal(0)
            };
            for first in self.solve(&expr.children[0], left, plan, depth + 1) {
                out.extend(self.solve(&expr.children[1], right, &first, depth + 1));
            }
        }

        out
    }
    pub(super) fn compare(
        &mut self,
        expr: &Expr,
        goal: Goal,
        plan: &Plan,
        depth: usize,
    ) -> Vec<Plan> {
        let mut out = Vec::new();

        for desired in [false, true]
            .into_iter()
            .filter(|n| goal.accepts(i32::from(*n)))
        {
            for side in [0, 1] {
                let other = &expr.children[1 - side];
                let mut anchors = self
                    .value(other, plan)
                    .into_iter()
                    .chain([0, 1])
                    .collect::<Vec<_>>();
                anchors.dedup();
                for anchor in anchors {
                    let Some(constraint) = comparison(expr.kind, desired, side == 1, anchor) else {
                        continue;
                    };
                    for first in self.solve(other, Goal::Equal(anchor), plan, depth + 1) {
                        out.extend(self.solve(&expr.children[side], constraint, &first, depth + 1));
                    }
                }
            }
        }

        out
    }
    pub(super) fn arithmetic(
        &mut self,
        expr: &Expr,
        goal: Goal,
        plan: &Plan,
        depth: usize,
    ) -> Vec<Plan> {
        let mut out = Vec::new();
        let targets = goal.targets(self.value(expr, plan));

        for target in targets {
            for side in [0, 1] {
                let other = &expr.children[1 - side];
                let mut anchors = self
                    .value(other, plan)
                    .into_iter()
                    .chain([0, 1])
                    .collect::<Vec<_>>();
                anchors.dedup();
                for anchor in anchors {
                    let inverses = inverse(expr.kind, side == 1, anchor, target);
                    if inverses.is_empty() {
                        continue;
                    }
                    for first in self.solve(other, Goal::Equal(anchor), plan, depth + 1) {
                        for n in &inverses {
                            out.extend(self.solve(
                                &expr.children[side],
                                Goal::Equal(*n),
                                &first,
                                depth + 1,
                            ));
                        }
                    }
                }
            }
        }

        out
    }
}
