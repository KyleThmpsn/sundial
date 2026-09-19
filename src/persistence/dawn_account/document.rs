//! Opening and holding one loaded Dawn player-state database.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::contract;
use super::error::{DawnAccountError, DawnAccountIncompatibility};
use super::reader::{self, DawnAllocators, DawnMetadata};
use super::{DawnAccountDocument, DawnAccountDocumentLoad, DawnAccountSnapshot};

/// Opens the database read-only so a load never creates, migrates, or checkpoints the file.
///
/// Dawn owns this database. It creates it on its first boot by importing settings.json, and it
/// refuses to start when the file is busy, corrupt, or newer than it understands. Sundial mirrors
/// that stance rather than repairing anything it finds.
pub(crate) fn load(path: &Path) -> Result<DawnAccountDocumentLoad, DawnAccountError> {
    if !path.try_exists().unwrap_or(false) {
        return Ok(DawnAccountDocumentLoad::Missing);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;

    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 0 {
        return Ok(DawnAccountDocumentLoad::Empty);
    }
    if version != contract::SCHEMA_VERSION {
        return Ok(DawnAccountDocumentLoad::Incompatible(
            DawnAccountIncompatibility::SchemaVersion { found: version },
        ));
    }

    macro_rules! read {
        ($call:expr) => {
            match $call? {
                Ok(value) => value,
                Err(problem) => return Ok(DawnAccountDocumentLoad::Incompatible(problem)),
            }
        };
    }

    let metadata: DawnMetadata = read!(reader::metadata(&connection));
    let allocators: DawnAllocators = read!(reader::allocators(&connection));
    let primary_soid = read!(reader::primary_soid(&connection));
    let profile = read!(reader::profile(&connection));
    let characters = read!(reader::characters(&connection));
    let (settings, settings_index) = read!(reader::settings(&connection));
    let loaded_settings = settings.clone();
    let carried = super::carried::read(&connection)?;

    Ok(DawnAccountDocumentLoad::Loaded(Box::new(
        DawnAccountDocument {
            path: PathBuf::from(path),
            metadata,
            allocators,
            snapshot: DawnAccountSnapshot {
                primary_soid,
                profile,
                characters,
                settings,
            },
            carried,
            settings_index,
            loaded_settings,
        },
    )))
}
