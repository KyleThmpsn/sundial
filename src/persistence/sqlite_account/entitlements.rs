//! Native ownership rows projected into the existing ownership editor.
use super::SqliteAccountError;
use rusqlite::Connection;
use serde_json::{Value, json};
const OWNERSHIP: [&str; 3] = ["none", "handle", "application"];
pub(super) fn load(db: &Connection) -> Result<Value, SqliteAccountError> {
    let mut stmt = db
        .prepare("SELECT position,name,ownership FROM entitlements ORDER BY position")
        .map_err(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, usize>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, usize>(2)?,
            ))
        })
        .map_err(sql)?;
    let mut values = Vec::new();
    for row in rows {
        let (position, name, owned) = row.map_err(sql)?;
        if position != values.len() || position >= 128 || name.is_empty() || name.len() > 31 {
            return Err(SqliteAccountError::invalid_data(
                "entitlements",
                "invalid native position or name",
            ));
        }
        values.push(json!({"name":name,"owned":OWNERSHIP.get(owned).ok_or_else(||SqliteAccountError::invalid_data("entitlements","unknown ownership"))?}));
    }
    let value = Value::Array(values);
    crate::game_settings::runtime::validate_native_entitlements(
        &json!({"server":{"entitlements":value}}),
    )
    .map_err(|error| SqliteAccountError::invalid_data("entitlements", error))?;
    Ok(value)
}
pub(super) fn save(
    db: &Connection,
    value: &Value,
    old: &[super::writer::NativeRow],
) -> Result<(), SqliteAccountError> {
    if load(db)? == *value {
        return Ok(());
    }
    db.execute("DELETE FROM entitlements", []).map_err(sql)?;
    for (position, row) in value
        .as_array()
        .ok_or_else(|| SqliteAccountError::invalid_data("entitlements", "expected an array"))?
        .iter()
        .enumerate()
    {
        let name = row["name"]
            .as_str()
            .ok_or_else(|| SqliteAccountError::invalid_data("entitlements", "missing name"))?;
        let owned = OWNERSHIP
            .iter()
            .position(|v| Some(*v) == row["owned"].as_str())
            .ok_or_else(|| SqliteAccountError::invalid_data("entitlements", "invalid ownership"))?;
        let mut native = old
            .iter()
            .find(|r| r.get("name") == Some(&super::package::Cell::Text(name.to_owned())))
            .cloned()
            .unwrap_or_default();
        native.insert(
            "position".into(),
            super::package::Cell::Integer(position as i64),
        );
        native.insert("name".into(), super::package::Cell::Text(name.to_owned()));
        native.insert(
            "ownership".into(),
            super::package::Cell::Integer(owned as i64),
        );
        super::writer::insert(db, "entitlements", native)?;
    }
    Ok(())
}
fn sql(error: rusqlite::Error) -> SqliteAccountError {
    SqliteAccountError::sqlite("edit entitlements in", error)
}
