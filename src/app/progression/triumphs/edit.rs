use super::*;
mod rewards;
use super::super::{
    mutations::{remove_investment_override, set_unlock_flag, set_unlock_value},
    state::InvestmentTable,
};

#[derive(Clone, Debug)]
pub(super) struct Issue {
    pub name: String,
    pub reason: String,
}

#[derive(Debug)]
pub(super) struct Job {
    source: Value,
    candidate: Value,
    records: Vec<RecordDefinition>,
    cursor: usize,
    complete: bool,
    accepted: Vec<usize>,
    /// Accepted records that actually change something, counted once the job completes.
    changing: usize,
    pub issues: Vec<Issue>,
    pub review: Option<super::super::impact::Review>,
    excluded: HashSet<usize>,
    pub conflicts: Vec<Issue>,
}

impl Job {
    pub fn new(document: &Value, records: Vec<RecordDefinition>, complete: bool) -> Self {
        Self {
            source: document.clone(),
            candidate: document.clone(),
            records,
            cursor: 0,
            complete,
            accepted: Vec::new(),
            changing: 0,
            issues: Vec::new(),
            review: None,
            excluded: HashSet::new(),
            conflicts: Vec::new(),
        }
    }
    pub fn progress(&self) -> (usize, usize) {
        (self.cursor, self.records.len())
    }
    pub fn supported_count(&self) -> usize {
        self.accepted.len()
    }
    /// Accepted records whose state the edit will really alter.
    pub fn changing_count(&self) -> usize {
        self.changing
    }
    pub fn direct_count(&self) -> usize {
        super::super::rewards::direct_count(&self.source, &self.candidate)
    }
    pub fn draw_consumables(&self, ui: &mut egui::Ui, catalog: &Catalog) {
        super::super::rewards::draw_review(ui, &self.source, &self.candidate, catalog);
    }
    pub fn queued_count(&self) -> usize {
        self.candidate
            .get("_progression_rewards")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
            .saturating_sub(
                self.source
                    .get("_progression_rewards")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
            )
    }
    pub fn step(&mut self, catalog: &Catalog) -> bool {
        let start = std::time::Instant::now();
        while self.cursor < self.records.len() {
            if self.excluded.contains(&self.cursor) {
                self.cursor += 1;
                continue;
            }
            let record = &self.records[self.cursor];
            match apply_record(&mut self.candidate, catalog, record, self.complete) {
                Ok(()) => self.accepted.push(self.cursor),
                Err(reason) => self.issues.push(Issue {
                    name: display_name(record, catalog),
                    reason,
                }),
            }
            self.cursor += 1;
            if start.elapsed() >= std::time::Duration::from_millis(8) {
                break;
            }
        }
        if self.cursor == self.records.len() && self.review.is_none() {
            if let (Some(before), Some(after)) = (
                collection_state_snapshot(&self.source),
                collection_state_snapshot(&self.candidate),
            ) {
                let conflicts = self
                    .accepted
                    .iter()
                    .filter_map(|index| {
                        verify(&after, catalog, &self.records[*index], self.complete)
                            .err()
                            .map(|reason| (*index, reason))
                    })
                    .collect::<Vec<_>>();
                if !conflicts.is_empty() {
                    for (index, reason) in conflicts {
                        self.excluded.insert(index);
                        self.conflicts.push(Issue {
                            name: display_name(&self.records[index], catalog),
                            reason,
                        });
                    }
                    // Rebuild every effect, including score and rewards, from the source.
                    // Retry capacity failures because excluded rewards may have occupied slots.
                    self.candidate = self.source.clone();
                    self.cursor = 0;
                    self.accepted.clear();
                    self.issues.clear();
                    return false;
                }
                self.changing = self
                    .accepted
                    .iter()
                    .filter(|index| {
                        verify(&before, catalog, &self.records[**index], self.complete).is_err()
                    })
                    .count();
                self.review = Some(super::super::impact::Review::build(
                    &before,
                    &after,
                    catalog,
                    &self
                        .accepted
                        .iter()
                        .map(|index| self.records[*index].hash)
                        .collect(),
                ));
            }
        }
        self.cursor == self.records.len()
    }
    pub fn finish(self, document: &mut Value, catalog: &Catalog) -> Result<usize, String> {
        if document != &self.source {
            return Err("The account changed during this edit. No changes were applied.".into());
        }
        let snapshot =
            collection_state_snapshot(&self.candidate).ok_or("Progression state is unavailable")?;
        for &index in &self.accepted {
            verify(&snapshot, catalog, &self.records[index], self.complete).map_err(|reason| {
                format!(
                    "{}: {reason}. No changes were applied.",
                    record_name(&self.records[index])
                )
            })?;
        }
        let initial =
            collection_state_snapshot(&self.source).ok_or("Progression state is unavailable")?;
        let changed = self
            .accepted
            .iter()
            .filter(|index| {
                verify(&initial, catalog, &self.records[**index], self.complete).is_err()
            })
            .count();
        *document = self.candidate;
        Ok(changed)
    }
}

