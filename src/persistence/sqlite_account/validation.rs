//! Runtime constraints from Sunrise's account and settings readers beyond the SQL schema.
use super::SqliteAccountError;
use rusqlite::Connection;

pub(super) fn validate(db: &Connection) -> Result<(), SqliteAccountError> {
    let verdict: String = db
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(|error| SqliteAccountError::sqlite("check integrity of", error))?;
    if verdict != "ok" {
        return Err(SqliteAccountError::invalid_data("database", verdict));
    }
    for (path, query) in [
        (
            "account_audio",
            "SELECT count(*) FROM account_audio WHERE migration_version != 8",
        ),
        (
            "account_display",
            "SELECT count(*) FROM account_display WHERE calibration_primary != 10000.0 OR calibration_alpha != 0.0",
        ),
        (
            "account_interface",
            "SELECT count(*) FROM account_interface WHERE reserved_text_mode != 0 OR subtitle_options_entry != 0",
        ),
        (
            "items.seen",
            "SELECT count(*) FROM items WHERE seen NOT IN (0,1)",
        ),
        (
            "profile_items.seen",
            "SELECT count(*) FROM profile_items WHERE seen NOT IN (0,1)",
        ),
        (
            "character_stacks",
            "SELECT count(*) FROM (
                 SELECT *, row_number() OVER (
                     PARTITION BY character_slot ORDER BY position
                 ) - 1 AS expected
                 FROM character_stacks
             ) WHERE position != expected
                OR position >= 32
                OR definition_hash NOT BETWEEN 0 AND 4294967295
                OR definition_hash = 2166136261
                OR quantity NOT BETWEEN 1 AND 2147483647
                OR mutation_serial NOT BETWEEN 0 AND 2147483647",
        ),
        (
            "character_stacks.definition_hash",
            "SELECT count(*) FROM (
                 SELECT character_slot, definition_hash FROM character_stacks
                 GROUP BY character_slot, definition_hash HAVING count(*) > 1
             )",
        ),
    ] {
        let invalid: i64 = db
            .query_row(query, [], |row| row.get(0))
            .map_err(|error| SqliteAccountError::sqlite("validate native rows in", error))?;
        if invalid != 0 {
            return Err(SqliteAccountError::invalid_data(
                path,
                "value is outside Sunrise's native runtime contract",
            ));
        }
    }
    Ok(())
}
