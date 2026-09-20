//! Fail closed before rewriting a graph whose extensions we cannot preserve.
use rusqlite::Connection;

use super::{contract, error::DawnAccountError};

const REWRITTEN: [&str; 6] = [
    "account",
    "characters",
    "character_items",
    "profile_items",
    "item_sockets",
    "item_rolls",
];

pub(super) fn validate(db: &Connection) -> Result<(), DawnAccountError> {
    let refuse = |detail: String| {
        DawnAccountError::Unwritable(format!(
            "Dawn's database layout is not supported: {detail}. No changes were saved. Update Sundial and reload before saving"
        ))
    };
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version != contract::SCHEMA_VERSION {
        return Err(refuse(format!("schema version {version}")));
    }
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(contract::SCHEMA)?;
    for table in REWRITTEN {
        if columns(db, table)? != columns(&reference, table)? {
            return Err(refuse(format!("{table} has unrecognized columns")));
        }
    }
    let mut query = db.prepare(
        "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )?;
    let tables = query
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for table in tables {
        let actual = graph_relations(db, &table)?;
        if actual != graph_relations(&reference, &table)? {
            return Err(refuse(format!(
                "{table} has unrecognized account relationships"
            )));
        }
    }
    // Even an unrelated trigger can write into the rebuilt graph. Unknown triggers cannot be
    // assumed side-effect-free. Dawn's reviewed schema defines none.
    let trigger: Option<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='trigger' LIMIT 1")?
        .query_map([], |r| r.get(0))?
        .next()
        .transpose()?;
    if let Some(trigger) = trigger {
        return Err(refuse(format!("unrecognized trigger {trigger}")));
    }
    Ok(())
}

fn columns(db: &Connection, table: &str) -> rusqlite::Result<Vec<Vec<rusqlite::types::Value>>> {
    let mut query = db.prepare("SELECT name,type,\"notnull\",dflt_value,pk,hidden FROM pragma_table_xinfo(?1) ORDER BY cid")?;
    query
        .query_map([table], |r| (0..6).map(|i| r.get(i)).collect())?
        .collect()
}

fn graph_relations(db: &Connection, table: &str) -> rusqlite::Result<Vec<Vec<String>>> {
    let mut query = db.prepare("SELECT \"table\",\"from\",\"to\",on_update,on_delete,\"match\" FROM pragma_foreign_key_list(?1) ORDER BY id,seq")?;
    let rows = query.query_map([table], |r| {
        (0..6)
            .map(|i| r.get::<_, Option<String>>(i).map(Option::unwrap_or_default))
            .collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let mut relations = Vec::new();
    for row in rows {
        let row = row?;
        if REWRITTEN
            .iter()
            .any(|table| table.eq_ignore_ascii_case(&row[0]))
        {
            relations.push(row);
        }
    }
    relations.sort();
    Ok(relations)
}
