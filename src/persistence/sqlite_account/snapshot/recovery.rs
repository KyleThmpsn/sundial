//! Guided backup restoration holds one write lock from comparison through commit.
use super::*;

pub(in super::super) fn restore(path: &Path, expected: &[u8], backup: &Path) -> Result<(), String> {
    restore_snapshot(path, expected, &read(backup)?)
}

pub(in super::super) fn restore_snapshot(
    path: &Path,
    expected: &[u8],
    updated: &[u8],
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
