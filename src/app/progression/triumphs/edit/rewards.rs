use super::*;

pub(super) fn queue(
    document: &mut Value,
    catalog: &Catalog,
    record: &RecordDefinition,
    before: &CollectionStateSnapshot,
    after: &CollectionStateSnapshot,
) -> Result<(), String> {
    let Some(runtime) = &record.runtime else {
        return Ok(());
    };
    let mut rewards = Vec::new();
    if let Some(index) = record
        .completion_flag
        .filter(|_| record.redeemed_intervals.is_none() || record.interval_count == 0)
    {
        let mut target = Target::exact(false, usize::from(index), 1);
        target.native = true;
        if target.actual(before, catalog) != Some(1) && target.actual(after, catalog) == Some(1) {
            rewards.extend(runtime.rewards.iter().copied());
        }
    } else if let Some(index) = record.redeemed_intervals {
        let mut target = Target::exact(true, usize::from(index), 0);
        target.native = true;
        let old = target.actual(before, catalog).unwrap_or(0).max(0) as usize;
        let new = target.actual(after, catalog).unwrap_or(0).max(0) as usize;
        for item in runtime.interval_items.iter().take(new).skip(old).flatten() {
            rewards.push((*item, 1));
        }
    }
    if rewards.is_empty() {
        return Ok(());
    }
    let rewards = rewards
        .into_iter()
        .map(|(index, quantity)| {
            catalog
                .item_hash_for_index(index)
                .map(|hash| (hash, quantity))
                .ok_or_else(|| format!("Reward item #{index} is not in the installed catalog"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    super::super::super::rewards::queue(document, catalog, rewards)
}
