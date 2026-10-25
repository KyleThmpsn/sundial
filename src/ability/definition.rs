//! Ability definitions: the rows a Subclass pool record names its ability by.
//!
//! A pool record never names an ability by hash. Its +0 hash is a key in the ability bank's
//! property rows. The ability is a one-byte row index: +0xB, the row that gets equipped, and +4,
//! the row a property key applies to. The rows live in two tables that hold one row each per
//! ability, in the same order:
//!
//! - globals slot 69 (`81613D26`, class `80805CA4`): 28 byte rows of class `80805CAA`, an
//!   identity hash at +0, the pattern hash at +4 that keys the ability's entity in the entity
//!   assignment table, then two empty strings and an empty icon.
//! - root slot 105 (`81319130`, class `80807AAF`): the identity hashes alone.
//!
//! Both are single terminal arrays: the payload size at +0, a count and a relative pointer at +8,
//! the array header at +0x20 and the rows from +0x30 to the end. The client reads a row index as a
//! signed byte, so the tables hold at most 128 rows.
use crate::package_payload::{native_array_at, u32_at, u64_at, write_bytes};

/// Globals slot of the definition table, and root slot of the identity table.
pub const DEFINITION_TABLE_SLOT: usize = 69;
pub const IDENTITY_TABLE_SLOT: usize = 105;
pub const DEFINITION_TABLE_CLASS: u32 = 0x8080_5CA4;
pub const DEFINITION_ROW_CLASS: u32 = 0x8080_5CAA;
pub const IDENTITY_TABLE_CLASS: u32 = 0x8080_7AAF;
const DESCRIPTOR: usize = 0x08;
const HEADER: usize = 0x20;
const DEFINITION_ROW_SIZE: usize = 28;
const IDENTITY_ROW_SIZE: usize = 4;
const ROW_IDENTITY: usize = 0x00;
const ROW_PATTERN: usize = 0x04;
/// Rows a signed byte can name.
pub const MOST_ROWS: usize = 128;

/// One ability row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Definition {
    pub identity: u32,
    /// The entity assignment key of the ability's entity.
    pub pattern: u32,
}

/// A terminal array's row count and first row.
fn rows(
    payload: &[u8],
    row_size: usize,
    row_class: Option<u32>,
    label: &str,
) -> Result<(usize, usize), String> {
    let size = usize::try_from(u64_at(payload, 0)?)
        .map_err(|_| format!("{label} size does not fit this platform"))?;
    if size != payload.len() {
        return Err(format!(
            "{label} size field {size} disagrees with its {} bytes",
            payload.len()
        ));
    }
    let (count, header, first, class) = native_array_at(payload, DESCRIPTOR)?;
    if header != HEADER {
        return Err(format!(
            "{label} array header is at 0x{header:X}, expected 0x{HEADER:X}"
        ));
    }
    if let Some(expected) = row_class
        && class != expected
    {
        return Err(format!(
            "{label} rows have class 0x{class:08X}, expected 0x{expected:08X}"
        ));
    }
    let end = count
        .checked_mul(row_size)
        .and_then(|bytes| bytes.checked_add(first))
        .ok_or_else(|| format!("{label} row extent overflowed"))?;
    if end != payload.len() {
        return Err(format!("{label} rows do not end the payload"));
    }
    Ok((count, first))
}

/// Every ability row, with the identity table checked against the definition table.
pub fn definitions(definitions: &[u8], identities: &[u8]) -> Result<Vec<Definition>, String> {
    let (count, first) = rows(
        definitions,
        DEFINITION_ROW_SIZE,
        Some(DEFINITION_ROW_CLASS),
        "Ability definition table",
    )?;
    let (identity_count, identity_first) = rows(
        identities,
        IDENTITY_ROW_SIZE,
        None,
        "Ability identity table",
    )?;
    if identity_count != count {
        return Err(format!(
            "The ability definition table has {count} rows and its identity table {identity_count}"
        ));
    }
    if count > MOST_ROWS {
        return Err(format!("The ability definition table has {count} rows"));
    }
    (0..count)
        .map(|row| {
            let at = first + row * DEFINITION_ROW_SIZE;
            let definition = Definition {
                identity: u32_at(definitions, at + ROW_IDENTITY)?,
                pattern: u32_at(definitions, at + ROW_PATTERN)?,
            };
            if u32_at(identities, identity_first + row * IDENTITY_ROW_SIZE)? != definition.identity
            {
                return Err(format!(
                    "Ability row {row} has identity 0x{:08X} in one table and another in the other",
                    definition.identity
                ));
            }
            Ok(definition)
        })
        .collect()
}

