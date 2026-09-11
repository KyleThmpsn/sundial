//! Reconstruct the condition inputs available from saved state and package definitions.
//! Saved-state accessors remain separate because editing must preserve the authored values.
use super::document::CollectionStateSnapshot;
use crate::app::collections_page::{ExpressionValue, evaluate_expression_value_with};
use crate::catalog::{
    Catalog, CollectionConditionTokenDef, ProgressionDefinition, ProgressionScope, UnlockWriter,
};

impl CollectionStateSnapshot {
    pub(in crate::app) fn evaluated_flag(&self, index: usize, catalog: &Catalog) -> Option<bool> {
        let definition = catalog.unlock_flag_definition(index)?;
        if self.is_native
            && let Some(season) = catalog.seasonal()
            && let Some(entry) = season.mod_for_flag(index)
        {
            // Seasonal seed imports set overrides, strips the rest, and uses character ownership.
            return Some(self.artifact_mask(season, true) & entry.bit() != 0);
        }
        // The native item-context reader bypasses the buffered value for kind 5.
        if definition.bank() == 5 {
            return None;
        }
        let authored = self.flag_overrides.get(&index).copied();
        if authored.is_some_and(|value| value > 2) {
            return None;
        }
        if let Some(value @ (1 | 2)) = authored {
            return Some(value == 2);
        }
        let mut derived = Some(false);
        for writer in &definition.runtime_writers {
            match writer {
                UnlockWriter::ProgressionStep {
                    definition_index,
                    step_index,
                } => {
                    match self.progression_rank(
                        catalog.progression_definition(usize::from(*definition_index))?,
                    ) {
                        Some(rank) if rank > i32::from(*step_index) => return Some(true),
                        Some(_) => {}
                        None => derived = None,
                    }
                }
                UnlockWriter::Context { .. } if authored.is_none() => derived = None,
                _ => {}
            }
        }
        if authored == Some(0) {
            return derived;
        }
        match (self.flag_value(index, definition), derived) {
            (Some(true), _) => Some(true),
            (saved, Some(false)) => saved,
            _ => None,
        }
    }

    pub(in crate::app) fn evaluated_value(&self, index: usize, catalog: &Catalog) -> Option<i32> {
        self.value_at_depth(index, catalog, &[])
    }

    fn value_at_depth(&self, index: usize, catalog: &Catalog, active: &[usize]) -> Option<i32> {
        if active.len() >= 64 || active.contains(&index) {
            return None;
        }
        let definition = catalog.unlock_value_definition(index)?;
        if self.is_native && super::seasonal::is_derived_value(index) {
            let season = catalog.seasonal()?;
            let experience = self.seasonal_experience(season).ok()?;
            return experience
                .values(self.artifact_mask(season, true).count_ones())
                .into_iter()
                .find_map(|(slot, value)| (slot == index).then_some(value));
        }
        if definition.bank() == 4 {
            return None;
        }
        if let Some(value) = self.value_overrides.get(&index) {
            return Some(*value);
        }
        let mut path = active.to_vec();
        path.push(index);
        let mut computed = None;
        for writer in &definition.runtime_writers {
            let value = match writer {
                UnlockWriter::ProgressionLevel { definition_index } => self.progression_rank(
                    catalog.progression_definition(usize::from(*definition_index))?,
                )?,
                UnlockWriter::ValueCounter { programs } => {
                    self.counter(programs, catalog, &path)?
                }
                UnlockWriter::Context { .. } => return None,
                UnlockWriter::ProgressionStep { .. } => continue,
            };
            // Conflicting writers require the native refresh context to order them.
            if computed.is_some_and(|previous| previous != value) {
                return None;
            }
            computed = Some(value);
        }
        computed.or_else(|| self.value(index, definition))
    }

    fn counter(
        &self,
        programs: &[Vec<[u32; 2]>],
        catalog: &Catalog,
        active: &[usize],
    ) -> Option<i32> {
        let mut count = 0_i32;
        for program in programs {
            // Counters invoke the numeric evaluator, where an empty program is zero.
            if program.is_empty() {
                continue;
            }
            let tokens = program
                .iter()
                .map(|&[kind, operand]| CollectionConditionTokenDef { kind, operand })
                .collect::<Vec<_>>();
            let passed = match evaluate_expression_value_with(
                &tokens,
                catalog.shared_expression_pool(),
                |index| self.evaluated_flag(index, catalog),
                |index| self.value_at_depth(index, catalog, active),
            )? {
                ExpressionValue::Boolean(value) => value,
                ExpressionValue::Number(value) => value != 0,
                ExpressionValue::Unknown => return None,
            };
            count = count.checked_add(i32::from(passed))?;
        }
        Some(count)
    }

