//! Format-neutral logical SQLite snapshots, including WAL-visible and unexposed rows.
use rusqlite::{
    Connection, OpenFlags, TransactionBehavior,
    types::{Value, ValueRef},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(String),
    Blob(Vec<u8>),
}
impl Cell {
    pub(crate) fn from_value(value: Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Integer(v) => Self::Integer(v),
            Value::Real(v) => Self::Real(v.to_bits()),
            Value::Text(v) => Self::Text(v),
            Value::Blob(v) => Self::Blob(v),
        }
    }

    pub(crate) fn value(&self) -> Value {
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
pub(crate) struct Table {
    pub(crate) columns: Vec<String>,
    pub(crate) rows: Vec<Vec<Cell>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub(crate) version: i64,
    pub(crate) application: i64,
    pub(crate) schema: Vec<(String, String, Option<String>)>,
    pub(crate) tables: BTreeMap<String, Table>,
}
pub(crate) fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
pub(crate) fn snapshot(db: &Connection) -> Result<Snapshot, String> {
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
pub(crate) fn open(path: &Path, write: bool) -> Result<Connection, String> {
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
pub(crate) fn capture(db: &Connection) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&snapshot(db)?).map_err(err)
}
pub(crate) fn insert_table(db: &Connection, name: &str, table: &Table) -> Result<(), String> {
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

/// Restore the exact pre-save rows only while the complete committed state still matches.
/// The comparison and restoration share one write lock. Runtime schema validation stays
/// with each adapter, and rollback never changes the schema.
pub(crate) fn rollback(path: &Path, expected: &[u8], original: &[u8]) -> Result<(), String> {
    let after: Snapshot = serde_json::from_slice(original).map_err(err)?;
    let mut db = open(path, true)?;
    // Restore all tables together without cascades removing rows restored earlier.
    db.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA synchronous=FULL;")
        .map_err(err)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    let before = snapshot(&tx)?;
    if serde_json::to_vec(&before).map_err(err)? != expected {
        return Err("The account database changed after saving. Reload before restoring".into());
    }
    if before.schema != after.schema
        || before.version != after.version
        || before.application != after.application
        || before.tables.keys().ne(after.tables.keys())
        || before
            .tables
            .iter()
            .any(|(name, table)| table.columns != after.tables[name].columns)
    {
        return Err("Rollback cannot change the account database schema".into());
    }
    for name in after.tables.keys() {
        tx.execute(&format!("DELETE FROM {}", quote(name)), [])
            .map_err(err)?;
    }
    for (name, table) in after
        .tables
        .iter()
        .filter(|(name, _)| name.as_str() != "sqlite_sequence")
    {
        insert_table(&tx, name, table)?;
    }
    // Explicit inserts may advance sequences, including those of extension tables.
    if let Some(table) = after.tables.get("sqlite_sequence") {
        insert_table(&tx, "sqlite_sequence", table)?;
    }
    let invalid = tx
        .prepare("PRAGMA foreign_key_check")
        .map_err(err)?
        .exists([])
        .map_err(err)?;
    if invalid || snapshot(&tx)? != after {
        return Err(
            "Rollback did not match the original account. The database was not changed".into(),
        );
    }
    tx.commit().map_err(err)
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