#[derive(Clone, Copy)]
struct Target {
    value: bool,
    index: usize,
    min: i32,
    max: i32,
    preferred: i32,
    native: bool,
}
impl Target {
    fn exact(value: bool, index: usize, target: i32) -> Self {
        Self {
            value,
            index,
            min: target,
            max: target,
            preferred: target,
            native: false,
        }
    }
    fn contains(self, value: Option<i32>) -> bool {
        value.is_some_and(|value| (self.min..=self.max).contains(&value))
    }
    fn actual(self, snapshot: &CollectionStateSnapshot, catalog: &Catalog) -> Option<i32> {
        if self.native {
            let definition = if self.value {
                catalog.unlock_value_definition(self.index)
            } else {
                catalog.unlock_flag_definition(self.index)
            }?;
            let slot = usize::from(definition.compact_slot?);
            if definition.bank() != 1 {
                return None;
            }
            return Some(if self.value {
                snapshot.values.get(&(1, slot)).copied().unwrap_or(0)
            } else {
                i32::from(snapshot.flags.contains(&(1, slot)))
            });
        }
        if self.value {
            snapshot.evaluated_value(self.index, catalog)
        } else {
            snapshot.evaluated_flag(self.index, catalog).map(i32::from)
        }
    }
}

fn objective_target(objective: &ObjectiveDef, complete: bool) -> Result<i32, String> {
    if complete {
        return Ok(objective.completion_value);
    }
    if !objective_complete(objective, 0) {
        return Ok(0);
    }
    if objective.is_counting_downward {
        objective
            .completion_value
            .checked_add(1)
            .ok_or_else(|| "This countdown has no representable incomplete value".into())
    } else if objective.allow_negative_value {
        objective
            .completion_value
            .checked_sub(1)
            .ok_or_else(|| "This objective has no representable incomplete value".into())
    } else {
        Ok(0)
    }
}

fn changes(
    record: &RecordDefinition,
    catalog: &Catalog,
    complete: bool,
) -> Result<Vec<Target>, String> {
    let mut edits = Vec::new();
    if let Some(index) = record.completion_flag {
        edits.push(Target::exact(
            false,
            usize::from(index),
            i32::from(complete),
        ));
    } else if record.objectives.is_empty()
        && record
            .runtime
            .as_ref()
            .is_none_or(|runtime| runtime.progress.is_empty())
    {
        return Err("No saved completion flag or objective is linked to this Triumph".into());
    }
    let mut reset_clears_objective = false;
    for &index in record
        .objectives
        .iter()
        .filter(|_| record.runtime.is_none())
    {
        let objective = catalog
            .objective_definition(index)
            .ok_or("An objective definition is unavailable")?;
        let value = objective
            .related_unlock_value_definition_index
            .ok_or_else(|| {
                format!(
                    "{} has no editable saved counter",
                    objective_table_text(objective, None)
                )
            })?;
        let mut target = Target::exact(
            true,
            usize::from(value),
            objective_target(objective, complete)?,
        );
        reset_clears_objective |= !objective_complete(objective, target.preferred);
        if complete {
            if objective.is_counting_downward {
                target.min = i32::MIN;
            } else {
                target.max = i32::MAX;
            }
        }
        edits.push(target);
    }
    if let Some(runtime) = &record.runtime {
        let claimed_slot = record
            .redeemed_intervals
            .filter(|_| record.interval_count > 0)
            .and_then(|index| catalog.unlock_value_definition(usize::from(index)))
            .and_then(|definition| definition.compact_slot);
        for progress in runtime
            .progress
            .iter()
            .filter(|progress| Some(progress.slot) != claimed_slot)
        {
            let (index, _) = catalog
                .unlock_value_for_state(1, usize::from(progress.slot))
                .ok_or_else(|| {
                    format!(
                        "Account progress slot {} has no installed counter mapping",
                        progress.slot
                    )
                })?;
            let mut target =
                Target::exact(true, index, if complete { progress.threshold } else { 0 });
            reset_clears_objective |= progress.threshold > 0;
            if complete {
                target.max = i32::MAX;
            }
            edits.push(target);
        }
    }
    if !complete && record.completion_flag.is_none() && !reset_clears_objective {
        return Err("This Triumph has no completion flag and its objectives are satisfied at their reset values".into());
    }
    if let Some(index) = record
        .redeemed_intervals
        .filter(|_| record.interval_count > 0)
    {
        let target = Target::exact(
            true,
            usize::from(index),
            if complete {
                i32::try_from(record.interval_count)
                    .map_err(|_| "The interval count exceeds the saved value range")?
            } else {
                0
            },
        );
        edits.push(target);
    }
    for target in &mut edits {
        target.native = record.runtime.is_some();
    }
    Ok(edits)
}