    fn progression_rank(&self, definition: &ProgressionDefinition) -> Option<i32> {
        let rows = match definition.scope {
            ProgressionScope::Account => &self.account_progressions,
            ProgressionScope::Character => &self.character_progressions,
            ProgressionScope::Unreplicated => return None,
        };
        let mut progress = rows
            .iter()
            .find(|row| row.definition_index == usize::from(definition.definition_index))
            .map_or(0, |row| row.lanes[0]);
        if self.is_native && (38..=41).contains(&definition.definition_index) {
            use crate::investment::seasonal as rules;
            let total = self.seasonal_xp();
            if total < 0 {
                return None;
            }
            progress = match usize::from(definition.definition_index) {
                rules::PASS_PROGRESSION => total.min(rules::PASS_XP_CAP),
                rules::HUD_PROGRESSION if total < rules::PASS_XP_CAP => total % rules::XP_PER_RANK,
                rules::HUD_PROGRESSION => total - rules::PASS_XP_CAP,
                _ => total,
            };
        }
        rank_for_progress(definition, progress)
    }
}

fn rank_for_progress(definition: &ProgressionDefinition, progress: i32) -> Option<i32> {
    let mut remaining = i64::from(progress.max(0));
    let mut rank = 0_i32;
    for step in &definition.steps {
        let cost = i64::from(step.cost);
        if cost < 0 {
            return None;
        }
        if remaining < cost {
            return Some(rank);
        }
        remaining -= cost;
        rank = rank.checked_add(1)?;
    }
    if definition.repeat_last_step && remaining > 0 {
        let cost = i64::from(definition.steps.last()?.cost);
        if cost <= 0 {
            return None;
        }
        rank = rank.checked_add(i32::try_from(remaining / cost).ok()?)?;
    }
    Some(rank)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::UnlockDefinition;
    use serde_json::json;

    fn ladder() -> ProgressionDefinition {
        serde_json::from_value(json!({"definition_index":0,"hash":1,"scope":"Account","scope_slot":0,"repeat_last_step":false,"steps":[{"cost":200},{"cost":850},{"cost":1050}]})).unwrap()
    }

    #[test]
    fn ranks_consume_each_cost_and_repeat_only_the_final_step() {
        let mut definition = ladder();
        for (progress, rank) in [
            (0, 0),
            (199, 0),
            (200, 1),
            (1049, 1),
            (1050, 2),
            (2100, 3),
            (4200, 3),
        ] {
            assert_eq!(rank_for_progress(&definition, progress), Some(rank));
        }
        definition.repeat_last_step = true;
        assert_eq!(rank_for_progress(&definition, 4200), Some(5));
        definition.steps[0].cost = -1;
        assert_eq!(rank_for_progress(&definition, 0), None);
    }

    fn catalog() -> Catalog {
        let flags = vec![
            UnlockDefinition {
                runtime_writers: vec![UnlockWriter::ProgressionStep {
                    definition_index: 0,
                    step_index: 0,
                }],
                ..Default::default()
            },
            UnlockDefinition {
                code: 5,
                ..Default::default()
            },
            UnlockDefinition {
                runtime_writers: vec![UnlockWriter::Context {
                    source: "Activity".into(),
                }],
                ..Default::default()
            },
        ];
        let values = vec![
            UnlockDefinition {
                runtime_writers: vec![UnlockWriter::ProgressionLevel {
                    definition_index: 0,
                }],
                ..Default::default()
            },
            UnlockDefinition {
                runtime_writers: vec![UnlockWriter::ValueCounter {
                    programs: vec![vec![[1, 0]], vec![[10, 0], [11, 2], [14, 0]], vec![]],
                }],
                ..Default::default()
            },
            UnlockDefinition {
                runtime_writers: vec![UnlockWriter::ValueCounter {
                    programs: vec![vec![[10, 2]]],
                }],
                ..Default::default()
            },
        ];
        Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
            flags,
            values,
            vec![ladder()],
        )
    }

    #[test]
    fn zero_override_falls_through_to_progression_but_false_blocks_it() {
        let catalog = catalog();
        let mut state = super::super::document::collection_state_snapshot(&json!({})).unwrap();
        state
            .account_progressions
            .push(super::super::document::ProgressionValue {
                definition_index: 0,
                lanes: [1050, 0, 0],
            });
        state.flag_overrides.insert(0, 0);
        assert_eq!(
            state.flag_value(0, catalog.unlock_flag_definition(0).unwrap()),
            Some(false)
        );
        assert_eq!(state.evaluated_flag(0, &catalog), Some(true));
        assert_eq!(state.evaluated_value(0, &catalog), Some(2));
        assert_eq!(state.evaluated_value(1, &catalog), Some(2));
        state.flag_overrides.insert(0, 1);
        assert_eq!(state.evaluated_flag(0, &catalog), Some(false));
        assert_eq!(state.evaluated_value(1, &catalog), Some(1));
    }

    #[test]
    fn live_context_and_counter_cycles_remain_unresolved() {
        let catalog = catalog();
        let mut state = super::super::document::collection_state_snapshot(&json!({})).unwrap();
        state.flag_overrides.insert(1, 2);
        assert_eq!(state.evaluated_flag(1, &catalog), None);
        assert_eq!(state.evaluated_flag(2, &catalog), None);
        assert_eq!(state.evaluated_value(2, &catalog), None);
        state.flag_overrides.insert(2, 2);
        assert_eq!(state.evaluated_flag(2, &catalog), Some(true));
    }
}