/// Every row's pattern hash, from the definition table alone.
pub fn patterns(definitions: &[u8]) -> Result<Vec<u32>, String> {
    let (count, first) = rows(
        definitions,
        DEFINITION_ROW_SIZE,
        Some(DEFINITION_ROW_CLASS),
        "Ability definition table",
    )?;
    (0..count)
        .map(|row| u32_at(definitions, first + row * DEFINITION_ROW_SIZE + ROW_PATTERN))
        .collect()
}

/// The pattern hash of ability row `row`.
pub fn pattern(definitions: &[u8], row: u8) -> Result<u32, String> {
    let (count, first) = rows(
        definitions,
        DEFINITION_ROW_SIZE,
        Some(DEFINITION_ROW_CLASS),
        "Ability definition table",
    )?;
    let row = usize::from(row);
    if row >= count {
        return Err(format!(
            "Ability row {row} is outside the {count}-row table"
        ));
    }
    u32_at(definitions, first + row * DEFINITION_ROW_SIZE + ROW_PATTERN)
}

/// The entity ability row `row` names, through the entity assignment table.
pub fn entity(definitions: &[u8], assignments: &[u8], row: u8) -> Result<Option<u32>, String> {
    crate::entity::weapon_entity_assignment(assignments, pattern(definitions, row)?)
}

/// Both tables with one more row: a copy of `template` under `identity` and `pattern`, which
/// keeps its strings and icon. Returns the two payloads and the new row.
pub fn append(
    definitions: &[u8],
    identities: &[u8],
    template: u8,
    Definition { identity, pattern }: Definition,
) -> Result<(Vec<u8>, Vec<u8>, u8), String> {
    let existing = self::definitions(definitions, identities)?;
    if existing.iter().any(|row| row.identity == identity) {
        return Err(format!(
            "Ability identity 0x{identity:08X} is already a row"
        ));
    }
    if existing.iter().any(|row| row.pattern == pattern) {
        return Err(format!("Ability pattern 0x{pattern:08X} is already a row"));
    }
    let row = existing.len();
    let new_row = u8::try_from(row)
        .ok()
        .filter(|row| usize::from(*row) < MOST_ROWS)
        .ok_or_else(|| format!("The ability tables are full at {MOST_ROWS} rows"))?;
    let template = usize::from(template);
    if template >= row {
        return Err(format!(
            "Ability template row {template} is outside the {row}-row table"
        ));
    }
    let (_, first) = rows(
        definitions,
        DEFINITION_ROW_SIZE,
        Some(DEFINITION_ROW_CLASS),
        "Ability definition table",
    )?;
    let copied = first + template * DEFINITION_ROW_SIZE;
    let mut authored = definitions.to_vec();
    authored.extend_from_slice(
        definitions
            .get(copied..copied + DEFINITION_ROW_SIZE)
            .ok_or("The ability template row is outside the table")?,
    );
    let at = first + row * DEFINITION_ROW_SIZE;
    write_bytes(&mut authored, at + ROW_IDENTITY, &identity.to_le_bytes())?;
    write_bytes(&mut authored, at + ROW_PATTERN, &pattern.to_le_bytes())?;
    let mut authored_identities = identities.to_vec();
    authored_identities.extend_from_slice(&identity.to_le_bytes());
    for payload in [&mut authored, &mut authored_identities] {
        let count = (row + 1) as u64;
        let size = payload.len() as u64;
        write_bytes(payload, 0, &size.to_le_bytes())?;
        write_bytes(payload, DESCRIPTOR, &count.to_le_bytes())?;
        write_bytes(payload, HEADER, &count.to_le_bytes())?;
    }
    let appended = self::definitions(&authored, &authored_identities)?;
    if appended.len() != row + 1
        || appended[..row] != existing[..]
        || appended[row] != (Definition { identity, pattern })
    {
        return Err("The appended ability row did not round-trip".into());
    }
    Ok((authored, authored_identities, new_row))
}
