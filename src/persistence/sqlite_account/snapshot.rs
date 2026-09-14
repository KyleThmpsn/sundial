//! Logical account snapshots, optimistic replacement, and backup restoration.
//! Includes WAL-visible state and unexposed tables. Package policy stays in `package`.
use super::validation::connection as validate;
use rusqlite::{
    Connection, OpenFlags, TransactionBehavior,
    types::{Value, ValueRef},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, time::Duration};
mod recovery;
pub(super) use recovery::{restore, restore_snapshot};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(String),
    Blob(Vec<u8>),
}
impl Cell {
    pub(super) fn from_value(value: Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Integer(v) => Self::Integer(v),
            Value::Real(v) => Self::Real(v.to_bits()),
            Value::Text(v) => Self::Text(v),
            Value::Blob(v) => Self::Blob(v),
        }
    }

    pub(super) fn value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Integer(v) => Value::Integer(*v),
            Self::Real(v) => Value::Real(f64::from_bits(*v)),
            Self::Text(v) => Value::Text(v.clone()),
            Self::Blob(v) => Value::Blob(v.clone()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Table {
    pub(super) columns: Vec<String>,
    pub(super) rows: Vec<Vec<Cell>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Snapshot {
    pub(super) version: i64,
    pub(super) application: i64,
    pub(super) schema: Vec<(String, String, Option<String>)>,
    pub(super) tables: BTreeMap<String, Table>,
}
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn snapshot(db: &Connection) -> Result<Snapshot, String> {
    let version = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(err)?;
    let application = db
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(err)?;
    let mut stmt = db
        .prepare("SELECT name,type,sql FROM sqlite_schema ORDER BY name")
        .map_err(err)?;
    let schema: Vec<(String, String, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut tables = BTreeMap::new();
    for (name, kind, _) in &schema {
        if kind != "table" {
            continue;
        }
        let mut stmt = db
            .prepare(&format!("SELECT * FROM {}", quote(name)))
            .map_err(err)?;
        let columns = stmt
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let order = (1..=columns.len())
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        stmt = db
            .prepare(&format!("SELECT * FROM {} ORDER BY {order}", quote(name)))
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                (0..columns.len())
                    .map(|i| {
                        Ok(match r.get_ref(i)? {
                            ValueRef::Null => Cell::Null,
                            ValueRef::Integer(v) => Cell::Integer(v),
                            ValueRef::Real(v) => Cell::Real(v.to_bits()),
                            ValueRef::Text(v) => Cell::Text(
                                std::str::from_utf8(v)
                                    .map_err(|e| {
                                        rusqlite::Error::FromSqlConversionFailure(
                                            i,
                                            rusqlite::types::Type::Text,
                                            Box::new(e),
                                        )
                                    })?
                                    .to_owned(),
                            ),
                            ValueRef::Blob(v) => Cell::Blob(v.to_vec()),
                        })
                    })
                    .collect::<Result<Vec<_>, rusqlite::Error>>()
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        tables.insert(name.clone(), Table { columns, rows });
    }
    Ok(Snapshot {
        version,
        application,
        schema,
        tables,
    })
}
pub(super) fn open(path: &Path, write: bool) -> Result<Connection, String> {
    let db = Connection::open_with_flags(
        path,
        if write {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        },
    )
    .map_err(err)?;
    db.busy_timeout(Duration::from_secs(2)).map_err(err)?;
    db.execute_batch("PRAGMA foreign_keys=ON;").map_err(err)?;
    Ok(db)
}
pub(crate) fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut db = open(path, false)?;
    let tx = db.transaction().map_err(err)?;
    validate(&tx)?;
    serde_json::to_vec(&snapshot(&tx)?).map_err(err)
}
pub(super) fn capture(db: &Connection) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&snapshot(db)?).map_err(err)
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

fn insert_table(db: &Connection, name: &str, table: &Table) -> Result<(), String> {
    // Explicit row IDs can advance this table while other tables are restored.
    if name == "sqlite_sequence" {
        db.execute("DELETE FROM sqlite_sequence", []).map_err(err)?;
    }
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote(name),
        table
            .columns
            .iter()
            .map(|n| quote(n))
            .collect::<Vec<_>>()
            .join(","),
        vec!["?"; table.columns.len()].join(",")
    );
    let mut stmt = db.prepare(&sql).map_err(err)?;
    for row in &table.rows {
        stmt.execute(rusqlite::params_from_iter(row.iter().map(Cell::value)))
            .map_err(err)?;
    }
    Ok(())
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