fn verify(
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    record: &RecordDefinition,
    complete: bool,
) -> Result<(), String> {
    for target in changes(record, catalog, complete)? {
        let overridden = target.native
            && if target.value {
                snapshot.value_overrides.contains_key(&target.index)
            } else {
                snapshot.flag_overrides.contains_key(&target.index)
            };
        let actual = target.actual(snapshot, catalog);
        if overridden || !target.contains(actual) {
            let expected = if target.min == target.max {
                target.min.to_string()
            } else if target.max == i32::MAX {
                format!("at least {}", target.min)
            } else if target.min == i32::MIN {
                format!("at most {}", target.max)
            } else {
                format!("{} to {}", target.min, target.max)
            };
            return Err(format!(
                "{} requires {expected}, planned value is {}{}",
                labels::unlock(catalog, target.index, target.value).text,
                actual.map_or_else(|| "unavailable".into(), |value| value.to_string()),
                if overridden {
                    " with an active override"
                } else {
                    ""
                },
            ));
        }
    }
    Ok(())
}

pub(super) fn apply_record(
    document: &mut Value,
    catalog: &Catalog,
    record: &RecordDefinition,
    complete: bool,
) -> Result<(), String> {
    let mut candidate = document.clone();
    let initial = collection_state_snapshot(document).ok_or("Progression state is unavailable")?;
    let mut assigned = BTreeMap::<(bool, usize), Target>::new();
    for target in changes(record, catalog, complete)? {
        let merged = assigned
            .entry((target.value, target.index))
            .or_insert(target);
        merged.min = merged.min.max(target.min);
        merged.max = merged.max.min(target.max);
        if merged.min > merged.max {
            return Err(
                "Its objectives require conflicting values in the same saved counter".into(),
            );
        }
    }
    for ((value, index), target) in assigned {
        let refreshed;
        // A native target reads the compact lane, which overrides cannot mask, so the snapshot
        // taken before this record is enough — and avoids a full parse per target in a batch.
        let snapshot = if target.native {
            &initial
        } else {
            refreshed =
                collection_state_snapshot(&candidate).ok_or("Progression state is unavailable")?;
            &refreshed
        };
        let actual = target.actual(snapshot, catalog);
        if target.native {
            let _ = remove_investment_override(
                &mut candidate,
                if value {
                    InvestmentTable::ValueOverrides
                } else {
                    InvestmentTable::FlagOverrides
                },
                index,
            );
        }
        if target.contains(actual) {
            continue;
        }
        let requested = actual
            .unwrap_or(target.preferred)
            .clamp(target.min, target.max);
        let applied = if target.native {
            let definition = if value {
                catalog.unlock_value_definition(index)
            } else {
                catalog.unlock_flag_definition(index)
            }
            .ok_or("The native record field is unavailable")?;
            if definition.bank() != 1 {
                return Err("This record field is not in the account bank".into());
            }
            let slot = usize::from(
                definition
                    .compact_slot
                    .ok_or("This record field has no saved account slot")?,
            );
            let _ = remove_investment_override(
                &mut candidate,
                if value {
                    InvestmentTable::ValueOverrides
                } else {
                    InvestmentTable::FlagOverrides
                },
                index,
            );
            if value {
                set_unlock_value(&mut candidate, "objective_values", slot, requested)
            } else {
                set_unlock_flag(&mut candidate, "account_flag_runs", slot, requested != 0)
            }
        } else if value {
            let definition = catalog
                .unlock_value_definition(index)
                .ok_or("The saved counter definition is unavailable")?;
            set_collection_value(&mut candidate, index, definition, requested)
        } else {
            let definition = catalog
                .unlock_flag_definition(index)
                .ok_or("The completion flag definition is unavailable")?;
            set_collection_flag(&mut candidate, index, definition, requested != 0)
        };
        if applied.refused() {
            return Err(format!(
                "{} cannot be edited in this account",
                labels::unlock(catalog, index, value).text
            ));
        }
    }
    let snapshot =
        collection_state_snapshot(&candidate).ok_or("Progression state is unavailable")?;
    verify(&snapshot, catalog, record, complete)?;
    update_score(&mut candidate, catalog, record, &initial, &snapshot)?;
    rewards::queue(&mut candidate, catalog, record, &initial, &snapshot)?;
    validate(&candidate)?;
    *document = candidate;
    Ok(())
}

