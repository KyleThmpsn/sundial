//! Logical database snapshots for Parhelion's durable package/account journal.
//! Never replace SQLite files as raw bytes. Commits and recovery compare a complete logical
//! snapshot under a write transaction, including WAL content and unexposed native tables.
use super::{SqliteAccountLoad, reader};
use crate::investment::{AuthoredAccountCleanup, AuthoredCollectionUnlock, AuthoredSocketChange};
use rusqlite::{
    Connection, OpenFlags, TransactionBehavior, params,
    types::{Value, ValueRef},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};
mod recovery;
pub(super) use recovery::restore;
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
struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<Cell>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Snapshot {
    version: i64,
    application: i64,
    schema: Vec<(String, String, Option<String>)>,
    tables: BTreeMap<String, Table>,
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
fn open(path: &Path, write: bool) -> Result<Connection, String> {
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
fn validate(db: &Connection) -> Result<(), String> {
    match reader::load_connection(db).map_err(err)? {
        SqliteAccountLoad::Loaded(_) => {
            super::progression::Progression::load(db).map_err(err)?;
            super::entitlements::load(db).map_err(err)?;
            super::runtime::load(db).map_err(err)?;
            Ok(())
        }
        _ => Err("A compatible Sunrise investment database is required".into()),
    }
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
        if !edited.contains(&name.as_str()) && after.tables.get(name) != Some(table) {
            return Err(format!(
                "Saving would change unedited table {name}. The database was not changed"
            ));
        }
    }
    Ok(())
}
pub(crate) fn replace(path: &Path, expected: &[u8], updated: &[u8]) -> Result<(), String> {
    replace_snapshot(path, expected, updated, false)
}
pub(super) fn rollback(path: &Path, expected: &[u8], updated: &[u8]) -> Result<(), String> {
    replace_snapshot(path, expected, updated, true)
}
fn replace_snapshot(
    path: &Path,
    expected: &[u8],
    updated: &[u8],
    all_tables: bool,
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
    const ORDER: [&str; 8] = [
        "sockets",
        "items",
        "profile_items",
        "dismantle_rewards",
        "character_stacks",
        "pending_rewards",
        "unlocks",
        "family5",
    ];
    for (name, table) in &after.tables {
        if before.tables[name] != *table
            && ((!all_tables && !ORDER.contains(&name.as_str()))
                || before.tables[name].columns != table.columns)
        {
            return Err(format!("Unsupported account proposal table {name}"));
        }
    }
    tx.execute_batch("PRAGMA defer_foreign_keys=ON;")
        .map_err(err)?;
    // Sockets reference item identities. Rewrite them with any changed item collection so that
    // ON DELETE CASCADE cannot discard unchanged socket rows.
    let items_changed = before.tables["items"] != after.tables["items"];
    let changed = if all_tables {
        before.tables.keys().map(String::as_str).collect::<Vec<_>>()
    } else {
        ORDER
            .into_iter()
            .filter(|name| {
                before.tables[*name] != after.tables[*name] || (*name == "sockets" && items_changed)
            })
            .collect::<Vec<_>>()
    };
    for name in &changed {
        tx.execute(&format!("DELETE FROM {}", quote(name)), [])
            .map_err(err)?;
    }
    for name in changed.iter().rev() {
        let table = &after.tables[*name];
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

pub(crate) fn preview(
    path: &Path,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    changes: &[AuthoredSocketChange],
) -> Result<AuthoredAccountCleanup, String> {
    let mut source = open(path, false)?;
    let tx = source.transaction().map_err(err)?;
    validate(&tx)?;
    let original_bytes = serde_json::to_vec(&snapshot(&tx)?).map_err(err)?;
    let mut staged = Connection::open_in_memory().map_err(err)?;
    rusqlite::backup::Backup::new(&tx, &mut staged)
        .map_err(err)?
        .run_to_completion(128, Duration::from_millis(1), None)
        .map_err(err)?;
    staged
        .execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(err)?;
    let mut report = AuthoredAccountCleanup {
        settings_path: path.to_path_buf(),
        original_bytes,
        cleaned_bytes: Vec::new(),
        removed_items: BTreeMap::new(),
        cleared_plugs: 0,
        removed_reward_rules: 0,
        cleared_unlocks: 0,
        resized_items: BTreeMap::new(),
    };
    for hash in hashes {
        report.cleared_plugs += staged
            .execute("DELETE FROM sockets WHERE plug_hash=?", [i64::from(*hash)])
            .map_err(err)?;
        let mut removed = 0;
        for table in ["items", "profile_items", "character_stacks"] {
            removed += staged
                .execute(
                    &format!("DELETE FROM {table} WHERE definition_hash=?"),
                    [i64::from(*hash)],
                )
                .map_err(err)?;
        }
        if removed != 0 {
            report.removed_items.insert(*hash, removed);
        }
        for table in ["dismantle_rewards", "pending_rewards"] {
            report.removed_reward_rules += staged
                .execute(
                    &format!("DELETE FROM {table} WHERE definition_hash=?"),
                    [i64::from(*hash)],
                )
                .map_err(err)?;
        }
    }
    for unlock in unlocks {
        let removed_override = staged
            .execute(
                "DELETE FROM family5 WHERE kind=0 AND slot=?",
                [unlock.definition_index],
            )
            .map_err(err)?;
        let bank = match unlock.bank {
            1 => 0,
            2 => 1,
            3 => 4,
            6 => 2,
            _ => return Err("Unsupported authored unlock bank".into()),
        };
        let removed_flag = staged
            .execute(
                "DELETE FROM unlocks WHERE bank=? AND slot=? AND lane=0",
                params![bank, unlock.slot],
            )
            .map_err(err)?;
        report.cleared_unlocks += usize::from(removed_override != 0 || removed_flag != 0);
    }
    for table in ["profile_items", "dismantle_rewards"] {
        compact(&staged, table, "1=1")?;
    }
    compact(&staged, "family5", "kind=0")?;
    for slot in 0..3 {
        compact(
            &staged,
            "items",
            &format!("character_slot={slot} AND location=1"),
        )?;
        compact(
            &staged,
            "character_stacks",
            &format!("character_slot={slot}"),
        )?;
    }
    resize(&staged, hashes, changes, &mut report.resized_items)?;
    validate(&staged)?;
    report.cleaned_bytes = serde_json::to_vec(&snapshot(&staged)?).map_err(err)?;
    Ok(report)
}
pub(super) fn compact(db: &Connection, table: &str, condition: &str) -> Result<(), String> {
    let mut stmt = db
        .prepare(&format!(
            "SELECT position FROM {table} WHERE {condition} ORDER BY position"
        ))
        .map_err(err)?;
    let positions = stmt
        .query_map([], |r| r.get::<_, i64>(0))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    for (new, old) in positions.into_iter().enumerate() {
        if new as i64 != old {
            db.execute(
                &format!("UPDATE {table} SET position=? WHERE {condition} AND position=?"),
                params![new, old],
            )
            .map_err(err)?;
        }
    }
    Ok(())
}
fn resize(
    db: &Connection,
    removed: &BTreeSet<u32>,
    changes: &[AuthoredSocketChange],
    report: &mut BTreeMap<u32, usize>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for change in changes {
        if removed.contains(&change.definition_hash)
            || !seen.insert(change.definition_hash)
            || change.previous_socket_count > 12
            || change.default_plugs.len() > 12
            || change
                .default_plugs
                .iter()
                .flatten()
                .any(|h| *h == 0 || *h == u32::MAX)
        {
            return Err("Conflicting or unsupported replacement socket layouts".into());
        }
        let mut stmt=db.prepare("SELECT instance_soid,plug_count FROM items WHERE definition_hash=? AND socket_policy=1").map_err(err)?;
        let rows = stmt
            .query_map([i64::from(change.definition_hash)], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, usize>(1)?))
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        for (soid, count) in rows {
            let new = change.default_plugs.len();
            if count == new {
                continue;
            }
            if count != change.previous_socket_count {
                return Err(
                    "An authored item's socket count differs from the reviewed package".into(),
                );
            }
            db.execute(
                "DELETE FROM sockets WHERE instance_soid=? AND lane>=?",
                params![soid, new],
            )
            .map_err(err)?;
            for (lane, hash) in change.default_plugs.iter().enumerate().skip(count) {
                if let Some(hash) = hash {
                    db.execute(
                        "INSERT INTO sockets(instance_soid,lane,plug_hash) VALUES(?,?,?)",
                        params![soid, lane, hash],
                    )
                    .map_err(err)?;
                }
            }
            db.execute(
                "UPDATE items SET plug_count=? WHERE instance_soid=?",
                params![new, soid],
            )
            .map_err(err)?;
            *report.entry(change.definition_hash).or_default() += 1;
        }
    }
    Ok(())
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
