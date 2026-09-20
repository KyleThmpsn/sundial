//! Logical account snapshots, optimistic replacement, and backup restoration.
//! Includes WAL-visible state and unexposed tables. Package policy stays in `package`.
use super::validation::connection as validate;
pub(super) use crate::persistence::native_account::snapshot::{Cell, Snapshot, capture, open};
use crate::persistence::native_account::snapshot::{insert_table, quote, snapshot};
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;
mod recovery;
pub(super) use recovery::{restore, restore_snapshot};

pub(crate) fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut db = open(path, false)?;
    let tx = db.transaction().map_err(err)?;
    validate(&tx)?;
    serde_json::to_vec(&snapshot(&tx)?).map_err(err)
}
pub(super) fn capture_path(path: &Path) -> Result<Vec<u8>, String> {
    let mut db = open(path, false)?;
    let tx = db.transaction().map_err(err)?;
    capture(&tx)
}
pub(super) fn verify_unedited_tables(
    before: &[u8],
    after: &[u8],
    edited: &[&str],
) -> Result<(), String> {
    let before: Snapshot = serde_json::from_slice(before).map_err(err)?;
    let after: Snapshot = serde_json::from_slice(after).map_err(err)?;
    for (name, table) in &before.tables {
        // An insert may advance only the sequence owned by an edited table.
        // Preserve sequence rows for every other table, including extensions.
        if name == "sqlite_sequence"
            && let Some(updated) = after.tables.get(name)
            && table.columns == updated.columns
            && let Some(column) = table.columns.iter().position(|column| column == "name")
        {
            let unedited = |row: &&Vec<Cell>| !matches!(row.get(column), Some(Cell::Text(owner)) if edited.contains(&owner.as_str()));
            if table
                .rows
                .iter()
                .filter(unedited)
                .eq(updated.rows.iter().filter(unedited))
            {
                continue;
            }
        }
        if !edited.contains(&name.as_str()) && after.tables.get(name) != Some(table) {
            return Err(format!(
                "Saving would change unedited table {name}. The database was not changed"
            ));
        }
    }
    Ok(())
}
pub(super) fn rollback(path: &Path, expected: &[u8], updated: &[u8]) -> Result<(), String> {
    replace_with(path, expected, updated, |before, _| {
        Ok(before.tables.keys().cloned().collect())
    })
}

pub(super) fn replace_with(
    path: &Path,
    expected: &[u8],
    updated: &[u8],
    plan: impl FnOnce(&Snapshot, &Snapshot) -> Result<Vec<String>, String>,
) -> Result<(), String> {
    let mut db = open(path, true)?;
    db.execute_batch("PRAGMA synchronous=FULL;").map_err(err)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    validate(&tx)?;
    let before = snapshot(&tx)?;
    if serde_json::to_vec(&before).map_err(err)? != expected {
        return Err(
            "The investment database changed after review. Reload the package operation".into(),
        );
    }
    let after: Snapshot = serde_json::from_slice(updated).map_err(err)?;
    if before.schema != after.schema
        || before.version != after.version
        || before.application != after.application
        || before.tables.keys().ne(after.tables.keys())
    {
        return Err("The account proposal changes the database schema".into());
    }
    if before
        .tables
        .iter()
        .any(|(name, table)| table.columns != after.tables[name].columns)
    {
        return Err("The account proposal changes table columns".into());
    }
    let changed = plan(&before, &after)?;
    tx.execute_batch("PRAGMA defer_foreign_keys=ON;")
        .map_err(err)?;
    for name in &changed {
        tx.execute(&format!("DELETE FROM {}", quote(name)), [])
            .map_err(err)?;
    }
    for name in changed.iter().rev() {
        let table = &after.tables[name];
        insert_table(&tx, name, table)?;
    }
    validate(&tx)?;
    if snapshot(&tx)? != after {
        return Err("The account update did not match the reviewed proposal".into());
    }
    tx.commit().map_err(err)
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
