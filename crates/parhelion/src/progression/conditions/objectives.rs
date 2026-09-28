use super::*;
pub(crate) fn pools_for_objective_roots(
    pools: &[u8],
    pool_rows: usize,
    roots: &BTreeSet<usize>,
    source_acquired_flag_index: u16,
) -> AuthoringResult<BTreeSet<usize>> {
    let mut candidates = BTreeMap::new();
    for &root in roots {
        let closure = pool_dependency_closure(pools, pool_rows, root)?;
        let relevant = closure
            .iter()
            .filter(|&&index| {
                let descriptor = pool_rows
                    + index * SHARED_EXPRESSION_POOL_ROW_SIZE
                    + SHARED_EXPRESSION_DESCRIPTOR_OFFSET;
                numeric_program_layout(pools, descriptor).is_ok_and(|layout| {
                    layout.tokens.iter().any(|token| {
                        *token == (NUMERIC_FLAG_INSTRUCTION, source_acquired_flag_index)
                    })
                })
            })
            .copied()
            .collect::<Vec<_>>();
        if relevant.is_empty() {
            continue;
        }
        let additive = relevant
            .iter()
            .copied()
            .filter(|&index| {
                let descriptor = pool_rows
                    + index * SHARED_EXPRESSION_POOL_ROW_SIZE
                    + SHARED_EXPRESSION_DESCRIPTOR_OFFSET;
                numeric_program_layout(pools, descriptor).is_ok_and(|layout| {
                    layout
                        .tokens
                        .iter()
                        .filter(|token| {
                            **token == (NUMERIC_FLAG_INSTRUCTION, source_acquired_flag_index)
                        })
                        .count()
                        == 1
                        && direct_flag_reaches_root_through_add(
                            &layout.tokens,
                            source_acquired_flag_index,
                        )
                        .unwrap_or(false)
                })
            })
            .collect::<Vec<_>>();
        // Some collection objectives reference the donor flag through a comparison or another
        // non-additive expression. Those programs do not contribute a count term and must remain
        // untouched.
        if additive.is_empty() {
            continue;
        }
        let mut terms = additive.into_iter().collect::<BTreeSet<_>>();
        if terms.len() > 1 {
            let mut closures = BTreeMap::new();
            for &term in &terms {
                closures.insert(term, pool_dependency_closure(pools, pool_rows, term)?);
            }
            terms = outermost_nested_terms(&terms, &closures);
        }
        candidates.insert(root, terms);
    }
    select_anchored_terms(&candidates)
}

// Some native counters repeat the same acquired flag in successive sum chunks.
// Add the authored flag only at the outermost chunk: it reaches the objective
// once, while the inner native count and every original term remain untouched.
fn outermost_nested_terms(
    terms: &BTreeSet<usize>,
    closures: &BTreeMap<usize, BTreeSet<usize>>,
) -> BTreeSet<usize> {
    let outer = terms
        .iter()
        .copied()
        .filter(|term| terms.is_subset(&closures[term]))
        .collect::<BTreeSet<_>>();
    if outer.len() == 1 {
        outer
    } else {
        terms.clone()
    }
}

// Broad ancestor counters can see multiple occurrences of one donor flag. The
// narrow, single-term page counters anchor which contribution belongs to the
// retained branch. Never choose an arbitrary occurrence or count both.
fn select_anchored_terms(
    candidates: &BTreeMap<usize, BTreeSet<usize>>,
) -> AuthoringResult<BTreeSet<usize>> {
    let selected = candidates
        .values()
        .filter(|s| s.len() == 1)
        .flat_map(|s| s.iter().copied())
        .collect::<BTreeSet<_>>();
    for (root, terms) in candidates {
        let matching = terms.intersection(&selected).count();
        if matching != 1 {
            return Err(invalid(format!(
                "Objective pool root {root} has {} donor terms and {matching} anchored contributions",
                terms.len()
            )));
        }
    }
    Ok(selected)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn broad_counter_uses_narrow_page_anchor() {
        let candidates = BTreeMap::from([(10, BTreeSet::from([3])), (20, BTreeSet::from([3, 7]))]);
        assert_eq!(
            select_anchored_terms(&candidates).unwrap(),
            BTreeSet::from([3])
        );
    }
    #[test]
    fn two_counted_contributions_are_rejected() {
        assert!(
            select_anchored_terms(&BTreeMap::from([
                (10, BTreeSet::from([3])),
                (11, BTreeSet::from([7])),
                (20, BTreeSet::from([3, 7]))
            ]))
            .is_err()
        );
    }
}
