//! Authored collection unlocks in a Dawn player-state database.
//!
//! Dawn keeps unlock flags in `durable_flags`, one sparse row per scope, owner and slot, which is
//! the same table its own `store_flag` writes. Nothing else here is touched: the row counts and
//! positions Dawn checks at load belong to the account graph, and a flag row has neither.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sundial_account::{AuthoredUnlock, FLAG_SET, UnlockScope};

use super::error::DawnAccountError;

/// What one authored-unlock write did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DawnUnlockReceipt {
    /// Absent when every flag was already set, since nothing was written to back up.
    pub backup: Option<PathBuf>,
    /// Definitions that gained a flag, counted the way the settings path counts them.
    pub changed: usize,
}

/// One `durable_flags` row: the scope ordinal, the owning SOID, and the slot in that bank.
type FlagRow = (i64, String, i64);

/// The ordinal Dawn's durable scope enum gives this bank.
const fn durable_scope(scope: UnlockScope) -> i64 {
    match scope {
        UnlockScope::Account => 0,
        UnlockScope::Profile => 1,
        UnlockScope::Character => 2,
        UnlockScope::CharacterObject => 3,
    }
}

/// Sets the flag backing each authored unlock, leaving the ones already set alone.
///
/// A per-character bank is written once for every character, which is what an account-wide unlock
/// means when each character owns its own copy of that bank.
pub(crate) fn apply_authored_unlocks(
    path: &Path,
    unlocks: &[AuthoredUnlock],
) -> Result<DawnUnlockReceipt, DawnAccountError> {
    let pending = pending_rows(path, unlocks)?;
    if pending.iter().all(Vec::is_empty) {
        return Ok(DawnUnlockReceipt {
            backup: None,
            changed: 0,
        });
    }
    let backup = super::writer::create_backup(path)?;
    let mut connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let revision: i64 = transaction.query_row(
        "SELECT value FROM metadata WHERE key='account_revision'",
        [],
        |row| row.get(0),
    )?;
    let mut changed = 0;
    for rows in &pending {
        let mut wrote = false;
        for (scope, owner, slot) in rows {
            // Re-read inside the transaction. The pass that chose these rows held no lock, so a
            // flag Dawn set in between is left as Dawn set it.
            match stored_flag(&transaction, scope, owner, slot)? {
                Some((_, value)) if value == i64::from(FLAG_SET) => continue,
                // The stored row keeps its own spelling of the owner, so no second row appears.
                Some((stored, _)) => transaction.execute(
                    "UPDATE durable_flags SET value=?4 WHERE scope=?1 AND owner_soid=?2 AND slot=?3",
                    params![scope, stored, slot, i64::from(FLAG_SET)],
                )?,
                None => transaction.execute(
                    "INSERT INTO durable_flags(scope,owner_soid,slot,value) VALUES(?1,?2,?3,?4)",
                    params![scope, owner, slot, i64::from(FLAG_SET)],
                )?,
            };
            wrote = true;
        }
        changed += usize::from(wrote);
    }
    if changed == 0 {
        // Every flag was set between the two passes. Nothing was written, so the revision stays
        // where it is rather than advancing over a commit that did nothing.
        drop(transaction);
        let _ = std::fs::remove_file(&backup);
        return Ok(DawnUnlockReceipt {
            backup: None,
            changed: 0,
        });
    }
    // Dawn advances the revision on every commit with a compare and swap, so a write that lands
    // while Sundial holds the account is refused instead of overwriting it.
    let advanced = transaction.execute(
        "UPDATE metadata SET value=value+1 WHERE key='account_revision' AND value=?1",
        params![revision],
    )?;
    if advanced != 1 {
        return Err(DawnAccountError::Conflict {
            expected: revision,
            found: -1,
        });
    }
    transaction.commit()?;
    Ok(DawnUnlockReceipt {
        backup: Some(backup),
        changed,
    })
}

/// The flag stored for this scope, owner and slot, with the owner spelled as it is stored.
///
/// Dawn and Sundial's reader both resolve an owner SOID as a hexadecimal number, so differently
/// cased spellings name one owner. Matching only the exact text would miss a stored row and write
/// a second one beside it, which the reader then refuses as a duplicate owner.
fn stored_flag(
    connection: &Connection,
    scope: &i64,
    owner: &String,
    slot: &i64,
) -> Result<Option<(String, i64)>, DawnAccountError> {
    let rows = connection
        .prepare(
            "SELECT owner_soid,value FROM durable_flags \
             WHERE scope=?1 AND upper(owner_soid)=upper(?2) AND slot=?3",
        )?
        .query_map(params![scope, owner, slot], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<(String, i64)>, _>>()?;
    if rows.len() > 1 {
        return Err(DawnAccountError::Unwritable(format!(
            "player-state.db stores durable flag {slot} more than once for owner {owner}. Nothing was changed"
        )));
    }
    Ok(rows.into_iter().next())
}

/// The rows each unlock still has to write, read before anything is backed up so an installation
/// whose unlocks are already set leaves the database and its backup alone.
fn pending_rows(
    path: &Path,
    unlocks: &[AuthoredUnlock],
) -> Result<Vec<Vec<FlagRow>>, DawnAccountError> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let account: String = connection
        .query_row("SELECT primary_soid FROM account WHERE id=1", [], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or_else(|| {
            DawnAccountError::Unwritable(
                "player-state.db holds no account, so an authored unlock has no owner to set it on"
                    .to_owned(),
            )
        })?;
    let mut statement = connection.prepare("SELECT soid FROM characters ORDER BY position")?;
    let characters = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut pending = Vec::with_capacity(unlocks.len());
    for unlock in unlocks {
        let (scope, slot) = unlock.target().map_err(DawnAccountError::Unwritable)?;
        let slot = i64::try_from(slot).map_err(|_| {
            DawnAccountError::Unwritable(format!(
                "Authored unlock definition {} uses a slot outside the range Dawn stores",
                unlock.definition_index
            ))
        })?;
        let owners: &[String] = if scope.per_character() {
            &characters
        } else {
            std::slice::from_ref(&account)
        };
        let mut rows = Vec::new();
        for owner in owners {
            let scope = durable_scope(scope);
            if stored_flag(&connection, &scope, owner, &slot)?.map(|(_, value)| value)
                != Some(i64::from(FLAG_SET))
            {
                rows.push((scope, owner.clone(), slot));
            }
        }
        pending.push(rows);
    }
    Ok(pending)
}

#[cfg(test)]
mod tests;
