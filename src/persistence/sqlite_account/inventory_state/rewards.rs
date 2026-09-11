//! Update delivery rows in place so extension columns and queue order survive edits.

use super::{PendingReward, SqliteAccountError, sql};
use rusqlite::{Connection, params};
use std::collections::BTreeMap;

pub(super) fn load(db: &Connection) -> Result<Vec<PendingReward>, SqliteAccountError> {
    let mut statement = db.prepare("SELECT id, character_slot, kind, definition_hash, quantity FROM pending_rewards ORDER BY id").map_err(sql)?;
    statement
        .query_map([], |row| {
            Ok(PendingReward {
                id: row.get(0)?,
                character_slot: row.get(1)?,
                kind: row.get(2)?,
                definition_hash: row.get(3)?,
                quantity: row.get(4)?,
            })
        })
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)
}

pub(super) fn save(
    db: &rusqlite::Transaction<'_>,
    rewards: &[PendingReward],
) -> Result<(), SqliteAccountError> {
    // The caller holds an immediate transaction and has checked the source revision.
    let mut original: BTreeMap<_, _> = load(db)?
        .into_iter()
        .map(|reward| (reward.id, reward))
        .collect();
    let mut ids = std::collections::BTreeSet::new();
    for reward in rewards {
        if reward.id <= 0 || !ids.insert(reward.id) {
            return Err(SqliteAccountError::invalid_data(
                "pending_rewards",
                "Invalid or duplicate reward ID",
            ));
        }
        if let Some(before) = original.remove(&reward.id) {
            if before.character_slot != reward.character_slot
                || before.kind != reward.kind
                || before.definition_hash != reward.definition_hash
            {
                return Err(SqliteAccountError::invalid_data(
                    "pending_rewards",
                    "An existing reward cannot change its recipient or definition",
                ));
            }
            if before.quantity != reward.quantity {
                db.execute(
                    "UPDATE pending_rewards SET quantity=? WHERE id=?",
                    params![reward.quantity, reward.id],
                )
                .map_err(sql)?;
            }
        } else {
            db.execute(
                "INSERT INTO pending_rewards(id, character_slot, kind, definition_hash, quantity) VALUES(?, ?, ?, ?, ?)",
                params![reward.id, reward.character_slot, reward.kind, reward.definition_hash, reward.quantity],
            ).map_err(sql)?;
        }
    }
    for id in original.keys() {
        db.execute("DELETE FROM pending_rewards WHERE id=?", [id])
            .map_err(sql)?;
    }
    Ok(())
}
