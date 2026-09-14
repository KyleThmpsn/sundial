//! Dense native list positions shared by inventory and progression persistence.
use rusqlite::{Connection, params};
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

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