fn earned_score(
    record: &RecordDefinition,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Result<i64, String> {
    let Some(runtime) = &record.runtime else {
        return Ok(0);
    };
    if let Some(index) = record
        .completion_flag
        .filter(|_| record.redeemed_intervals.is_none() || record.interval_count == 0)
    {
        let mut target = Target::exact(false, usize::from(index), 1);
        target.native = true;
        return Ok(if target.actual(snapshot, catalog) == Some(1) {
            i64::from(runtime.score)
        } else {
            0
        });
    }
    let Some(index) = record.redeemed_intervals else {
        return Ok(0);
    };
    let mut target = Target::exact(true, usize::from(index), 0);
    target.native = true;
    let count = target
        .actual(snapshot, catalog)
        .ok_or("Claimed stages have no saved account slot")?;
    // Older Sundial builds could put the objective threshold in this count.
    // Clamp the old score contribution so repairing it cannot award points again.
    let count = count.clamp(0, runtime.interval_scores.len() as i32);
    Ok(runtime
        .interval_scores
        .iter()
        .take(count as usize)
        .map(|score| i64::from(*score))
        .sum())
}

/// The Triumph score every record's saved state implies, for repairing a total that the running
/// delta cannot explain. A record whose score cannot be read contributes nothing.
fn rebuilt_score(catalog: &Catalog, snapshot: &CollectionStateSnapshot) -> i64 {
    catalog.records().map_or(0, |records| {
        records
            .iter()
            .map(|record| earned_score(record, snapshot, catalog).unwrap_or(0))
            .sum()
    })
}

fn update_score(
    document: &mut Value,
    catalog: &Catalog,
    record: &RecordDefinition,
    before: &CollectionStateSnapshot,
    after: &CollectionStateSnapshot,
) -> Result<(), String> {
    let delta = earned_score(record, after, catalog)? - earned_score(record, before, catalog)?;
    if delta == 0 {
        return Ok(());
    }
    // Sunrise records::kTriumphScoreValueIndex. Preserve score from records outside this edit.
    let previous = i64::from(before.values.get(&(1, 2115)).copied().unwrap_or(0));
    let mut total = previous + delta;
    if total < 0 {
        // The slot was never a running total, which is normal on an imported account where the
        // game tracked score elsewhere. Rebuild it from every record instead of clamping to zero.
        total = rebuilt_score(catalog, after);
    }
    let score = i32::try_from(total.max(0))
        .map_err(|_| "The resulting Triumph score exceeds the saved range")?;
    // One slot can back several definitions, and an override on any of them keeps masking it.
    for index in before.value_overrides.keys() {
        if catalog
            .unlock_value_definition(*index)
            .is_some_and(|definition| {
                definition.bank() == 1 && definition.compact_slot == Some(2115)
            })
        {
            let _ = remove_investment_override(document, InvestmentTable::ValueOverrides, *index);
        }
    }
    if set_unlock_value(document, "objective_values", 2115, score).refused() {
        return Err("The Triumph score could not be saved".into());
    }
    Ok(())
}
