//! Native account and character fields used by the runtime settings page.
use super::SqliteAccountError;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
pub(super) fn load(db: &Connection) -> Result<Value, SqliteAccountError> {
    let setup: bool = db
        .query_row(
            "SELECT profile_setup_completed FROM account WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    let mut stmt=db.prepare("SELECT preview_available,appearance_value,last_orbited_destination,content_bypass,level,equipped_title FROM characters ORDER BY slot").map_err(sql)?;
    let rows=stmt.query_map([],|r|Ok(json!({"preview_available":r.get::<_,bool>(0)?,"appearance_value":r.get::<_,f64>(1)?,"last_orbited_destination":r.get::<_,u32>(2)?,"content_bypass":r.get::<_,bool>(3)?,"level":r.get::<_,u8>(4)?,"equipped_title":r.get::<_,u16>(5)?}))).map_err(sql)?.collect::<Result<Vec<_>,_>>().map_err(sql)?;
    Ok(json!({"account":{"profile_setup_completed":setup},"characters":rows}))
}
pub(super) fn save(db: &Connection, value: &Value) -> Result<(), SqliteAccountError> {
    let setup = value["account"]["profile_setup_completed"]
        .as_bool()
        .ok_or_else(invalid)?;
    db.execute(
        "UPDATE account SET profile_setup_completed=? WHERE id=1",
        [setup],
    )
    .map_err(sql)?;
    let mut stmt = db
        .prepare("SELECT slot FROM characters ORDER BY slot")
        .map_err(sql)?;
    let slots = stmt
        .query_map([], |r| r.get::<_, i32>(0))
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    let rows = value["characters"].as_array().ok_or_else(invalid)?;
    if slots.len() != rows.len() {
        return Err(invalid());
    }
    for (&slot, row) in slots.iter().zip(rows) {
        let preview = row["preview_available"].as_bool().ok_or_else(invalid)?;
        let content = row["content_bypass"].as_bool().ok_or_else(invalid)?;
        let appearance = row["appearance_value"].as_f64().ok_or_else(invalid)?;
        let destination = crate::hash::parse_unsigned_value(&row["last_orbited_destination"])
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(invalid)?;
        let level = row["level"]
            .as_u64()
            .and_then(|v| u8::try_from(v).ok())
            .ok_or_else(invalid)?;
        let title = row["equipped_title"]
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(invalid)?;
        db.execute("UPDATE characters SET preview_available=?,appearance_value=?,last_orbited_destination=?,content_bypass=?,level=?,equipped_title=? WHERE slot=?",params![preview,appearance,destination,content,level,title,slot]).map_err(sql)?;
    }
    Ok(())
}
fn invalid() -> SqliteAccountError {
    SqliteAccountError::invalid_data("characters", "invalid native runtime details")
}
fn sql(error: rusqlite::Error) -> SqliteAccountError {
    SqliteAccountError::sqlite("edit runtime details in", error)
}
