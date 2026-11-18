//! Runtime constraints from Sunrise's account and settings readers beyond the SQL schema.
use super::{Schema, SunriseAccountError};
use rusqlite::Connection;

/// Material stacks before schema 7: a dense list of at most 32 rows per character, one per
/// material.
const STACKS: [(&str, &str); 2] = [
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
];

/// Rows from schema 7 on: a dense stack list of at most 333 rows per character, where a material may
/// fill several stacks and each stack has its own mutation serial, and a placement on every item
/// and stack. An equipped item sits in its authored bucket and a recovery bucket holds single
/// items.
const PLACED_ROWS: [(&str, &str); 3] = [
    (
        "character_stacks",
        "SELECT count(*) FROM (
             SELECT *, row_number() OVER (
                 PARTITION BY character_slot ORDER BY position
             ) - 1 AS expected
             FROM character_stacks
         ) WHERE position != expected
            OR position >= 333
            OR definition_hash NOT BETWEEN 0 AND 4294967295
            OR definition_hash = 2166136261
            OR quantity NOT BETWEEN 1 AND 2147483647
            OR mutation_serial NOT BETWEEN 0 AND 2147483647
            OR placement NOT IN (0,1)",
    ),
    (
        "character_stacks.mutation_serial",
        "SELECT count(*) FROM (
             SELECT character_slot, mutation_serial FROM character_stacks
             GROUP BY character_slot, mutation_serial HAVING count(*) > 1
         )",
    ),
    (
        "items.placement",
        "SELECT count(*) FROM items WHERE placement NOT IN (0,1)
            OR (location = 0 AND placement != 0)
            OR (placement = 1 AND quantity != 1)",
    ),
];

/// Schema 8 flag rows as Sunrise writes them: a mod identifier of at most 64 characters, a name
/// of 1 to 64, and a value of 0 or 1 wherever the row is marked boolean.
const MISSION_FLAGS: (&str, &str) = (
    "mission_flags",
    "SELECT count(*) FROM mission_flags WHERE length(mod_id) > 64
        OR length(name) NOT BETWEEN 1 AND 64
        OR boolean NOT IN (0,1)
        OR (boolean = 1 AND value NOT IN (0,1))",
);

pub(super) fn validate(db: &Connection, schema: Schema) -> Result<(), SunriseAccountError> {
    let verdict: String = db
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(|error| SunriseAccountError::sqlite("check integrity of", error))?;
    if verdict != "ok" {
        return Err(SunriseAccountError::invalid_data("database", verdict));
    }
    let schema_rows: &[(&str, &str)] = if schema.has_placement() {
        &PLACED_ROWS
    } else {
        &STACKS
    };
    for &(path, query) in [
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
    ]
    .iter()
    .chain(schema_rows)
    .chain(schema.has_mission_flags().then_some(&MISSION_FLAGS))
    {
        let invalid: i64 = db
            .query_row(query, [], |row| row.get(0))
            .map_err(|error| SunriseAccountError::sqlite("validate native rows in", error))?;
        if invalid != 0 {
            return Err(SunriseAccountError::invalid_data(
                path,
                "value is outside Sunrise's native runtime contract",
            ));
        }
    }
    Ok(())
}

pub(super) fn connection(db: &Connection) -> Result<(), String> {
    match super::reader::load_connection(db).map_err(|error| error.to_string())? {
        super::SunriseAccountLoad::Loaded(_) => {
            super::progression::Progression::load(db).map_err(|error| error.to_string())?;
            super::entitlements::load(db).map_err(|error| error.to_string())?;
            super::runtime::load(db).map_err(|error| error.to_string())?;
            Ok(())
        }
        _ => Err("A compatible Sunrise investment database is required".into()),
    }
}
