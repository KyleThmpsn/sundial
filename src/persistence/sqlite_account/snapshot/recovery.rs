//! Guided backup restoration holds one write lock from comparison through commit.
use super::*;

#[cfg(test)]
pub(in super::super) fn restore(path: &Path, expected: &[u8], backup: &Path) -> Result<(), String> {
    restore_checked(path, expected, backup, || Ok(()))
}

pub(in super::super) fn restore_checked(
    path: &Path,
    expected: &[u8],
    backup: &Path,
    require_closed: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    restore_snapshot(path, expected, &read(backup)?, require_closed)
}

/// Replaces every row and the schema with `updated`, refused when the database no longer matches
/// `expected`. `require_closed` runs last, just before the commit, since a game started while
/// the tables were rewritten has loaded the account this would replace.
pub(in super::super) fn restore_snapshot(
    path: &Path,
    expected: &[u8],
    updated: &[u8],
    mut require_closed: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let after: Snapshot = serde_json::from_slice(updated).map_err(err)?;
    let mut db = open(path, true)?;
    // All rows and their schema are restored together. Foreign keys are verified before commit.
    db.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA synchronous=FULL;")
        .map_err(err)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    let before = snapshot(&tx)?;
    if serde_json::to_vec(&before).map_err(err)? != expected {
        return Err(
            "The investment database changed after its recovery snapshot. Reload before restoring"
                .into(),
        );
    }
    // Guided recovery can repair an unreadable format, but cannot upgrade or downgrade
    // either supported account format. Migration belongs to Sunrise.
    if matches!(before.version, 2 | 3) && before.version != after.version {
        return Err(format!(
            "Cannot restore schema {} over schema {}. Restore a matching account backup",
            after.version, before.version
        ));
    }
    if before.schema != after.schema {
        replace_schema(&tx, &before, &after)?;
    }
    for name in after.tables.keys() {
        tx.execute(&format!("DELETE FROM {}", quote(name)), [])
            .map_err(err)?;
    }
    for (name, table) in &after.tables {
        insert_table(&tx, name, table)?;
    }
    tx.pragma_update(None, "user_version", after.version)
        .map_err(err)?;
    tx.pragma_update(None, "application_id", after.application)
        .map_err(err)?;
    validate(&tx)?;
    if snapshot(&tx)? != after {
        return Err(
            "The restored database did not match its backup. The database was not changed".into(),
        );
    }
    require_closed()?;
    tx.commit().map_err(err)
}

fn replace_schema(db: &Connection, before: &Snapshot, after: &Snapshot) -> Result<(), String> {
    for kind in ["trigger", "view", "table"] {
        for (name, ty, _) in &before.schema {
            if ty == kind && !name.starts_with("sqlite_") {
                db.execute_batch(&format!("DROP {kind} {}", quote(name)))
                    .map_err(err)?;
            }
        }
    }
    for kind in ["table", "index", "view", "trigger"] {
        for (name, ty, sql) in &after.schema {
            if ty == kind
                && !name.starts_with("sqlite_")
                && let Some(sql) = sql
            {
                db.execute_batch(sql).map_err(err)?;
            }
        }
    }
    if after.tables.contains_key("sqlite_stat1") {
        db.execute_batch("ANALYZE sqlite_schema").map_err(err)?;
    }
    Ok(())
}
