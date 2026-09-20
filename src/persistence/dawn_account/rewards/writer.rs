//! Update pending debts atomically. Completed deliveries are never deleted or replayed.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn invalid(message: &str) -> DawnAccountError {
    DawnAccountError::Unwritable(message.into())
}

pub(in crate::persistence::dawn_account) fn save(
    db: &Connection,
    document: &DawnAccountDocument,
) -> Result<Vec<RewardDebt>, DawnAccountError> {
    let before = &document.loaded_reward_debts;
    if document.reward_debts == *before {
        return Ok(before.clone());
    }
    if load(db)? != *before {
        return Err(invalid(
            "Dawn reward delivery changed while the queue was open. Reload before saving.",
        ));
    }
    let sequence = sequence(db)?;
    let mut originals: BTreeMap<_, _> = before.iter().map(|row| (row.id, row)).collect();
    let mut ids = BTreeSet::new();
    let mut after = document.reward_debts.clone();
    for row in &after {
        if row.id <= 0 || !ids.insert(row.id) {
            return Err(invalid("Invalid reward identity"));
        }
        if let Some(old) = originals.remove(&row.id) {
            update(db, document, old, row)?;
        } else {
            insert(db, document, sequence, row)?;
        }
    }
    // Undoing a saved enqueue cancels it. Never erase an idempotency record.
    for old in originals.values() {
        let mut kept = (*old).clone();
        if !kept.delivered {
            if kept.credited != 0 {
                return Err(invalid("A credited reward cannot be cancelled"));
            }
            kept.delivered = true;
            update(db, document, old, &kept)?;
        }
        after.push(kept);
    }
    after.sort_by_key(|debt| debt.id);
    Ok(after)
}

fn update(
    db: &Connection,
    document: &DawnAccountDocument,
    old: &RewardDebt,
    row: &RewardDebt,
) -> Result<(), DawnAccountError> {
    if old == row {
        return Ok(());
    }
    let mut identity = row.clone();
    identity.quantity = old.quantity;
    identity.delivered = old.delivered;
    if identity != *old
        || row.quantity <= 0
        || row.credited != 0
        || row.account_soid != document.primary_soid().get()
        || (old.delivered && !document.editor_cancelled_debts.contains(&old.id))
    {
        return Err(invalid(
            "A finished delivery or its mission/run identity cannot be changed",
        ));
    }
    db.execute(
        "UPDATE reward_debts SET quantity=?2,delivered=?3 WHERE debt_id=?1",
        params![row.id, row.quantity, row.delivered],
    )?;
    Ok(())
}

fn insert(
    db: &Connection,
    document: &DawnAccountDocument,
    sequence: i64,
    row: &RewardDebt,
) -> Result<(), DawnAccountError> {
    if row.id <= sequence
        || row.account_soid != document.primary_soid().get()
        || row.mission_hash != EDITOR_MISSION
        || row.session_id != EDITOR_SESSION
        || row.runtime_epoch != document.metadata.reward_epoch as u64
        || row.run_id != row.id as u64
        || row.quantity <= 0
        || row.credited != 0
        || matches!(row.definition_hash, 0 | 0x811C9DC5 | u32::MAX)
        || !document.characters().characters().iter().any(|character| {
            character
                .soid
                .is_some_and(|id| id.get() == row.character_soid)
        })
    {
        return Err(invalid(
            "The new Dawn reward has an invalid or stale delivery identity",
        ));
    }
    db.execute("INSERT INTO reward_debts(debt_id,account_soid,character_soid,runtime_epoch,session_id,run_id,mission_hash,definition_hash,quantity,credited,delivered) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,?10)", params![row.id,contract::format_soid(row.account_soid),contract::format_soid(row.character_soid),contract::format_soid(row.runtime_epoch),contract::format_soid(row.session_id),contract::format_soid(row.run_id),row.mission_hash,row.definition_hash,row.quantity,row.delivered])?;
    Ok(())
}
